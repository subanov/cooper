//! Repository and service ports of the service desk.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    AnalystId, AuditRecord, DepartmentId, EventEnvelope, Limit, RequesterId, Sensitivity, TeamId,
    TenantId, TenantScope, TicketId,
};
use thiserror::Error;
use uuid::Uuid;

use super::{
    Category, Lookup, LookupKind, MovedRows, Note, Ticket, TicketField, TicketLink, TicketMerge,
    TicketPresence, TicketSplit, TicketTypeRule, TimeEntry,
};

/// A storage operation failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A unique constraint was violated (constraint name).
    #[error("conflict on {0}")]
    Conflict(String),
    /// A referenced record does not exist (constraint name).
    #[error("missing reference {0}")]
    MissingReference(String),
    /// The record does not exist or is not visible.
    #[error("not found")]
    NotFound,
    /// Anything else; for logs only.
    #[error("storage error: {0}")]
    Storage(String),
}

/// Which tickets a caller may see beyond the tenant scope (FR-TKT-014, FR-GEN-004).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visibility {
    /// Holds `tickets.view_confidential`.
    pub all_confidential: bool,
    /// Departments served by the caller's Teams: their Confidential tickets are visible.
    pub confidential_departments: BTreeSet<DepartmentId>,
    /// Include soft-deleted tickets.
    pub include_deleted: bool,
}

/// Sortable columns of ticket lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortField {
    /// Creation time (default, newest first).
    #[default]
    CreatedAt,
    /// Last change.
    UpdatedAt,
    /// Ticket Number.
    Number,
}

/// Filters of ticket lists (FR-GEN-003). Empty sets mean "any".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListFilter {
    /// Only these Companies.
    pub tenant_ids: BTreeSet<TenantId>,
    /// Only these statuses.
    pub status_ids: BTreeSet<Uuid>,
    /// `Some(true)`: only open tickets; `Some(false)`: only closed.
    pub open: Option<bool>,
    /// Only these Priorities.
    pub priority_ids: BTreeSet<Uuid>,
    /// Only these Ticket Types.
    pub type_ids: BTreeSet<Uuid>,
    /// Only these Queues.
    pub team_ids: BTreeSet<TeamId>,
    /// Only these Assignees.
    pub analyst_ids: BTreeSet<AnalystId>,
    /// Only tickets without an Assignee.
    pub unassigned: bool,
    /// Only this Requester.
    pub requester_id: Option<RequesterId>,
    /// Any of these Requesters (Portal managers, FR-PRT-004); ignored when empty.
    pub requester_ids: BTreeSet<RequesterId>,
    /// Only this Department.
    pub department_id: Option<DepartmentId>,
    /// Only this Category.
    pub category_id: Option<Uuid>,
    /// Only this Sensitivity.
    pub sensitivity: Option<Sensitivity>,
    /// `Some(true)`: only snoozed; `Some(false)`: hide snoozed.
    pub snoozed: Option<bool>,
    /// Only tickets closed after this time.
    pub closed_since: Option<DateTime<Utc>>,
    /// Only soft-deleted tickets (needs `include_deleted` visibility).
    pub deleted: bool,
    /// Full-text search over number, subject, description and Notes (FR-TKT-041).
    pub search: Option<String>,
}

/// A ticket list request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketListQuery {
    /// Filters.
    pub filter: ListFilter,
    /// Sort column.
    pub sort: SortField,
    /// Newest / highest first.
    pub descending: bool,
    /// Continue after this `(sort value, id)`.
    pub after: Option<(String, Uuid)>,
    /// Page size; repositories fetch `limit + 1`.
    pub limit: Limit,
    /// The time used for snooze filters.
    pub now: Option<DateTime<Utc>>,
}

/// A ticket list row.
pub type TicketSummary = Ticket;
/// A ticket loaded for display.
pub type TicketView = Ticket;

/// Ticket reference data.
#[async_trait]
pub trait ReferenceData: Send + Sync {
    /// Every row of a kind, ordered.
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError>;
    /// Stores a row; when it is the new default, other rows of its kind lose the flag in
    /// the same transaction.
    async fn save_lookup(
        &self,
        lookup: &Lookup,
        audit: &AuditRecord,
        created: bool,
    ) -> Result<(), RepoError>;
    /// Every Category.
    async fn categories(&self) -> Result<Vec<Category>, RepoError>;
    /// Stores a Category.
    async fn save_category(
        &self,
        category: &Category,
        audit: &AuditRecord,
        created: bool,
    ) -> Result<(), RepoError>;
    /// Custom Ticket Fields (FR-TKT-029), active and retired.
    async fn ticket_fields(&self) -> Result<Vec<TicketField>, RepoError>;
    /// Inserts or updates a Ticket Field.
    async fn save_ticket_field(
        &self,
        field: &TicketField,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Ticket Type Rules (FR-TKT-011).
    async fn type_rules(&self) -> Result<Vec<TicketTypeRule>, RepoError>;
    /// Inserts or updates a Ticket Type Rule.
    async fn save_type_rule(
        &self,
        rule: &TicketTypeRule,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Deletes a Ticket Type Rule (rules have no dependants).
    async fn delete_type_rule(
        &self,
        rule: &TicketTypeRule,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
}

/// Tickets, Notes and Time Entries (tenant-scoped).
#[async_trait]
pub trait TicketRepository: Send + Sync {
    /// Takes the next value of the Company's Ticket Number sequence (committed at once;
    /// a failed create leaves a gap, never a duplicate).
    async fn next_sequence(&self, tenant: TenantId) -> Result<u64, RepoError>;
    /// Inserts a ticket with its audit record and events.
    async fn insert(
        &self,
        ticket: &Ticket,
        fields: &std::collections::BTreeMap<Uuid, serde_json::Value>,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Updates a ticket with its audit record and events.
    async fn update(
        &self,
        ticket: &Ticket,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Loads a ticket visible to the caller.
    async fn get(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        id: TicketId,
    ) -> Result<Option<Ticket>, RepoError>;
    /// Loads a ticket by its number.
    async fn get_by_number(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        number: &str,
    ) -> Result<Option<Ticket>, RepoError>;
    /// Lists tickets.
    async fn list(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        query: &TicketListQuery,
    ) -> Result<Vec<Ticket>, RepoError>;
    /// Adds a Note with its audit record and events.
    async fn add_note(
        &self,
        note: &Note,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Notes of a ticket, oldest first.
    async fn notes(&self, scope: &TenantScope, ticket: TicketId) -> Result<Vec<Note>, RepoError>;
    /// Adds a Time Entry.
    async fn add_time_entry(&self, entry: &TimeEntry, audit: &AuditRecord)
    -> Result<(), RepoError>;
    /// Time Entries of a ticket.
    async fn time_entries(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TimeEntry>, RepoError>;
    /// Audit history of a ticket, oldest first.
    async fn history(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<cooper_kernel::AuditRecord>, RepoError>;
}

/// What the service desk needs to know about a Department.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartmentInfo {
    /// Sensitivity of new tickets.
    pub default_sensitivity: Sensitivity,
    /// Whether it is active.
    pub is_active: bool,
}

/// Read access to the identity context (people, Teams, Departments, Companies), through
/// its public application API.
#[async_trait]
pub trait Directory: Send + Sync {
    /// The Company of a Requester, if they exist and are visible in `scope`.
    async fn requester_company(
        &self,
        scope: &TenantScope,
        id: RequesterId,
    ) -> Result<Option<TenantId>, RepoError>;
    /// A Department.
    async fn department(&self, id: DepartmentId) -> Result<Option<DepartmentInfo>, RepoError>;
    /// Members of an active Team (`None` when it does not exist or is inactive).
    async fn team_members(&self, id: TeamId) -> Result<Option<BTreeSet<AnalystId>>, RepoError>;
    /// Whether an Analyst exists and is active.
    async fn analyst_is_active(&self, id: AnalystId) -> Result<bool, RepoError>;
    /// Departments served by the active Teams an Analyst belongs to.
    async fn departments_served_by(
        &self,
        analyst: AnalystId,
    ) -> Result<BTreeSet<DepartmentId>, RepoError>;
    /// Teams an Analyst belongs to.
    async fn teams_of(&self, analyst: AnalystId) -> Result<BTreeSet<TeamId>, RepoError>;
    /// A Company's Ticket Code (`None` when the Company is not visible).
    async fn ticket_code(
        &self,
        scope: &TenantScope,
        tenant: TenantId,
    ) -> Result<Option<String>, RepoError>;
    /// A Requester's name and email.
    async fn requester_contact(
        &self,
        scope: &TenantScope,
        id: RequesterId,
    ) -> Result<Option<RequesterContact>, RepoError>;
    /// An Analyst's name and Signature.
    async fn analyst_profile(&self, id: AnalystId) -> Result<Option<AnalystProfile>, RepoError>;
    /// Requesters with this email address in any Company: `(id, Company, active)`.
    async fn requesters_by_email(
        &self,
        email: &str,
    ) -> Result<Vec<(RequesterId, TenantId, bool)>, RepoError>;
    /// The Company whose Sender Domains include `domain` (never a Freemail Domain).
    async fn company_for_domain(&self, domain: &str) -> Result<Option<TenantId>, RepoError>;
    /// Creates a Requester for an unknown sender (FR-EML-004).
    async fn create_requester(
        &self,
        tenant: TenantId,
        name: &str,
        email: &str,
    ) -> Result<RequesterId, RepoError>;
    /// The Requesters whose tickets a Portal principal may read as a Manager
    /// (FR-PRT-004). Empty unless Managers are switched on.
    async fn managed_scope(
        &self,
        principal: &cooper_kernel::Principal,
    ) -> Result<ManagedTickets, RepoError>;
}

/// Service desk settings Administrators control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketSettings {
    /// Ticket Number format.
    pub number_format: String,
    /// Closing needs a Closure Category.
    pub require_closure_category: bool,
    /// The Assignee must belong to the Queue.
    pub assignee_must_be_team_member: bool,
    /// Share of an SLA target after which it is in warning.
    pub sla_warning_percent: u8,
    /// `From` of Replies.
    pub from_address: String,
    /// Largest attachment in bytes (FR-GEN-007).
    pub max_attachment_bytes: u64,
}

impl Default for TicketSettings {
    fn default() -> Self {
        Self {
            number_format: super::NumberFormat::DEFAULT.to_owned(),
            require_closure_category: false,
            assignee_must_be_team_member: true,
            sla_warning_percent: 80,
            from_address: "Cooper <cooper@localhost>".to_owned(),
            max_attachment_bytes: 25 * 1024 * 1024,
        }
    }
}

/// Reads [`TicketSettings`].
#[async_trait]
pub trait TicketSettingsProvider: Send + Sync {
    /// The settings now in force.
    async fn settings(&self) -> TicketSettings;
}

/// SLA configuration.
#[async_trait]
pub trait SlaRepository: Send + Sync {
    /// Every SLA Calendar.
    async fn calendars(&self) -> Result<Vec<super::SlaCalendar>, RepoError>;
    /// Stores a calendar; a new default takes the flag from the previous one.
    async fn save_calendar(
        &self,
        calendar: &super::SlaCalendar,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Every SLA Notification Rule.
    async fn rules(&self) -> Result<Vec<super::SlaNotificationRule>, RepoError>;
    /// Stores a rule.
    async fn save_rule(
        &self,
        rule: &super::SlaNotificationRule,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
}

/// Attachment metadata as the service desk sees it (content is in the blob store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentInfo {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// The ticket.
    pub ticket_id: TicketId,
    /// File name.
    pub file_name: String,
    /// Detected media type.
    pub media_type: String,
    /// Size in bytes.
    pub size_bytes: i64,
    /// Content hash.
    pub sha256: String,
    /// Who uploaded it.
    pub created_by: cooper_kernel::Actor,
    /// When.
    pub created_at: DateTime<Utc>,
}

/// Ticket emails, Reply Templates and attachments.
#[async_trait]
pub trait ContentRepository: Send + Sync {
    /// Stores an email with its audit record and events.
    async fn add_email(
        &self,
        email: &super::TicketEmail,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Emails of a ticket, oldest first.
    async fn emails(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<super::TicketEmail>, RepoError>;
    /// The ticket an email with one of these `Message-ID`s belongs to (threading).
    async fn ticket_for_messages(
        &self,
        message_ids: &[String],
    ) -> Result<Option<TicketId>, RepoError>;
    /// Whether an inbound email with this `Message-ID` was already imported.
    async fn inbound_exists(&self, message_id: &str) -> Result<bool, RepoError>;
    /// Whether an outbound email with this `Message-ID` was already recorded.
    async fn outbound_exists(&self, message_id: &str) -> Result<bool, RepoError>;
    /// Every Email Template.
    async fn email_templates(&self) -> Result<Vec<super::EmailTemplate>, RepoError>;
    /// Stores an Email Template.
    async fn save_email_template(
        &self,
        template: &super::EmailTemplate,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Every Reply Template.
    async fn templates(&self) -> Result<Vec<super::ReplyTemplate>, RepoError>;
    /// Stores a Reply Template.
    async fn save_template(
        &self,
        template: &super::ReplyTemplate,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Stores attachment metadata (after the content).
    async fn add_attachment(
        &self,
        attachment: &AttachmentInfo,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Attachments of a ticket.
    async fn attachments(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<AttachmentInfo>, RepoError>;
}

/// Contact details of a Requester.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequesterContact {
    /// Display name.
    pub name: String,
    /// Email.
    pub email: String,
}

/// Name and Signature of an Analyst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalystProfile {
    /// Display name.
    pub name: String,
    /// Email Signature.
    pub signature: String,
    /// Email address.
    pub email: String,
    /// Whether the Analyst can sign in.
    pub is_active: bool,
}

/// Mailboxes, their credentials and the Rejected Email log.
#[async_trait]
pub trait MailboxRepository: Send + Sync {
    /// Every Mailbox.
    async fn mailboxes(&self) -> Result<Vec<super::Mailbox>, RepoError>;
    /// Stores a Mailbox; `secret` replaces the stored password or credentials when given.
    async fn save(
        &self,
        mailbox: &super::Mailbox,
        secret: Option<&str>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// The decrypted password or credentials.
    async fn secret(&self, mailbox: Uuid) -> Result<Option<String>, RepoError>;
    /// Stores poll health (and an event, e.g. after repeated failures).
    async fn record_health(
        &self,
        mailbox: &super::Mailbox,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Logs a rejected email.
    async fn log_rejected(
        &self,
        mailbox: Uuid,
        email: &super::InboundEmail,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    /// Recent rejected emails of a Mailbox: `(from, subject, reason, received)`.
    async fn rejected(
        &self,
        mailbox: Uuid,
        limit: i64,
    ) -> Result<Vec<(String, String, String, DateTime<Utc>)>, RepoError>;
}

/// A retired Ticket Number (FR-TKT-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiredNumber {
    /// The number.
    pub number: String,
    /// Why (`renumbered`).
    pub reason: String,
    /// When it stopped being the ticket's number.
    pub retired_at: DateTime<Utc>,
}

/// Relations between tickets and per-ticket extras (ADR-0024): Ticket Links, Ticket
/// Number History, Merge and Split, Ticket TicketPresence and Ticket Field values.
#[async_trait]
pub trait TicketRelations: Send + Sync {
    /// Links that involve `ticket`.
    async fn links(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TicketLink>, RepoError>;
    /// One link.
    async fn link(&self, scope: &TenantScope, id: Uuid) -> Result<Option<TicketLink>, RepoError>;
    /// The parent, grandparent, … of `ticket` (nearest first).
    async fn ancestors(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TicketId>, RepoError>;
    /// Stores a new link.
    async fn add_link(&self, link: &TicketLink, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Removes a link.
    async fn remove_link(&self, link: &TicketLink, audit: &AuditRecord) -> Result<(), RepoError>;

    /// Saves a renumbered ticket and retires `old_number` into the history, atomically.
    async fn renumber(
        &self,
        ticket: &Ticket,
        old_number: &str,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// The ticket's previous numbers, newest first.
    async fn number_history(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<RetiredNumber>, RepoError>;

    /// Merges: moves every conversation row of `source` to `target`, saves `source`
    /// (closed and pointing at the target), stores `link` and the merge record (with
    /// the moved rows filled in), all in one transaction. Returns the stored record.
    async fn merge(
        &self,
        source: &Ticket,
        target: &Ticket,
        merge: TicketMerge,
        link: Option<&TicketLink>,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<TicketMerge, RepoError>;
    /// One merge.
    async fn merge_record(
        &self,
        scope: &TenantScope,
        id: Uuid,
    ) -> Result<Option<TicketMerge>, RepoError>;
    /// Merges in which `ticket` was the source or the target, newest first.
    async fn merges_of(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<Vec<TicketMerge>, RepoError>;
    /// Undoes a merge: moves the recorded rows that are still on the target back,
    /// removes the merge's link, saves the restored source and the undone record.
    async fn unmerge(
        &self,
        source: &Ticket,
        merge: &TicketMerge,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;

    /// Splits: inserts `new_ticket`, moves `split.moved` (which must all belong to the
    /// source) to it, adds `marker` to the source, stores `link` and the split record.
    #[allow(clippy::too_many_arguments)]
    async fn split(
        &self,
        new_ticket: &Ticket,
        split: &TicketSplit,
        marker: &Note,
        link: &TicketLink,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// One split.
    async fn split_record(
        &self,
        scope: &TenantScope,
        id: Uuid,
    ) -> Result<Option<TicketSplit>, RepoError>;
    /// Conversation rows on `ticket` that are not in `moved` (new activity since a split).
    async fn other_rows(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
        moved: &MovedRows,
    ) -> Result<usize, RepoError>;
    /// Undoes a split: moves the rows back, deletes the marker Note and the link, saves
    /// `new_ticket` (soft-deleted) and the undone record.
    async fn unsplit(
        &self,
        split: &TicketSplit,
        new_ticket: &Ticket,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;

    /// Records a heartbeat; returns whether what others see changed (a new viewer or
    /// a change in composing), so only then is a Live Update sent.
    async fn heartbeat(
        &self,
        ticket: &Ticket,
        analyst: AnalystId,
        composing: bool,
        now: DateTime<Utc>,
    ) -> Result<bool, RepoError>;
    /// Removes the Analyst from the ticket; returns whether they were there.
    async fn leave(&self, ticket: &Ticket, analyst: AnalystId) -> Result<bool, RepoError>;
    /// Heartbeats on the ticket newer than `since`.
    async fn presence(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
        since: DateTime<Utc>,
    ) -> Result<Vec<TicketPresence>, RepoError>;

    /// Ticket Field values of a ticket, by field ID.
    async fn field_values(
        &self,
        scope: &TenantScope,
        ticket: TicketId,
    ) -> Result<std::collections::BTreeMap<Uuid, serde_json::Value>, RepoError>;
    /// Sets (`Some`) or clears (`None`) Ticket Field values.
    async fn set_field_values(
        &self,
        ticket: &Ticket,
        values: &std::collections::BTreeMap<Uuid, Option<serde_json::Value>>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
}

/// What a Portal Manager may read (FR-PRT-004).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedTickets {
    /// The covered Requesters.
    pub requester_ids: BTreeSet<RequesterId>,
    /// Whether their Confidential tickets are included.
    pub include_confidential: bool,
}
