//! Service desk use cases.

mod content;
mod email_templates;
mod intake;
mod portal;
mod reference;
mod sla;
mod tickets;

use std::sync::Arc;

use cooper_kernel::{Actor, Clock, Forbidden, Principal};
use thiserror::Error;

use crate::domain::{
    ContentRepository, Directory, MailboxRepository, ReferenceData, RepoError, SlaRepository,
    TicketError, TicketRepository, TicketSettingsProvider,
};

pub use content::{ReplyInput, StoredFile};
pub use intake::IntakeOutcome;
pub use portal::{PortalTicket, PortalTicketInput, PortalTimelineEntry};
pub use reference::LookupInput;
pub use tickets::{
    CreateTicket, ListRequest, TicketDetail, TicketView as View, TimelineEntry, ViewName,
};

/// Capabilities the service desk checks (FR-IAM-010).
pub mod capabilities {
    /// See tickets.
    pub const READ: &str = "tickets.read";
    /// Create tickets.
    pub const CREATE: &str = "tickets.create";
    /// Edit tickets, add Notes and Time Entries, snooze.
    pub const UPDATE: &str = "tickets.update";
    /// Assign tickets to a Queue and Assignee.
    pub const ASSIGN: &str = "tickets.assign";
    /// Close and reopen tickets.
    pub const CLOSE: &str = "tickets.close";
    /// Delete tickets (soft delete).
    pub const DELETE: &str = "tickets.delete";
    /// See every Confidential ticket.
    pub const VIEW_CONFIDENTIAL: &str = "tickets.view_confidential";
    /// Configure Ticket Statuses, Priorities, Types, Origins, Resolution Codes and
    /// Categories.
    pub const CONFIGURE: &str = "tickets.configure";

    /// Every Capability declared by the service desk.
    pub const ALL: &[&str] = &[
        READ,
        CREATE,
        UPDATE,
        ASSIGN,
        CLOSE,
        DELETE,
        VIEW_CONFIDENTIAL,
        CONFIGURE,
    ];
}

/// System Settings the service desk reads.
pub mod settings {
    use cooper_kernel::{SettingDefinition, SettingKind};

    /// Ticket Number format (FR-TKT-002).
    pub const NUMBER_FORMAT: SettingDefinition = SettingDefinition {
        key: "tickets.number_format",
        description: "Ticket Number format. Tokens: {COMPANY} (Ticket Code), {TYPE} (Ticket Type prefix), {YEAR}, and {SEQ:n} (sequence padded to n digits) exactly once.",
        kind: SettingKind::Text { max_chars: 50 },
        default: "\"{COMPANY}-{SEQ:6}\"",
    };

    /// Closing a ticket needs a Closure Category (FR-TKT-008).
    pub const REQUIRE_CLOSURE_CATEGORY: SettingDefinition = SettingDefinition {
        key: "tickets.require_closure_category",
        description: "Require a Closure Category when a ticket is closed.",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// The Assignee must be a member of the Queue (FR-TKT-010).
    pub const ASSIGNEE_MUST_BE_TEAM_MEMBER: SettingDefinition = SettingDefinition {
        key: "tickets.assignee_must_be_team_member",
        description: "When a ticket has both a Queue and an Assignee, the Assignee must belong to the Queue.",
        kind: SettingKind::Bool,
        default: "true",
    };

    /// Share of an SLA target after which it is in warning (FR-SLA-005).
    pub const SLA_WARNING_PERCENT: SettingDefinition = SettingDefinition {
        key: "sla.warning_percent",
        description: "An SLA target is in warning once this percentage of its working time has passed.",
        kind: SettingKind::Integer { min: 1, max: 99 },
        default: "80",
    };

    /// Largest attachment in megabytes (FR-GEN-007).
    pub const MAX_ATTACHMENT_MB: SettingDefinition = SettingDefinition {
        key: "attachments.max_size_mb",
        description: "Largest file that can be attached, in megabytes.",
        kind: SettingKind::Integer { min: 1, max: 100 },
        default: "25",
    };

    /// Every setting declared by the service desk.
    pub const ALL: &[SettingDefinition] = &[
        MAX_ATTACHMENT_MB,
        SLA_WARNING_PERCENT,
        NUMBER_FORMAT,
        REQUIRE_CLOSURE_CATEGORY,
        ASSIGNEE_MUST_BE_TEAM_MEMBER,
    ];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] TicketError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Conflict(constraint) => Self::Conflict(match constraint.as_str() {
                c if c.ends_with("_name_key") => "a row with this name already exists".into(),
                "ticket_categories_sibling_name_key" => {
                    "a sibling Category with this name exists".into()
                }
                "tickets_ticket_number_key" => {
                    "the Ticket Number is already used; check the number format".into()
                }
                _ => "the record conflicts with an existing one".into(),
            }),
            RepoError::MissingReference(constraint) => Self::Rule(TicketError::invalid(
                "reference",
                format!("refers to a record that does not exist ({constraint})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "service desk storage error");
                Self::Internal
            }
        }
    }
}

impl From<cooper_kernel::CursorError> for AppError {
    fn from(_: cooper_kernel::CursorError) -> Self {
        Self::Rule(TicketError::invalid("cursor", "is not valid"))
    }
}

/// The service desk's dependencies.
#[derive(Clone)]
pub struct ServiceDeskPorts {
    /// Tickets.
    pub tickets: Arc<dyn TicketRepository>,
    /// Reference data.
    pub reference: Arc<dyn ReferenceData>,
    /// SLA configuration.
    pub sla: Arc<dyn SlaRepository>,
    /// Emails, Reply Templates and attachments.
    pub content: Arc<dyn ContentRepository>,
    /// Mailboxes.
    pub mailboxes: Arc<dyn MailboxRepository>,
    /// The identity context.
    pub directory: Arc<dyn Directory>,
    /// Settings.
    pub settings: Arc<dyn TicketSettingsProvider>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// The service desk application service.
#[derive(Clone)]
pub struct ServiceDesk {
    ports: ServiceDeskPorts,
}

impl ServiceDesk {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: ServiceDeskPorts) -> Self {
        Self { ports }
    }

    /// The current time, for presenting SLA state.
    #[must_use]
    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.ports.clock.now()
    }
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}
