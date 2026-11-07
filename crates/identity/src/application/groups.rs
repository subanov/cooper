//! User Groups (FR-IAM-016) and Manager Grants (FR-IAM-015, FR-PRT-004).

use std::collections::BTreeSet;

use cooper_kernel::{
    Actor, AuditAction, AuditRecord, Principal, RequesterId, TenantId, TenantScope,
};
use uuid::Uuid;

use super::{AppError, IdentityService, capabilities};
use crate::domain::{
    GrantKind, IdentityError, ManagerGrant, Member, Membership, UserGroup, audit_record,
    managed_requesters,
};

/// Which Requesters' tickets a Portal manager may read (FR-PRT-004).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedScope {
    /// The covered Requesters (never the manager).
    pub requester_ids: BTreeSet<RequesterId>,
    /// Whether their Confidential tickets are included.
    pub include_confidential: bool,
}

impl IdentityService {
    // --- User Groups ---------------------------------------------------------------------

    /// Every User Group (needs `groups.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn user_groups(&self, principal: &Principal) -> Result<Vec<UserGroup>, AppError> {
        principal.require(capabilities::GROUPS_MANAGE)?;
        Ok(self.ports.groups.groups().await?)
    }

    async fn user_group(&self, id: Uuid) -> Result<UserGroup, AppError> {
        self.ports
            .groups
            .groups()
            .await?
            .into_iter()
            .find(|g| g.id() == id)
            .ok_or(AppError::NotFound("User Group"))
    }

    /// Creates a User Group (needs `groups.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_user_group(
        &self,
        principal: &Principal,
        name: &str,
        description: &str,
    ) -> Result<UserGroup, AppError> {
        principal.require(capabilities::GROUPS_MANAGE)?;
        let now = self.ports.clock.now();
        let group = UserGroup::create(name, description, now)?;
        let audit = audit_record(None, &group, AuditAction::Created, principal.actor(), now);
        self.ports.groups.save_group(&group, &audit).await?;
        Ok(group)
    }

    /// Changes a User Group; an inactive group grants nothing (needs `groups.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_user_group(
        &self,
        principal: &Principal,
        id: Uuid,
        name: &str,
        description: &str,
        is_active: bool,
    ) -> Result<UserGroup, AppError> {
        principal.require(capabilities::GROUPS_MANAGE)?;
        let mut group = self.user_group(id).await?;
        let before = group.clone();
        let now = self.ports.clock.now();
        group.update(name, description, is_active, now)?;
        let audit = audit_record(
            Some(&before),
            &group,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        self.ports.groups.save_group(&group, &audit).await?;
        Ok(group)
    }

    /// The members of a User Group, expired ones included (needs `groups.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn user_group_members(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<Membership>, AppError> {
        principal.require(capabilities::GROUPS_MANAGE)?;
        self.user_group(id).await?;
        Ok(self.ports.groups.members(id).await?)
    }

    /// Replaces the members of a User Group (needs `groups.manage`). Requesters must be
    /// in a Company the caller can see.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, unknown members or duplicates.
    pub async fn set_user_group_members(
        &self,
        principal: &Principal,
        id: Uuid,
        members: Vec<Membership>,
    ) -> Result<Vec<Membership>, AppError> {
        principal.require(capabilities::GROUPS_MANAGE)?;
        self.user_group(id).await?;
        let mut seen = BTreeSet::new();
        for m in &members {
            if !seen.insert(m.member) {
                return Err(IdentityError::invalid("members", "list each member once").into());
            }
            let exists = match m.member {
                Member::Analyst(a) => self.ports.analysts.get(a).await?.is_some(),
                Member::Requester(r) => self
                    .ports
                    .requesters
                    .get(principal.scope(), r)
                    .await?
                    .is_some(),
            };
            if !exists {
                return Err(IdentityError::invalid(
                    "members",
                    "refers to an unknown Analyst or Requester",
                )
                .into());
            }
        }
        let now = self.ports.clock.now();
        let old = self.ports.groups.members(id).await?;
        let describe = |list: &[Membership]| -> Vec<String> {
            list.iter()
                .map(|m| format!("{}:{}", m.member.kind(), m.member.id()))
                .collect()
        };
        let audit = AuditRecord::new(
            None,
            "user_group",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("members", Some(&describe(&old)), Some(&describe(&members)));
        self.ports
            .groups
            .set_members(id, &members, &audit, now)
            .await?;
        Ok(self.ports.groups.members(id).await?)
    }

    // --- Manager Grants -----------------------------------------------------------------

    async fn requester_tenant(
        &self,
        principal: &Principal,
        id: RequesterId,
    ) -> Result<TenantId, AppError> {
        Ok(self
            .ports
            .requesters
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Requester"))?
            .tenant_id())
    }

    /// The Manager Grants of a Requester (needs `requesters.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn manager_grants(
        &self,
        principal: &Principal,
        manager: RequesterId,
    ) -> Result<Vec<ManagerGrant>, AppError> {
        principal.require(capabilities::REQUESTERS_READ)?;
        let tenant = self.requester_tenant(principal, manager).await?;
        Ok(self
            .ports
            .groups
            .grants(tenant)
            .await?
            .into_iter()
            .filter(|g| g.manager_id() == manager)
            .collect())
    }

    /// Adds a Manager Grant or exclusion (needs `requesters.manage`). The target must be
    /// in the manager's Company.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a target in another Company, or a duplicate.
    pub async fn add_manager_grant(
        &self,
        principal: &Principal,
        manager: RequesterId,
        kind: GrantKind,
        is_exclusion: bool,
    ) -> Result<ManagerGrant, AppError> {
        principal.require(capabilities::REQUESTERS_MANAGE)?;
        let tenant = self.requester_tenant(principal, manager).await?;
        let same = TenantScope::single(tenant);
        let ok = match kind {
            GrantKind::Everyone | GrantKind::Reports => true,
            GrantKind::Requester(r) => self.ports.requesters.get(&same, r).await?.is_some(),
            GrantKind::CostCentre(c) => self.ports.cost_centres.get(&same, c).await?.is_some(),
            GrantKind::Group(g) => self
                .ports
                .groups
                .groups()
                .await?
                .iter()
                .any(|x| x.id() == g),
        };
        if !ok {
            return Err(IdentityError::CrossCompanyReference("the grant's target").into());
        }
        let now = self.ports.clock.now();
        let grant = ManagerGrant::new(tenant, manager, kind, is_exclusion, principal.actor(), now)?;
        let audit = audit_record(None, &grant, AuditAction::Created, principal.actor(), now);
        self.ports.groups.add_grant(&grant, &audit).await?;
        Ok(grant)
    }

    /// Removes a Manager Grant (needs `requesters.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn remove_manager_grant(
        &self,
        principal: &Principal,
        manager: RequesterId,
        id: Uuid,
    ) -> Result<(), AppError> {
        principal.require(capabilities::REQUESTERS_MANAGE)?;
        let grant = self
            .manager_grants(principal, manager)
            .await?
            .into_iter()
            .find(|g| g.id() == id)
            .ok_or(AppError::NotFound("Manager Grant"))?;
        let now = self.ports.clock.now();
        let audit = audit_record(
            Some(&grant),
            &grant,
            AuditAction::Deleted,
            principal.actor(),
            now,
        );
        self.ports.groups.remove_grant(&grant, &audit).await?;
        Ok(())
    }

    /// The active User Groups an Analyst or Requester currently belongs to (expired
    /// memberships do not count). For other contexts' permission checks, such as
    /// Knowledge ACLs; reveals nothing to the caller.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn groups_of(
        &self,
        member: crate::domain::Member,
    ) -> Result<BTreeSet<Uuid>, AppError> {
        Ok(self
            .ports
            .groups
            .groups_of(member, self.ports.clock.now())
            .await?)
    }

    /// The Requesters whose tickets the signed-in Portal Requester may read as a
    /// manager. Empty unless Managers are switched on (fail closed).
    ///
    /// # Errors
    ///
    /// Not a Portal principal, or storage failures.
    pub async fn managed_scope(&self, principal: &Principal) -> Result<ManagedScope, AppError> {
        let Actor::Requester(me) = principal.actor() else {
            return Err(cooper_kernel::Forbidden("requester".into()).into());
        };
        let settings = self.ports.settings.managers().await;
        if !settings.enabled {
            return Ok(ManagedScope::default());
        }
        let tenant = self.requester_tenant(principal, me).await?;
        let company = self.ports.groups.company_requesters(tenant).await?;
        let Some(manager) = company.iter().find(|r| r.id == me).copied() else {
            return Ok(ManagedScope::default());
        };
        let grants = self.ports.groups.grants(tenant).await?;
        let groups = self
            .ports
            .groups
            .requester_members(self.ports.clock.now())
            .await?;
        Ok(ManagedScope {
            requester_ids: managed_requesters(&manager, settings, &grants, &company, &groups),
            include_confidential: settings.include_confidential,
        })
    }
}
