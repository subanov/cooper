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

    async fn requester_contact(
        &self,
        scope: &TenantScope,
        id: RequesterId,
    ) -> Result<Option<cooper_service_desk::domain::RequesterContact>, RepoError> {
        Ok(found!(
            self.identity
                .get_requester(&system(scope.clone()), id)
                .await
        )
        .map(|r| cooper_service_desk::domain::RequesterContact {
            name: r.full_name().to_string(),
            email: r.email().to_string(),
        }))
    }

    async fn analyst_profile(
        &self,
        id: AnalystId,
    ) -> Result<Option<cooper_service_desk::domain::AnalystProfile>, RepoError> {
        Ok(
            found!(self.identity.get_analyst(&admin(), id).await).map(|a| {
                cooper_service_desk::domain::AnalystProfile {
                    name: a.full_name().to_string(),
                    signature: a.signature().to_owned(),
                }
            }),
        )
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
            from_address: value(cooper_notifications::email::settings::FROM_ADDRESS.key)
                .await
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or(defaults.from_address),
            max_attachment_bytes: value(desk_settings::MAX_ATTACHMENT_MB.key)
                .await
                .and_then(|v| v.as_u64())
                .map_or(defaults.max_attachment_bytes, |mb| mb * 1024 * 1024),
            sla_warning_percent: value(desk_settings::SLA_WARNING_PERCENT.key)
                .await
                .and_then(|v| v.as_u64())
                .and_then(|v| u8::try_from(v).ok())
                .unwrap_or(defaults.sla_warning_percent),
        }
    }
}

/// The notifications context's view of tickets.
pub struct DeskTickets {
    pub desk: cooper_service_desk::application::ServiceDesk,
}

#[async_trait]
impl cooper_notifications::handlers::TicketLookup for DeskTickets {
    async fn ticket(
        &self,
        id: cooper_kernel::TicketId,
    ) -> Result<Option<cooper_notifications::handlers::TicketFacts>, String> {
        match self.desk.ticket(&admin(), id).await {
            Ok(d) if d.ticket.deleted_at().is_none() => {
                Ok(Some(cooper_notifications::handlers::TicketFacts {
                    number: d.ticket.number().to_owned(),
                    subject: d.ticket.details().subject.clone(),
                    tenant_id: d.ticket.tenant_id(),
                    assignee: d.ticket.assigned_analyst_id(),
                    team: d.ticket.assigned_team_id(),
                }))
            }
            Ok(_) | Err(cooper_service_desk::application::AppError::NotFound(_)) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    async fn sla_recipients(
        &self,
        trigger: &str,
        target: &str,
        assignee: Option<AnalystId>,
        team_members: &BTreeSet<AnalystId>,
    ) -> Result<BTreeSet<AnalystId>, String> {
        use cooper_service_desk::domain::{SlaTarget, SlaTrigger};
        let trigger = if trigger == "breach" {
            SlaTrigger::Breach
        } else {
            SlaTrigger::Warning
        };
        let target = if target == "response" {
            SlaTarget::Response
        } else {
            SlaTarget::Resolution
        };
        self.desk
            .sla_recipients(trigger, target, assignee, team_members)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait]
impl cooper_notifications::handlers::PeopleLookup for IdentityDirectory {
    async fn team_members(&self, team: TeamId) -> Result<BTreeSet<AnalystId>, String> {
        Ok(Directory::team_members(self, team)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_default())
    }

    async fn analysts_by_username(
        &self,
        usernames: &[String],
    ) -> Result<BTreeSet<AnalystId>, String> {
        Ok(self
            .identity
            .active_analysts_by_username(usernames)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect())
    }
}

/// Sends password reset emails through the outbound email queue.
pub struct OutboxResetMailer {
    pub outbox: cooper_notifications::email::EmailOutbox,
    pub admin: AdministrationService,
}

#[async_trait]
impl cooper_identity::domain::ResetMailer for OutboxResetMailer {
    async fn send_reset(
        &self,
        to: &str,
        name: &str,
        token: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), cooper_identity::domain::RepoError> {
        let text = |key: &'static str, default: &str| {
            let admin = self.admin.clone();
            let default = default.to_owned();
            async move {
                admin
                    .setting_value(key)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or(default)
            }
        };
        let from = text(
            cooper_notifications::email::settings::FROM_ADDRESS.key,
            "Cooper <cooper@localhost>",
        )
        .await;
        let base = text(
            cooper_administration::application::settings::BASE_URL.key,
            "http://localhost:8080",
        )
        .await;
        let instance = text(
            cooper_administration::application::settings::INSTANCE_NAME.key,
            "Cooper",
        )
        .await;
        let id = uuid::Uuid::now_v7();
        let domain = from
            .rsplit('@')
            .next()
            .unwrap_or("localhost")
            .trim_end_matches('>')
            .to_owned();
        let email = cooper_notifications::email::OutboundEmail {
            id,
            tenant_id: None,
            kind: "password_reset".into(),
            from,
            to: vec![to.to_owned()],
            cc: Vec::new(),
            subject: format!("{instance}: reset your password"),
            text_body: format!(
                "Hello {name},\n\nSomeone asked to reset your {instance} password. If it was you, open this link before {expires_at} (UTC):\n\n{}/reset-password?token={token}\n\nIf it was not you, ignore this email; your password stays the same.\n",
                base.trim_end_matches('/')
            ),
            message_id: format!("<{id}@{domain}>"),
            in_reply_to: None,
            references: Vec::new(),
        };
        self.outbox
            .queue(&email, chrono::Utc::now())
            .await
            .map(|_| ())
            .map_err(|e| cooper_identity::domain::RepoError::Storage(e.to_string()))
    }
}
