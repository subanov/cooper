//! Company use cases (FR-IAM-008).

use std::collections::BTreeSet;

use cooper_kernel::{AuditAction, Page, Principal, TenantId};

use super::{
    AppError, IdentityService, ListRequest, capabilities, page_by_name, require, require_staff,
};
use crate::domain::{Company, DomainName, Name, TicketCode, audit_record};

/// Editable fields of a Company.
#[derive(Debug, Clone)]
pub struct CompanyInput {
    /// Name.
    pub name: Name,
    /// Ticket Code.
    pub ticket_code: TicketCode,
    /// Sender Domains.
    pub sender_domains: BTreeSet<DomainName>,
}

impl IdentityService {
    /// Lists the Companies the caller can see.
    ///
    /// # Errors
    ///
    /// Invalid cursor or storage failures.
    pub async fn list_companies(
        &self,
        principal: &Principal,
        request: &ListRequest,
    ) -> Result<Page<Company>, AppError> {
        require_staff(principal)?;
        let query = request.query()?;
        let rows = self.ports.companies.list(principal.scope(), &query).await?;
        page_by_name(rows, query.limit, |c| {
            (c.name().as_str().to_lowercase(), c.id().as_uuid())
        })
    }

    /// Loads a Company the caller can see.
    ///
    /// # Errors
    ///
    /// [`AppError::NotFound`] if it does not exist or is outside the caller's scope.
    pub async fn get_company(
        &self,
        principal: &Principal,
        id: TenantId,
    ) -> Result<Company, AppError> {
        require_staff(principal)?;
        self.ports
            .companies
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Company"))
    }

    /// Creates a Company. Needs `companies.manage` and access to all Companies, because
    /// the new Company would otherwise be invisible to its creator.
    ///
    /// # Errors
    ///
    /// Forbidden, validation or conflict errors.
    pub async fn create_company(
        &self,
        principal: &Principal,
        input: CompanyInput,
    ) -> Result<Company, AppError> {
        require(principal, capabilities::COMPANIES_MANAGE)?;
        if principal.scope() != &cooper_kernel::TenantScope::All {
            return Err(AppError::Forbidden(cooper_kernel::Forbidden(
                "access to all Companies".into(),
            )));
        }
        let now = self.ports.clock.now();
        let freemail = self.ports.companies.freemail_domains().await?;
        let company = Company::create(
            input.name,
            input.ticket_code,
            input.sender_domains,
            false,
            &freemail,
            now,
        )?;
        let audit = audit_record(None, &company, AuditAction::Created, principal.actor(), now);
        self.ports.companies.insert(&company, &audit).await?;
        Ok(company)
    }

    /// Updates a Company.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, validation or conflict errors.
    pub async fn update_company(
        &self,
        principal: &Principal,
        id: TenantId,
        input: CompanyInput,
    ) -> Result<Company, AppError> {
        require(principal, capabilities::COMPANIES_MANAGE)?;
        let mut company = self.get_company(principal, id).await?;
        let before = company.clone();
        let now = self.ports.clock.now();
        let freemail = self.ports.companies.freemail_domains().await?;
        company.update(
            input.name,
            input.ticket_code,
            input.sender_domains,
            &freemail,
            now,
        )?;
        self.save_company(principal, &before, &company).await?;
        Ok(company)
    }

    /// Activates or deactivates a Company.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or the default Company.
    pub async fn set_company_active(
        &self,
        principal: &Principal,
        id: TenantId,
        active: bool,
    ) -> Result<Company, AppError> {
        require(principal, capabilities::COMPANIES_MANAGE)?;
        let mut company = self.get_company(principal, id).await?;
        let before = company.clone();
        let now = self.ports.clock.now();
        if active {
            company.reactivate(now)?;
        } else {
            company.deactivate(now)?;
        }
        self.save_company(principal, &before, &company).await?;
        Ok(company)
    }

    async fn save_company(
        &self,
        principal: &Principal,
        before: &Company,
        after: &Company,
    ) -> Result<(), AppError> {
        let now = self.ports.clock.now();
        let audit = audit_record(
            Some(before),
            after,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports.companies.update(after, &audit).await?;
        }
        Ok(())
    }
}
