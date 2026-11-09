//! FR-GEN-002 for Knowledge: Row-Level Security hides other Companies' Folders,
//! Articles, versions, access lists, access log and ticket links, while shared rows (no
//! Company) stay visible to every Company.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &[
    "knowledge_folders",
    "knowledge_articles",
    "knowledge_article_versions",
    "knowledge_acl",
    "knowledge_access_log",
    "knowledge_article_tickets",
];

async fn exec(pool: &PgPool, sql: &'static str, binds: &[Option<Uuid>]) {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    q.execute(pool).await.unwrap();
}

/// One of everything in `tenant` (none: shared), plus a ticket link when `ticket` is set.
async fn seed(pool: &PgPool, tenant: Option<Uuid>, ticket: Option<Uuid>) {
    let (folder, article) = (Uuid::now_v7(), Uuid::now_v7());
    exec(
        pool,
        "INSERT INTO knowledge_folders (id, tenant_id, name, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, $1::text, 'automation', now(), now())",
        &[Some(folder), tenant],
    )
    .await;
    exec(
        pool,
        "INSERT INTO knowledge_articles (id, tenant_id, folder_id, title, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, $3, 'a', 'automation', now(), now())",
        &[Some(article), tenant, Some(folder)],
    )
    .await;
    exec(
        pool,
        "INSERT INTO knowledge_article_versions (article_id, tenant_id, version_no, title, body, saved_by_kind, saved_at)
         VALUES ($1, $2, 1, 'a', '', 'automation', now())",
        &[Some(article), tenant],
    )
    .await;
    exec(
        pool,
        "INSERT INTO knowledge_acl (tenant_id, object_type, object_id, principal_type, principal_id, created_by_kind, created_at)
         VALUES ($1, 'folder', $2, 'analyst', $3, 'automation', now())",
        &[tenant, Some(folder), Some(Uuid::now_v7())],
    )
    .await;
    exec(
        pool,
        "INSERT INTO knowledge_access_log (id, tenant_id, article_id, action, actor_kind, occurred_at)
         VALUES ($1, $2, $3, 'viewed', 'automation', now())",
        &[Some(Uuid::now_v7()), tenant, Some(article)],
    )
    .await;
    if let (Some(t), Some(ticket)) = (tenant, ticket) {
        exec(
            pool,
            "INSERT INTO knowledge_article_tickets (tenant_id, article_id, ticket_id, linked_by_kind, linked_at)
             VALUES ($1, $2, $3, 'automation', now())",
            &[Some(t), Some(article), Some(ticket)],
        )
        .await;
    }
}

async fn company(pool: &PgPool, n: u32) -> (TenantId, Uuid) {
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
    (TenantId::from_uuid(tenant), ticket)
}

async fn count(pool: &PgPool, scope: &TenantScope, table: &str) -> i64 {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&mut *tx)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn knowledge_tables_are_isolated_and_shared_rows_are_visible_to_all(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, ticket_a) = company(&pool, 1).await;
    let (b, ticket_b) = company(&pool, 2).await;
    seed(&pool, Some(a.as_uuid()), Some(ticket_a)).await;
    seed(&pool, Some(b.as_uuid()), Some(ticket_b)).await;
    seed(&pool, None, None).await;
    for table in TABLES {
        let linked = *table == "knowledge_article_tickets";
        let (own, all) = if linked { (1, 2) } else { (2, 3) };
        assert_eq!(
            count(&pool, &TenantScope::single(a), table).await,
            own,
            "{table} in A (own + shared)"
        );
        assert_eq!(
            count(&pool, &TenantScope::single(b), table).await,
            own,
            "{table} in B (own + shared)"
        );
        assert_eq!(
            count(&pool, &TenantScope::All, table).await,
            all,
            "{table} in all"
        );
        assert_eq!(
            count(&pool, &TenantScope::none(), table).await,
            own - 1,
            "{table} with no scope: shared only"
        );
    }
}
