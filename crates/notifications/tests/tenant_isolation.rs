//! FR-GEN-002 for Portal Notifications and Notification Preferences (FR-PRT-007):
//! Row-Level Security hides other Companies' rows even when a query has no tenant
//! filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &["portal_notifications", "portal_notification_preferences"];

async fn company(pool: &PgPool, n: u32) -> TenantId {
    let (tenant, requester) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO requesters (id, tenant_id, full_name, email, created_at, updated_at) VALUES ($1, $2, 'Sam', 'sam@acme.example', now(), now())")
        .bind(requester)
        .bind(tenant)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO portal_notifications (id, tenant_id, requester_id, kind, title, source_event_id, created_at)
         VALUES ($1, $2, $3, 'ticket_created', 'Ticket logged', $4, now())",
    )
    .bind(Uuid::now_v7())
    .bind(tenant)
    .bind(requester)
    .bind(Uuid::now_v7())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO portal_notification_preferences (tenant_id, requester_id, kind, in_app, email, updated_at)
         VALUES ($1, $2, 'ticket_closed', false, true, now())",
    )
    .bind(tenant)
    .bind(requester)
    .execute(pool)
    .await
    .unwrap();
    TenantId::from_uuid(tenant)
}

async fn visible(pool: &PgPool, scope: &TenantScope, table: &str) -> Vec<Uuid> {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    let mut rows: Vec<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT tenant_id FROM {table}"
    )))
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    rows.sort();
    rows
}

#[sqlx::test(migrations = "../../migrations")]
async fn portal_notification_tables_are_isolated_by_company(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, b) = (company(&pool, 1).await, company(&pool, 2).await);
    for table in TABLES {
        assert_eq!(
            visible(&pool, &TenantScope::single(a), table).await,
            [a.as_uuid()],
            "{table} in A"
        );
        assert_eq!(
            visible(&pool, &TenantScope::single(b), table).await,
            [b.as_uuid()],
            "{table} in B"
        );
        assert_eq!(visible(&pool, &TenantScope::All, table).await.len(), 2);
        assert!(visible(&pool, &TenantScope::none(), table).await.is_empty());
    }
    // Writing into another Company is refused too.
    let mut tx = testing::begin_as_app(&pool, &TenantScope::single(a))
        .await
        .unwrap();
    let foreign =
        sqlx::query("UPDATE portal_notifications SET read_at = now() WHERE tenant_id = $1")
            .bind(b.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
    assert_eq!(foreign.rows_affected(), 0);
}
