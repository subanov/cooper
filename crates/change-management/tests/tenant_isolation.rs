//! FR-GEN-002 for Change Management: Row-Level Security hides other Companies' Changes
//! and everything hanging off them, even when a query has no tenant filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &[
    "changes",
    "change_cab_members",
    "change_approval_votes",
    "change_checklist_items",
    "change_comments",
    "change_links",
    "change_text_revisions",
];

async fn exec(pool: &PgPool, sql: &'static str, binds: &[Uuid]) {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    q.execute(pool).await.unwrap();
}

async fn seed_company(pool: &PgPool, n: u32, analyst: Uuid) -> TenantId {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    let change = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO changes (id, tenant_id, change_number, title, type_id, status_id, priority_id, impact_id,
                              created_by_kind, created_at, updated_at)
         VALUES ($1, $2, $3, 'c', (SELECT id FROM change_types WHERE is_default), (SELECT id FROM change_statuses WHERE is_default),
                 (SELECT id FROM change_priorities WHERE is_default), (SELECT id FROM change_impacts WHERE is_default),
                 'automation', now(), now())",
    )
    .bind(change)
    .bind(tenant)
    .bind(format!("CHG-{n}"))
    .execute(pool)
    .await
    .unwrap();
    exec(
        pool,
        "INSERT INTO change_cab_members (tenant_id, change_id, analyst_id) VALUES ($1, $2, $3)",
        &[tenant, change, analyst],
    )
    .await;
    exec(
        pool,
        "INSERT INTO change_approval_votes (id, tenant_id, change_id, round_no, analyst_id, decision, voted_at)
         VALUES ($1, $2, $3, 1, $4, 'approve', now())",
        &[Uuid::now_v7(), tenant, change, analyst],
    )
    .await;
    exec(
        pool,
        "INSERT INTO change_checklist_items (id, tenant_id, change_id, text, created_at) VALUES ($1, $2, $3, 'i', now())",
        &[Uuid::now_v7(), tenant, change],
    )
    .await;
    exec(
        pool,
        "INSERT INTO change_comments (id, tenant_id, change_id, body, created_at) VALUES ($1, $2, $3, 'b', now())",
        &[Uuid::now_v7(), tenant, change],
    )
    .await;
    exec(
        pool,
        "INSERT INTO change_links (tenant_id, change_id, kind, target_id, linked_by_kind, linked_at)
         VALUES ($1, $2, 'change', $3, 'automation', now())",
        &[tenant, change, Uuid::now_v7()],
    )
    .await;
    exec(
        pool,
        "INSERT INTO change_text_revisions (id, tenant_id, change_id, field, body, author_kind, created_at)
         VALUES ($1, $2, $3, 'description', 'd', 'automation', now())",
        &[Uuid::now_v7(), tenant, change],
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
async fn change_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let analyst = Uuid::now_v7();
    exec(
        &pool,
        "INSERT INTO analysts (id, username, email, full_name, created_at, updated_at) VALUES ($1, 'x', 'x@x.example', 'X', now(), now())",
        &[analyst],
    )
    .await;
    let (a, b) = (
        seed_company(&pool, 1, analyst).await,
        seed_company(&pool, 2, analyst).await,
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
