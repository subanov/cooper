//! FR-GEN-002 for the identity context: Row-Level Security hides other Companies'
//! Companies, Sender Domains, Requesters and Cost Centres even when a query has no
//! tenant filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use std::collections::BTreeSet;
use std::sync::Arc;

use cooper_identity::application::{
    CompanyInput, CostCentreInput, FirstRunSetup, IdentityService, RequesterInput, SignIn,
    capabilities,
};
use cooper_identity::domain::{
    CostCentreCode, DomainName, EmailAddress, GrantKind, Name, Password, SessionPolicy, TicketCode,
    Username,
};
use cooper_identity::infrastructure::{
    FixedIdentitySettings, FixedSecurityPolicy, LoggingResetMailer, PgIdentityStore,
};
use cooper_kernel::{Capability, SystemClock, TenantId, TenantScope};
use cooper_platform::crypto::Keyring;
use cooper_platform::testing;
use sqlx::PgPool;

const TABLES: &[&str] = &[
    "tenants",
    "tenant_domains",
    "requesters",
    "cost_centres",
    "manager_grants",
];

fn service(pool: &PgPool) -> IdentityService {
    let ports = PgIdentityStore::ports(
        pool.clone(),
        Arc::new(SystemClock),
        Keyring::ephemeral().unwrap(),
        Arc::new(FixedSecurityPolicy::default()),
        Arc::new(LoggingResetMailer),
        Arc::new(FixedIdentitySettings::default()),
    )
    .unwrap();
    let catalogue: BTreeSet<Capability> = capabilities::ALL
        .iter()
        .map(|c| c.parse().unwrap())
        .collect();
    IdentityService::new(ports, catalogue, SessionPolicy::default())
}

/// Two Companies, each with a Sender Domain, a Cost Centre and a Requester.
async fn seed(pool: &PgPool) -> (TenantId, TenantId) {
    let svc = service(pool);
    svc.complete_setup(FirstRunSetup {
        company_name: Name::try_from("Alpha").unwrap(),
        ticket_code: TicketCode::try_from("ALP").unwrap(),
        sender_domains: BTreeSet::from([DomainName::try_from("alpha.example").unwrap()]),
        username: Username::try_from("admin").unwrap(),
        email: EmailAddress::try_from("admin@alpha.example").unwrap(),
        full_name: Name::try_from("Admin").unwrap(),
        password: Password::new("correct horse battery".into()).unwrap(),
    })
    .await
    .unwrap();
    let admin = svc
        .sign_in(SignIn {
            login: "admin".into(),
            password: "correct horse battery".into(),
            ip_address: Some("127.0.0.1".parse().unwrap()),
            ..SignIn::default()
        })
        .await
        .unwrap()
        .principal;
    let alpha = svc
        .list_companies(&admin, &Default::default())
        .await
        .unwrap()
        .items[0]
        .id();
    let beta = svc
        .create_company(
            &admin,
            CompanyInput {
                name: Name::try_from("Beta").unwrap(),
                ticket_code: TicketCode::try_from("BET").unwrap(),
                sender_domains: BTreeSet::from([DomainName::try_from("beta.example").unwrap()]),
            },
        )
        .await
        .unwrap()
        .id();
    for (tenant, name) in [(alpha, "alpha"), (beta, "beta")] {
        let cc = svc
            .create_cost_centre(
                &admin,
                tenant,
                CostCentreInput {
                    code: CostCentreCode::try_from(format!("CC-{name}")).unwrap(),
                    name: Name::try_from(format!("{name} finance")).unwrap(),
                    is_active: true,
                },
            )
            .await
            .unwrap();
        let person = svc
            .create_requester(
                &admin,
                tenant,
                RequesterInput {
                    full_name: Name::try_from(format!("{name} person")).unwrap(),
                    email: EmailAddress::try_from(format!("person@{name}.example")).unwrap(),
                    username: None,
                    phone: None,
                    manager_id: None,
                    cost_centre_id: Some(cc.id()),
                    department_id: None,
                },
            )
            .await
            .unwrap();
        svc.add_manager_grant(&admin, person.id(), GrantKind::Reports, false)
            .await
            .unwrap();
    }
    (alpha, beta)
}

async fn count(pool: &PgPool, scope: &TenantScope, table: &str) -> i64 {
    let mut tx = testing::begin_as_app(pool, scope).await.unwrap();
    // Deliberately unfiltered: RLS alone must hide other Companies' rows.
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(&mut *tx)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn identity_tables_are_isolated_by_row_level_security(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (alpha, beta) = seed(&pool).await;
    for table in TABLES {
        assert_eq!(
            count(&pool, &TenantScope::single(alpha), table).await,
            1,
            "{table} in Alpha"
        );
        assert_eq!(
            count(&pool, &TenantScope::single(beta), table).await,
            1,
            "{table} in Beta"
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
async fn rows_cannot_be_written_into_another_company(pool: PgPool) {
    testing::grant_probe_role(&pool, TABLES).await.unwrap();
    let (alpha, beta) = seed(&pool).await;
    let mut tx = testing::begin_as_app(&pool, &TenantScope::single(alpha))
        .await
        .unwrap();
    let result = sqlx::query(
        "INSERT INTO requesters (id, tenant_id, full_name, email, created_at, updated_at)
         VALUES (gen_random_uuid(), $1, 'Mallory', 'm@beta.example', now(), now())",
    )
    .bind(beta.as_uuid())
    .execute(&mut *tx)
    .await;
    let err = result.unwrap_err();
    assert!(err.to_string().contains("row-level security"), "{err}");
}
