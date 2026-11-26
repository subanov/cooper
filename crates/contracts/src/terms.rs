//! Configurable Contract Terms (FR-CON-005, ADR-0042). Tabs are reference data; each Contract
//! holds a value per tab.

use chrono::{DateTime, Utc};
use cooper_kernel::Actor;
use thiserror::Error;
use uuid::Uuid;

/// A Contract Terms problem.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TermsError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
}

fn invalid(field: &'static str, reason: impl Into<String>) -> TermsError {
    TermsError::Invalid { field, reason: reason.into() }
}

/// Longest value of a term, in characters.
pub const MAX_VALUE_LEN: usize = 5000;

/// A Contract Terms tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermTab {
    /// ID.
    pub id: Uuid,
    /// Name (1 to 100 characters, unique).
    pub name: String,
    /// Display order.
    pub sort_order: i32,
    /// Active tabs can be filled in; inactive ones keep their values.
    pub active: bool,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// Who created it.
    pub created_by: Actor,
}

impl TermTab {
    /// Validates a new tab.
    ///
    /// # Errors
    ///
    /// An empty or too long name.
    pub fn new(name: &str, sort_order: i32, now: DateTime<Utc>, by: Actor) -> Result<Self, TermsError> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(invalid("name", "must be 1 to 100 characters"));
        }
        Ok(Self { id: Uuid::now_v7(), name, sort_order, active: true, created_at: now, created_by: by })
    }

    /// Changes the name, order and active flag.
    ///
    /// # Errors
    ///
    /// An empty or too long name.
    pub fn edit(&mut self, name: &str, sort_order: i32, active: bool) -> Result<(), TermsError> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(invalid("name", "must be 1 to 100 characters"));
        }
        self.name = name;
        self.sort_order = sort_order;
        self.active = active;
        Ok(())
    }
}

/// The values of a Contract's Terms, by tab. Blank values are not kept.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TermValues(pub std::collections::BTreeMap<Uuid, String>);

impl TermValues {
    /// Checks values against the tabs: each tab must exist and be active, and values are
    /// trimmed and limited in length.
    ///
    /// # Errors
    ///
    /// An unknown or inactive tab, or a value that is too long.
    pub fn check(tabs: &[TermTab], given: &[(Uuid, String)]) -> Result<Self, TermsError> {
        let mut out = std::collections::BTreeMap::new();
        for (tab, value) in given {
            let Some(t) = tabs.iter().find(|t| t.id == *tab) else {
                return Err(invalid("tab_id", "is not a Contract Terms tab"));
            };
            if !t.active {
                return Err(invalid("tab_id", "is an inactive tab"));
            }
            let value = value.trim();
            if value.chars().count() > MAX_VALUE_LEN {
                return Err(invalid("value", "must be at most 5000 characters"));
            }
            if !value.is_empty() {
                out.insert(*tab, value.to_owned());
            }
        }
        Ok(Self(out))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test code

    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 29, 9, 0, 0).unwrap()
    }

    #[test]
    fn tabs_need_a_name_and_can_be_deactivated() {
        assert!(TermTab::new("  ", 0, now(), Actor::Automation).is_err());
        let mut tab = TermTab::new(" SLA credits ", 2, now(), Actor::Automation).unwrap();
        assert_eq!(tab.name, "SLA credits");
        tab.edit("SLA credits", 2, false).unwrap();
        assert!(!tab.active);
    }

    #[test]
    fn values_only_go_to_active_tabs_and_blank_values_are_dropped() {
        let open = TermTab::new("Exit", 1, now(), Actor::Automation).unwrap();
        let mut closed = TermTab::new("Old", 2, now(), Actor::Automation).unwrap();
        closed.active = false;
        let tabs = vec![open.clone(), closed.clone()];
        let blank = TermValues::check(&tabs, &[(open.id, "  ".into())]).unwrap();
        assert!(blank.0.is_empty(), "blank values are not stored");
        let ok = TermValues::check(&tabs, &[(open.id, " 90 days ".into())]).unwrap();
        assert_eq!(ok.0.get(&open.id).map(String::as_str), Some("90 days"));
        assert!(TermValues::check(&tabs, &[(closed.id, "x".into())]).is_err());
        assert!(TermValues::check(&tabs, &[(Uuid::now_v7(), "x".into())]).is_err());
        assert!(TermValues::check(&tabs, &[(open.id, "x".repeat(5001))]).is_err());
    }
}
