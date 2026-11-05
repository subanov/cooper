//! Adapters connecting bounded contexts through their public application APIs
//! (AGENTS.md §3: contexts never read each other's tables).

use std::collections::BTreeSet;

use async_trait::async_trait;
use cooper_administration::application::AdministrationService;
use cooper_identity::application::{AppError as IdentityError, IdentityService, ListRequest};
use cooper_kernel::{
    Actor, AnalystId, Capability, DepartmentId, Limit, Principal, RequesterId, TeamId, TenantId,
    TenantScope,
};
use cooper_service_desk::application::settings as desk_settings;
use cooper_service_desk::domain::{
    DepartmentInfo, Directory, RepoError, TicketSettings, TicketSettingsProvider,
};

/// The service desk's view of the identity context.
pub struct IdentityDirectory {
    pub identity: IdentityService,
}

fn system(scope: TenantScope) -> Principal {
    let caps = ["requesters.read"]
        .iter()
        .filter_map(|c| Capability::from_static(c))
        .collect();
    Principal::new(Actor::Automation, false, caps, scope)
}

fn admin() -> Principal {
    Principal::automation()
}

fn map(err: IdentityError) -> Result<(), RepoError> {
    match err {
        IdentityError::NotFound(_) => Ok(()),
        other => Err(RepoError::Storage(other.to_string())),
    }
}

macro_rules! found {
    ($e:expr) => {
        match $e {
            Ok(v) => Some(v),
            Err(err) => {
                map(err)?;
                None
            }
        }
    };
}

impl IdentityDirectory {
    async fn all_teams(&self) -> Result<Vec<cooper_identity::domain::Team>, RepoError> {
        let mut teams = Vec::new();
        let mut request = ListRequest {
            limit: Limit::new(Limit::MAX).unwrap_or_default(),
            ..ListRequest::default()
        };
        loop {
            let page = self
                .identity
                .list_teams(&admin(), &request)
                .await
                .map_err(|e| RepoError::Storage(e.to_string()))?;
            teams.extend(page.items);
            match page.next_cursor {
                Some(c) => request.cursor = Some(c),
                None => return Ok(teams),
            }
        }
    }
}

#[async_trait]
impl Directory for IdentityDirectory {
    async fn requester_company(
        &self,
        scope: &TenantScope,
        id: RequesterId,
    ) -> Result<Option<TenantId>, RepoError> {
        Ok(found!(
            self.identity
                .get_requester(&system(scope.clone()), id)
                .await
        )
        .map(|r| r.tenant_id()))
    }

    async fn department(&self, id: DepartmentId) -> Result<Option<DepartmentInfo>, RepoError> {
        Ok(
            found!(self.identity.get_department(&admin(), id).await).map(|d| DepartmentInfo {
                default_sensitivity: d.default_sensitivity(),
                is_active: d.is_active(),
            }),
        )
    }

    async fn team_members(&self, id: TeamId) -> Result<Option<BTreeSet<AnalystId>>, RepoError> {
        Ok(found!(self.identity.get_team(&admin(), id).await)
            .filter(|t| t.is_active())
            .map(|t| t.members().clone()))
    }

    async fn analyst_is_active(&self, id: AnalystId) -> Result<bool, RepoError> {
        Ok(found!(self.identity.get_analyst(&admin(), id).await).is_some_and(|a| a.is_active()))
    }

    async fn departments_served_by(
        &self,
        analyst: AnalystId,
    ) -> Result<BTreeSet<DepartmentId>, RepoError> {
        let teams = self.teams_of(analyst).await?;
        if teams.is_empty() {
            return Ok(BTreeSet::new());
        }
        let mut departments = BTreeSet::new();
        let mut request = ListRequest {
            limit: Limit::new(Limit::MAX).unwrap_or_default(),
            ..ListRequest::default()
        };
        loop {
            let page = self
                .identity
                .list_departments(&admin(), &request)
                .await
                .map_err(|e| RepoError::Storage(e.to_string()))?;
            departments.extend(
                page.items
                    .iter()
                    .filter(|d| d.teams().iter().any(|t| teams.contains(t)))
                    .map(|d| d.id()),
            );
            match page.next_cursor {
                Some(c) => request.cursor = Some(c),
                None => return Ok(departments),
            }
        }
    }

    async fn teams_of(&self, analyst: AnalystId) -> Result<BTreeSet<TeamId>, RepoError> {
        Ok(self
            .all_teams()
            .await?
            .into_iter()
            .filter(|t| t.is_active() && t.members().contains(&analyst))
            .map(|t| t.id())
            .collect())
    }

    async fn ticket_code(
        &self,
        scope: &TenantScope,
        tenant: TenantId,
    ) -> Result<Option<String>, RepoError> {
        Ok(found!(
            self.identity
                .get_company(&system(scope.clone()), tenant)
                .await
        )
        .filter(|c| c.is_active())
        .map(|c| c.ticket_code().to_string()))
    }
}

/// The service desk's settings, read from System Settings.
pub struct DeskSettings {
    pub admin: AdministrationService,
}

#[async_trait]
impl TicketSettingsProvider for DeskSettings {
    async fn settings(&self) -> TicketSettings {
        let defaults = TicketSettings::default();
        let value = |key: &'static str| {
            let admin = self.admin.clone();
            async move { admin.setting_value(key).await.ok().flatten() }
        };
        TicketSettings {
            number_format: value(desk_settings::NUMBER_FORMAT.key)
                .await
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or(defaults.number_format),
            require_closure_category: value(desk_settings::REQUIRE_CLOSURE_CATEGORY.key)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.require_closure_category),
            assignee_must_be_team_member: value(desk_settings::ASSIGNEE_MUST_BE_TEAM_MEMBER.key)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.assignee_must_be_team_member),
        }
    }
}
