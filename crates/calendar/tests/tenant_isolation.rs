//! FR-GEN-002 for the Calendar: Row-Level Security hides other Companies' Calendar
//! Events and generated-event watermarks, even when a query has no tenant filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &["calendar_events", "calendar_sources"];

async fn exec(pool: &PgPool, sql: &'static str, binds: &[Uuid]) {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    q.execute(pool).await.unwrap();
}

async fn seed(pool: &PgPool, n: u32, category: Uuid) -> TenantId {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    exec(
        pool,
        "INSERT INTO calendar_events (id, tenant_id, title, category_id, all_day, start_date, end_date,
                                      created_by_kind, created_at, updated_at)
         VALUES ($1, $2, 'e', $3, true, current_date, current_date, 'automation', now(), now())",
        &[Uuid::now_v7(), tenant, category],
    )
    .await;
    exec(
        pool,
        "INSERT INTO calendar_sources (tenant_id, source_kind, source_id, synced_event_id, synced_at)
         VALUES ($1, 'contract', $2, $3, now())",
        &[tenant, Uuid::now_v7(), Uuid::now_v7()],
    )
    .await;
    TenantId::from_uuid(tenant)
}

async fn count(pool: &PgPool, scope: &TenantScope, table: &str) -> i64 {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&mut *tx)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn calendar_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let category = Uuid::now_v7();
    exec(
        &pool,
        "INSERT INTO calendar_categories (id, name, colour, created_at, updated_at) VALUES ($1, 'Meetings', '#2563eb', now(), now())",
        &[category],
    )
    .await;
    let (a, b) = (
        seed(&pool, 1, category).await,
        seed(&pool, 2, category).await,
    );
    for table in TABLES {
        assert_eq!(
            count(&pool, &TenantScope::single(a), table).await,
            1,
            "{table} in A"
        );
        assert_eq!(
            count(&pool, &TenantScope::single(b), table).await,
            1,
            "{table} in B"
        );
        assert_eq!(
            count(&pool, &TenantScope::All, table).await,
            2,
            "{table} in all"
        );
        assert_eq!(
            count(&pool, &TenantScope::none(), table).await,
            0,
            "{table} with no scope"
        );
    }
}
