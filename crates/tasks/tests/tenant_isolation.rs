//! FR-GEN-002 for Tasks: Row-Level Security hides other Companies' Tasks and everything
//! hanging off them, even when a query has no tenant filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &[
    "task_recurrences",
    "tasks",
    "task_collaborators",
    "task_comments",
    "task_time_entries",
    "task_links",
];

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
    let (series, task) = (Uuid::now_v7(), Uuid::now_v7());
    exec(
        pool,
        "INSERT INTO task_recurrences (id, tenant_id, rule, created_at, updated_at) VALUES ($1, $2, '{}', now(), now())",
        &[series, tenant],
    )
    .await;
    exec(
        pool,
        "INSERT INTO tasks (id, tenant_id, title, status_id, priority_id, recurrence_id, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, 't', (SELECT id FROM task_statuses WHERE is_default), (SELECT id FROM task_priorities WHERE is_default), $3,
                 'automation', now(), now())",
        &[task, tenant, series],
    )
    .await;
    exec(
        pool,
        "INSERT INTO task_collaborators (tenant_id, task_id, analyst_id) VALUES ($1, $2, $3)",
        &[tenant, task, analyst],
    )
    .await;
    exec(pool, "INSERT INTO task_comments (id, tenant_id, task_id, body, created_at) VALUES ($1, $2, $3, 'c', now())", &[Uuid::now_v7(), tenant, task]).await;
    exec(
        pool,
        "INSERT INTO task_time_entries (id, tenant_id, task_id, analyst_id, work_date, minutes, created_at) VALUES ($1, $2, $3, $4, current_date, 5, now())",
        &[Uuid::now_v7(), tenant, task, analyst],
    )
    .await;
    exec(
        pool,
        "INSERT INTO task_links (tenant_id, task_id, kind, target_id, linked_by_kind, linked_at) VALUES ($1, $2, 'change', $3, 'automation', now())",
        &[tenant, task, Uuid::now_v7()],
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
async fn task_tables_are_isolated_by_row_level_security(pool: PgPool) {
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
