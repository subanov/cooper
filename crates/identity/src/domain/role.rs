//! RBAC Role aggregate (FR-IAM-010).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use cooper_kernel::{Capability, TenantId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit::Audited;
use super::{IdentityError, Name};

cooper_kernel::define_id!(
    /// Identifies a Role.
    RoleId
);

/// A named set of Capabilities, assigned to Analysts or Teams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    id: RoleId,
    name: Name,
    description: String,
    capabilities: BTreeSet<Capability>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn check_known(
    capabilities: &BTreeSet<Capability>,
    catalogue: &BTreeSet<Capability>,
) -> Result<(), IdentityError> {
    match capabilities.iter().find(|c| !catalogue.contains(*c)) {
        Some(unknown) => Err(IdentityError::UnknownCapability(unknown.to_string())),
        None => Ok(()),
    }
}

fn clean_description(description: &str) -> Result<String, IdentityError> {
    let d = description.trim();
    if d.chars().count() > 2000 {
        return Err(IdentityError::Invalid {
            field: "description",
            reason: "must be at most 2000 characters".into(),
        });
    }
    Ok(d.to_owned())
}

impl Role {
    /// Creates a Role. Every Capability must be in `catalogue` (the Capabilities the
    /// contexts declare).
    ///
    /// # Errors
    ///
    /// [`IdentityError::UnknownCapability`] or an invalid description.
    pub fn create(
        name: Name,
        description: &str,
        capabilities: BTreeSet<Capability>,
        catalogue: &BTreeSet<Capability>,
        now: DateTime<Utc>,
    ) -> Result<Self, IdentityError> {
        check_known(&capabilities, catalogue)?;
        Ok(Self {
            id: RoleId::new(),
            name,
            description: clean_description(description)?,
            capabilities,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a Role from storage.
    #[must_use]
    pub const fn restore(
        id: RoleId,
        name: Name,
        description: String,
        capabilities: BTreeSet<Capability>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            capabilities,
            created_at,
            updated_at,
        }
    }

    /// Replaces name, description and Capabilities.
    ///
    /// # Errors
    ///
    /// [`IdentityError::UnknownCapability`] or an invalid description.
    pub fn update(
        &mut self,
        name: Name,
        description: &str,
        capabilities: BTreeSet<Capability>,
        catalogue: &BTreeSet<Capability>,
        now: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        check_known(&capabilities, catalogue)?;
        self.description = clean_description(description)?;
        self.name = name;
        self.capabilities = capabilities;
        self.updated_at = now;
        Ok(())
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> RoleId {
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
    /// The Capabilities.
    #[must_use]
    pub const fn capabilities(&self) -> &BTreeSet<Capability> {
        &self.capabilities
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

impl Audited for Role {
    const AGGREGATE_TYPE: &'static str = "role";
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
            ("capabilities", json!(self.capabilities)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn cap(s: &str) -> Capability {
        s.parse().unwrap()
    }

    #[test]
    fn role_rejects_unknown_capabilities() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let catalogue = BTreeSet::from([cap("tickets.read")]);
        let err = Role::create(
            Name::try_from("Desk").unwrap(),
            "",
            BTreeSet::from([cap("tickets.read"), cap("tickets.fly")]),
            &catalogue,
            now,
        )
        .unwrap_err();
        assert_eq!(err, IdentityError::UnknownCapability("tickets.fly".into()));
        let role = Role::create(
            Name::try_from("Desk").unwrap(),
            "  Front line ",
            BTreeSet::from([cap("tickets.read")]),
            &catalogue,
            now,
        )
        .unwrap();
        assert_eq!(role.description(), "Front line");
    }
}
