//! Department aggregate (FR-IAM-007).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use cooper_kernel::{DepartmentId, Sensitivity, TeamId, TenantId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::Name;
use super::audit::Audited;

/// A service area (IT, HR, Facilities, …) with a default Sensitivity, served by Teams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Department {
    id: DepartmentId,
    name: Name,
    default_sensitivity: Sensitivity,
    is_active: bool,
    teams: BTreeSet<TeamId>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Department {
    /// Creates an active Department.
    #[must_use]
    pub fn create(
        name: Name,
        default_sensitivity: Sensitivity,
        teams: BTreeSet<TeamId>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: DepartmentId::new(),
            name,
            default_sensitivity,
            is_active: true,
            teams,
            created_at: now,
            updated_at: now,
        }
    }

    /// Rebuilds a Department from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: DepartmentId,
        name: Name,
        default_sensitivity: Sensitivity,
        is_active: bool,
        teams: BTreeSet<TeamId>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            default_sensitivity,
            is_active,
            teams,
            created_at,
            updated_at,
        }
    }

    /// Replaces every editable field.
    pub fn update(
        &mut self,
        name: Name,
        default_sensitivity: Sensitivity,
        is_active: bool,
        teams: BTreeSet<TeamId>,
        now: DateTime<Utc>,
    ) {
        self.name = name;
        self.default_sensitivity = default_sensitivity;
        self.is_active = is_active;
        self.teams = teams;
        self.updated_at = now;
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> DepartmentId {
        self.id
    }
    /// The name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }
    /// The Sensitivity new tickets in this Department get.
    #[must_use]
    pub const fn default_sensitivity(&self) -> Sensitivity {
        self.default_sensitivity
    }
    /// Whether the Department is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }
    /// Teams that serve the Department.
    #[must_use]
    pub const fn teams(&self) -> &BTreeSet<TeamId> {
        &self.teams
    }
    /// When it was created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// When it was last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl Audited for Department {
    const AGGREGATE_TYPE: &'static str = "department";
    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("default_sensitivity", json!(self.default_sensitivity)),
            ("is_active", json!(self.is_active)),
            ("teams", json!(self.teams)),
        ]
    }
}
