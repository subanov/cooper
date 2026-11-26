//! FR-GEN-002 for the Asset extras: Row-Level Security hides other Companies' software,
//! licences and their links, even without a tenant filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::TenantScope;
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &["software_applications", "licences", "licence_applications"];

async fn seed(pool: &PgPool, n: u32) -> Uuid {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("E{n}"))
        .execute(pool)
        .await
        .unwrap();
    let app = Uuid::now_v7();
    sqlx::query("INSERT INTO software_applications (id, tenant_id, name, publisher, created_at, updated_at) VALUES ($1, $2, 'Git', '', now(), now())")
        .bind(app)
        .bind(tenant)
        .execute(pool)
        .await
        .unwrap();
    let licence = Uuid::now_v7();
    sqlx::query("INSERT INTO licences (id, tenant_id, licence_type, quantity, status, created_by_kind, created_at, updated_at) VALUES ($1, $2, 'Git', 1, 'active', 'automation', now(), now())")
        .bind(licence)
        .bind(tenant)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO licence_applications (licence_id, tenant_id, application_id) VALUES ($1, $2, $3)")
        .bind(licence)
        .bind(tenant)
        .bind(app)
        .execute(pool)
        .await
        .unwrap();
    tenant
}

async fn count(pool: &PgPool, scope: &TenantScope, table: &str) -> i64 {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&mut *tx)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn extras_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let a = seed(&pool, 1).await;
    let b = seed(&pool, 2).await;
    for table in TABLES {
        assert_eq!(count(&pool, &TenantScope::single(cooper_kernel::TenantId::from_uuid(a)), table).await, 1, "{table} in A");
        assert_eq!(count(&pool, &TenantScope::single(cooper_kernel::TenantId::from_uuid(b)), table).await, 1, "{table} in B");
        assert_eq!(count(&pool, &TenantScope::All, table).await, 2, "{table} in all");
        assert_eq!(count(&pool, &TenantScope::none(), table).await, 0, "{table} with no scope");
    }
}
