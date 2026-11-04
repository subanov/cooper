//! Tenant isolation test harness (feature `testing`).
//!
//! Tests normally connect as the database owner, which bypasses Row-Level Security
//! (owners and superusers do). To prove isolation the way production sees it, run the
//! code under test with [`begin_as_app`]: it switches the transaction to a
//! non-privileged role and applies a tenant scope.
//!
//! ```ignore
//! #[sqlx::test(migrations = "../../migrations")]
//! async fn tickets_are_isolated(pool: PgPool) {
//!     testing::grant_probe_role(&pool, &["tickets"]).await.unwrap();
//!     let mut tx = testing::begin_as_app(&pool, &TenantScope::single(company_a)).await.unwrap();
//!     // ... rows of company_b must be invisible here ...
//! }
//! ```

use cooper_kernel::TenantScope;
use sqlx::{AssertSqlSafe, PgPool};

use crate::db::Tx;
use crate::tenancy;

/// Name of the non-privileged role tests switch to. Cluster-wide, created on demand.
pub const PROBE_ROLE: &str = "cooper_rls_probe";

/// Creates [`PROBE_ROLE`] if needed and grants it read/write access to `tables` and use
/// of the shared tenant functions.
///
/// # Errors
///
/// Fails on database errors.
pub async fn grant_probe_role(pool: &PgPool, tables: &[&str]) -> Result<(), sqlx::Error> {
    // Concurrent tests may race to create the role; both outcomes are fine.
    let create = format!(
        "DO $$ BEGIN CREATE ROLE {PROBE_ROLE} NOLOGIN; \
         EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; END $$"
    );
    sqlx::query(AssertSqlSafe(create)).execute(pool).await?;
    sqlx::query(AssertSqlSafe(format!(
        "GRANT USAGE ON SCHEMA public TO {PROBE_ROLE}"
    )))
    .execute(pool)
    .await?;
    for table in tables {
        let quoted = quote_ident(table);
        sqlx::query(AssertSqlSafe(format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON {quoted} TO {PROBE_ROLE}"
        )))
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Opens a transaction as [`PROBE_ROLE`] with `scope` applied, so Row-Level Security is
/// enforced exactly as for the application role.
///
/// # Errors
///
/// Fails on database errors (for example if [`grant_probe_role`] was not called).
pub async fn begin_as_app(pool: &PgPool, scope: &TenantScope) -> Result<Tx, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(AssertSqlSafe(format!("SET LOCAL ROLE {PROBE_ROLE}")))
        .execute(&mut *tx)
        .await?;
    tenancy::apply_scope(&mut tx, scope).await?;
    Ok(tx)
}

fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}
