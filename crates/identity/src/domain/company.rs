//! Company (Tenant) aggregate (FR-IAM-008).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use cooper_kernel::TenantId;
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit::Audited;
use super::{DomainName, IdentityError, Name, TicketCode};

/// A customer organisation served by this instance. Called **Company** in the UI and
/// **Tenant** in code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Company {
    id: TenantId,
    name: Name,
    ticket_code: TicketCode,
    sender_domains: BTreeSet<DomainName>,
    is_default: bool,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn check_domains(
    domains: &BTreeSet<DomainName>,
    freemail: &BTreeSet<String>,
) -> Result<(), IdentityError> {
    match domains.iter().find(|d| freemail.contains(d.as_str())) {
        Some(d) => Err(IdentityError::FreemailDomain(d.to_string())),
        None => Ok(()),
    }
}

impl Company {
    /// Creates a Company. `freemail` is the list of Freemail Domains, which can never be
    /// Sender Domains.
    ///
    /// # Errors
    ///
    /// [`IdentityError::FreemailDomain`] if a Sender Domain is a Freemail Domain.
    pub fn create(
        name: Name,
        ticket_code: TicketCode,
        sender_domains: BTreeSet<DomainName>,
        is_default: bool,
        freemail: &BTreeSet<String>,
        now: DateTime<Utc>,
    ) -> Result<Self, IdentityError> {
        check_domains(&sender_domains, freemail)?;
        Ok(Self {
            id: TenantId::new(),
            name,
            ticket_code,
            sender_domains,
            is_default,
            is_active: true,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a Company from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: TenantId,
        name: Name,
        ticket_code: TicketCode,
        sender_domains: BTreeSet<DomainName>,
        is_default: bool,
        is_active: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            ticket_code,
            sender_domains,
            is_default,
            is_active,
            created_at,
            updated_at,
        }
    }

    /// Changes name, Ticket Code and Sender Domains.
    ///
    /// # Errors
    ///
    /// [`IdentityError::FreemailDomain`] if a Sender Domain is a Freemail Domain.
    pub fn update(
        &mut self,
        name: Name,
        ticket_code: TicketCode,
        sender_domains: BTreeSet<DomainName>,
        freemail: &BTreeSet<String>,
        now: DateTime<Utc>,
    ) -> Result<(), IdentityError> {
        check_domains(&sender_domains, freemail)?;
        self.name = name;
        self.ticket_code = ticket_code;
        self.sender_domains = sender_domains;
        self.updated_at = now;
        Ok(())
    }

    /// Deactivates the Company. Its data stays; it is hidden from pickers.
    ///
    /// # Errors
    ///
    /// [`IdentityError::DefaultCompanyRequired`] for the default Company.
    pub fn deactivate(&mut self, now: DateTime<Utc>) -> Result<(), IdentityError> {
        if self.is_default {
            return Err(IdentityError::DefaultCompanyRequired);
        }
        if !self.is_active {
            return Err(IdentityError::InvalidState(
                "the Company is already inactive",
            ));
        }
        self.is_active = false;
        self.updated_at = now;
        Ok(())
    }

    /// Reactivates the Company.
    ///
    /// # Errors
    ///
    /// [`IdentityError::InvalidState`] if it is already active.
    pub fn reactivate(&mut self, now: DateTime<Utc>) -> Result<(), IdentityError> {
        if self.is_active {
            return Err(IdentityError::InvalidState("the Company is already active"));
        }
        self.is_active = true;
        self.updated_at = now;
        Ok(())
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> TenantId {
        self.id
    }
    /// The name.
    #[must_use]
    pub const fn name(&self) -> &Name {
        &self.name
    }
    /// The Ticket Code.
    #[must_use]
    pub const fn ticket_code(&self) -> &TicketCode {
        &self.ticket_code
    }
    /// The Sender Domains.
    #[must_use]
    pub const fn sender_domains(&self) -> &BTreeSet<DomainName> {
        &self.sender_domains
    }
    /// Whether this is the default Company.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.is_default
    }
    /// Whether the Company is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
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

impl Audited for Company {
    const AGGREGATE_TYPE: &'static str = "company";

    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }

    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.id)
    }

    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("ticket_code", json!(self.ticket_code)),
            ("sender_domains", json!(self.sender_domains)),
            ("is_default", json!(self.is_default)),
            ("is_active", json!(self.is_active)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap()
    }

    fn freemail() -> BTreeSet<String> {
        BTreeSet::from(["gmail.com".to_owned()])
    }

    fn company(is_default: bool) -> Company {
        Company::create(
            Name::try_from("Acme").unwrap(),
            TicketCode::try_from("ACME").unwrap(),
            BTreeSet::from([DomainName::try_from("acme.example").unwrap()]),
            is_default,
            &freemail(),
            now(),
        )
        .unwrap()
    }

    #[test]
    fn freemail_domain_cannot_be_a_sender_domain() {
        let err = Company::create(
            Name::try_from("Acme").unwrap(),
            TicketCode::try_from("ACME").unwrap(),
            BTreeSet::from([DomainName::try_from("Gmail.com").unwrap()]),
            false,
            &freemail(),
            now(),
        )
        .unwrap_err();
        assert_eq!(err, IdentityError::FreemailDomain("gmail.com".into()));

        let mut c = company(false);
        let gmail = BTreeSet::from([DomainName::try_from("gmail.com").unwrap()]);
        assert!(
            c.update(
                c.name().clone(),
                c.ticket_code().clone(),
                gmail,
                &freemail(),
                now()
            )
            .is_err()
        );
        assert_eq!(c.sender_domains().len(), 1, "failed update changes nothing");
    }

    #[test]
    fn default_company_cannot_be_deactivated() {
        assert_eq!(
            company(true).deactivate(now()),
            Err(IdentityError::DefaultCompanyRequired)
        );
        let mut other = company(false);
        other.deactivate(now()).unwrap();
        assert!(!other.is_active());
        assert!(other.deactivate(now()).is_err());
        other.reactivate(now()).unwrap();
        assert!(other.is_active());
    }
}
