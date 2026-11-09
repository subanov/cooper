//! FR-GEN-002 for Service Status: Row-Level Security hides other Companies' Services,
//! Incidents, updates and maintenance, even when a query has no tenant filter. Shared
//! records (no Company) are visible in every scope, the empty one included, which is
//! what the Public Status Page reads with (ADR-0030).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &[
    "status_services",
    "planned_maintenances",
    "planned_maintenance_services",
    "status_incidents",
    "status_incident_services",
    "status_incident_updates",
    "status_incident_update_services",
];

const OPERATIONAL: &str = "019a0000-0000-7000-8000-000000000c05";
const INVESTIGATING: &str = "019a0000-0000-7000-8000-000000000c11";

async fn exec(pool: &PgPool, sql: &'static str, tenant: Option<Uuid>, ids: &[Uuid]) {
    let mut q = sqlx::query(sql).bind(tenant);
    for b in ids {
        q = q.bind(*b);
    }
    q.execute(pool).await.unwrap();
}

/// One row in every table, for `tenant` (`None`: shared).
async fn seed(pool: &PgPool, tenant: Option<Uuid>) {
    let (service, maintenance, incident, update) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    let (level, status): (Uuid, Uuid) =
        (OPERATIONAL.parse().unwrap(), INVESTIGATING.parse().unwrap());
    exec(
        pool,
        "INSERT INTO status_services (tenant_id, id, name, created_at, updated_at) VALUES ($1, $2, $3::text, now(), now())",
        tenant,
        &[service, service],
    )
    .await;
    exec(
        pool,
        "INSERT INTO planned_maintenances (tenant_id, id, title, planned_start, planned_end, state, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, 'm', now(), now() + interval '1 hour', 'scheduled', 'automation', now(), now())",
        tenant,
        &[maintenance],
    )
    .await;
    exec(
        pool,
        "INSERT INTO planned_maintenance_services (tenant_id, planned_maintenance_id, service_id, impact_level_id) VALUES ($1, $2, $3, $4)",
        tenant,
        &[maintenance, service, level],
    )
    .await;
    exec(
        pool,
        "INSERT INTO status_incidents (tenant_id, id, title, status_id, started_at, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, 'i', $3, now(), 'automation', now(), now())",
        tenant,
        &[incident, status],
    )
    .await;
    exec(
        pool,
        "INSERT INTO status_incident_services (tenant_id, incident_id, service_id, impact_level_id) VALUES ($1, $2, $3, $4)",
        tenant,
        &[incident, service, level],
    )
    .await;
    exec(
        pool,
        "INSERT INTO status_incident_updates (tenant_id, id, incident_id, status_id, author_kind, posted_at)
         VALUES ($1, $2, $3, $4, 'automation', now())",
        tenant,
        &[update, incident, status],
    )
    .await;
    exec(
        pool,
        "INSERT INTO status_incident_update_services (tenant_id, update_id, service_id, impact_level_id) VALUES ($1, $2, $3, $4)",
        tenant,
        &[update, service, level],
    )
    .await;
}

async fn company(pool: &PgPool, n: u32) -> TenantId {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    seed(pool, Some(tenant)).await;
    TenantId::from_uuid(tenant)
}

async fn visible(pool: &PgPool, scope: &TenantScope, table: &str) -> Vec<Option<Uuid>> {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    let mut rows: Vec<Option<Uuid>> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT tenant_id FROM {table}"
    )))
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    rows.sort();
    rows
}

#[sqlx::test(migrations = "../../migrations")]
async fn service_status_tables_are_isolated_and_shared_rows_are_seen_by_every_scope(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (a, b) = (company(&pool, 1).await, company(&pool, 2).await);
    seed(&pool, None).await;
    for table in TABLES {
        let mut in_a = vec![None, Some(a.as_uuid())];
        in_a.sort();
        assert_eq!(
            visible(&pool, &TenantScope::single(a), table).await,
            in_a,
            "{table} in A: its own and shared"
        );
        assert!(
            !visible(&pool, &TenantScope::single(b), table)
                .await
                .contains(&Some(a.as_uuid())),
            "{table}: B never sees A"
        );
        assert_eq!(
            visible(&pool, &TenantScope::All, table).await.len(),
            3,
            "{table} in all"
        );
        assert_eq!(
            visible(&pool, &TenantScope::none(), table).await,
            vec![None],
            "{table} with no scope: shared rows only"
        );
    }
}
