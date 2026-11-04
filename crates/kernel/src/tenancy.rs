//! Which Companies a unit of work may see (FR-GEN-002, ADR-0004).

use std::collections::BTreeSet;

use crate::id::TenantId;

/// The set of Companies visible to the current request or job.
///
/// Repositories filter every query by this scope, and the database enforces it a
/// second time with Row-Level Security. An empty scope sees nothing: isolation fails
/// closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantScope {
    /// Exactly these Companies. May be empty.
    Tenants(BTreeSet<TenantId>),
    /// Every Company. Only for Automation (background jobs, the outbox dispatcher) and
    /// Analysts granted access to all Companies.
    All,
}

impl TenantScope {
    /// A scope with a single Company.
    #[must_use]
    pub fn single(tenant: TenantId) -> Self {
        Self::Tenants(BTreeSet::from([tenant]))
    }

    /// A scope that sees nothing.
    #[must_use]
    pub const fn none() -> Self {
        Self::Tenants(BTreeSet::new())
    }

    /// Whether `tenant` is visible in this scope.
    #[must_use]
    pub fn allows(&self, tenant: TenantId) -> bool {
        match self {
            Self::Tenants(set) => set.contains(&tenant),
            Self::All => true,
        }
    }
}

impl FromIterator<TenantId> for TenantScope {
    fn from_iter<I: IntoIterator<Item = TenantId>>(iter: I) -> Self {
        Self::Tenants(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scope_allows_nothing() {
        assert!(!TenantScope::none().allows(TenantId::new()));
    }

    #[test]
    fn explicit_scope_allows_only_members() {
        let a = TenantId::new();
        let b = TenantId::new();
        let scope = TenantScope::single(a);
        assert!(scope.allows(a));
        assert!(!scope.allows(b));
        assert!(TenantScope::All.allows(b));
        assert!([a, b].into_iter().collect::<TenantScope>().allows(b));
    }
}
