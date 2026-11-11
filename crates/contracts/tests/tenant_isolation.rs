//! FR-GEN-002 for Contracts: Row-Level Security hides other Companies' Suppliers,
//! Contacts, Contracts and reminders, even when a query has no tenant filter; and the
//! store's own tenant filter keeps them apart too.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use cooper_contracts::domain::{ContractId, SupplierId};
use cooper_contracts::infrastructure::PgContractsStore;
use cooper_contracts::ports::{ContractQuery, ContractsRepository, SupplierQuery};
use cooper_kernel::{TenantId, TenantScope};
use cooper_platform::testing;
use sqlx::PgPool;
use uuid::Uuid;

const TABLES: &[&str] = &[
    "suppliers",
    "supplier_contacts",
    "contracts",
    "contract_reminders",
];

async fn exec(pool: &PgPool, sql: &'static str, binds: &[Uuid]) {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    q.execute(pool).await.unwrap();
}

struct Seeded {
    tenant: TenantId,
    supplier: Uuid,
    contract: Uuid,
}

async fn seed(pool: &PgPool, n: u32, analyst: Uuid) -> Seeded {
    let tenant = Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, name, ticket_code, created_at, updated_at) VALUES ($1, $2, $3, now(), now())")
        .bind(tenant)
        .bind(format!("Company {n}"))
        .bind(format!("C{n}"))
        .execute(pool)
        .await
        .unwrap();
    let (supplier, contract) = (Uuid::now_v7(), Uuid::now_v7());
    exec(
        pool,
        "INSERT INTO suppliers (id, tenant_id, legal_name, created_by_kind, created_at, updated_at)
         VALUES ($1, $2, 'Acme Ltd', 'automation', now(), now())",
        &[supplier, tenant],
    )
    .await;
    exec(
        pool,
        "INSERT INTO supplier_contacts (id, tenant_id, supplier_id, full_name, created_at, updated_at)
         VALUES ($1, $2, $3, 'Pat', now(), now())",
        &[Uuid::now_v7(), tenant, supplier],
    )
    .await;
    exec(
        pool,
        "INSERT INTO contracts (id, tenant_id, contract_number, title, party_kind, supplier_id, owner_id, end_date,
                                created_by_kind, created_at, updated_at)
         VALUES ($1, $2, 'CON-1', 'Lease', 'supplier', $3, $4, current_date + 10, 'automation', now(), now())",
        &[contract, tenant, supplier, analyst],
    )
    .await;
    exec(
        pool,
        "INSERT INTO contract_reminders (tenant_id, contract_id, kind, reminded_date, owner_id, sent_at)
         VALUES ($1, $2, 'end_date', current_date + 10, $3, now())",
        &[tenant, contract, analyst],
    )
    .await;
    Seeded {
        tenant: TenantId::from_uuid(tenant),
        supplier,
        contract,
    }
}

async fn count(pool: &PgPool, scope: &TenantScope, table: &str) -> i64 {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&mut *tx)
        .await
        .unwrap()
}

async fn analyst(pool: &PgPool) -> Uuid {
    let analyst = Uuid::now_v7();
    exec(pool, "INSERT INTO analysts (id, username, email, full_name, created_at, updated_at) VALUES ($1, 'x', 'x@x.example', 'X', now(), now())", &[analyst]).await;
    analyst
}

#[sqlx::test(migrations = "../../migrations")]
async fn contract_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let analyst = analyst(&pool).await;
    let (a, b) = (seed(&pool, 1, analyst).await, seed(&pool, 2, analyst).await);
    for table in TABLES {
        assert_eq!(
            count(&pool, &TenantScope::single(a.tenant), table).await,
            1,
            "{table} in A"
        );
        assert_eq!(
            count(&pool, &TenantScope::single(b.tenant), table).await,
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
async fn store_reads_only_the_scoped_companies(pool: PgPool) {
    let analyst = analyst(&pool).await;
    let (a, b) = (seed(&pool, 1, analyst).await, seed(&pool, 2, analyst).await);
    let store = PgContractsStore::new(pool.clone());
    let only_a = TenantScope::single(a.tenant);
    assert!(
        store
            .supplier(&only_a, SupplierId::from_uuid(a.supplier))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .supplier(&only_a, SupplierId::from_uuid(b.supplier))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .contract(&only_a, ContractId::from_uuid(b.contract))
            .await
            .unwrap()
            .is_none()
    );
    let suppliers = store
        .suppliers(&only_a, &SupplierQuery::default())
        .await
        .unwrap();
    assert_eq!(suppliers.len(), 1);
    let contracts = store
        .contracts(&only_a, &ContractQuery::default())
        .await
        .unwrap();
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0].tenant_id(), a.tenant);
    let contacts = store
        .contacts(&only_a, SupplierId::from_uuid(b.supplier))
        .await
        .unwrap();
    assert!(contacts.is_empty());
}
