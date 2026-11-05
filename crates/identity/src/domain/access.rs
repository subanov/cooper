//! Company access granted to Analysts and Teams (FR-IAM-009).

use std::collections::BTreeSet;

use cooper_kernel::{TenantId, TenantScope};
use serde::{Deserialize, Serialize};

/// Which Companies an Analyst or Team may see.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "company_ids", rename_all = "snake_case")]
pub enum CompanyAccess {
    /// Every Company, including ones created later.
    All,
    /// Only these Companies (possibly none).
    Companies(BTreeSet<TenantId>),
    /// No Company access.
    #[default]
    None,
}

impl CompanyAccess {
    /// Normalises an explicit list: an empty set means [`CompanyAccess::None`].
    #[must_use]
    pub fn companies(ids: BTreeSet<TenantId>) -> Self {
        if ids.is_empty() {
            Self::None
        } else {
            Self::Companies(ids)
        }
    }

    /// The union of several grants (an Analyst's own plus their Teams').
    #[must_use]
    pub fn union<'a>(grants: impl IntoIterator<Item = &'a Self>) -> Self {
        let mut ids = BTreeSet::new();
        for grant in grants {
            match grant {
                Self::All => return Self::All,
                Self::Companies(set) => ids.extend(set.iter().copied()),
                Self::None => {}
            }
        }
        Self::companies(ids)
    }

    /// The tenant scope this access gives.
    #[must_use]
    pub fn scope(&self) -> TenantScope {
        match self {
            Self::All => TenantScope::All,
            Self::Companies(ids) => TenantScope::Tenants(ids.clone()),
            Self::None => TenantScope::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_of_grants() {
        let (a, b) = (TenantId::new(), TenantId::new());
        let one = CompanyAccess::companies(BTreeSet::from([a]));
        let two = CompanyAccess::companies(BTreeSet::from([b]));
        assert_eq!(
            CompanyAccess::union([&one, &two, &CompanyAccess::None]),
            CompanyAccess::Companies(BTreeSet::from([a, b]))
        );
        assert_eq!(
            CompanyAccess::union([&one, &CompanyAccess::All]),
            CompanyAccess::All
        );
        assert_eq!(CompanyAccess::union([]), CompanyAccess::None);
        assert_eq!(CompanyAccess::None.scope(), TenantScope::none());
    }
}
