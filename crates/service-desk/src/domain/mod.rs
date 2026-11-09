//! Service desk domain. Pure Rust: no I/O.

mod catalogue;
mod email;
mod email_template;
mod events;
mod fields;
mod links;
mod lookups;
mod mailbox;
mod merge;
mod number;
mod ports;
mod sla;
mod statistics;
mod ticket;
mod type_rules;

pub use catalogue::{portal_catalogue, portal_ticket_type};
pub use email::{
    Direction, PLACEHOLDERS, Placeholders, PortalReplyDraft, ReplyDraft, ReplyTemplate,
    TicketEmail, parse_sdref, sdref_subject,
};
pub use email_template::{EmailTemplate, EmailTemplateEvent, EmailTemplateInput, select_template};
pub use events::{
    MailboxPollFailed, ReplyEmail, SlaBreached, SlaWarningRaised, TicketAssigned,
    TicketAutomaticEmailSent, TicketClosed, TicketCreated, TicketEmailReceived, TicketMergeUndone,
    TicketMerged, TicketNoteAdded, TicketPriorityChanged, TicketRenumbered, TicketReopened,
    TicketReplySent, TicketSplit as TicketSplitEvent, TicketSplitUndone, TicketStatusChanged,
    mentions,
};
pub use fields::{
    FieldCheck, FieldType, TicketField, TicketFieldInput, missing_required, parse_values,
};
pub use links::{LinkKind, LinkRelation, TicketLink, check_no_cycle};
pub use lookups::{Category, CategoryInput, Lookup, LookupFlags, LookupKind, MAX_CATEGORY_DEPTH};
pub use mailbox::{
    AfterImport, AfterReject, FAILURES_BEFORE_ALERT, ImapSettings, InboundEmail, Mailbox,
    MailboxHealth, MailboxSettings, OAuthCredentials, Provider, Security, WhitelistEntry,
};
pub use merge::{
    MovedRows, PRESENCE_STALE_SECONDS, SourceState, TicketMerge, TicketPresence, TicketSplit,
    Undone,
};
pub use number::{NumberContext, NumberFormat};
pub use ports::{
    AnalystProfile, AttachmentInfo, ContentRepository, DepartmentInfo, Directory, ListFilter,
    MailboxRepository, ManagedTickets, ReferenceData, RepoError, RequesterContact, RetiredNumber,
    SlaRepository, SortField, TicketListQuery, TicketRelations, TicketRepository, TicketSettings,
    TicketSettingsProvider, TicketSummary, TicketView, Visibility,
};
pub use sla::{
    DayHours, RuleTarget, SlaCalendar, SlaCalendarInput, SlaNotificationRule, SlaRuleInput,
    SlaTarget, SlaTrigger, TargetState, WorkingTime,
};
pub use statistics::{
    GroupValue, Interval, SlaTally, StatisticsFilter, TargetTally, TicketGrouping,
    TicketStatistics, TicketTally, TimeGrouping, TimeTally,
};
pub use ticket::{
    NewTicket, Note, SlaTargets, StatusTransition, Ticket, TicketDetails, TicketSla, TimeEntry,
};
pub use type_rules::{
    Channel, RuleSubject, TicketTypeRule, TicketTypeRuleConditions, TicketTypeRuleInput,
    choose_type,
};

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditAction, AuditRecord, TenantId};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

/// A business rule of the service desk was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TicketError {
    /// A value failed validation.
    #[error("{field} {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Closing needs a Resolution Code.
    #[error("closing a ticket needs a Resolution Code")]
    ResolutionCodeRequired,
    /// Closing needs a Closure Category (System Setting).
    #[error("closing a ticket needs a Closure Category")]
    ClosureCategoryRequired,
    /// The Assignee is not a member of the Queue.
    #[error("the Assignee must be a member of the assigned Team")]
    AssigneeNotInTeam,
    /// The ticket is closed and cannot be changed this way.
    #[error("the ticket is closed")]
    Closed,
    /// The ticket is deleted.
    #[error("the ticket is deleted")]
    Deleted,
    /// The record is not in a state that allows the change.
    #[error("{0}")]
    InvalidState(&'static str),
    /// Ticket Fields that must have a value first (FR-TKT-008, FR-TKT-029).
    #[error("these Ticket Fields need a value: {}", .0.join(", "))]
    FieldsRequired(Vec<String>),
}

impl TicketError {
    /// An [`TicketError::Invalid`].
    #[must_use]
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }
}

/// A record whose fields are audited.
pub trait Audited {
    /// Aggregate type in the audit log.
    fn audit_type(&self) -> &'static str;
    /// Aggregate ID.
    fn audit_id(&self) -> Uuid;
    /// Company.
    fn audit_tenant(&self) -> Option<TenantId>;
    /// Audited fields.
    fn audit_fields(&self) -> Vec<(&'static str, Value)>;
}

/// The audit record of a change from `before` to `after` (created when `before` is `None`).
#[must_use]
pub fn audit_record<A: Audited>(
    before: Option<&A>,
    after: &A,
    action: AuditAction,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let mut record = AuditRecord::new(
        after.audit_tenant(),
        after.audit_type(),
        after.audit_id(),
        action,
        actor,
        now,
    );
    let old = before.map(Audited::audit_fields).unwrap_or_default();
    for (field, new) in after.audit_fields() {
        let previous = old.iter().find(|(n, _)| *n == field).map(|(_, v)| v);
        record = record.change(field, previous, Some(&new));
    }
    record
}
