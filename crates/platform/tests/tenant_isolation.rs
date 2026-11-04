//! FR-GEN-002: one Company never sees another Company's rows, even when a query forgets
//! to filter by tenant. Exercised on the platform's own tenant-scoped tables.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: failures should abort the test

use chrono::Utc;
use cooper_kernel::{Actor, AuditAction, AuditRecord, EventEnvelope, TenantId, TenantScope};
use cooper_platform::{audit, outbox, tenancy, testing};
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &["audit_entries", "outbox_events"];

async fn seed_audit(pool: &PgPool, tenant: Option<TenantId>) -> Uuid {
    let id = Uuid::now_v7();
    let rec = AuditRecord::new(
        tenant,
        "widget",
        id,
        AuditAction::Created,
        Actor::Automation,
        Utc::now(),
    );
    let mut tx = tenancy::begin(pool, &TenantScope::All).await.unwrap();
    audit::record(&mut tx, &rec).await.unwrap();
    tx.commit().await.unwrap();
    id
}

async fn visible_audit_rows(pool: &PgPool, scope: &TenantScope) -> i64 {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    // Deliberately no tenant filter: RLS alone must hide other Companies' rows.
    sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM audit_entries"#)
        .fetch_one(&mut *tx)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn unfiltered_query_sees_only_rows_of_scoped_companies(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, b) = (TenantId::new(), TenantId::new());
    seed_audit(&pool, Some(a)).await;
    seed_audit(&pool, Some(b)).await;
    seed_audit(&pool, Some(b)).await;

    assert_eq!(visible_audit_rows(&pool, &TenantScope::single(a)).await, 1);
    assert_eq!(visible_audit_rows(&pool, &TenantScope::single(b)).await, 2);
    assert_eq!(
        visible_audit_rows(&pool, &[a, b].into_iter().collect()).await,
        3
    );
    assert_eq!(visible_audit_rows(&pool, &TenantScope::All).await, 3);
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_or_missing_scope_sees_nothing(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    seed_audit(&pool, Some(TenantId::new())).await;
    seed_audit(&pool, None).await;

    assert_eq!(visible_audit_rows(&pool, &TenantScope::none()).await, 0);

    // No scope applied at all: still nothing (fails closed).
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SET LOCAL ROLE {}",
        testing::PROBE_ROLE
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    let n = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM audit_entries"#)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn cannot_write_rows_for_a_company_outside_the_scope(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, b) = (TenantId::new(), TenantId::new());
    let mut tx = testing::begin_as_app(&pool, &TenantScope::single(a))
        .await
        .unwrap();

    let event = EventEnvelope {
        id: cooper_kernel::EventId::new(),
        event_type: "widget.created".to_owned(),
        aggregate_type: "widget".to_owned(),
        aggregate_id: Uuid::now_v7(),
        tenant_id: Some(b),
        actor: Actor::Automation,
        occurred_at: Utc::now(),
        payload: serde_json::json!({}),
    };
    let err = outbox::append(&mut tx, &event).await.unwrap_err();
    let code = err
        .as_database_error()
        .and_then(|e| e.code())
        .map(|c| c.into_owned());
    assert_eq!(
        code.as_deref(),
        Some("42501"),
        "expected an RLS violation, got {err}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn audit_history_respects_scope(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, b) = (TenantId::new(), TenantId::new());
    let id = seed_audit(&pool, Some(b)).await;

    let mut tx = testing::begin_as_app(&pool, &TenantScope::single(a))
        .await
        .unwrap();
    assert!(
        audit::history(&mut tx, "widget", id)
            .await
            .unwrap()
            .is_empty()
    );
    drop(tx);

    let mut tx = testing::begin_as_app(&pool, &TenantScope::single(b))
        .await
        .unwrap();
    assert_eq!(
        audit::history(&mut tx, "widget", id).await.unwrap().len(),
        1
    );
}
