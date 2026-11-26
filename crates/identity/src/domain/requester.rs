//! Requester aggregate (FR-IAM-011).

use chrono::{DateTime, Utc};
use cooper_kernel::{DepartmentId, RequesterId, TenantId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit::Audited;
use super::team::clean_text;
use super::{CostCentreId, EmailAddress, IdentityError, Name, Username};

/// Input for a new or updated Requester.
#[derive(Debug, Clone)]
pub struct NewRequester {
    /// Display name.
    pub full_name: Name,
    /// Email address (unique per Company).
    pub email: EmailAddress,
    /// Optional sign-in name (unique per Company).
    pub username: Option<Username>,
    /// Optional phone number.
    pub phone: Option<String>,
    /// The Requester's Manager, in the same Company.
    pub manager_id: Option<RequesterId>,
    /// The Requester's Cost Centre, in the same Company.
    pub cost_centre_id: Option<CostCentreId>,
    /// The Requester's Department (FR-FRM-002 audiences).
    pub department_id: Option<DepartmentId>,
}

/// A person who asks IT for help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requester {
    id: RequesterId,
    tenant_id: TenantId,
    full_name: Name,
    email: EmailAddress,
    username: Option<Username>,
    phone: Option<String>,
    manager_id: Option<RequesterId>,
    cost_centre_id: Option<CostCentreId>,
    department_id: Option<DepartmentId>,
    deactivated_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn clean_phone(phone: Option<String>) -> Result<Option<String>, IdentityError> {
    match phone {
        None => Ok(None),
        Some(p) => {
            let p = clean_text("phone", &p, 50)?;
            Ok((!p.is_empty()).then_some(p))
        }
    }
}

impl Requester {
    /// Creates an active Requester in `tenant_id`. The application checks that the
    /// Manager and Cost Centre belong to the same Company.
    ///
    /// # Errors
    ///
    /// An invalid phone number.
    pub fn create(
        tenant_id: TenantId,
        new: NewRequester,
        now: DateTime<Utc>,
    ) -> Result<Self, IdentityError> {
        Ok(Self {
            id: RequesterId::new(),
            tenant_id,
            full_name: new.full_name,
            email: new.email,
            username: new.username,
            phone: clean_phone(new.phone)?,
            manager_id: new.manager_id,
            cost_centre_id: new.cost_centre_id,
            department_id: new.department_id,
            deactivated_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a Requester from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: RequesterId,
        tenant_id: TenantId,
        full_name: Name,
        email: EmailAddress,
        username: Option<Username>,
        phone: Option<String>,
        manager_id: Option<RequesterId>,
        cost_centre_id: Option<CostCentreId>,
        deactivated_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            full_name,
            email,
            username,
            phone,
            manager_id,
            cost_centre_id,
            department_id: None,
            deactivated_at,
            created_at,
            updated_at,
        }
    }

    /// Sets the Department of a Requester read from storage.
    #[must_use]
    pub(crate) const fn with_department(mut self, department: Option<DepartmentId>) -> Self {
        self.department_id = department;
        self
    }

    /// Replaces the editable fields.
    ///
    /// # Errors
    ///
    /// [`IdentityError::ManagerCycle`] if the Requester would manage themselves, or an
    /// invalid phone number.
    pub fn update(&mut self, new: NewRequester, now: DateTime<Utc>) -> Result<(), IdentityError> {
        if new.manager_id == Some(self.id) {
            return Err(IdentityError::ManagerCycle);
        }
        self.phone = clean_phone(new.phone)?;
        self.full_name = new.full_name;
        self.email = new.email;
        self.username = new.username;
        self.manager_id = new.manager_id;
        self.cost_centre_id = new.cost_centre_id;
        self.department_id = new.department_id;
        self.updated_at = now;
        Ok(())
    }

    /// Deactivates the Requester, recording when (FR-IAM-011).
    ///
    /// # Errors
    ///
    /// [`IdentityError::InvalidState`] if already inactive.
    pub fn deactivate(&mut self, now: DateTime<Utc>) -> Result<(), IdentityError> {
        if self.deactivated_at.is_some() {
            return Err(IdentityError::InvalidState(
                "the Requester is already inactive",
            ));
        }
        self.deactivated_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    /// Reactivates the Requester.
    ///
    /// # Errors
    ///
    /// [`IdentityError::InvalidState`] if already active.
    pub fn reactivate(&mut self, now: DateTime<Utc>) -> Result<(), IdentityError> {
        if self.deactivated_at.is_none() {
            return Err(IdentityError::InvalidState(
                "the Requester is already active",
            ));
        }
        self.deactivated_at = None;
        self.updated_at = now;
        Ok(())
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> RequesterId {
        self.id
    }
    /// The Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// The display name.
    #[must_use]
    pub const fn full_name(&self) -> &Name {
        &self.full_name
    }
    /// The email address.
    #[must_use]
    pub const fn email(&self) -> &EmailAddress {
        &self.email
    }
    /// The sign-in name.
    #[must_use]
    pub const fn username(&self) -> Option<&Username> {
        self.username.as_ref()
    }
    /// The phone number.
    #[must_use]
    pub fn phone(&self) -> Option<&str> {
        self.phone.as_deref()
    }
    /// The Manager.
    #[must_use]
    pub const fn manager_id(&self) -> Option<RequesterId> {
        self.manager_id
    }
    /// The Cost Centre.
    #[must_use]
    pub const fn cost_centre_id(&self) -> Option<CostCentreId> {
        self.cost_centre_id
    }

    /// The Department, if any.
    #[must_use]
    pub const fn department_id(&self) -> Option<DepartmentId> {
        self.department_id
    }
    /// Whether the Requester is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.deactivated_at.is_none()
    }
    /// When the Requester was deactivated.
    #[must_use]
    pub const fn deactivated_at(&self) -> Option<DateTime<Utc>> {
        self.deactivated_at
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

impl Audited for Requester {
    const AGGREGATE_TYPE: &'static str = "requester";
    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("full_name", json!(self.full_name)),
            ("email", json!(self.email)),
            ("username", json!(self.username)),
            ("phone", json!(self.phone)),
            ("manager_id", json!(self.manager_id)),
            ("cost_centre_id", json!(self.cost_centre_id)),
            ("department_id", json!(self.department_id)),
            ("deactivated_at", json!(self.deactivated_at)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap()
    }

    fn input() -> NewRequester {
        NewRequester {
            full_name: Name::try_from("Sam Smith").unwrap(),
            email: EmailAddress::try_from("sam@acme.example").unwrap(),
            username: None,
            phone: Some("  ".into()),
            manager_id: None,
            cost_centre_id: None,
            department_id: None,
        }
    }

    #[test]
    fn requester_cannot_manage_themselves() {
        let mut r = Requester::create(TenantId::new(), input(), now()).unwrap();
        assert_eq!(r.phone(), None, "blank phone is stored as none");
        let mut changed = input();
        changed.manager_id = Some(r.id());
        assert_eq!(r.update(changed, now()), Err(IdentityError::ManagerCycle));
    }

    #[test]
    fn deactivation_is_timestamped() {
        let mut r = Requester::create(TenantId::new(), input(), now()).unwrap();
        r.deactivate(now()).unwrap();
        assert_eq!(r.deactivated_at(), Some(now()));
        assert!(!r.is_active());
        assert!(r.deactivate(now()).is_err());
        r.reactivate(now()).unwrap();
        assert!(r.is_active());
    }
}
