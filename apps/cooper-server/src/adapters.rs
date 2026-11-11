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
                    email: a.email().to_string(),
                    is_active: a.is_active(),
                }
            }),
        )
    }

    async fn requesters_by_email(
        &self,
        email: &str,
    ) -> Result<Vec<(RequesterId, TenantId, bool)>, RepoError> {
        let found = self
            .identity
            .requesters_by_email(&system(TenantScope::All), email)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(found
            .into_iter()
            .map(|r| (r.id(), r.tenant_id(), r.is_active()))
            .collect())
    }

    async fn company_for_domain(&self, domain: &str) -> Result<Option<TenantId>, RepoError> {
        self.identity
            .company_for_sender_domain(domain)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))
    }

    async fn create_requester(
        &self,
        tenant: TenantId,
        name: &str,
        email: &str,
    ) -> Result<RequesterId, RepoError> {
        use cooper_identity::application::RequesterInput;
        use cooper_identity::domain::{EmailAddress, Name};
        let input = RequesterInput {
            full_name: Name::try_from(name)
                .or_else(|_| Name::try_from("Requester"))
                .map_err(|e| RepoError::Storage(e.to_string()))?,
            email: EmailAddress::try_from(email).map_err(|e| RepoError::Storage(e.to_string()))?,
            username: None,
            phone: None,
            manager_id: None,
            cost_centre_id: None,
        };
        let r = self
            .identity
            .create_requester(&admin(), tenant, input)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(r.id())
    }

    async fn managed_scope(
        &self,
        principal: &cooper_kernel::Principal,
    ) -> Result<cooper_service_desk::domain::ManagedTickets, RepoError> {
        let scope = self
            .identity
            .managed_scope(principal)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(cooper_service_desk::domain::ManagedTickets {
            requester_ids: scope.requester_ids,
            include_confidential: scope.include_confidential,
        })
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
    /// `COOPER_PUBLIC_URL`, which wins over the `general.base_url` setting.
    pub public_url: Option<String>,
}

/// Requesters' Notification Preferences for Email Templates (FR-PRT-007).
pub struct PortalPreferences {
    pub portal: cooper_notifications::portal::PortalNotifications,
}

#[async_trait]
impl cooper_service_desk::domain::RequesterNotificationPreferences for PortalPreferences {
    async fn wants_email(
        &self,
        tenant: cooper_kernel::TenantId,
        requester: cooper_kernel::RequesterId,
        event: cooper_service_desk::domain::EmailTemplateEvent,
    ) -> Result<bool, cooper_service_desk::domain::RepoError> {
        use cooper_notifications::portal::PortalNotificationKind as Kind;
        use cooper_service_desk::domain::EmailTemplateEvent as Event;
        let kind = match event {
            Event::TicketCreated => Kind::TicketCreated,
            Event::TicketReplied => Kind::TicketReplied,
            Event::TicketClosed => Kind::TicketClosed,
            // Sent to the Assignee, not the Requester.
            Event::TicketAssigned => return Ok(true),
        };
        self.portal
            .wants_email(tenant, requester, kind)
            .await
            .map_err(|e| cooper_service_desk::domain::RepoError::Storage(e.to_string()))
    }
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
            public_url: match &self.public_url {
                Some(url) => url.clone(),
                None => value(cooper_administration::application::settings::BASE_URL.key)
                    .await
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or(defaults.public_url),
            },
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
                    requester: d.ticket.details().requester_id,
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

    async fn administrators(&self) -> Result<BTreeSet<AnalystId>, String> {
        let mut admins = BTreeSet::new();
        let mut request = ListRequest {
            limit: Limit::new(Limit::MAX).unwrap_or_default(),
            ..ListRequest::default()
        };
        loop {
            let page = self
                .identity
                .list_analysts(&admin(), &request)
                .await
                .map_err(|e| e.to_string())?;
            admins.extend(
                page.items
                    .iter()
                    .filter(|a| a.is_admin() && a.is_active())
                    .map(|a| a.id()),
            );
            match page.next_cursor {
                Some(c) => request.cursor = Some(c),
                None => return Ok(admins),
            }
        }
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

/// Sends password reset and Portal link emails through the outbound email queue.
pub struct OutboxResetMailer {
    pub outbox: cooper_notifications::email::EmailOutbox,
    pub admin: AdministrationService,
}

impl OutboxResetMailer {
    async fn text(&self, key: &'static str, default: &str) -> String {
        self.admin
            .setting_value(key)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| default.to_owned())
    }

    /// From address, base URL and instance name.
    async fn branding(&self) -> (String, String, String) {
        let from = self
            .text(
                cooper_notifications::email::settings::FROM_ADDRESS.key,
                "Cooper <cooper@localhost>",
            )
            .await;
        let base = self
            .text(
                cooper_administration::application::settings::BASE_URL.key,
                "http://localhost:8080",
            )
            .await;
        let instance = self
            .text(
                cooper_administration::application::settings::INSTANCE_NAME.key,
                "Cooper",
            )
            .await;
        (from, base.trim_end_matches('/').to_owned(), instance)
    }

    async fn queue(
        &self,
        kind: &str,
        from: String,
        to: &str,
        subject: String,
        text_body: String,
    ) -> Result<(), cooper_identity::domain::RepoError> {
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
            kind: kind.into(),
            from,
            to: vec![to.to_owned()],
            cc: Vec::new(),
            subject,
            text_body,
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

#[async_trait]
impl cooper_identity::domain::ResetMailer for OutboxResetMailer {
    async fn send_portal_link(
        &self,
        link: &cooper_identity::domain::PortalLinkEmail,
    ) -> Result<(), cooper_identity::domain::RepoError> {
        use cooper_identity::domain::PortalLinkPurpose;
        let (from, base, instance) = self.branding().await;
        let url = format!("{base}/portal/set-password?token={}", link.token);
        let (kind, subject, intro) = match link.purpose {
            PortalLinkPurpose::Invitation => (
                "portal_invitation",
                format!("{instance}: you are invited to the Self-Service Portal"),
                format!(
                    "You can now raise and follow your IT requests on the {instance} Self-Service Portal. Choose a password with this link"
                ),
            ),
            PortalLinkPurpose::Reset => (
                "portal_password_reset",
                format!("{instance}: set your Portal password"),
                format!(
                    "Someone asked to set or reset your {instance} Portal password. If it was you, open this link"
                ),
            ),
            PortalLinkPurpose::Registration => (
                "portal_registration",
                format!("{instance}: confirm your Portal account"),
                format!(
                    "Thanks for registering for the {instance} Self-Service Portal. Confirm your address and choose a password with this link"
                ),
            ),
        };
        let body = format!(
            "Hello {},\n\n{intro} before {} (UTC):\n\n{url}\n\nIf you did not expect this email, ignore it.\n",
            link.name, link.expires_at
        );
        self.queue(kind, from, &link.to, subject, body).await
    }

    async fn send_reset(
        &self,
        to: &str,
        name: &str,
        token: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), cooper_identity::domain::RepoError> {
        let (from, base, instance) = self.branding().await;
        let body = format!(
            "Hello {name},\n\nSomeone asked to reset your {instance} password. If it was you, open this link before {expires_at} (UTC):\n\n{base}/reset-password?token={token}\n\nIf it was not you, ignore this email; your password stays the same.\n"
        );
        self.queue(
            "password_reset",
            from,
            to,
            format!("{instance}: reset your password"),
            body,
        )
        .await
    }
}

#[async_trait]
impl cooper_problem_management::domain::Tickets for DeskTickets {
    async fn ticket(
        &self,
        principal: &cooper_kernel::Principal,
        id: cooper_kernel::TicketId,
    ) -> Result<
        Option<cooper_problem_management::domain::TicketSummary>,
        cooper_problem_management::domain::RepoError,
    > {
        use cooper_service_desk::application::AppError;
        match self.desk.ticket(principal, id).await {
            Ok(d) => Ok(Some(cooper_problem_management::domain::TicketSummary {
                id,
                tenant_id: d.ticket.tenant_id(),
                number: d.ticket.number().to_owned(),
                subject: d.ticket.details().subject.clone(),
                is_closed: d.ticket.closed_at().is_some(),
            })),
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(cooper_problem_management::domain::RepoError::Storage(
                e.to_string(),
            )),
        }
    }

    async fn apply(
        &self,
        principal: &cooper_kernel::Principal,
        id: cooper_kernel::TicketId,
        action: &cooper_problem_management::domain::TicketAction,
    ) -> Result<(), String> {
        use cooper_problem_management::domain::TicketAction;
        use cooper_service_desk::application::BulkAction;
        match action {
            TicketAction::Close { resolution_code_id } => {
                let outcome = self
                    .desk
                    .bulk(
                        principal,
                        vec![id],
                        BulkAction::Close {
                            resolution_code_id: *resolution_code_id,
                            closure_category_id: None,
                        },
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                match outcome.into_iter().next().map(|o| o.result) {
                    Some(Ok(())) | None => Ok(()),
                    Some(Err(e)) => Err(e.to_string()),
                }
            }
            TicketAction::Note { body, public } => self
                .desk
                .add_note(principal, id, body, *public)
                .await
                .map(drop)
                .map_err(|e| e.to_string()),
        }
    }
}

/// Change Management's view of Teams, tickets and Problems, through their application
/// APIs (ADR-0027).
pub struct ChangeDirectory {
    /// Teams.
    pub identity: IdentityDirectory,
    /// Tickets.
    pub desk: cooper_service_desk::application::ServiceDesk,
    /// Problems.
    pub problems: cooper_problem_management::application::ProblemService,
}

#[async_trait]
impl cooper_change_management::ports::Directory for ChangeDirectory {
    async fn teams_of(
        &self,
        analyst: AnalystId,
    ) -> Result<BTreeSet<TeamId>, cooper_change_management::ports::RepoError> {
        Directory::teams_of(&self.identity, analyst)
            .await
            .map_err(|e| cooper_change_management::ports::RepoError::Storage(e.to_string()))
    }

    async fn ticket(
        &self,
        principal: &Principal,
        id: uuid::Uuid,
    ) -> Result<Option<(TenantId, String)>, cooper_change_management::ports::RepoError> {
        use cooper_service_desk::application::AppError;
        match self
            .desk
            .ticket(principal, cooper_kernel::TicketId::from_uuid(id))
            .await
        {
            Ok(d) => Ok(Some((d.ticket.tenant_id(), d.ticket.number().to_owned()))),
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(cooper_change_management::ports::RepoError::Storage(
                e.to_string(),
            )),
        }
    }

    async fn problem(
        &self,
        principal: &Principal,
        id: uuid::Uuid,
    ) -> Result<Option<(TenantId, String)>, cooper_change_management::ports::RepoError> {
        use cooper_problem_management::application::AppError;
        match self.problems.get(principal, id).await {
            Ok(p) => Ok(Some((p.tenant_id(), p.number().to_owned()))),
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(cooper_change_management::ports::RepoError::Storage(
                e.to_string(),
            )),
        }
    }
}

#[async_trait]
impl cooper_change_management::ports::UploadLimit for DeskSettings {
    async fn max_upload_bytes(&self) -> u64 {
        self.settings().await.max_attachment_bytes
    }
}

/// Change facts for Change Notifications.
pub struct ChangeFactsLookup {
    /// Change Management.
    pub changes: cooper_change_management::application::ChangeService,
}

#[async_trait]
impl cooper_notifications::handlers::ChangeLookup for ChangeFactsLookup {
    async fn change(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<cooper_notifications::handlers::ChangeFacts>, String> {
        let Some((change, cab)) = self
            .changes
            .for_notifications(id)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let d = change.details();
        let deciders = if d.cab_required {
            cab.iter().map(|m| m.analyst_id).collect()
        } else {
            d.approver.into_iter().collect()
        };
        Ok(Some(cooper_notifications::handlers::ChangeFacts {
            number: change.number().to_owned(),
            title: d.title.clone(),
            tenant_id: change.tenant_id(),
            owners: d.requested_by.into_iter().chain(d.assigned_to).collect(),
            deciders,
        }))
    }
}

/// Knowledge's view of Teams, User Groups and tickets, through their application APIs
/// (ADR-0028).
pub struct KnowledgeDirectory {
    /// Teams and User Groups.
    pub identity: IdentityDirectory,
    /// Tickets.
    pub desk: cooper_service_desk::application::ServiceDesk,
}

fn knowledge_err(e: impl std::fmt::Display) -> cooper_knowledge::ports::RepoError {
    cooper_knowledge::ports::RepoError::Storage(e.to_string())
}

#[async_trait]
impl cooper_knowledge::ports::Directory for KnowledgeDirectory {
    async fn teams_of(
        &self,
        analyst: AnalystId,
    ) -> Result<BTreeSet<TeamId>, cooper_knowledge::ports::RepoError> {
        Directory::teams_of(&self.identity, analyst)
            .await
            .map_err(knowledge_err)
    }

    async fn analyst_groups(
        &self,
        analyst: AnalystId,
    ) -> Result<BTreeSet<uuid::Uuid>, cooper_knowledge::ports::RepoError> {
        self.identity
            .identity
            .groups_of(cooper_identity::domain::Member::Analyst(analyst))
            .await
            .map_err(knowledge_err)
    }

    async fn requester_groups(
        &self,
        requester: RequesterId,
    ) -> Result<BTreeSet<uuid::Uuid>, cooper_knowledge::ports::RepoError> {
        self.identity
            .identity
            .groups_of(cooper_identity::domain::Member::Requester(requester))
            .await
            .map_err(knowledge_err)
    }

    async fn ticket(
        &self,
        principal: &Principal,
        id: uuid::Uuid,
    ) -> Result<Option<(TenantId, String)>, cooper_knowledge::ports::RepoError> {
        use cooper_service_desk::application::AppError;
        match self
            .desk
            .ticket(principal, cooper_kernel::TicketId::from_uuid(id))
            .await
        {
            Ok(d) => Ok(Some((d.ticket.tenant_id(), d.ticket.number().to_owned()))),
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(knowledge_err(e)),
        }
    }
}

/// Publishes Known Errors through Knowledge's application API (FR-PRB-005).
pub struct KnownErrorPublisher {
    /// Knowledge.
    pub knowledge: cooper_knowledge::application::KnowledgeService,
}

#[async_trait]
impl cooper_problem_management::domain::Knowledge for KnownErrorPublisher {
    async fn publish_known_error(
        &self,
        principal: &Principal,
        facts: &cooper_problem_management::domain::KnownErrorFacts,
    ) -> Result<uuid::Uuid, cooper_problem_management::domain::KnowledgeRefusal> {
        use cooper_knowledge::application::AppError;
        use cooper_problem_management::domain::KnowledgeRefusal;
        let ke = cooper_knowledge::application::KnownError {
            problem_id: facts.problem_id,
            tenant_id: facts.tenant_id,
            number: facts.number.clone(),
            title: facts.title.clone(),
            root_cause: facts.root_cause.clone(),
            workaround: facts.workaround.clone(),
        };
        match self.knowledge.publish_known_error(principal, &ke).await {
            Ok(article) => Ok(article.id()),
            Err(AppError::Forbidden(f)) => Err(KnowledgeRefusal::Forbidden(f.0)),
            Err(e @ (AppError::Rule(_) | AppError::Conflict(_) | AppError::NotFound(_))) => {
                Err(KnowledgeRefusal::Rule(e.to_string()))
            }
            Err(e) => Err(KnowledgeRefusal::Failed(e.to_string())),
        }
    }
}

/// Article facts for Review Date Notifications.
pub struct ArticleFactsLookup {
    /// Knowledge.
    pub knowledge: cooper_knowledge::application::KnowledgeService,
}

#[async_trait]
impl cooper_notifications::handlers::ArticleLookup for ArticleFactsLookup {
    async fn article(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<cooper_notifications::handlers::ArticleFacts>, String> {
        Ok(self
            .knowledge
            .for_notifications(id)
            .await
            .map_err(|e| e.to_string())?
            .filter(|a| !a.is_archived())
            .map(|a| cooper_notifications::handlers::ArticleFacts {
                title: a.title().to_owned(),
                tenant_id: a.tenant_id(),
                owner: a.owner_id(),
            }))
    }
}

/// Tasks' view of tickets, Changes, Analysts and Teams, through their application APIs
/// (ADR-0029).
pub struct TaskDirectory {
    /// Analysts and Teams.
    pub identity: IdentityDirectory,
    /// Tickets.
    pub desk: cooper_service_desk::application::ServiceDesk,
    /// Changes.
    pub changes: cooper_change_management::application::ChangeService,
}

fn task_err(e: impl std::fmt::Display) -> cooper_tasks::ports::RepoError {
    cooper_tasks::ports::RepoError::Storage(e.to_string())
}

#[async_trait]
impl cooper_tasks::ports::Directory for TaskDirectory {
    async fn ticket(
        &self,
        principal: &Principal,
        id: uuid::Uuid,
    ) -> Result<Option<(TenantId, String)>, cooper_tasks::ports::RepoError> {
        use cooper_service_desk::application::AppError;
        match self
            .desk
            .ticket(principal, cooper_kernel::TicketId::from_uuid(id))
            .await
        {
            Ok(d) => Ok(Some((d.ticket.tenant_id(), d.ticket.number().to_owned()))),
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(task_err(e)),
        }
    }

    async fn change(
        &self,
        principal: &Principal,
        id: uuid::Uuid,
    ) -> Result<Option<(TenantId, String)>, cooper_tasks::ports::RepoError> {
        use cooper_change_management::application::AppError;
        match self.changes.get(principal, id).await {
            Ok(c) => Ok(Some((c.tenant_id(), c.number().to_owned()))),
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(task_err(e)),
        }
    }

    async fn analyst_active(
        &self,
        analyst: AnalystId,
    ) -> Result<bool, cooper_tasks::ports::RepoError> {
        Directory::analyst_is_active(&self.identity, analyst)
            .await
            .map_err(task_err)
    }

    async fn team_exists(&self, team: TeamId) -> Result<bool, cooper_tasks::ports::RepoError> {
        Ok(Directory::team_members(&self.identity, team)
            .await
            .map_err(task_err)?
            .is_some())
    }
}

/// Task facts for assignment Notifications.
pub struct TaskFactsLookup {
    /// Tasks.
    pub tasks: cooper_tasks::application::TaskService,
}

#[async_trait]
impl cooper_notifications::handlers::TaskLookup for TaskFactsLookup {
    async fn task(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<cooper_notifications::handlers::TaskFacts>, String> {
        Ok(self
            .tasks
            .for_notifications(id)
            .await
            .map_err(|e| e.to_string())?
            .map(|t| cooper_notifications::handlers::TaskFacts {
                title: t.details().title.clone(),
                tenant_id: t.tenant_id(),
                assignee: t.details().assigned_analyst,
                is_closed: t.is_closed(),
            }))
    }
}

/// Service Status publication settings, read from System Settings (FR-SST-009,
/// FR-PRT-006).
pub struct StatusPublication {
    /// Administration.
    pub admin: AdministrationService,
}

#[async_trait]
impl cooper_service_status::ports::PublicationSettingsProvider for StatusPublication {
    async fn settings(&self) -> cooper_service_status::ports::PublicationSettings {
        use cooper_service_status::application::settings;
        let defaults = cooper_service_status::ports::PublicationSettings::default();
        let value = |key: &'static str| {
            let admin = self.admin.clone();
            async move { admin.setting_value(key).await.ok().flatten() }
        };
        cooper_service_status::ports::PublicationSettings {
            public_page: value(settings::PUBLIC_PAGE.key)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.public_page),
            show_incidents: value(settings::SHOW_INCIDENTS.key)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.show_incidents),
            recent_days: value(settings::RECENT_DAYS.key)
                .await
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(defaults.recent_days),
        }
    }
}

/// Morning Checks' view of tickets, Tasks, Analysts and Teams.
pub struct MorningCheckDirectory {
    /// Analysts and Teams.
    pub identity: IdentityDirectory,
    /// Tickets.
    pub desk: cooper_service_desk::application::ServiceDesk,
    /// Tasks.
    pub tasks: cooper_tasks::application::TaskService,
    /// Module switches (follow-up Tasks need the Tasks Module).
    pub gate: cooper_administration::api::ModuleGate,
}

fn check_err(e: impl std::fmt::Display) -> cooper_morning_checks::ports::RepoError {
    cooper_morning_checks::ports::RepoError::Storage(e.to_string())
}

#[async_trait]
impl cooper_morning_checks::ports::Directory for MorningCheckDirectory {
    async fn ticket(
        &self,
        principal: &Principal,
        id: uuid::Uuid,
    ) -> Result<Option<(TenantId, String)>, cooper_morning_checks::ports::RepoError> {
        use cooper_service_desk::application::AppError;
        match self
            .desk
            .ticket(principal, cooper_kernel::TicketId::from_uuid(id))
            .await
        {
            Ok(d) => Ok(Some((d.ticket.tenant_id(), d.ticket.number().to_owned()))),
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(check_err(e)),
        }
    }

    async fn task(
        &self,
        principal: &Principal,
        id: uuid::Uuid,
    ) -> Result<Option<(TenantId, String)>, cooper_morning_checks::ports::RepoError> {
        use cooper_tasks::application::AppError;
        if !self.gate.is_enabled(cooper_kernel::Module::Tasks).await {
            return Ok(None);
        }
        match self.tasks.get(principal, id).await {
            Ok(t) => Ok(Some((t.tenant_id(), t.details().title.clone()))),
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
            Err(e) => Err(check_err(e)),
        }
    }

    async fn create_task(
        &self,
        principal: &Principal,
        task: cooper_morning_checks::ports::FollowUpTask,
    ) -> Result<(uuid::Uuid, String), cooper_morning_checks::ports::FollowUpError> {
        use cooper_morning_checks::ports::FollowUpError;
        use cooper_tasks::application::AppError;
        if !self.gate.is_enabled(cooper_kernel::Module::Tasks).await {
            return Err(FollowUpError::Forbidden("the Tasks Module is off".into()));
        }
        let new = cooper_tasks::application::NewTask {
            tenant_id: Some(task.tenant_id),
            details: cooper_tasks::domain::TaskDetails {
                title: task.title,
                description: task.description,
                priority_id: uuid::Uuid::nil(),
                start_date: None,
                due_date: None,
                estimate_minutes: None,
                assigned_analyst: task.assigned_analyst,
                assigned_team: task.assigned_team,
                tags: Vec::new(),
            },
            status_id: None,
            parent_id: None,
            collaborators: BTreeSet::new(),
        };
        match self.tasks.create(principal, new).await {
            Ok(t) => Ok((t.id(), t.details().title.clone())),
            Err(AppError::Forbidden(f)) => Err(FollowUpError::Forbidden(f.0)),
            Err(e @ (AppError::Rule(_) | AppError::NotFound(_) | AppError::Conflict(_))) => {
                Err(FollowUpError::Invalid(e.to_string()))
            }
            Err(e) => Err(FollowUpError::Storage(e.to_string())),
        }
    }

    async fn analyst_active(
        &self,
        analyst: AnalystId,
    ) -> Result<bool, cooper_morning_checks::ports::RepoError> {
        Directory::analyst_is_active(&self.identity, analyst)
            .await
            .map_err(check_err)
    }

    async fn team_members(
        &self,
        team: TeamId,
    ) -> Result<Option<BTreeSet<AnalystId>>, cooper_morning_checks::ports::RepoError> {
        Directory::team_members(&self.identity, team)
            .await
            .map_err(check_err)
    }

    async fn teams_of(
        &self,
        analyst: AnalystId,
    ) -> Result<BTreeSet<TeamId>, cooper_morning_checks::ports::RepoError> {
        Directory::teams_of(&self.identity, analyst)
            .await
            .map_err(check_err)
    }

    async fn analyst_name(
        &self,
        analyst: AnalystId,
    ) -> Result<Option<String>, cooper_morning_checks::ports::RepoError> {
        Ok(Directory::analyst_profile(&self.identity, analyst)
            .await
            .map_err(check_err)?
            .map(|p| p.name))
    }
}

/// Morning Checks settings, read from System Settings (`general.time_zone` and the
/// `morning_checks.*` reminder settings).
pub struct MorningCheckSettings {
    /// Administration.
    pub admin: AdministrationService,
}

#[async_trait]
impl cooper_morning_checks::ports::MorningCheckSettingsProvider for MorningCheckSettings {
    async fn settings(&self) -> cooper_morning_checks::ports::MorningCheckSettings {
        use cooper_morning_checks::application::settings;
        let defaults = cooper_morning_checks::ports::MorningCheckSettings::default();
        let value = |key: &'static str| {
            let admin = self.admin.clone();
            async move { admin.setting_value(key).await.ok().flatten() }
        };
        let text = |v: Option<serde_json::Value>, default: String| {
            v.and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or(default)
        };
        cooper_morning_checks::ports::MorningCheckSettings {
            time_zone: text(
                value(cooper_administration::application::settings::TIME_ZONE.key).await,
                defaults.time_zone,
            ),
            reminders: value(settings::REMINDERS.key)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.reminders),
            reminder_time: text(
                value(settings::REMINDER_TIME.key).await,
                defaults.reminder_time,
            ),
            reminder_weekends: value(settings::REMINDER_WEEKENDS.key)
                .await
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.reminder_weekends),
        }
    }
}

/// Unrecorded Morning Checks, for reminder Notifications.
pub struct UnrecordedChecks {
    /// Morning Checks.
    pub checks: cooper_morning_checks::application::MorningChecksService,
}

#[async_trait]
impl cooper_notifications::handlers::UnrecordedChecksLookup for UnrecordedChecks {
    async fn unrecorded(
        &self,
        tenant: TenantId,
        check_date: chrono::NaiveDate,
        check_group: Option<uuid::Uuid>,
        analyst: AnalystId,
    ) -> Result<u32, String> {
        self.checks
            .unrecorded_for(
                tenant,
                check_date,
                check_group.map(cooper_morning_checks::domain::CheckGroupId::from_uuid),
                analyst,
            )
            .await
            .map_err(|e| e.to_string())
    }
}

/// Assets' view of Requesters, Analysts, Cost Centres, Suppliers, tickets and Contracts,
/// through their application APIs (ADR-0034).
pub struct AssetDirectory {
    /// Requesters, Analysts and Cost Centres.
    pub identity: IdentityDirectory,
    /// Tickets.
    pub desk: cooper_service_desk::application::ServiceDesk,
    /// Suppliers and Contracts.
    pub contracts: cooper_contracts::application::ContractsService,
}

fn asset_err(e: impl std::fmt::Display) -> cooper_assets::ports::RepoError {
    cooper_assets::ports::RepoError::Storage(e.to_string())
}

#[async_trait]
impl cooper_assets::ports::Directory for AssetDirectory {
    async fn requester_in(
        &self,
        tenant: TenantId,
        id: RequesterId,
    ) -> Result<bool, cooper_assets::ports::RepoError> {
        match self
            .identity
            .identity
            .get_requester(&system(TenantScope::single(tenant)), id)
            .await
        {
            Ok(r) => Ok(r.tenant_id() == tenant && r.is_active()),
            Err(IdentityError::NotFound(_)) => Ok(false),
            Err(e) => Err(asset_err(e)),
        }
    }

    async fn analyst_active(&self, id: AnalystId) -> Result<bool, cooper_assets::ports::RepoError> {
        Directory::analyst_is_active(&self.identity, id)
            .await
            .map_err(asset_err)
    }

    async fn cost_centre_in(
        &self,
        tenant: TenantId,
        id: uuid::Uuid,
    ) -> Result<bool, cooper_assets::ports::RepoError> {
        match self
            .identity
            .identity
            .get_cost_centre(
                &system(TenantScope::single(tenant)),
                cooper_identity::domain::CostCentreId::from_uuid(id),
            )
            .await
        {
            Ok(c) => Ok(c.tenant_id() == tenant),
            Err(IdentityError::NotFound(_)) => Ok(false),
            Err(e) => Err(asset_err(e)),
        }
    }

    async fn supplier_exists(
        &self,
        tenant: TenantId,
        id: uuid::Uuid,
    ) -> Result<bool, cooper_assets::ports::RepoError> {
        self.contracts
            .supplier_exists(
                &TenantScope::single(tenant),
                tenant,
                cooper_contracts::domain::SupplierId::from_uuid(id),
            )
            .await
            .map_err(asset_err)
    }

    async fn link_target(
        &self,
        principal: &Principal,
        tenant: TenantId,
        kind: cooper_assets::ports::LinkKind,
        id: uuid::Uuid,
    ) -> Result<Option<String>, cooper_assets::ports::RepoError> {
        use cooper_assets::ports::LinkKind;
        use cooper_service_desk::application::AppError;
        match kind {
            LinkKind::Ticket => match self
                .desk
                .ticket(principal, cooper_kernel::TicketId::from_uuid(id))
                .await
            {
                Ok(d) => Ok((d.ticket.tenant_id() == tenant).then(|| d.ticket.number().to_owned())),
                Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(None),
                Err(e) => Err(asset_err(e)),
            },
            // A Contract of the same Company that the caller may read.
            LinkKind::Contract => {
                if !principal.can("contracts.read") {
                    return Ok(None);
                }
                let summary = self
                    .contracts
                    .contract_summary(
                        principal.scope(),
                        cooper_contracts::domain::ContractId::from_uuid(id),
                    )
                    .await
                    .map_err(asset_err)?;
                Ok(summary
                    .filter(|c| c.tenant_id == tenant)
                    .map(|c| c.contract_number))
            }
        }
    }
}

/// Contracts' view of Analysts, Requesters and Cost Centres, through the identity
/// application API (ADR-0035).
pub struct ContractDirectory {
    /// Analysts, Requesters and Cost Centres.
    pub identity: IdentityDirectory,
}

fn contract_err(e: impl std::fmt::Display) -> cooper_contracts::ports::RepoError {
    cooper_contracts::ports::RepoError::Storage(e.to_string())
}

#[async_trait]
impl cooper_contracts::ports::Directory for ContractDirectory {
    async fn analyst_active(
        &self,
        analyst: AnalystId,
    ) -> Result<bool, cooper_contracts::ports::RepoError> {
        Directory::analyst_is_active(&self.identity, analyst)
            .await
            .map_err(contract_err)
    }

    async fn requester_company(
        &self,
        requester: RequesterId,
    ) -> Result<Option<TenantId>, cooper_contracts::ports::RepoError> {
        Directory::requester_company(&self.identity, &TenantScope::All, requester)
            .await
            .map_err(contract_err)
    }

    async fn cost_centre_company(
        &self,
        cost_centre: uuid::Uuid,
    ) -> Result<Option<TenantId>, cooper_contracts::ports::RepoError> {
        match self
            .identity
            .identity
            .get_cost_centre(
                &system(TenantScope::All),
                cooper_identity::domain::CostCentreId::from_uuid(cost_centre),
            )
            .await
        {
            Ok(c) => Ok(Some(c.tenant_id())),
            Err(IdentityError::NotFound(_)) => Ok(None),
            Err(e) => Err(contract_err(e)),
        }
    }
}

/// The reminder window and time zone of Contracts, read from System Settings
/// (FR-CON-003).
pub struct ContractSettings {
    /// Administration.
    pub admin: AdministrationService,
}

#[async_trait]
impl cooper_contracts::ports::ContractSettingsProvider for ContractSettings {
    async fn settings(&self) -> cooper_contracts::ports::ContractSettings {
        let defaults = cooper_contracts::ports::ContractSettings::default();
        let value = |key: &'static str| {
            let admin = self.admin.clone();
            async move { admin.setting_value(key).await.ok().flatten() }
        };
        cooper_contracts::ports::ContractSettings {
            reminder_days: value(cooper_contracts::application::settings::REMINDER_DAYS.key)
                .await
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(defaults.reminder_days),
            time_zone: value(cooper_administration::application::settings::TIME_ZONE.key)
                .await
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or(defaults.time_zone),
        }
    }
}

/// Contract facts for reminder Notifications.
pub struct ContractFactsLookup {
    /// Contracts.
    pub contracts: cooper_contracts::application::ContractsService,
}

#[async_trait]
impl cooper_notifications::handlers::ContractLookup for ContractFactsLookup {
    async fn contract(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<cooper_notifications::handlers::ContractFacts>, String> {
        Ok(self
            .contracts
            .for_notifications(cooper_contracts::domain::ContractId::from_uuid(id))
            .await
            .map_err(|e| e.to_string())?
            .map(|c| cooper_notifications::handlers::ContractFacts {
                contract_number: c.details().contract_number.clone(),
                title: c.details().title.clone(),
                tenant_id: c.tenant_id(),
                owner: c.details().owner_id,
                notice_date: c.notice_date(),
                end_date: c.details().end_date,
                is_active: c.is_active(),
            }))
    }
}
