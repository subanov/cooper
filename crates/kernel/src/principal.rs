//! The authenticated caller and what it may do (FR-IAM-010, ADR-0011).

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::actor::Actor;
use crate::tenancy::TenantScope;

/// A permission in the form `<module>.<action>`, for example `tickets.update`.
///
/// Both parts are lower-case ASCII letters, digits, `_` or `-`, and start with a letter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Capability(String);

/// A string is not a valid [`Capability`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid capability {0:?}: expected `<module>.<action>` in lower case")]
pub struct ParseCapabilityError(pub String);

impl Capability {
    /// Builds a capability from a compile-time constant. Returns `None` if `value` is
    /// invalid; use it with constants that are covered by a test.
    #[must_use]
    pub fn from_static(value: &'static str) -> Option<Self> {
        value.parse().ok()
    }

    /// The capability string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The module part, for example `tickets`.
    #[must_use]
    pub fn module(&self) -> &str {
        self.0.split_once('.').map_or("", |(module, _)| module)
    }
}

fn valid_part(part: &str) -> bool {
    let mut chars = part.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl FromStr for Capability {
    type Err = ParseCapabilityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('.') {
            Some((module, action)) if valid_part(module) && valid_part(action) => {
                Ok(Self(s.to_owned()))
            }
            _ => Err(ParseCapabilityError(s.to_owned())),
        }
    }
}

impl TryFrom<String> for Capability {
    type Error = ParseCapabilityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Capability> for String {
    fn from(value: Capability) -> Self {
        value.0
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The caller lacks a Capability.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("missing capability {0}")]
pub struct Forbidden(pub String);

/// The authenticated caller of a request: who it is, what it may do and which Companies
/// it may see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    actor: Actor,
    is_admin: bool,
    capabilities: BTreeSet<Capability>,
    scope: TenantScope,
}

impl Principal {
    /// A principal with explicit Capabilities and tenant scope.
    #[must_use]
    pub fn new(
        actor: Actor,
        is_admin: bool,
        capabilities: BTreeSet<Capability>,
        scope: TenantScope,
    ) -> Self {
        // Administrators see every Company (ADR-0011).
        let scope = if is_admin { TenantScope::All } else { scope };
        Self {
            actor,
            is_admin,
            capabilities,
            scope,
        }
    }

    /// Cooper itself, for background jobs: every Capability, every Company.
    #[must_use]
    pub fn automation() -> Self {
        Self::new(Actor::Automation, true, BTreeSet::new(), TenantScope::All)
    }

    /// Who the caller is.
    #[must_use]
    pub const fn actor(&self) -> Actor {
        self.actor
    }

    /// Whether the caller is an Administrator (or Automation).
    #[must_use]
    pub const fn is_admin(&self) -> bool {
        self.is_admin
    }

    /// The Companies the caller may see.
    #[must_use]
    pub const fn scope(&self) -> &TenantScope {
        &self.scope
    }

    /// The Capabilities granted through Roles. Administrators have every Capability
    /// even though this set may be empty; use [`Principal::can`] to check.
    #[must_use]
    pub const fn capabilities(&self) -> &BTreeSet<Capability> {
        &self.capabilities
    }

    /// Whether the caller holds `capability` (given as `<module>.<action>`).
    #[must_use]
    pub fn can(&self, capability: &str) -> bool {
        self.is_admin || self.capabilities.iter().any(|c| c.as_str() == capability)
    }

    /// Fails unless the caller holds `capability`.
    ///
    /// # Errors
    ///
    /// Returns [`Forbidden`] naming the missing capability.
    pub fn require(&self, capability: &str) -> Result<(), Forbidden> {
        if self.can(capability) {
            Ok(())
        } else {
            Err(Forbidden(capability.to_owned()))
        }
    }

    /// Fails unless the caller is an Administrator.
    ///
    /// # Errors
    ///
    /// Returns [`Forbidden`] with the pseudo-capability `admin`.
    pub fn require_admin(&self) -> Result<(), Forbidden> {
        if self.is_admin {
            Ok(())
        } else {
            Err(Forbidden("admin".to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{AnalystId, TenantId};
    use proptest::prelude::*;

    fn cap(s: &str) -> Capability {
        s.parse().unwrap()
    }

    #[test]
    fn capability_requires_module_and_action() {
        assert_eq!(cap("tickets.update").module(), "tickets");
        for bad in [
            "tickets",
            ".update",
            "tickets.",
            "Tickets.update",
            "tickets.up date",
            "a.b.c",
            "1t.x",
        ] {
            assert!(
                bad.parse::<Capability>().is_err(),
                "{bad} should be rejected"
            );
        }
        assert!(
            cap("morning-checks.manage")
                .as_str()
                .starts_with("morning-checks")
        );
    }

    #[test]
    fn capability_serde_validates() {
        assert!(serde_json::from_str::<Capability>("\"tickets.read\"").is_ok());
        assert!(serde_json::from_str::<Capability>("\"nope\"").is_err());
    }

    #[test]
    fn analyst_needs_the_capability() {
        let p = Principal::new(
            Actor::Analyst(AnalystId::new()),
            false,
            BTreeSet::from([cap("tickets.read")]),
            TenantScope::single(TenantId::new()),
        );
        assert!(p.can("tickets.read"));
        assert_eq!(
            p.require("tickets.update"),
            Err(Forbidden("tickets.update".into()))
        );
        assert!(p.require_admin().is_err());
    }

    #[test]
    fn administrator_can_do_everything_and_sees_all_companies() {
        let p = Principal::new(
            Actor::Analyst(AnalystId::new()),
            true,
            BTreeSet::new(),
            TenantScope::none(),
        );
        assert!(p.can("anything.at_all"));
        assert_eq!(p.scope(), &TenantScope::All);
    }

    proptest! {
        #[test]
        fn valid_capabilities_round_trip(m in "[a-z][a-z0-9_-]{0,15}", a in "[a-z][a-z0-9_-]{0,15}") {
            let s = format!("{m}.{a}");
            let c: Capability = s.parse().unwrap();
            prop_assert_eq!(c.to_string(), s);
            prop_assert_eq!(c.module(), m.as_str());
        }
    }
}
