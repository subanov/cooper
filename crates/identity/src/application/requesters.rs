//! Requester and Cost Centre use cases (FR-IAM-011). Tenant-scoped: every call runs
//! inside the caller's tenant scope.

use cooper_kernel::{AuditAction, Page, Principal, RequesterId, TenantId};

use super::{AppError, IdentityService, ListRequest, capabilities, page_by_name, require};
use crate::domain::{
    CostCentre, CostCentreCode, CostCentreId, EmailAddress, IdentityError, Name, NewRequester,
    Requester, Username, audit_record,
};

/// Editable fields of a Requester.
#[derive(Debug, Clone)]
pub struct RequesterInput {
    /// Display name.
    pub full_name: Name,
    /// Email.
    pub email: EmailAddress,
    /// Sign-in name.
    pub username: Option<Username>,
    /// Phone number.
    pub phone: Option<String>,
    /// Manager (same Company).
    pub manager_id: Option<RequesterId>,
    /// Cost Centre (same Company).
    pub cost_centre_id: Option<CostCentreId>,
    /// Department.
    pub department_id: Option<cooper_kernel::DepartmentId>,
}

impl From<RequesterInput> for NewRequester {
    fn from(i: RequesterInput) -> Self {
        Self {
            full_name: i.full_name,
            email: i.email,
            username: i.username,
            phone: i.phone,
            manager_id: i.manager_id,
            cost_centre_id: i.cost_centre_id,
            department_id: i.department_id,
        }
    }
}

/// Editable fields of a Cost Centre.
#[derive(Debug, Clone)]
pub struct CostCentreInput {
    /// Code.
    pub code: CostCentreCode,
    /// Name.
    pub name: Name,
    /// Active flag.
    pub is_active: bool,
}

/// Longest Manager chain checked for cycles.
const MAX_MANAGER_DEPTH: u32 = 50;

impl IdentityService {
    /// Lists Requesters in the caller's scope.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid cursor or storage failures.
    pub async fn list_requesters(
        &self,
        principal: &Principal,
        request: &ListRequest,
    ) -> Result<Page<Requester>, AppError> {
        require(principal, capabilities::REQUESTERS_READ)?;
        let query = request.query()?;
        let rows = self
            .ports
            .requesters
            .list(principal.scope(), &query)
            .await?;
        page_by_name(rows, query.limit, |r| {
            (r.full_name().as_str().to_lowercase(), r.id().as_uuid())
        })
    }

    /// Loads a Requester in the caller's scope.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get_requester(
        &self,
        principal: &Principal,
        id: RequesterId,
    ) -> Result<Requester, AppError> {
        require(principal, capabilities::REQUESTERS_READ)?;
        self.ports
            .requesters
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Requester"))
    }

    /// Creates a Requester in `tenant_id`.
    ///
    /// # Errors
    ///
    /// Forbidden, Company not visible, cross-Company references, validation or conflict.
    pub async fn create_requester(
        &self,
        principal: &Principal,
        tenant_id: TenantId,
        input: RequesterInput,
    ) -> Result<Requester, AppError> {
        require(principal, capabilities::REQUESTERS_MANAGE)?;
        self.get_company(principal, tenant_id).await?;
        self.check_requester_references(principal, tenant_id, None, &input)
            .await?;
        let now = self.ports.clock.now();
        let requester = Requester::create(tenant_id, input.into(), now)?;
        let audit = audit_record(
            None,
            &requester,
            AuditAction::Created,
            principal.actor(),
            now,
        );
        self.ports
            .requesters
            .insert(principal.scope(), &requester, &audit)
            .await?;
        Ok(requester)
    }

    /// Updates a Requester.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, Manager cycle, cross-Company references, validation or
    /// conflict.
    pub async fn update_requester(
        &self,
        principal: &Principal,
        id: RequesterId,
        input: RequesterInput,
    ) -> Result<Requester, AppError> {
        require(principal, capabilities::REQUESTERS_MANAGE)?;
        let mut requester = self.get_requester(principal, id).await?;
        self.check_requester_references(principal, requester.tenant_id(), Some(id), &input)
            .await?;
        let before = requester.clone();
        let now = self.ports.clock.now();
        requester.update(input.into(), now)?;
        self.save_requester(principal, &before, &requester).await?;
        Ok(requester)
    }

    /// Activates or deactivates a Requester. Deactivation is timestamped.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or already in that state.
    pub async fn set_requester_active(
        &self,
        principal: &Principal,
        id: RequesterId,
        active: bool,
    ) -> Result<Requester, AppError> {
        require(principal, capabilities::REQUESTERS_MANAGE)?;
        let mut requester = self.get_requester(principal, id).await?;
        let before = requester.clone();
        let now = self.ports.clock.now();
        if active {
            requester.reactivate(now)?;
        } else {
            requester.deactivate(now)?;
        }
        self.save_requester(principal, &before, &requester).await?;
        Ok(requester)
    }

    async fn save_requester(
        &self,
        principal: &Principal,
        before: &Requester,
        after: &Requester,
    ) -> Result<(), AppError> {
        let audit = audit_record(
            Some(before),
            after,
            AuditAction::Updated,
            principal.actor(),
            self.ports.clock.now(),
        );
        if audit.is_meaningful() {
            self.ports
                .requesters
                .update(principal.scope(), after, &audit)
                .await?;
        }
        Ok(())
    }

    async fn check_requester_references(
        &self,
        principal: &Principal,
        tenant_id: TenantId,
        id: Option<RequesterId>,
        input: &RequesterInput,
    ) -> Result<(), AppError> {
        if let Some(manager_id) = input.manager_id {
            if Some(manager_id) == id {
                return Err(IdentityError::ManagerCycle.into());
            }
            let manager = self
                .ports
                .requesters
                .get(principal.scope(), manager_id)
                .await?
                .ok_or(AppError::Invalid {
                    field: "manager_id",
                    reason: "does not exist".into(),
                })?;
            if manager.tenant_id() != tenant_id {
                return Err(IdentityError::CrossCompanyReference("the Manager").into());
            }
            if let Some(id) = id {
                let chain = self
                    .ports
                    .requesters
                    .manager_chain(principal.scope(), manager_id, MAX_MANAGER_DEPTH)
                    .await?;
                if chain.contains(&id) {
                    return Err(IdentityError::ManagerCycle.into());
                }
            }
        }
        if let Some(department_id) = input.department_id {
            self.ports
                .departments
                .get(department_id)
                .await?
                .ok_or(AppError::Invalid {
                    field: "department_id",
                    reason: "does not exist".into(),
                })?;
        }
        if let Some(cc_id) = input.cost_centre_id {
            let cc = self
                .ports
                .cost_centres
                .get(principal.scope(), cc_id)
                .await?
                .ok_or(AppError::Invalid {
                    field: "cost_centre_id",
                    reason: "does not exist".into(),
                })?;
            if cc.tenant_id() != tenant_id {
                return Err(IdentityError::CrossCompanyReference("the Cost Centre").into());
            }
        }
        Ok(())
    }

    /// Requesters with this email address in the caller's scope (email intake).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn requesters_by_email(
        &self,
        principal: &Principal,
        email: &str,
    ) -> Result<Vec<Requester>, AppError> {
        require(principal, capabilities::REQUESTERS_READ)?;
        Ok(self
            .ports
            .requesters
            .find_by_email(principal.scope(), email.trim())
            .await?)
    }

    /// The Company whose Sender Domains include `domain`; never a Freemail Domain
    /// (FR-EML-004).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn company_for_sender_domain(
        &self,
        domain: &str,
    ) -> Result<Option<TenantId>, AppError> {
        Ok(self
            .ports
            .companies
            .find_by_sender_domain(domain.trim())
            .await?)
    }

    // ----- Cost Centres ---------------------------------------------------------------

    /// Lists Cost Centres in the caller's scope.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid cursor or storage failures.
    pub async fn list_cost_centres(
        &self,
        principal: &Principal,
        request: &ListRequest,
    ) -> Result<Page<CostCentre>, AppError> {
        require(principal, capabilities::REQUESTERS_READ)?;
        let query = request.query()?;
        let rows = self
            .ports
            .cost_centres
            .list(principal.scope(), &query)
            .await?;
        page_by_name(rows, query.limit, |c| {
            (c.name().as_str().to_lowercase(), c.id().as_uuid())
        })
    }

    /// Loads a Cost Centre.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get_cost_centre(
        &self,
        principal: &Principal,
        id: CostCentreId,
    ) -> Result<CostCentre, AppError> {
        require(principal, capabilities::REQUESTERS_READ)?;
        self.ports
            .cost_centres
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Cost Centre"))
    }

    /// Creates a Cost Centre in `tenant_id`.
    ///
    /// # Errors
    ///
    /// Forbidden, Company not visible or conflict.
    pub async fn create_cost_centre(
        &self,
        principal: &Principal,
        tenant_id: TenantId,
        input: CostCentreInput,
    ) -> Result<CostCentre, AppError> {
        require(principal, capabilities::REQUESTERS_MANAGE)?;
        self.get_company(principal, tenant_id).await?;
        let now = self.ports.clock.now();
        let mut cc = CostCentre::create(tenant_id, input.code.clone(), input.name.clone(), now);
        if !input.is_active {
            cc.update(input.code, input.name, false, now);
        }
        let audit = audit_record(None, &cc, AuditAction::Created, principal.actor(), now);
        self.ports
            .cost_centres
            .insert(principal.scope(), &cc, &audit)
            .await?;
        Ok(cc)
    }

    /// Updates a Cost Centre.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or conflict.
    pub async fn update_cost_centre(
        &self,
        principal: &Principal,
        id: CostCentreId,
        input: CostCentreInput,
    ) -> Result<CostCentre, AppError> {
        require(principal, capabilities::REQUESTERS_MANAGE)?;
        let mut cc = self.get_cost_centre(principal, id).await?;
        let before = cc.clone();
        let now = self.ports.clock.now();
        cc.update(input.code, input.name, input.is_active, now);
        let audit = audit_record(
            Some(&before),
            &cc,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports
                .cost_centres
                .update(principal.scope(), &cc, &audit)
                .await?;
        }
        Ok(cc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::fakes::{Fakes, setup_input};
    use crate::application::{CompanyInput, ListRequest};
    use cooper_kernel::{Actor, AnalystId, Capability, TenantScope};
    use std::collections::BTreeSet;

    fn input(name: &str, manager: Option<RequesterId>) -> RequesterInput {
        RequesterInput {
            full_name: Name::try_from(name).unwrap(),
            email: EmailAddress::try_from(format!("{name}@acme.example")).unwrap(),
            username: None,
            phone: None,
            manager_id: manager,
            cost_centre_id: None,
            department_id: None,
        }
    }

    #[tokio::test]
    async fn requesters_are_isolated_by_company() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let root = fakes.sign_in_admin(&svc).await.principal;
        let other = svc
            .create_company(
                &root,
                CompanyInput {
                    name: Name::try_from("Other").unwrap(),
                    ticket_code: crate::domain::TicketCode::try_from("OTH").unwrap(),
                    sender_domains: BTreeSet::new(),
                },
            )
            .await
            .unwrap();
        let default_id = fakes.default_company_id();
        let a = svc
            .create_requester(&root, default_id, input("alice", None))
            .await
            .unwrap();
        let b = svc
            .create_requester(&root, other.id(), input("bob", None))
            .await
            .unwrap();

        let caps: BTreeSet<Capability> = ["requesters.read", "requesters.manage"]
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        let scoped = Principal::new(
            Actor::Analyst(AnalystId::new()),
            false,
            caps,
            TenantScope::single(default_id),
        );
        let page = svc
            .list_requesters(&scoped, &ListRequest::default())
            .await
            .unwrap();
        assert_eq!(
            page.items.iter().map(Requester::id).collect::<Vec<_>>(),
            vec![a.id()]
        );
        assert_eq!(
            svc.get_requester(&scoped, b.id()).await.unwrap_err(),
            AppError::NotFound("Requester")
        );
        assert_eq!(
            svc.create_requester(&scoped, other.id(), input("eve", None))
                .await
                .unwrap_err(),
            AppError::NotFound("Company")
        );
        // A Manager from another Company is rejected even for an all-Companies caller.
        assert_eq!(
            svc.create_requester(&root, default_id, input("carol", Some(b.id())))
                .await
                .unwrap_err(),
            AppError::Rule(IdentityError::CrossCompanyReference("the Manager"))
        );
    }

    #[tokio::test]
    async fn manager_cycles_are_rejected() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let root = fakes.sign_in_admin(&svc).await.principal;
        let t = fakes.default_company_id();
        let top = svc
            .create_requester(&root, t, input("top", None))
            .await
            .unwrap();
        let mid = svc
            .create_requester(&root, t, input("mid", Some(top.id())))
            .await
            .unwrap();
        let low = svc
            .create_requester(&root, t, input("low", Some(mid.id())))
            .await
            .unwrap();
        assert_eq!(
            svc.update_requester(&root, top.id(), input("top", Some(low.id())))
                .await
                .unwrap_err(),
            AppError::Rule(IdentityError::ManagerCycle)
        );
        let deactivated = svc
            .set_requester_active(&root, low.id(), false)
            .await
            .unwrap();
        assert!(deactivated.deactivated_at().is_some());
    }
}
