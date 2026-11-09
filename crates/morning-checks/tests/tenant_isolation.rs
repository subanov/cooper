//! FR-GEN-002 for Morning Checks: Row-Level Security hides other Companies' Check
//! Groups, Checks, Check Results, Result Links and reminders, even when a query has no
//! tenant filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &[
    "check_groups",
    "checks",
    "check_results",
    "check_result_links",
    "check_reminders",
];

const GREEN: &str = "019a0000-0000-7000-8000-000000000c01";

async fn exec(pool: &PgPool, sql: &'static str, binds: &[Uuid]) {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    q.execute(pool).await.unwrap();
}

async fn seed(pool: &PgPool, n: u32, analyst: Uuid) -> TenantId {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    let (group, check, result) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    exec(
        pool,
        "INSERT INTO check_groups (id, tenant_id, name, assigned_analyst_id, created_at, updated_at)
         VALUES ($1, $2, 'Infrastructure', $3, now(), now())",
        &[group, tenant, analyst],
    )
    .await;
    exec(
        pool,
        "INSERT INTO checks (id, tenant_id, check_group_id, name, created_at, updated_at)
         VALUES ($1, $2, $3, 'Backups completed', now(), now())",
        &[check, tenant, group],
    )
    .await;
    exec(
        pool,
        "INSERT INTO check_results (id, tenant_id, check_id, check_date, status_id, recorded_by_kind, recorded_at,
                                    modified_by_kind, updated_at)
         VALUES ($1, $2, $3, current_date, $4, 'automation', now(), 'automation', now())",
        &[result, tenant, check, GREEN.parse().unwrap()],
    )
    .await;
    exec(
        pool,
        "INSERT INTO check_result_links (tenant_id, check_result_id, kind, target_id, linked_by_kind, linked_at)
         VALUES ($1, $2, 'task', $3, 'automation', now())",
        &[tenant, result, Uuid::now_v7()],
    )
    .await;
    exec(
        pool,
        "INSERT INTO check_reminders (id, tenant_id, check_date, check_group_id, analyst_id, unrecorded, sent_at)
         VALUES ($1, $2, current_date, $3, $4, 1, now())",
        &[Uuid::now_v7(), tenant, group, analyst],
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
async fn morning_check_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let analyst = Uuid::now_v7();
    exec(&pool, "INSERT INTO analysts (id, username, email, full_name, created_at, updated_at) VALUES ($1, 'x', 'x@x.example', 'X', now(), now())", &[analyst]).await;
    let (a, b) = (seed(&pool, 1, analyst).await, seed(&pool, 2, analyst).await);
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

#[sqlx::test(migrations = "../../migrations")]
async fn a_check_cannot_join_a_group_of_another_company(pool: PgPool) {
    let analyst = Uuid::now_v7();
    exec(&pool, "INSERT INTO analysts (id, username, email, full_name, created_at, updated_at) VALUES ($1, 'x', 'x@x.example', 'X', now(), now())", &[analyst]).await;
    let (a, b) = (seed(&pool, 1, analyst).await, seed(&pool, 2, analyst).await);
    let group: Uuid = sqlx::query_scalar("SELECT id FROM check_groups WHERE tenant_id = $1")
        .bind(a.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    let r = sqlx::query(
        "INSERT INTO checks (id, tenant_id, check_group_id, name, created_at, updated_at) VALUES ($1, $2, $3, 'x', now(), now())",
    )
    .bind(Uuid::now_v7())
    .bind(b.as_uuid())
    .bind(group)
    .execute(&pool)
    .await;
    assert!(r.is_err(), "the composite key keeps a Check in its Company");
}
