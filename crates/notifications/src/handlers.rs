//! Event handlers that turn domain events into Notifications (FR-GEN-006, FR-SLA-006).
//!
//! Payloads are read by field name, so this context depends on the published event
//! contract (`ticket.assigned`, `ticket.note_added`, `ticket.sla_warning`,
//! `ticket.sla_breached`), not on the service desk crate.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use cooper_kernel::{AnalystId, EventEnvelope, RequesterId, TeamId, TenantId, TicketId};
use cooper_platform::outbox::{EventHandler, HandlerError};
use serde_json::Value;
use uuid::Uuid;

use crate::{Draft, NotificationKind, Notifications};

/// What a Notification needs to know about a ticket.
#[derive(Debug, Clone)]
pub struct TicketFacts {
    /// Ticket Number.
    pub number: String,
    /// Subject.
    pub subject: String,
    /// Company.
    pub tenant_id: TenantId,
    /// Assignee.
    pub assignee: Option<AnalystId>,
    /// Queue.
    pub team: Option<TeamId>,
    /// Requester.
    pub requester: Option<RequesterId>,
}

/// Tells Requesters about their tickets on the Portal (FR-PRT-007): a ticket logged for
/// them, a Reply or public Note, and closure. Respects their Notification Preferences;
/// never about their own actions.
pub struct PortalNotifier {
    /// The Portal Notifications service.
    pub notifications: crate::portal::PortalNotifications,
    /// Tickets.
    pub tickets: Arc<dyn TicketLookup>,
}

#[async_trait]
impl EventHandler for PortalNotifier {
    fn name(&self) -> &'static str {
        "portal-notifier"
    }

    fn handles(&self, event_type: &str) -> bool {
        matches!(
            event_type,
            "ticket.created" | "ticket.reply_sent" | "ticket.note_added" | "ticket.closed"
        )
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        use crate::portal::{PortalDraft, PortalNotificationKind as Kind};
        if event.event_type == "ticket.note_added"
            && field(&event.payload, "is_public").and_then(Value::as_bool) != Some(true)
        {
            return Ok(());
        }
        let Some(facts) = self
            .tickets
            .ticket(TicketId::from_uuid(event.aggregate_id))
            .await
            .map_err(handler_error)?
        else {
            return Ok(());
        };
        let Some(requester) = facts.requester else {
            return Ok(());
        };
        let n = &facts.number;
        let (kind, title) = match event.event_type.as_str() {
            "ticket.created" => (Kind::TicketCreated, format!("Ticket {n} was logged")),
            "ticket.reply_sent" => (Kind::TicketReplied, format!("New reply on ticket {n}")),
            "ticket.note_added" => (Kind::TicketReplied, format!("New update on ticket {n}")),
            "ticket.closed" => (Kind::TicketClosed, format!("Ticket {n} was closed")),
            _ => return Ok(()),
        };
        let draft = PortalDraft {
            kind,
            tenant_id: facts.tenant_id,
            requester_id: requester,
            title,
            body: facts.subject,
            ticket_id: Some(TicketId::from_uuid(event.aggregate_id)),
            source_event_id: event.id,
        };
        self.notifications
            .send(draft, event.actor)
            .await
            .map(|_| ())
            .map_err(|e| handler_error(e.to_string()))
    }
}

/// Reads tickets (through the service desk's application API).
#[async_trait]
pub trait TicketLookup: Send + Sync {
    /// A ticket, if it exists and is not deleted.
    async fn ticket(&self, id: TicketId) -> Result<Option<TicketFacts>, String>;
    /// Analysts to notify for an SLA trigger (`warning`/`breach`) on a target
    /// (`response`/`resolution`), per the SLA Notification Rules.
    async fn sla_recipients(
        &self,
        trigger: &str,
        target: &str,
        assignee: Option<AnalystId>,
        team_members: &BTreeSet<AnalystId>,
    ) -> Result<BTreeSet<AnalystId>, String>;
}

/// Reads people (through the identity context's application API).
#[async_trait]
pub trait PeopleLookup: Send + Sync {
    /// Members of a Team (empty if it does not exist).
    async fn team_members(&self, team: TeamId) -> Result<BTreeSet<AnalystId>, String>;
    /// Active Analysts with these usernames (case-insensitive).
    async fn analysts_by_username(
        &self,
        usernames: &[String],
    ) -> Result<BTreeSet<AnalystId>, String>;
    /// Active Administrators.
    async fn administrators(&self) -> Result<BTreeSet<AnalystId>, String>;
}

/// Notifies Analysts about ticket events.
pub struct TicketNotifier {
    /// The notifications service.
    pub notifications: Notifications,
    /// Tickets.
    pub tickets: Arc<dyn TicketLookup>,
    /// People.
    pub people: Arc<dyn PeopleLookup>,
}

fn field<'a>(payload: &'a Value, name: &str) -> Option<&'a Value> {
    payload.get(name).filter(|v| !v.is_null())
}

fn uuid(payload: &Value, name: &str) -> Option<Uuid> {
    field(payload, name)
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
}

fn handler_error(message: String) -> HandlerError {
    message.into()
}

impl TicketNotifier {
    async fn facts(&self, event: &EventEnvelope) -> Result<Option<TicketFacts>, HandlerError> {
        self.tickets
            .ticket(TicketId::from_uuid(event.aggregate_id))
            .await
            .map_err(handler_error)
    }

    async fn send(
        &self,
        event: &EventEnvelope,
        kind: NotificationKind,
        facts: &TicketFacts,
        title: String,
        recipients: BTreeSet<AnalystId>,
    ) -> Result<(), HandlerError> {
        let draft = Draft {
            kind,
            tenant_id: Some(facts.tenant_id),
            title,
            body: facts.subject.clone(),
            ticket_id: Some(TicketId::from_uuid(event.aggregate_id)),
            change_id: None,
            article_id: None,
            task_id: None,
            source_event_id: event.id,
        };
        self.notifications
            .send(draft, recipients, event.actor)
            .await
            .map(|_| ())
            .map_err(|e| handler_error(e.to_string()))
    }
}

#[async_trait]
impl EventHandler for TicketNotifier {
    fn name(&self) -> &'static str {
        "ticket-notifier"
    }

    fn handles(&self, event_type: &str) -> bool {
        matches!(
            event_type,
            "ticket.email_received"
                | "ticket.assigned"
                | "ticket.note_added"
                | "ticket.sla_warning"
                | "ticket.sla_breached"
        )
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        if event.event_type == "ticket.email_received" {
            let Some(facts) = self.facts(event).await? else {
                return Ok(());
            };
            let Some(assignee) = facts.assignee else {
                return Ok(());
            };
            let reopened = field(&event.payload, "reopened")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let title = if reopened {
                format!(
                    "Ticket {} was reopened by a reply from the Requester",
                    facts.number
                )
            } else {
                format!("The Requester replied on ticket {}", facts.number)
            };
            return self
                .send(
                    event,
                    NotificationKind::ReplyReceived,
                    &facts,
                    title,
                    BTreeSet::from([assignee]),
                )
                .await;
        }
        let Some(facts) = self.facts(event).await? else {
            return Ok(());
        };
        let p = &event.payload;
        match event.event_type.as_str() {
            "ticket.assigned" => {
                let Some(analyst) = uuid(p, "analyst_id").map(AnalystId::from_uuid) else {
                    return Ok(());
                };
                if uuid(p, "previous_analyst_id") == Some(analyst.as_uuid()) {
                    return Ok(());
                }
                let title = format!("Ticket {} was assigned to you", facts.number);
                self.send(
                    event,
                    NotificationKind::TicketAssigned,
                    &facts,
                    title,
                    BTreeSet::from([analyst]),
                )
                .await
            }
            "ticket.note_added" => {
                let names: Vec<String> = field(p, "mentions")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                if names.is_empty() {
                    return Ok(());
                }
                let recipients = self
                    .people
                    .analysts_by_username(&names)
                    .await
                    .map_err(handler_error)?;
                let title = format!("You were mentioned on ticket {}", facts.number);
                self.send(
                    event,
                    NotificationKind::Mentioned,
                    &facts,
                    title,
                    recipients,
                )
                .await
            }
            "ticket.sla_warning" | "ticket.sla_breached" => {
                let target = field(p, "target")
                    .and_then(Value::as_str)
                    .unwrap_or("resolution");
                let breach = event.event_type == "ticket.sla_breached";
                let members = match facts.team {
                    Some(team) => self
                        .people
                        .team_members(team)
                        .await
                        .map_err(handler_error)?,
                    None => BTreeSet::new(),
                };
                let trigger = if breach { "breach" } else { "warning" };
                let recipients = self
                    .tickets
                    .sla_recipients(trigger, target, facts.assignee, &members)
                    .await
                    .map_err(handler_error)?;
                let (kind, verb) = if breach {
                    (NotificationKind::SlaBreached, "missed")
                } else {
                    (NotificationKind::SlaWarning, "is about to be missed")
                };
                let title = format!("Ticket {}: the {target} target {verb}", facts.number);
                self.send(event, kind, &facts, title, recipients).await
            }
            _ => Ok(()),
        }
    }
}

/// Tells Administrators when a Mailbox keeps failing (FR-EML-010).
pub struct MailboxAlerts {
    /// The notifications service.
    pub notifications: Notifications,
    /// People.
    pub people: Arc<dyn PeopleLookup>,
}

#[async_trait]
impl EventHandler for MailboxAlerts {
    fn name(&self) -> &'static str {
        "mailbox-alerts"
    }

    fn handles(&self, event_type: &str) -> bool {
        event_type == "mailbox.poll_failed"
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let p = &event.payload;
        let name = field(p, "name")
            .and_then(Value::as_str)
            .unwrap_or("A Mailbox");
        let error = field(p, "error")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let admins = self.people.administrators().await.map_err(handler_error)?;
        let draft = Draft {
            kind: NotificationKind::MailboxFailing,
            tenant_id: None,
            title: format!("Mailbox {name} keeps failing to poll"),
            body: error.to_owned(),
            ticket_id: None,
            change_id: None,
            article_id: None,
            task_id: None,
            source_event_id: event.id,
        };
        self.notifications
            .send(draft, admins, event.actor)
            .await
            .map(|_| ())
            .map_err(|e| handler_error(e.to_string()))
    }
}

/// What a Notification needs to know about a Change.
#[derive(Debug, Clone)]
pub struct ChangeFacts {
    /// Change Number.
    pub number: String,
    /// Title.
    pub title: String,
    /// Company.
    pub tenant_id: TenantId,
    /// Requested By and Assigned To: told about outcomes.
    pub owners: BTreeSet<AnalystId>,
    /// Approver or CAB members: asked to vote.
    pub deciders: BTreeSet<AnalystId>,
}

/// Reads Changes (through Change Management's application API).
#[async_trait]
pub trait ChangeLookup: Send + Sync {
    /// A Change, if it exists.
    async fn change(&self, id: Uuid) -> Result<Option<ChangeFacts>, String>;
}

/// Notifies Analysts about Change events (FR-CHG-014): deciders when a Change is
/// submitted, the requester and assignee when it is approved, rejected, scheduled or
/// finished.
pub struct ChangeNotifier {
    /// The notifications service.
    pub notifications: Notifications,
    /// Changes.
    pub changes: Arc<dyn ChangeLookup>,
}

#[async_trait]
impl EventHandler for ChangeNotifier {
    fn name(&self) -> &'static str {
        "change-notifier"
    }

    fn handles(&self, event_type: &str) -> bool {
        matches!(
            event_type,
            "change.submitted"
                | "change.approved"
                | "change.rejected"
                | "change.scheduled"
                | "change.completed"
        )
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let Some(facts) = self
            .changes
            .change(event.aggregate_id)
            .await
            .map_err(handler_error)?
        else {
            return Ok(());
        };
        let (kind, verb, recipients) = match event.event_type.as_str() {
            "change.submitted" => (
                NotificationKind::ChangeSubmitted,
                "awaits your approval",
                facts.deciders.clone(),
            ),
            "change.approved" => (
                NotificationKind::ChangeApproved,
                "was approved",
                facts.owners.clone(),
            ),
            "change.rejected" => (
                NotificationKind::ChangeRejected,
                "was rejected",
                facts.owners.clone(),
            ),
            "change.scheduled" => (
                NotificationKind::ChangeScheduled,
                "was scheduled",
                facts.owners.clone(),
            ),
            "change.completed" => (
                NotificationKind::ChangeCompleted,
                "was finished",
                facts.owners.clone(),
            ),
            _ => return Ok(()),
        };
        let draft = Draft {
            kind,
            tenant_id: Some(facts.tenant_id),
            title: format!("{} {verb}", facts.number),
            body: facts.title,
            ticket_id: None,
            change_id: Some(event.aggregate_id),
            article_id: None,
            task_id: None,
            source_event_id: event.id,
        };
        self.notifications
            .send(draft, recipients, event.actor)
            .await
            .map(|_| ())
            .map_err(|e| handler_error(e.to_string()))
    }
}

/// What a Review Date Notification needs to know about an Article.
#[derive(Debug, Clone)]
pub struct ArticleFacts {
    /// Title.
    pub title: String,
    /// Company, if any.
    pub tenant_id: Option<TenantId>,
    /// Owner now.
    pub owner: Option<AnalystId>,
}

/// Reads Articles (through Knowledge's application API).
#[async_trait]
pub trait ArticleLookup: Send + Sync {
    /// An Article that is not archived.
    async fn article(&self, id: Uuid) -> Result<Option<ArticleFacts>, String>;
}

/// Tells an Article's Owner that its Review Date has come (FR-KNW-007).
pub struct ArticleReviewNotifier {
    /// The notifications service.
    pub notifications: Notifications,
    /// Articles.
    pub articles: Arc<dyn ArticleLookup>,
}

#[async_trait]
impl EventHandler for ArticleReviewNotifier {
    fn name(&self) -> &'static str {
        "article-review-notifier"
    }

    fn handles(&self, event_type: &str) -> bool {
        event_type == "knowledge_article.review_due"
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let Some(facts) = self
            .articles
            .article(event.aggregate_id)
            .await
            .map_err(handler_error)?
        else {
            return Ok(());
        };
        let Some(owner) = facts.owner else {
            return Ok(());
        };
        let draft = Draft {
            kind: NotificationKind::ArticleReviewDue,
            tenant_id: facts.tenant_id,
            title: format!("Review due: {}", facts.title),
            body: String::new(),
            ticket_id: None,
            change_id: None,
            article_id: Some(event.aggregate_id),
            task_id: None,
            source_event_id: event.id,
        };
        self.notifications
            .send(draft, BTreeSet::from([owner]), event.actor)
            .await
            .map(|_| ())
            .map_err(|e| handler_error(e.to_string()))
    }
}

/// What an assignment Notification needs to know about a Task.
#[derive(Debug, Clone)]
pub struct TaskFacts {
    /// Title.
    pub title: String,
    /// Company.
    pub tenant_id: TenantId,
    /// Assigned Analyst now.
    pub assignee: Option<AnalystId>,
    /// Closed.
    pub is_closed: bool,
}

/// Reads Tasks (through the Tasks application API).
#[async_trait]
pub trait TaskLookup: Send + Sync {
    /// A Task.
    async fn task(&self, id: Uuid) -> Result<Option<TaskFacts>, String>;
}

/// Tells an Analyst a Task was assigned to them (FR-TSK-001), unless it has moved on
/// since (reassigned or closed).
pub struct TaskNotifier {
    /// The notifications service.
    pub notifications: Notifications,
    /// Tasks.
    pub tasks: Arc<dyn TaskLookup>,
}

#[async_trait]
impl EventHandler for TaskNotifier {
    fn name(&self) -> &'static str {
        "task-notifier"
    }

    fn handles(&self, event_type: &str) -> bool {
        event_type == "task.assigned"
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let Some(to) = uuid(&event.payload, "analyst_id").map(AnalystId::from_uuid) else {
            return Ok(());
        };
        let Some(facts) = self
            .tasks
            .task(event.aggregate_id)
            .await
            .map_err(handler_error)?
        else {
            return Ok(());
        };
        if facts.is_closed || facts.assignee != Some(to) {
            return Ok(());
        }
        let draft = Draft {
            kind: NotificationKind::TaskAssigned,
            tenant_id: Some(facts.tenant_id),
            title: format!("Task assigned: {}", facts.title),
            body: String::new(),
            ticket_id: None,
            change_id: None,
            article_id: None,
            task_id: Some(event.aggregate_id),
            source_event_id: event.id,
        };
        self.notifications
            .send(draft, BTreeSet::from([to]), event.actor)
            .await
            .map(|_| ())
            .map_err(|e| handler_error(e.to_string()))
    }
}

/// Reads Morning Checks (through the Morning Checks application API).
#[async_trait]
pub trait UnrecordedChecksLookup: Send + Sync {
    /// How many of an Analyst's Checks of a Check Group (`None`: ungrouped) are still
    /// unrecorded on a Check Date.
    async fn unrecorded(
        &self,
        tenant: TenantId,
        check_date: chrono::NaiveDate,
        check_group: Option<Uuid>,
        analyst: AnalystId,
    ) -> Result<u32, String>;
}

/// Reminds an Analyst of Morning Checks still unrecorded at the reminder time
/// (FR-MCK-006), unless they have all been recorded since.
pub struct CheckReminderNotifier {
    /// The notifications service.
    pub notifications: Notifications,
    /// Morning Checks.
    pub checks: Arc<dyn UnrecordedChecksLookup>,
}

#[async_trait]
impl EventHandler for CheckReminderNotifier {
    fn name(&self) -> &'static str {
        "check-reminder-notifier"
    }

    fn handles(&self, event_type: &str) -> bool {
        event_type == "morning_checks.unrecorded"
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let (Some(tenant), Some(to), Some(date)) = (
            event.tenant_id,
            uuid(&event.payload, "analyst_id").map(AnalystId::from_uuid),
            field(&event.payload, "check_date")
                .and_then(Value::as_str)
                .and_then(|d| d.parse::<chrono::NaiveDate>().ok()),
        ) else {
            return Ok(());
        };
        let group = uuid(&event.payload, "check_group_id");
        let left = self
            .checks
            .unrecorded(tenant, date, group, to)
            .await
            .map_err(handler_error)?;
        if left == 0 {
            return Ok(());
        }
        let name = field(&event.payload, "check_group_name")
            .and_then(Value::as_str)
            .unwrap_or("Ungrouped Checks");
        let (s, verb) = if left == 1 {
            ("", "has")
        } else {
            ("s", "have")
        };
        let draft = Draft {
            kind: NotificationKind::ChecksUnrecorded,
            tenant_id: Some(tenant),
            title: format!("Morning Checks not recorded: {name}"),
            body: format!("{left} Check{s} for {date} still {verb} no result."),
            ticket_id: None,
            change_id: None,
            article_id: None,
            task_id: None,
            source_event_id: event.id,
        };
        self.notifications
            .send(draft, BTreeSet::from([to]), event.actor)
            .await
            .map(|_| ())
            .map_err(|e| handler_error(e.to_string()))
    }
}
