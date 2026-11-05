//! Modules: functional areas an Administrator can turn on or off (FR-GEN-005).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A functional area of Cooper (fixed set). When a Module is off it is hidden from
/// navigation and its API answers `404`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Module {
    /// Tickets, Mailboxes, SLA and Rota. Always on: everything else builds on it.
    Tickets,
    /// The Self-Service Portal for Requesters.
    Portal,
    /// Problem Management.
    Problems,
    /// Change Management.
    Changes,
    /// Tasks, Checklists and Projects.
    Tasks,
    /// Assets and Software.
    Assets,
    /// Configuration Management Database.
    Cmdb,
    /// Knowledge articles.
    Knowledge,
    /// Service Status and Planned Maintenance.
    ServiceStatus,
    /// Morning Checks.
    MorningChecks,
    /// Contracts and Suppliers.
    Contracts,
    /// Forms.
    Forms,
    /// Calendar.
    Calendar,
    /// Workflows and Webhooks.
    Workflow,
    /// Reports and Dashboards.
    Reporting,
}

/// A string is not a [`Module`] key.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown module {0:?}")]
pub struct ParseModuleError(pub String);

impl Module {
    /// Every Module, in display order.
    pub const ALL: [Self; 15] = [
        Self::Tickets,
        Self::Portal,
        Self::Problems,
        Self::Changes,
        Self::Tasks,
        Self::Assets,
        Self::Cmdb,
        Self::Knowledge,
        Self::ServiceStatus,
        Self::MorningChecks,
        Self::Contracts,
        Self::Forms,
        Self::Calendar,
        Self::Workflow,
        Self::Reporting,
    ];

    /// Stable key used in storage, the API and Capability prefixes.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Tickets => "tickets",
            Self::Portal => "portal",
            Self::Problems => "problems",
            Self::Changes => "changes",
            Self::Tasks => "tasks",
            Self::Assets => "assets",
            Self::Cmdb => "cmdb",
            Self::Knowledge => "knowledge",
            Self::ServiceStatus => "service-status",
            Self::MorningChecks => "morning-checks",
            Self::Contracts => "contracts",
            Self::Forms => "forms",
            Self::Calendar => "calendar",
            Self::Workflow => "workflow",
            Self::Reporting => "reporting",
        }
    }

    /// Display name (UI copy).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tickets => "Tickets",
            Self::Portal => "Self-Service Portal",
            Self::Problems => "Problems",
            Self::Changes => "Changes",
            Self::Tasks => "Tasks",
            Self::Assets => "Assets",
            Self::Cmdb => "CMDB",
            Self::Knowledge => "Knowledge",
            Self::ServiceStatus => "Service Status",
            Self::MorningChecks => "Morning Checks",
            Self::Contracts => "Contracts",
            Self::Forms => "Forms",
            Self::Calendar => "Calendar",
            Self::Workflow => "Workflow",
            Self::Reporting => "Reporting",
        }
    }

    /// Whether an Administrator may turn the Module off.
    #[must_use]
    pub const fn can_disable(self) -> bool {
        !matches!(self, Self::Tickets)
    }
}

impl FromStr for Module {
    type Err = ParseModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|m| m.key() == s)
            .ok_or_else(|| ParseModuleError(s.to_owned()))
    }
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_match_serde() {
        for m in Module::ALL {
            assert_eq!(m.key().parse::<Module>(), Ok(m));
            assert_eq!(serde_json::to_value(m).unwrap(), serde_json::json!(m.key()));
        }
        assert!("plugins".parse::<Module>().is_err());
        assert!(!Module::Tickets.can_disable());
        assert!(Module::Assets.can_disable());
    }
}
