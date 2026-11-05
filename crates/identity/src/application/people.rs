//! Analyst, Team, Department and Role use cases (FR-IAM-001, FR-IAM-006, FR-IAM-007,
//! FR-IAM-009, FR-IAM-010).
//!
//! Granting rules prevent privilege escalation: only Administrators can create
//! Administrators, and a non-Administrator can only grant Capabilities they hold
//! themselves and Company access within their own scope.

use std::collections::BTreeSet;

use cooper_kernel::{
    AnalystId, AuditAction, Capability, DepartmentId, Forbidden, Page, Principal, Sensitivity,
    TeamId, TenantScope,
};

use super::{
    AppError, IdentityService, ListRequest, acting_analyst, capabilities, page_by_name, require,
    require_staff,
};
use crate::domain::{
    Analyst, CompanyAccess, Department, EmailAddress, IdentityError, Name, NewAnalyst, Password,
    Revocations, Role, RoleId, Team, Username, audit_record,
};

/// Editable fields of an Analyst.
#[derive(Debug, Clone)]
pub struct AnalystInput {
    /// Sign-in name.
    pub username: Username,
    /// Email.
    pub email: EmailAddress,
    /// Display name.
    pub full_name: Name,
    /// Administrator flag.
    pub is_admin: bool,
}

/// Editable fields of a Team.
#[derive(Debug, Clone)]
pub struct TeamInput {
    /// Name.
    pub name: Name,
    /// Description.
    pub description: String,
    /// Active flag.
    pub is_active: bool,
}

/// Editable fields of a Department.
#[derive(Debug, Clone)]
pub struct DepartmentInput {
    /// Name.
    pub name: Name,
    /// Default Sensitivity of its tickets.
    pub default_sensitivity: Sensitivity,
    /// Active flag.
    pub is_active: bool,
    /// Teams that serve it.
    pub team_ids: BTreeSet<TeamId>,
}

/// Editable fields of a Role.
#[derive(Debug, Clone)]
pub struct RoleInput {
    /// Name.
    pub name: Name,
    /// Description.
    pub description: String,
    /// Capabilities.
    pub capabilities: BTreeSet<Capability>,
}

fn forbidden(what: &str) -> AppError {
    AppError::Forbidden(Forbidden(what.to_owned()))
}

/// A non-Administrator may only grant Capabilities they hold.
fn ensure_can_grant_capabilities(
    principal: &Principal,
    granted: &BTreeSet<Capability>,
) -> Result<(), AppError> {
    match granted.iter().find(|c| !principal.can(c.as_str())) {
        Some(c) => Err(forbidden(c.as_str())),
        None => Ok(()),
    }
}

/// A non-Administrator may only grant Company access within their own scope.
fn ensure_can_grant_access(principal: &Principal, access: &CompanyAccess) -> Result<(), AppError> {
    let ok = match (principal.scope(), access) {
        (TenantScope::All, _) | (_, CompanyAccess::None) => true,
        (TenantScope::Tenants(_), CompanyAccess::All) => false,
        (scope, CompanyAccess::Companies(ids)) => ids.iter().all(|id| scope.allows(*id)),
    };
    if ok {
        Ok(())
    } else {
        Err(forbidden("access to all granted Companies"))
    }
}

impl IdentityService {
    // ----- Analysts ---------------------------------------------------------------

    /// Lists Analysts. Any signed-in staff member may, to assign work.
    ///
    /// # Errors
    ///
    /// Invalid cursor or storage failures.
    pub async fn list_analysts(
        &self,
        principal: &Principal,
        request: &ListRequest,
    ) -> Result<Page<Analyst>, AppError> {
        require_staff(principal)?;
        let query = request.query()?;
        let rows = self.ports.analysts.list(&query).await?;
        page_by_name(rows, query.limit, |a| {
            (a.full_name().as_str().to_lowercase(), a.id().as_uuid())
        })
    }

    /// Loads an Analyst.
    ///
    /// # Errors
    ///
    /// [`AppError::NotFound`].
    pub async fn get_analyst(
        &self,
        principal: &Principal,
        id: AnalystId,
    ) -> Result<Analyst, AppError> {
        require_staff(principal)?;
        self.ports
            .analysts
            .get(id)
            .await?
            .ok_or(AppError::NotFound("Analyst"))
    }

    /// Creates an Analyst, optionally with an initial password.
    ///
    /// # Errors
    ///
    /// Forbidden, validation or conflict errors.
    pub async fn create_analyst(
        &self,
        principal: &Principal,
        input: AnalystInput,
        password: Option<Password>,
    ) -> Result<Analyst, AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        if input.is_admin {
            principal.require_admin()?;
        }
        let now = self.ports.clock.now();
        let mut analyst = Analyst::create(
            NewAnalyst {
                username: input.username,
                email: input.email,
                full_name: input.full_name,
                is_admin: input.is_admin,
            },
            now,
        );
        if let Some(password) = password {
            analyst.set_password(self.ports.hasher.hash(&password).await?, now);
        }
        let audit = audit_record(None, &analyst, AuditAction::Created, principal.actor(), now);
        self.ports.analysts.insert(&analyst, &audit).await?;
        Ok(analyst)
    }

    /// Updates an Analyst's profile and Administrator flag.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, self-lockout, last Administrator, validation or conflict.
    pub async fn update_analyst(
        &self,
        principal: &Principal,
        id: AnalystId,
        input: AnalystInput,
    ) -> Result<Analyst, AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        let mut analyst = self.get_analyst(principal, id).await?;
        if input.is_admin != analyst.is_admin() || analyst.is_admin() {
            // Only Administrators may change Administrators or the flag itself.
            principal.require_admin()?;
        }
        if analyst.is_admin() && !input.is_admin {
            self.ensure_other_admin_remains(&analyst).await?;
        }
        let before = analyst.clone();
        let now = self.ports.clock.now();
        analyst.update(
            input.username,
            input.email,
            input.full_name,
            input.is_admin,
            acting_analyst(principal),
            now,
        )?;
        self.save_analyst(principal, &before, &analyst, false)
            .await?;
        Ok(analyst)
    }

    /// Activates or deactivates an Analyst. Deactivation ends their sessions.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, self-lockout or last Administrator.
    pub async fn set_analyst_active(
        &self,
        principal: &Principal,
        id: AnalystId,
        active: bool,
    ) -> Result<Analyst, AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        let mut analyst = self.get_analyst(principal, id).await?;
        if analyst.is_admin() {
            principal.require_admin()?;
        }
        let before = analyst.clone();
        let now = self.ports.clock.now();
        if active {
            analyst.reactivate(now)?;
        } else {
            analyst.deactivate(acting_analyst(principal), now)?;
            if analyst.is_admin() {
                self.ensure_other_admin_remains(&before).await?;
            }
        }
        self.save_analyst(principal, &before, &analyst, !active)
            .await?;
        Ok(analyst)
    }

    /// Sets an Analyst's password (administrative reset). Ends their sessions.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn set_analyst_password(
        &self,
        principal: &Principal,
        id: AnalystId,
        password: Password,
    ) -> Result<(), AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        let mut analyst = self.get_analyst(principal, id).await?;
        if analyst.is_admin() {
            principal.require_admin()?;
        }
        let before = analyst.clone();
        analyst.set_password(
            self.ports.hasher.hash(&password).await?,
            self.ports.clock.now(),
        );
        self.save_analyst(principal, &before, &analyst, true).await
    }

    /// Replaces the Roles assigned directly to an Analyst.
    ///
    /// # Errors
    ///
    /// Forbidden (including granting Capabilities the caller lacks), not found, or an
    /// unknown Role.
    pub async fn assign_analyst_roles(
        &self,
        principal: &Principal,
        id: AnalystId,
        roles: BTreeSet<RoleId>,
    ) -> Result<Analyst, AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        self.ensure_can_assign_roles(principal, &roles).await?;
        let mut analyst = self.get_analyst(principal, id).await?;
        let before = analyst.clone();
        analyst.assign_roles(roles, self.ports.clock.now());
        self.save_analyst(principal, &before, &analyst, false)
            .await?;
        Ok(analyst)
    }

    /// Replaces the Company access granted directly to an Analyst.
    ///
    /// # Errors
    ///
    /// Forbidden (including Companies outside the caller's scope) or not found.
    pub async fn grant_analyst_company_access(
        &self,
        principal: &Principal,
        id: AnalystId,
        access: CompanyAccess,
    ) -> Result<Analyst, AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        ensure_can_grant_access(principal, &access)?;
        let mut analyst = self.get_analyst(principal, id).await?;
        let before = analyst.clone();
        analyst.grant_company_access(access, self.ports.clock.now());
        self.save_analyst(principal, &before, &analyst, false)
            .await?;
        Ok(analyst)
    }

    async fn ensure_other_admin_remains(&self, admin: &Analyst) -> Result<(), AppError> {
        if admin.is_active() && self.ports.analysts.count_active_admins().await? <= 1 {
            return Err(IdentityError::LastAdministrator.into());
        }
        Ok(())
    }

    async fn ensure_can_assign_roles(
        &self,
        principal: &Principal,
        roles: &BTreeSet<RoleId>,
    ) -> Result<(), AppError> {
        for id in roles {
            let role = self
                .ports
                .roles
                .get(*id)
                .await?
                .ok_or_else(|| AppError::Invalid {
                    field: "role_ids",
                    reason: format!("Role {id} does not exist"),
                })?;
            ensure_can_grant_capabilities(principal, role.capabilities())?;
        }
        Ok(())
    }

    async fn save_analyst(
        &self,
        principal: &Principal,
        before: &Analyst,
        after: &Analyst,
        revoke_sessions: bool,
    ) -> Result<(), AppError> {
        let audit = audit_record(
            Some(before),
            after,
            AuditAction::Updated,
            principal.actor(),
            self.ports.clock.now(),
        );
        if audit.is_meaningful() || revoke_sessions {
            let revoke = if revoke_sessions {
                Revocations::ALL
            } else {
                Revocations::NONE
            };
            self.ports.analysts.update(after, &audit, revoke).await?;
        }
        Ok(())
    }

    // ----- Teams ------------------------------------------------------------------

    /// Lists Teams.
    ///
    /// # Errors
    ///
    /// Invalid cursor or storage failures.
    pub async fn list_teams(
        &self,
        principal: &Principal,
        request: &ListRequest,
    ) -> Result<Page<Team>, AppError> {
        require_staff(principal)?;
        let query = request.query()?;
        let rows = self.ports.teams.list(&query).await?;
        page_by_name(rows, query.limit, |t| {
            (t.name().as_str().to_lowercase(), t.id().as_uuid())
        })
    }

    /// Loads a Team.
    ///
    /// # Errors
    ///
    /// [`AppError::NotFound`].
    pub async fn get_team(&self, principal: &Principal, id: TeamId) -> Result<Team, AppError> {
        require_staff(principal)?;
        self.ports
            .teams
            .get(id)
            .await?
            .ok_or(AppError::NotFound("Team"))
    }

    /// Creates a Team.
    ///
    /// # Errors
    ///
    /// Forbidden, validation or conflict errors.
    pub async fn create_team(
        &self,
        principal: &Principal,
        input: TeamInput,
    ) -> Result<Team, AppError> {
        require(principal, capabilities::TEAMS_MANAGE)?;
        let now = self.ports.clock.now();
        let mut team = Team::create(input.name, &input.description, now)?;
        if !input.is_active {
            let name = team.name().clone();
            team.update(name, &input.description, false, now)?;
        }
        let audit = audit_record(None, &team, AuditAction::Created, principal.actor(), now);
        self.ports.teams.insert(&team, &audit).await?;
        Ok(team)
    }

    /// Updates a Team.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, validation or conflict errors.
    pub async fn update_team(
        &self,
        principal: &Principal,
        id: TeamId,
        input: TeamInput,
    ) -> Result<Team, AppError> {
        require(principal, capabilities::TEAMS_MANAGE)?;
        let mut team = self.get_team(principal, id).await?;
        let before = team.clone();
        team.update(
            input.name,
            &input.description,
            input.is_active,
            self.ports.clock.now(),
        )?;
        self.save_team(principal, &before, &team).await?;
        Ok(team)
    }

    /// Replaces a Team's members.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or an unknown Analyst.
    pub async fn set_team_members(
        &self,
        principal: &Principal,
        id: TeamId,
        members: BTreeSet<AnalystId>,
    ) -> Result<Team, AppError> {
        require(principal, capabilities::TEAMS_MANAGE)?;
        let mut team = self.get_team(principal, id).await?;
        // Joining a Team grants its Roles and access, so the caller must be able to grant them.
        let added: Vec<_> = members.difference(team.members()).collect();
        if !added.is_empty() {
            self.ensure_can_assign_roles(principal, team.roles())
                .await?;
            ensure_can_grant_access(principal, team.company_access())?;
        }
        let before = team.clone();
        team.set_members(members, self.ports.clock.now());
        self.save_team(principal, &before, &team).await?;
        Ok(team)
    }

    /// Replaces the Roles assigned to a Team.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or an unknown Role.
    pub async fn assign_team_roles(
        &self,
        principal: &Principal,
        id: TeamId,
        roles: BTreeSet<RoleId>,
    ) -> Result<Team, AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        self.ensure_can_assign_roles(principal, &roles).await?;
        let mut team = self.get_team(principal, id).await?;
        let before = team.clone();
        team.assign_roles(roles, self.ports.clock.now());
        self.save_team(principal, &before, &team).await?;
        Ok(team)
    }

    /// Replaces the Company access granted to a Team.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn grant_team_company_access(
        &self,
        principal: &Principal,
        id: TeamId,
        access: CompanyAccess,
    ) -> Result<Team, AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        ensure_can_grant_access(principal, &access)?;
        let mut team = self.get_team(principal, id).await?;
        let before = team.clone();
        team.grant_company_access(access, self.ports.clock.now());
        self.save_team(principal, &before, &team).await?;
        Ok(team)
    }

    async fn save_team(
        &self,
        principal: &Principal,
        before: &Team,
        after: &Team,
    ) -> Result<(), AppError> {
        let audit = audit_record(
            Some(before),
            after,
            AuditAction::Updated,
            principal.actor(),
            self.ports.clock.now(),
        );
        if audit.is_meaningful() {
            self.ports.teams.update(after, &audit).await?;
        }
        Ok(())
    }

    // ----- Departments --------------------------------------------------------------

    /// Lists Departments.
    ///
    /// # Errors
    ///
    /// Invalid cursor or storage failures.
    pub async fn list_departments(
        &self,
        principal: &Principal,
        request: &ListRequest,
    ) -> Result<Page<Department>, AppError> {
        require_staff(principal)?;
        let query = request.query()?;
        let rows = self.ports.departments.list(&query).await?;
        page_by_name(rows, query.limit, |d| {
            (d.name().as_str().to_lowercase(), d.id().as_uuid())
        })
    }

    /// Loads a Department.
    ///
    /// # Errors
    ///
    /// [`AppError::NotFound`].
    pub async fn get_department(
        &self,
        principal: &Principal,
        id: DepartmentId,
    ) -> Result<Department, AppError> {
        require_staff(principal)?;
        self.ports
            .departments
            .get(id)
            .await?
            .ok_or(AppError::NotFound("Department"))
    }

    /// Creates a Department.
    ///
    /// # Errors
    ///
    /// Forbidden, validation, unknown Team or conflict errors.
    pub async fn create_department(
        &self,
        principal: &Principal,
        input: DepartmentInput,
    ) -> Result<Department, AppError> {
        require(principal, capabilities::DEPARTMENTS_MANAGE)?;
        let now = self.ports.clock.now();
        let mut department = Department::create(
            input.name.clone(),
            input.default_sensitivity,
            input.team_ids.clone(),
            now,
        );
        if !input.is_active {
            department.update(
                input.name,
                input.default_sensitivity,
                false,
                input.team_ids,
                now,
            );
        }
        let audit = audit_record(
            None,
            &department,
            AuditAction::Created,
            principal.actor(),
            now,
        );
        self.ports.departments.insert(&department, &audit).await?;
        Ok(department)
    }

    /// Updates a Department.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, unknown Team or conflict errors.
    pub async fn update_department(
        &self,
        principal: &Principal,
        id: DepartmentId,
        input: DepartmentInput,
    ) -> Result<Department, AppError> {
        require(principal, capabilities::DEPARTMENTS_MANAGE)?;
        let mut department = self.get_department(principal, id).await?;
        let before = department.clone();
        let now = self.ports.clock.now();
        department.update(
            input.name,
            input.default_sensitivity,
            input.is_active,
            input.team_ids,
            now,
        );
        let audit = audit_record(
            Some(&before),
            &department,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports.departments.update(&department, &audit).await?;
        }
        Ok(department)
    }

    // ----- Roles --------------------------------------------------------------------

    /// Lists Roles.
    ///
    /// # Errors
    ///
    /// Invalid cursor or storage failures.
    pub async fn list_roles(
        &self,
        principal: &Principal,
        request: &ListRequest,
    ) -> Result<Page<Role>, AppError> {
        require_staff(principal)?;
        let query = request.query()?;
        let rows = self.ports.roles.list(&query).await?;
        page_by_name(rows, query.limit, |r| {
            (r.name().as_str().to_lowercase(), r.id().as_uuid())
        })
    }

    /// Loads a Role.
    ///
    /// # Errors
    ///
    /// [`AppError::NotFound`].
    pub async fn get_role(&self, principal: &Principal, id: RoleId) -> Result<Role, AppError> {
        require_staff(principal)?;
        self.ports
            .roles
            .get(id)
            .await?
            .ok_or(AppError::NotFound("Role"))
    }

    /// Creates a Role.
    ///
    /// # Errors
    ///
    /// Forbidden, unknown Capability or conflict errors.
    pub async fn create_role(
        &self,
        principal: &Principal,
        input: RoleInput,
    ) -> Result<Role, AppError> {
        require(principal, capabilities::ROLES_MANAGE)?;
        ensure_can_grant_capabilities(principal, &input.capabilities)?;
        let now = self.ports.clock.now();
        let role = Role::create(
            input.name,
            &input.description,
            input.capabilities,
            &self.catalogue,
            now,
        )?;
        let audit = audit_record(None, &role, AuditAction::Created, principal.actor(), now);
        self.ports.roles.insert(&role, &audit).await?;
        Ok(role)
    }

    /// Updates a Role. Everyone holding it gets the new Capabilities on their next request.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, unknown Capability or conflict errors.
    pub async fn update_role(
        &self,
        principal: &Principal,
        id: RoleId,
        input: RoleInput,
    ) -> Result<Role, AppError> {
        require(principal, capabilities::ROLES_MANAGE)?;
        let mut role = self.get_role(principal, id).await?;
        ensure_can_grant_capabilities(principal, role.capabilities())?;
        ensure_can_grant_capabilities(principal, &input.capabilities)?;
        let before = role.clone();
        let now = self.ports.clock.now();
        role.update(
            input.name,
            &input.description,
            input.capabilities,
            &self.catalogue,
            now,
        )?;
        let audit = audit_record(
            Some(&before),
            &role,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports.roles.update(&role, &audit).await?;
        }
        Ok(role)
    }

    /// Deletes a Role and its assignments.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn delete_role(&self, principal: &Principal, id: RoleId) -> Result<(), AppError> {
        require(principal, capabilities::ROLES_MANAGE)?;
        let role = self.get_role(principal, id).await?;
        ensure_can_grant_capabilities(principal, role.capabilities())?;
        let audit = audit_record(
            None,
            &role,
            AuditAction::Deleted,
            principal.actor(),
            self.ports.clock.now(),
        );
        self.ports.roles.delete(&role, &audit).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::fakes::{Fakes, setup_input};
    use cooper_kernel::{Actor, TenantId};

    fn cap(s: &str) -> Capability {
        s.parse().unwrap()
    }

    fn input(username: &str, is_admin: bool) -> AnalystInput {
        AnalystInput {
            username: Username::try_from(username).unwrap(),
            email: EmailAddress::try_from(format!("{username}@example.com")).unwrap(),
            full_name: Name::try_from(username).unwrap(),
            is_admin,
        }
    }

    async fn admin(fakes: &Fakes) -> (crate::application::IdentityService, Principal) {
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let p = fakes.sign_in_admin(&svc).await.principal;
        (svc, p)
    }

    fn manager(caps: &[&str], scope: TenantScope) -> Principal {
        Principal::new(
            Actor::Analyst(AnalystId::new()),
            false,
            caps.iter().map(|c| cap(c)).collect(),
            scope,
        )
    }

    #[tokio::test]
    async fn last_administrator_cannot_be_demoted_or_deactivated() {
        let fakes = Fakes::new();
        let (svc, root) = admin(&fakes).await;
        let other = svc
            .create_analyst(&root, input("other", true), None)
            .await
            .unwrap();
        // Two admins: demoting one is fine.
        svc.update_analyst(&root, other.id(), input("other", false))
            .await
            .unwrap();
        // Now root is the last admin; a second admin cannot remove root's flag.
        let second = svc
            .create_analyst(&root, input("second", true), None)
            .await
            .unwrap();
        svc.set_analyst_active(&root, second.id(), false)
            .await
            .unwrap();
        let p2 = Principal::new(
            Actor::Analyst(second.id()),
            true,
            BTreeSet::new(),
            TenantScope::All,
        );
        let root_id = crate::application::acting_analyst(&root).unwrap();
        assert_eq!(
            svc.set_analyst_active(&p2, root_id, false)
                .await
                .unwrap_err(),
            AppError::Rule(IdentityError::LastAdministrator)
        );
    }

    #[tokio::test]
    async fn non_administrators_cannot_escalate() {
        let fakes = Fakes::new();
        let (svc, root) = admin(&fakes).await;
        let role = svc
            .create_role(
                &root,
                RoleInput {
                    name: Name::try_from("Powerful").unwrap(),
                    description: String::new(),
                    capabilities: BTreeSet::from([cap("roles.manage"), cap("teams.manage")]),
                },
            )
            .await
            .unwrap();
        let target = svc
            .create_analyst(&root, input("target", false), None)
            .await
            .unwrap();
        let company = TenantId::new();
        let mgr = manager(&["analysts.manage"], TenantScope::single(company));

        // Cannot create an Administrator.
        assert!(matches!(
            svc.create_analyst(&mgr, input("boss", true), None).await,
            Err(AppError::Forbidden(_))
        ));
        // Cannot hand out Capabilities they do not hold.
        assert!(matches!(
            svc.assign_analyst_roles(&mgr, target.id(), BTreeSet::from([role.id()]))
                .await,
            Err(AppError::Forbidden(_))
        ));
        // Cannot grant all Companies, or Companies outside their scope.
        assert!(matches!(
            svc.grant_analyst_company_access(&mgr, target.id(), CompanyAccess::All)
                .await,
            Err(AppError::Forbidden(_))
        ));
        assert!(matches!(
            svc.grant_analyst_company_access(
                &mgr,
                target.id(),
                CompanyAccess::companies(BTreeSet::from([TenantId::new()]))
            )
            .await,
            Err(AppError::Forbidden(_))
        ));
        svc.grant_analyst_company_access(
            &mgr,
            target.id(),
            CompanyAccess::companies(BTreeSet::from([company])),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn roles_only_use_catalogue_capabilities_and_need_the_capability() {
        let fakes = Fakes::new();
        let (svc, root) = admin(&fakes).await;
        let err = svc
            .create_role(
                &root,
                RoleInput {
                    name: Name::try_from("Bad").unwrap(),
                    description: String::new(),
                    capabilities: BTreeSet::from([cap("tickets.teleport")]),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(
            err,
            AppError::Rule(IdentityError::UnknownCapability("tickets.teleport".into()))
        );
        let nobody = manager(&[], TenantScope::All);
        assert!(matches!(
            svc.create_team(
                &nobody,
                TeamInput {
                    name: Name::try_from("Desk").unwrap(),
                    description: String::new(),
                    is_active: true,
                }
            )
            .await,
            Err(AppError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn every_change_is_audited() {
        let fakes = Fakes::new();
        let (svc, root) = admin(&fakes).await;
        let before = fakes.state().audits.len();
        let team = svc
            .create_team(
                &root,
                TeamInput {
                    name: Name::try_from("Desk").unwrap(),
                    description: "First line".into(),
                    is_active: true,
                },
            )
            .await
            .unwrap();
        let a = svc
            .create_analyst(&root, input("member", false), None)
            .await
            .unwrap();
        svc.set_team_members(&root, team.id(), BTreeSet::from([a.id()]))
            .await
            .unwrap();
        // No-op update writes nothing.
        svc.update_team(
            &root,
            team.id(),
            TeamInput {
                name: Name::try_from("Desk").unwrap(),
                description: "First line".into(),
                is_active: true,
            },
        )
        .await
        .unwrap();
        let audits = &fakes.state().audits;
        assert_eq!(audits.len() - before, 3);
        assert_eq!(audits.last().unwrap().changes[0].field, "members");
    }
}
