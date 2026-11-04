//! FR-GEN-001: append-only audit entries with actor, timestamp, field, old and new value.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: failures should abort the test

use chrono::{TimeZone, Utc};
use cooper_kernel::{Actor, AnalystId, AuditAction, AuditRecord, TenantId, TenantScope};
use cooper_platform::{audit, tenancy};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = "../../migrations")]
async fn records_one_row_per_changed_field(pool: PgPool) {
    let tenant = TenantId::new();
    let analyst = AnalystId::new();
    let id = Uuid::now_v7();
    let at = Utc.with_ymd_and_hms(2026, 2, 3, 10, 0, 0).unwrap();
    let rec = AuditRecord::new(
        Some(tenant),
        "ticket",
        id,
        AuditAction::Updated,
        Actor::Analyst(analyst),
        at,
    )
    .change("priority", Some(&"Normal"), Some(&"High"))
    .change("subject", Some(&"VPN"), Some(&"VPN down"));

    let mut tx = tenancy::begin(&pool, &TenantScope::single(tenant))
        .await
        .unwrap();
    audit::record(&mut tx, &rec).await.unwrap();
    let history = audit::history(&mut tx, "ticket", id).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(history.len(), 2);
    assert!(
        history
            .iter()
            .all(|e| e.change_set_id == history[0].change_set_id)
    );
    assert!(
        history
            .iter()
            .all(|e| e.actor == Actor::Analyst(analyst) && e.occurred_at == at)
    );
    let priority = history
        .iter()
        .find(|e| e.field.as_deref() == Some("priority"))
        .unwrap();
    assert_eq!(priority.old_value, Some(serde_json::json!("Normal")));
    assert_eq!(priority.new_value, Some(serde_json::json!("High")));
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_without_field_changes_writes_a_single_row(pool: PgPool) {
    let id = Uuid::now_v7();
    let rec = AuditRecord::new(
        None,
        "ticket",
        id,
        AuditAction::Deleted,
        Actor::Automation,
        Utc::now(),
    );
    let mut tx = tenancy::begin(&pool, &TenantScope::All).await.unwrap();
    audit::record(&mut tx, &rec).await.unwrap();
    let history = audit::history(&mut tx, "ticket", id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].action, AuditAction::Deleted);
    assert_eq!(history[0].field, None);
}

#[sqlx::test(migrations = "../../migrations")]
async fn update_without_changes_writes_nothing(pool: PgPool) {
    let id = Uuid::now_v7();
    let rec = AuditRecord::new(
        None,
        "ticket",
        id,
        AuditAction::Updated,
        Actor::Automation,
        Utc::now(),
    )
    .change("subject", Some(&"same"), Some(&"same"));
    let mut tx = tenancy::begin(&pool, &TenantScope::All).await.unwrap();
    audit::record(&mut tx, &rec).await.unwrap();
    assert!(
        audit::history(&mut tx, "ticket", id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn audit_entries_cannot_be_updated_deleted_or_truncated(pool: PgPool) {
    let rec = AuditRecord::new(
        None,
        "ticket",
        Uuid::now_v7(),
        AuditAction::Created,
        Actor::Automation,
        Utc::now(),
    );
    let mut tx = tenancy::begin(&pool, &TenantScope::All).await.unwrap();
    audit::record(&mut tx, &rec).await.unwrap();
    tx.commit().await.unwrap();

    // Even the table owner is refused: the trigger does not depend on grants.
    for statement in [
        "UPDATE audit_entries SET field = 'x'",
        "DELETE FROM audit_entries",
        "TRUNCATE audit_entries",
    ] {
        let err = sqlx::query(statement).execute(&pool).await.unwrap_err();
        assert!(
            err.to_string().contains("append-only"),
            "{statement}: {err}"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn audit_rolls_back_with_the_change(pool: PgPool) {
    let id = Uuid::now_v7();
    let rec = AuditRecord::new(
        None,
        "ticket",
        id,
        AuditAction::Created,
        Actor::Automation,
        Utc::now(),
    );
    let mut tx = tenancy::begin(&pool, &TenantScope::All).await.unwrap();
    audit::record(&mut tx, &rec).await.unwrap();
    tx.rollback().await.unwrap();

    let mut tx = tenancy::begin(&pool, &TenantScope::All).await.unwrap();
    assert!(
        audit::history(&mut tx, "ticket", id)
            .await
            .unwrap()
            .is_empty()
    );
}
