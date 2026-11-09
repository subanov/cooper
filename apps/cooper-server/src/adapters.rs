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
