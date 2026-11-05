//! Team aggregate (FR-IAM-006). A Team is also a ticket Queue.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use cooper_kernel::{AnalystId, TeamId, TenantId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit::Audited;
use super::{CompanyAccess, IdentityError, Name, RoleId};

/// A group of Analysts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    id: TeamId,
    name: Name,
    description: String,
    is_active: bool,
    members: BTreeSet<AnalystId>,
    roles: BTreeSet<RoleId>,
    company_access: CompanyAccess,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

pub(super) fn clean_text(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, IdentityError> {
    let v = value.trim();
    if v.chars().count() > max {
        return Err(IdentityError::Invalid {
            field,
            reason: format!("must be at most {max} characters"),
        });
    }
    Ok(v.to_owned())
}

impl Team {
    /// Creates an active Team without members.
    ///
    /// # Errors
    ///
    /// An invalid description.
    pub fn create(
        name: Name,
        description: &str,
        now: DateTime<Utc>,
    ) -> Result<Self, IdentityError> {
        Ok(Self {
            id: TeamId::new(),
            name,
            description: clean_text("description", description, 2000)?,
            is_active: true,
            members: BTreeSet::new(),
            roles: BTreeSet::new(),
            company_access: CompanyAccess::None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a Team from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: TeamId,
        name: Name,
        description: String,
        is_active: bool,
        members: BTreeSet<AnalystId>,
        roles: BTreeSet<RoleId>,
        company_access: CompanyAccess,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            is_active,
            members,
            roles,
            company_access,
            created_at,
            updated_at,
        }
    }

    /// Changes name, description and active flag.
    ///
    /// # Errors
    ///
    /// An invalid description.
    pub fn update(
        &mut self,
        name: Name,
        description: &str,
        is_active: bool,
        now: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        self.description = clean_text("description", description, 2000)?;
        self.name = name;
        self.is_active = is_active;
        self.updated_at = now;
        Ok(())
    }

    /// Replaces the members.
    pub fn set_members(&mut self, members: BTreeSet<AnalystId>, now: DateTime<Utc>) {
        self.members = members;
        self.updated_at = now;
    }

    /// Replaces the Roles assigned to the Team.
    pub fn assign_roles(&mut self, roles: BTreeSet<RoleId>, now: DateTime<Utc>) {
        self.roles = roles;
        self.updated_at = now;
    }

    /// Replaces the Company access granted to the Team.
    pub fn grant_company_access(&mut self, access: CompanyAccess, now: DateTime<Utc>) {
        self.company_access = access;
        self.updated_at = now;
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> TeamId {
        self.id
    }
    /// The name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }
    /// The description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
    /// Whether the Team is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }
    /// The members.
    #[must_use]
    pub const fn members(&self) -> &BTreeSet<AnalystId> {
        &self.members
    }
    /// Roles assigned to the Team.
    #[must_use]
    pub const fn roles(&self) -> &BTreeSet<RoleId> {
        &self.roles
    }
    /// Company access granted to the Team.
    #[must_use]
    pub const fn company_access(&self) -> &CompanyAccess {
        &self.company_access
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

impl Audited for Team {
    const AGGREGATE_TYPE: &'static str = "team";
    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("description", json!(self.description)),
            ("is_active", json!(self.is_active)),
            ("members", json!(self.members)),
            ("roles", json!(self.roles)),
            ("company_access", json!(self.company_access)),
        ]
    }
}
