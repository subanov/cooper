//! FR-GEN-002 for Problem Management: Row-Level Security hides other Companies'
//! Problems, Notes and linked tickets even when a query has no tenant filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &["problems", "problem_notes", "problem_tickets"];

async fn seed_company(pool: &PgPool, n: u32) -> TenantId {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    let ticket = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO tickets (id, tenant_id, ticket_number, subject, priority_id, status_id, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, $3, 'x', (SELECT id FROM ticket_priorities WHERE is_default), (SELECT id FROM ticket_statuses WHERE is_default),
                 'automation', now(), now())",
    )
    .bind(ticket)
    .bind(tenant)
    .bind(format!("C{n}-1"))
    .execute(pool)
    .await
    .unwrap();
    let problem = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO problems (id, tenant_id, problem_number, title, status_id, priority_id, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, $3, 'p', (SELECT id FROM problem_statuses WHERE is_default), (SELECT id FROM problem_priorities WHERE is_default),
                 'automation', now(), now())",
    )
    .bind(problem)
    .bind(tenant)
    .bind(format!("PRB-{n}"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO problem_notes (id, tenant_id, problem_id, body, created_at) VALUES ($1, $2, $3, 'n', now())")
        .bind(Uuid::now_v7())
        .bind(tenant)
        .bind(problem)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO problem_tickets (tenant_id, problem_id, ticket_id, linked_by_kind, linked_at) VALUES ($1, $2, $3, 'automation', now())")
        .bind(tenant)
        .bind(problem)
        .bind(ticket)
        .execute(pool)
        .await
        .unwrap();
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
async fn problem_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, b) = (seed_company(&pool, 1).await, seed_company(&pool, 2).await);
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
