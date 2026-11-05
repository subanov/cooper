//! Ticket Number sequences under concurrency (FR-TKT-002) and Row-Level Security on the
//! service desk tables (FR-GEN-002).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use std::collections::BTreeSet;

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use cooper_service_desk::domain::TicketRepository;
use cooper_service_desk::infrastructure::PgServiceDeskStore;
use sqlx::PgPool;

async fn company(pool: &PgPool, code: &str) -> TenantId {
    let id = TenantId::new();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $2, now(), now())")
        .bind(id.as_uuid())
        .bind(code)
        .execute(pool)
        .await
        .unwrap();
    id
}

#[sqlx::test(migrations = "../../migrations")]
async fn concurrent_creates_never_share_a_number(pool: PgPool) {
    let tenant = company(&pool, "ACME").await;
    let store = PgServiceDeskStore::new(pool.clone());
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..40 {
        let store = store.clone();
        tasks.spawn(async move { store.next_sequence(tenant).await.unwrap() });
    }
    let mut values = BTreeSet::new();
    while let Some(v) = tasks.join_next().await {
        assert!(values.insert(v.unwrap()), "duplicate sequence value");
    }
    assert_eq!(values, (1..=40).collect());
    let other = company(&pool, "OTHER").await;
    assert_eq!(
        store.next_sequence(other).await.unwrap(),
        1,
        "sequences are per Company"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn service_desk_tables_are_isolated(pool: PgPool) {
    const TABLES: &[&str] = &[
        "tickets",
        "ticket_notes",
        "ticket_time_entries",
        "ticket_number_counters",
    ];
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, b) = (company(&pool, "AAA").await, company(&pool, "BBB").await);
    let analyst = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO analysts (id, username, email, full_name, created_at, updated_at) VALUES ($1, 'x', 'x@x.example', 'X', now(), now())")
        .bind(analyst).execute(&pool).await.unwrap();
    for (tenant, n) in [(a, "AAA-1"), (b, "BBB-1")] {
        let ticket = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO tickets (id, tenant_id, ticket_number, subject, priority_id, status_id, created_by_kind, created_at, updated_at)
             VALUES ($1, $2, $3, 's', '019a0000-0000-7000-8000-000000000202', '019a0000-0000-7000-8000-000000000101', 'automation', now(), now())",
        )
        .bind(ticket).bind(tenant.as_uuid()).bind(n).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ticket_notes (id, tenant_id, ticket_id, body, created_at) VALUES ($1, $2, $3, 'n', now())")
            .bind(uuid::Uuid::now_v7()).bind(tenant.as_uuid()).bind(ticket).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ticket_time_entries (id, tenant_id, ticket_id, analyst_id, minutes, work_date, created_at) VALUES ($1, $2, $3, $4, 5, current_date, now())")
            .bind(uuid::Uuid::now_v7()).bind(tenant.as_uuid()).bind(ticket).bind(analyst).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ticket_number_counters (tenant_id, next_value) VALUES ($1, 2)")
            .bind(tenant.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
    }
    for table in TABLES {
        let mut tx = testing::begin_as_app(&pool, &TenantScope::single(a))
            .await
            .unwrap();
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(n, 1, "{table}");
        drop(tx);
        let mut tx = testing::begin_as_app(&pool, &TenantScope::none())
            .await
            .unwrap();
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(n, 0, "{table} without scope");
    }
}
