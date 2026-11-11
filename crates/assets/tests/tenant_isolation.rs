//! FR-GEN-002 for Assets: Row-Level Security hides other Companies' Assets and
//! everything hanging off them, even when a query has no tenant filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &["assets", "asset_checkouts", "asset_links"];

async fn exec(pool: &PgPool, sql: &'static str, binds: &[Uuid]) {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    q.execute(pool).await.unwrap();
}

async fn seed(pool: &PgPool, n: u32) -> TenantId {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    let asset = Uuid::now_v7();
    exec(
        pool,
        "INSERT INTO assets (id, tenant_id, asset_tag, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, 'A-1', 'automation', now(), now())",
        &[asset, tenant],
    )
    .await;
    exec(
        pool,
        "INSERT INTO asset_checkouts (id, tenant_id, asset_id, holder_kind, holder_id, checked_out_at, checked_out_by_kind)
         VALUES ($1, $2, $3, 'analyst', $4, now(), 'automation')",
        &[Uuid::now_v7(), tenant, asset, Uuid::now_v7()],
    )
    .await;
    exec(
        pool,
        "INSERT INTO asset_links (tenant_id, asset_id, kind, target_id, linked_by_kind, linked_at)
         VALUES ($1, $2, 'contract', $3, 'automation', now())",
        &[tenant, asset, Uuid::now_v7()],
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
async fn asset_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, b) = (seed(&pool, 1).await, seed(&pool, 2).await);
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
