//! Append-only Audit trail (FR-GEN-001).
//!
//! Call [`record`] with the same connection (transaction) that changes the aggregate, so
//! the change and its audit entry commit or roll back together.

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditAction, AuditRecord, ParseActorError, TenantId};
use sqlx::PgConnection;
use thiserror::Error;
use uuid::Uuid;

/// Reading the audit trail failed.
#[derive(Debug, Error)]
pub enum AuditError {
    /// Database error.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// A stored row has an invalid actor.
    #[error("corrupt audit entry {id}: {source}")]
    CorruptActor {
        /// The row.
        id: Uuid,
        /// What is wrong.
        source: ParseActorError,
    },
    /// A stored row has an unknown action.
    #[error("corrupt audit entry {id}: unknown action {action:?}")]
    CorruptAction {
        /// The row.
        id: Uuid,
        /// The stored value.
        action: String,
    },
}

/// Writes one audit record: one row per changed field, or a single row without a field
/// when nothing field-level changed (for example a delete). An `Updated` record without
/// changes is skipped.
///
/// # Errors
///
/// Fails on database errors, including a Row-Level Security violation when the record's
/// Company is outside the transaction's tenant scope.
pub async fn record(conn: &mut PgConnection, record: &AuditRecord) -> Result<(), sqlx::Error> {
    if !record.is_meaningful() {
        return Ok(());
    }
    let change_set_id = Uuid::now_v7();
    let tenant_id = record.tenant_id.map(|t| t.as_uuid());
    let rows: Vec<(
        Option<&str>,
        Option<&serde_json::Value>,
        Option<&serde_json::Value>,
    )> = if record.changes.is_empty() {
        vec![(None, None, None)]
    } else {
        record
            .changes
            .iter()
            .map(|c| {
                (
                    Some(c.field.as_str()),
                    c.old_value.as_ref(),
                    c.new_value.as_ref(),
                )
            })
            .collect()
    };
    for (field, old_value, new_value) in rows {
        sqlx::query!(
            r#"
            INSERT INTO audit_entries (
                id, change_set_id, tenant_id, aggregate_type, aggregate_id, action,
                field, old_value, new_value, actor_kind, actor_id, occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
            Uuid::now_v7(),
            change_set_id,
            tenant_id,
            record.aggregate_type,
            record.aggregate_id,
            record.action.as_str(),
            field,
            old_value,
            new_value,
            record.actor.kind().as_str(),
            record.actor.id(),
            record.occurred_at,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// One stored audit row.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    /// Row ID.
    pub id: Uuid,
    /// Rows written for the same action share this ID.
    pub change_set_id: Uuid,
    /// Company, if any.
    pub tenant_id: Option<TenantId>,
    /// What happened.
    pub action: AuditAction,
    /// Changed field, if the row describes a field change.
    pub field: Option<String>,
    /// Value before.
    pub old_value: Option<serde_json::Value>,
    /// Value after.
    pub new_value: Option<serde_json::Value>,
    /// Who did it.
    pub actor: Actor,
    /// When.
    pub occurred_at: DateTime<Utc>,
}

/// The audit history of one aggregate, oldest first. Only rows visible in the
/// connection's tenant scope are returned.
///
/// # Errors
///
/// Fails on database errors or corrupt rows.
pub async fn history(
    conn: &mut PgConnection,
    aggregate_type: &str,
    aggregate_id: Uuid,
) -> Result<Vec<AuditEntry>, AuditError> {
    let rows = sqlx::query!(
        r#"
        SELECT id, change_set_id, tenant_id, action, field, old_value, new_value,
               actor_kind, actor_id, occurred_at
        FROM audit_entries
        WHERE aggregate_type = $1 AND aggregate_id = $2
        ORDER BY occurred_at, id
        "#,
        aggregate_type,
        aggregate_id,
    )
    .fetch_all(conn)
    .await?;

    rows.into_iter()
        .map(|r| {
            let action = parse_action(&r.action).ok_or_else(|| AuditError::CorruptAction {
                id: r.id,
                action: r.action.clone(),
            })?;
            let actor = Actor::from_parts(&r.actor_kind, r.actor_id)
                .map_err(|source| AuditError::CorruptActor { id: r.id, source })?;
            Ok(AuditEntry {
                id: r.id,
                change_set_id: r.change_set_id,
                tenant_id: r.tenant_id.map(TenantId::from_uuid),
                action,
                field: r.field,
                old_value: r.old_value,
                new_value: r.new_value,
                actor,
                occurred_at: r.occurred_at,
            })
        })
        .collect()
}

fn parse_action(value: &str) -> Option<AuditAction> {
    [
        AuditAction::Created,
        AuditAction::Updated,
        AuditAction::Deleted,
        AuditAction::Restored,
    ]
    .into_iter()
    .find(|a| a.as_str() == value)
}
