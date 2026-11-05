//! Analyst aggregate (FR-IAM-001, FR-IAM-009, FR-IAM-010).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use cooper_kernel::{AnalystId, TenantId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit::Audited;
use super::totp::TotpSecret;
use super::{CompanyAccess, EmailAddress, IdentityError, Name, PasswordHash, RoleId, Username};

/// Two-factor sign-in state of an Analyst (FR-IAM-003).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TotpState {
    /// The confirmed secret; two-factor sign-in is on when set.
    pub secret: Option<TotpSecret>,
    /// A secret waiting for its first code during enrollment.
    pub pending: Option<TotpSecret>,
    /// When two-factor sign-in was turned on.
    pub enabled_at: Option<DateTime<Utc>>,
    /// The last accepted time step; codes up to it are rejected (replay protection).
    pub last_step: Option<i64>,
}

/// Input for a new Analyst.
#[derive(Debug, Clone)]
pub struct NewAnalyst {
    /// Sign-in name.
    pub username: Username,
    /// Email address.
    pub email: EmailAddress,
    /// Display name.
    pub full_name: Name,
    /// Whether the Analyst is an Administrator.
    pub is_admin: bool,
}

/// A member of IT staff who signs in to Cooper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analyst {
    id: AnalystId,
    username: Username,
    email: EmailAddress,
    full_name: Name,
    is_admin: bool,
    password_hash: Option<PasswordHash>,
    password_changed_at: Option<DateTime<Utc>>,
    deactivated_at: Option<DateTime<Utc>>,
    roles: BTreeSet<RoleId>,
    company_access: CompanyAccess,
    totp: TotpState,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Analyst {
    /// Creates an active Analyst without a password, Roles or Company access.
    #[must_use]
    pub fn create(new: NewAnalyst, now: DateTime<Utc>) -> Self {
        Self {
            id: AnalystId::new(),
            username: new.username,
            email: new.email,
            full_name: new.full_name,
            is_admin: new.is_admin,
            password_hash: None,
            password_changed_at: None,
            deactivated_at: None,
            roles: BTreeSet::new(),
            company_access: CompanyAccess::None,
            totp: TotpState::default(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Rebuilds an Analyst from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: AnalystId,
        username: Username,
        email: EmailAddress,
        full_name: Name,
        is_admin: bool,
        password_hash: Option<PasswordHash>,
        password_changed_at: Option<DateTime<Utc>>,
        deactivated_at: Option<DateTime<Utc>>,
        roles: BTreeSet<RoleId>,
        company_access: CompanyAccess,
        totp: TotpState,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            username,
            email,
            full_name,
            is_admin,
            password_hash,
            password_changed_at,
            deactivated_at,
            roles,
            company_access,
            totp,
            created_at,
            updated_at,
        }
    }

    /// Changes username, email, name and the Administrator flag. `by` is the Analyst
    /// making the change: nobody can remove their own Administrator flag.
    ///
    /// # Errors
    ///
    /// [`IdentityError::SelfLockout`].
    pub fn update(
        &mut self,
        username: Username,
        email: EmailAddress,
        full_name: Name,
        is_admin: bool,
        by: Option<AnalystId>,
        now: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        if by == Some(self.id) && self.is_admin && !is_admin {
            return Err(IdentityError::SelfLockout);
        }
        self.username = username;
        self.email = email;
        self.full_name = full_name;
        self.is_admin = is_admin;
        self.updated_at = now;
        Ok(())
    }

    /// Sets a new password hash.
    pub fn set_password(&mut self, hash: PasswordHash, now: DateTime<Utc>) {
        self.password_hash = Some(hash);
        self.password_changed_at = Some(now);
        self.updated_at = now;
    }

    /// Deactivates the Analyst. They can no longer sign in; their history stays.
    ///
    /// # Errors
    ///
    /// [`IdentityError::SelfLockout`] when `by` is this Analyst, or
    /// [`IdentityError::InvalidState`] if already inactive.
    pub fn deactivate(
        &mut self,
        by: Option<AnalystId>,
        now: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        if by == Some(self.id) {
            return Err(IdentityError::SelfLockout);
        }
        if self.deactivated_at.is_some() {
            return Err(IdentityError::InvalidState(
                "the Analyst is already inactive",
            ));
        }
        self.deactivated_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    /// Reactivates the Analyst.
    ///
    /// # Errors
    ///
    /// [`IdentityError::InvalidState`] if already active.
    pub fn reactivate(&mut self, now: DateTime<Utc>) -> Result<(), IdentityError> {
        if self.deactivated_at.is_none() {
            return Err(IdentityError::InvalidState("the Analyst is already active"));
        }
        self.deactivated_at = None;
        self.updated_at = now;
        Ok(())
    }

    /// Starts (or restarts) two-factor enrollment with a new secret. Two-factor
    /// sign-in is only on after [`Analyst::confirm_totp`].
    pub fn begin_totp_enrollment(&mut self, secret: TotpSecret, now: DateTime<Utc>) {
        self.totp.pending = Some(secret);
        self.updated_at = now;
    }

    /// Confirms enrollment with a code from the authenticator app.
    ///
    /// # Errors
    ///
    /// [`IdentityError::InvalidState`] without a pending enrollment, or
    /// [`IdentityError::InvalidTotpCode`].
    pub fn confirm_totp(&mut self, code: &str, now: DateTime<Utc>) -> Result<(), IdentityError> {
        let pending = self
            .totp
            .pending
            .clone()
            .ok_or(IdentityError::InvalidState(
                "start two-factor enrollment first",
            ))?;
        let step = pending
            .verify(code, now.timestamp(), None)
            .ok_or(IdentityError::InvalidTotpCode)?;
        self.totp = TotpState {
            secret: Some(pending),
            pending: None,
            enabled_at: Some(now),
            last_step: Some(step),
        };
        self.updated_at = now;
        Ok(())
    }

    /// Checks a sign-in code and remembers its step so it cannot be used again.
    /// Returns `false` when two-factor sign-in is off or the code is wrong or reused.
    pub fn verify_totp(&mut self, code: &str, now: DateTime<Utc>) -> bool {
        let Some(secret) = &self.totp.secret else {
            return false;
        };
        match secret.verify(code, now.timestamp(), self.totp.last_step) {
            Some(step) => {
                self.totp.last_step = Some(step);
                true
            }
            None => false,
        }
    }

    /// Turns two-factor sign-in off and drops any pending enrollment.
    pub fn disable_totp(&mut self, now: DateTime<Utc>) {
        self.totp = TotpState::default();
        self.updated_at = now;
    }

    /// Whether two-factor sign-in is on.
    #[must_use]
    pub const fn totp_enabled(&self) -> bool {
        self.totp.secret.is_some()
    }

    /// The two-factor state, for storage.
    #[must_use]
    pub const fn totp(&self) -> &TotpState {
        &self.totp
    }

    /// Replaces the Roles assigned directly to this Analyst.
    pub fn assign_roles(&mut self, roles: BTreeSet<RoleId>, now: DateTime<Utc>) {
        self.roles = roles;
        self.updated_at = now;
    }

    /// Replaces the Company access granted directly to this Analyst.
    pub fn grant_company_access(&mut self, access: CompanyAccess, now: DateTime<Utc>) {
        self.company_access = access;
        self.updated_at = now;
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> AnalystId {
        self.id
    }
    /// The sign-in name.
    #[must_use]
    pub const fn username(&self) -> &Username {
        &self.username
    }
    /// The email address.
    #[must_use]
    pub const fn email(&self) -> &EmailAddress {
        &self.email
    }
    /// The display name.
    #[must_use]
    pub const fn full_name(&self) -> &Name {
        &self.full_name
    }
    /// Whether the Analyst is an Administrator.
    #[must_use]
    pub const fn is_admin(&self) -> bool {
        self.is_admin
    }
    /// Whether the Analyst is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.deactivated_at.is_none()
    }
    /// The password hash, if a password is set.
    #[must_use]
    pub const fn password_hash(&self) -> Option<&PasswordHash> {
        self.password_hash.as_ref()
    }
    /// When the password was last set.
    #[must_use]
    pub const fn password_changed_at(&self) -> Option<DateTime<Utc>> {
        self.password_changed_at
    }
    /// When the Analyst was deactivated.
    #[must_use]
    pub const fn deactivated_at(&self) -> Option<DateTime<Utc>> {
        self.deactivated_at
    }
    /// Roles assigned directly.
    #[must_use]
    pub const fn roles(&self) -> &BTreeSet<RoleId> {
        &self.roles
    }
    /// Company access granted directly.
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

impl Audited for Analyst {
    const AGGREGATE_TYPE: &'static str = "analyst";

    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }

    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }

    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        // The password hash is never audited; only the fact that it changed.
        vec![
            ("username", json!(self.username)),
            ("email", json!(self.email)),
            ("full_name", json!(self.full_name)),
            ("is_admin", json!(self.is_admin)),
            ("is_active", json!(self.is_active())),
            ("password_changed_at", json!(self.password_changed_at)),
            ("roles", json!(self.roles)),
            ("company_access", json!(self.company_access)),
            ("totp_enabled_at", json!(self.totp.enabled_at)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::audit_record as audit_record_for_tests;
    use chrono::TimeZone;
    use cooper_kernel::{Actor, AuditAction};

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap()
    }

    fn analyst(is_admin: bool) -> Analyst {
        Analyst::create(
            NewAnalyst {
                username: Username::try_from("jdoe").unwrap(),
                email: EmailAddress::try_from("jdoe@example.com").unwrap(),
                full_name: Name::try_from("Jane Doe").unwrap(),
                is_admin,
            },
            now(),
        )
    }

    #[test]
    fn administrators_cannot_lock_themselves_out() {
        let mut a = analyst(true);
        let me = Some(a.id());
        assert_eq!(a.deactivate(me, now()), Err(IdentityError::SelfLockout));
        let (u, e, n) = (
            a.username().clone(),
            a.email().clone(),
            a.full_name().clone(),
        );
        assert_eq!(
            a.update(u.clone(), e.clone(), n.clone(), false, me, now()),
            Err(IdentityError::SelfLockout)
        );
        assert!(a.is_admin());
        // Another Administrator may do both.
        let other = Some(AnalystId::new());
        a.update(u, e, n, false, other, now()).unwrap();
        a.deactivate(other, now()).unwrap();
        assert!(!a.is_active());
    }

    #[test]
    fn deactivate_and_reactivate_record_the_timestamp() {
        let mut a = analyst(false);
        a.deactivate(None, now()).unwrap();
        assert_eq!(a.deactivated_at(), Some(now()));
        assert!(a.deactivate(None, now()).is_err());
        a.reactivate(now()).unwrap();
        assert!(a.is_active());
        assert!(a.reactivate(now()).is_err());
    }

    #[test]
    fn password_hash_is_not_audited() {
        let before = analyst(false);
        let mut after = before.clone();
        after.set_password(PasswordHash::from_phc("$argon2id$secret".into()), now());
        let rec = audit_record_for_tests(
            Some(&before),
            &after,
            AuditAction::Updated,
            Actor::Automation,
            now(),
        );
        assert_eq!(rec.changes.len(), 1);
        assert_eq!(rec.changes[0].field, "password_changed_at");
        assert!(!serde_json::to_string(&rec).unwrap().contains("secret"));
    }
}
