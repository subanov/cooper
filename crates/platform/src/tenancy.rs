//! Tenant-scoped transactions (FR-GEN-002, ADR-0004).
//!
//! Every unit of work that touches tenant data runs in a transaction opened with
//! [`begin`]. It sets the transaction-local settings read by the
//! `cooper_tenant_visible(tenant_id)` Row-Level Security predicate, so PostgreSQL hides
//! other Companies' rows even if a repository forgets its `tenant_id` filter.
//! Repositories still filter by tenant explicitly: RLS is the second layer, not the first.

use cooper_kernel::TenantScope;
use sqlx::{PgConnection, PgPool};

use crate::db::Tx;

/// Opens a transaction limited to `scope`.
///
/// # Errors
///
/// Fails if the transaction cannot be started.
pub async fn begin(pool: &PgPool, scope: &TenantScope) -> Result<Tx, sqlx::Error> {
    let mut tx = pool.begin().await?;
    apply_scope(&mut tx, scope).await?;
    Ok(tx)
}

/// Sets the tenant scope for the rest of the current transaction.
///
/// Must be called inside a transaction: outside one the settings would not reset and
/// could leak to the next user of a pooled connection.
///
/// # Errors
///
/// Fails if the settings cannot be written.
pub async fn apply_scope(conn: &mut PgConnection, scope: &TenantScope) -> Result<(), sqlx::Error> {
    let (kind, ids) = scope_settings(scope);
    sqlx::query!(
        "SELECT set_config('cooper.tenant_scope', $1, true) AS scope, \
                set_config('cooper.tenant_ids', $2, true) AS ids",
        kind,
        ids,
    )
    .fetch_one(conn)
    .await?;
    Ok(())
}

fn scope_settings(scope: &TenantScope) -> (&'static str, String) {
    match scope {
        TenantScope::All => ("all", String::new()),
        TenantScope::Tenants(set) => {
            let ids: Vec<String> = set.iter().map(ToString::to_string).collect();
            ("tenants", ids.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cooper_kernel::TenantId;

    #[test]
    fn settings_encode_scope() {
        assert_eq!(scope_settings(&TenantScope::All), ("all", String::new()));
        assert_eq!(
            scope_settings(&TenantScope::none()),
            ("tenants", String::new())
        );
        let (a, b) = (TenantId::new(), TenantId::new());
        let (kind, ids) = scope_settings(&[a, b].into_iter().collect());
        assert_eq!(kind, "tenants");
        assert_eq!(ids, format!("{a},{b}"));
    }
}
