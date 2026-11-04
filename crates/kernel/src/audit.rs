//! Audit records (FR-GEN-001).
//!
//! Every create, update or delete of a domain record writes an append-only audit entry:
//! actor, timestamp, field, old value and new value. Aggregates (or application
//! services) build an [`AuditRecord`]; the platform audit log persists it in the same
//! transaction as the change.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::actor::Actor;
use crate::id::TenantId;

/// What happened to the audited record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// The record was created.
    Created,
    /// One or more fields changed.
    Updated,
    /// The record was (soft or hard) deleted.
    Deleted,
    /// A soft-deleted record was restored.
    Restored,
}

impl AuditAction {
    /// Stable lower-case name used in storage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Deleted => "deleted",
            Self::Restored => "restored",
        }
    }
}

/// One changed field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditChange {
    /// Field name as used in the API, for example `status_id`.
    pub field: String,
    /// Value before the change; `None` when the field was unset or the record is new.
    pub old_value: Option<serde_json::Value>,
    /// Value after the change; `None` when the field was cleared or the record deleted.
    pub new_value: Option<serde_json::Value>,
}

/// One audited action on one record, with the fields it changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Company the record belongs to, or `None` for instance-wide records.
    pub tenant_id: Option<TenantId>,
    /// Aggregate type, for example `ticket`.
    pub aggregate_type: String,
    /// Aggregate ID.
    pub aggregate_id: Uuid,
    /// What happened.
    pub action: AuditAction,
    /// Who did it.
    pub actor: Actor,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
    /// Changed fields. May be empty, for example for a delete.
    pub changes: Vec<AuditChange>,
}

impl AuditRecord {
    /// Starts a record.
    #[must_use]
    pub fn new(
        tenant_id: Option<TenantId>,
        aggregate_type: impl Into<String>,
        aggregate_id: Uuid,
        action: AuditAction,
        actor: Actor,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            tenant_id,
            aggregate_type: aggregate_type.into(),
            aggregate_id,
            action,
            actor,
            occurred_at,
            changes: Vec::new(),
        }
    }

    /// Adds a field change if `old` and `new` differ. Values are serialized to JSON;
    /// values that fail to serialize are recorded as `null` rather than lost silently.
    #[must_use]
    pub fn change<T: Serialize + PartialEq>(
        mut self,
        field: impl Into<String>,
        old: Option<&T>,
        new: Option<&T>,
    ) -> Self {
        if old != new {
            let to_json = |v: &T| serde_json::to_value(v).unwrap_or(serde_json::Value::Null);
            self.changes.push(AuditChange {
                field: field.into(),
                old_value: old.map(to_json),
                new_value: new.map(to_json),
            });
        }
        self
    }

    /// Whether the record carries anything worth storing: every action except
    /// `Updated` is always stored, an `Updated` only when a field actually changed.
    #[must_use]
    pub fn is_meaningful(&self) -> bool {
        self.action != AuditAction::Updated || !self.changes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn record(action: AuditAction) -> AuditRecord {
        AuditRecord::new(
            Some(TenantId::new()),
            "ticket",
            Uuid::now_v7(),
            action,
            Actor::Automation,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        )
    }

    #[test]
    fn unchanged_fields_are_not_recorded() {
        let rec = record(AuditAction::Updated)
            .change("subject", Some(&"Printer"), Some(&"Printer"))
            .change("priority", Some(&"Normal"), Some(&"High"));
        assert_eq!(rec.changes.len(), 1);
        assert_eq!(rec.changes[0].field, "priority");
        assert_eq!(rec.changes[0].old_value, Some(serde_json::json!("Normal")));
        assert_eq!(rec.changes[0].new_value, Some(serde_json::json!("High")));
    }

    #[test]
    fn update_without_changes_is_not_meaningful() {
        assert!(!record(AuditAction::Updated).is_meaningful());
        assert!(record(AuditAction::Deleted).is_meaningful());
    }

    #[test]
    fn cleared_field_records_none() {
        let rec = record(AuditAction::Updated).change("assignee", Some(&1), None);
        assert_eq!(rec.changes[0].new_value, None);
    }
}
