//! Domain events of the service desk (ADR-0005). The SLA clock and notifications react
//! to them.

use chrono::{DateTime, Utc};
use cooper_kernel::{AnalystId, DomainEvent, TeamId, TenantId, TicketId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! ticket_event {
    ($(#[$meta:meta])* $name:ident, $type:literal { $($(#[$fmeta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name {
            /// The ticket.
            pub ticket_id: TicketId,
            /// Its Company.
            pub tenant_id: TenantId,
            $($(#[$fmeta])* pub $field: $ty,)*
        }

        impl DomainEvent for $name {
            const EVENT_TYPE: &'static str = $type;
            const AGGREGATE_TYPE: &'static str = "ticket";
            fn aggregate_id(&self) -> Uuid {
                self.ticket_id.as_uuid()
            }
            fn tenant_id(&self) -> Option<TenantId> {
                Some(self.tenant_id)
            }
        }
    };
}

ticket_event!(
    /// A ticket was created.
    TicketCreated, "ticket.created" {
        /// Ticket Number.
        number: String,
        /// Priority.
        priority_id: Uuid,
        /// Status.
        status_id: Uuid,
        /// Creation time.
        created_at: DateTime<Utc>,
    }
);

ticket_event!(
    /// The Queue or Assignee changed.
    TicketAssigned, "ticket.assigned" {
        /// New Queue.
        team_id: Option<TeamId>,
        /// New Assignee.
        analyst_id: Option<AnalystId>,
        /// Previous Assignee.
        previous_analyst_id: Option<AnalystId>,
    }
);

ticket_event!(
    /// The status changed between open statuses.
    TicketStatusChanged, "ticket.status_changed" {
        /// Previous status.
        from_status_id: Uuid,
        /// New status.
        to_status_id: Uuid,
    }
);

ticket_event!(
    /// The ticket was closed.
    TicketClosed, "ticket.closed" {
        /// Closing status.
        status_id: Uuid,
        /// Resolution Code.
        resolution_code_id: Uuid,
        /// When.
        closed_at: DateTime<Utc>,
    }
);

ticket_event!(
    /// A closed ticket was reopened.
    TicketReopened, "ticket.reopened" {
        /// New status.
        status_id: Uuid,
    }
);

ticket_event!(
    /// The Priority changed (SLA targets are recalculated, FR-SLA-001).
    TicketPriorityChanged, "ticket.priority_changed" {
        /// Previous Priority.
        from_priority_id: Uuid,
        /// New Priority.
        to_priority_id: Uuid,
    }
);

ticket_event!(
    /// An SLA target reached its warning point (FR-SLA-005). Raised once per target.
    SlaWarningRaised, "ticket.sla_warning" {
        /// Response or resolution.
        target: crate::domain::SlaTarget,
        /// When the target is due.
        due_at: DateTime<Utc>,
    }
);

ticket_event!(
    /// An SLA target was missed (FR-SLA-005). Raised once per target.
    SlaBreached, "ticket.sla_breached" {
        /// Response or resolution.
        target: crate::domain::SlaTarget,
        /// When the target was due.
        due_at: DateTime<Utc>,
    }
);

ticket_event!(
    /// A Note was added (FR-TKT-020). Carries the `@username` mentions in its text so
    /// Notifications can tell the mentioned Analysts (FR-GEN-006).
    TicketNoteAdded, "ticket.note_added" {
        /// The Note.
        note_id: Uuid,
        /// Its author.
        author_id: Option<AnalystId>,
        /// Visible to the Requester.
        is_public: bool,
        /// Usernames mentioned with `@`.
        mentions: Vec<String>,
    }
);

/// `@username` mentions in a text: `@` at the start or after whitespace, followed by
/// letters, digits, `.`, `_` or `-`. Unique, in order of appearance.
#[must_use]
pub fn mentions(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c != '@' || (i > 0 && !chars[i - 1].is_whitespace()) {
            continue;
        }
        let name: String = chars[i + 1..]
            .iter()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .collect();
        let name = name.trim_end_matches('.').to_owned();
        if !name.is_empty() && !found.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
            found.push(name);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::mentions;

    #[test]
    fn finds_mentions_but_not_email_addresses() {
        assert_eq!(
            mentions("@jdoe can you check? cc @Ann.Lee. Mail bob@example.com @jdoe"),
            ["jdoe", "Ann.Lee"]
        );
        assert!(mentions("no mentions @ all").is_empty());
    }
}

/// The email part of a [`TicketReplySent`]: the published shape the notifications
/// context queues for sending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyEmail {
    /// ID of the queued email (the ticket email ID).
    pub id: Uuid,
    /// Company.
    pub tenant_id: Option<TenantId>,
    /// `ticket_reply` or `email_template`.
    pub kind: String,
    /// `From`.
    pub from: String,
    /// `To`.
    pub to: Vec<String>,
    /// `Cc`.
    pub cc: Vec<String>,
    /// Subject with SDREF.
    pub subject: String,
    /// Body with Signature.
    pub text_body: String,
    /// `Message-ID`.
    pub message_id: String,
    /// `In-Reply-To`.
    pub in_reply_to: Option<String>,
    /// `References`.
    pub references: Vec<String>,
}

ticket_event!(
    /// An Analyst replied to the Requester (FR-TKT-021). The notifications context
    /// queues the email.
    TicketReplySent, "ticket.reply_sent" {
        /// The email to send.
        email: ReplyEmail,
    }
);

ticket_event!(
    /// An Email Template was sent for a ticket event (FR-EML-009). The notifications
    /// context queues the email. Unlike a Reply, it is not an SLA response.
    TicketAutomaticEmailSent, "ticket.automatic_email_sent" {
        /// The email to send.
        email: ReplyEmail,
    }
);

ticket_event!(
    /// An email from the Requester arrived on an existing ticket (FR-EML-003). The
    /// Assignee is notified (FR-GEN-006).
    TicketEmailReceived, "ticket.email_received" {
        /// The email.
        email_id: Uuid,
        /// Sender.
        from: String,
        /// Whether the email reopened a closed ticket.
        reopened: bool,
    }
);

/// A Mailbox failed to poll several times in a row (FR-EML-010).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxPollFailed {
    /// The Mailbox.
    pub mailbox_id: Uuid,
    /// Its name.
    pub name: String,
    /// The last error.
    pub error: String,
    /// Failures in a row.
    pub failures: u32,
}

impl DomainEvent for MailboxPollFailed {
    const EVENT_TYPE: &'static str = "mailbox.poll_failed";
    const AGGREGATE_TYPE: &'static str = "mailbox";
    fn aggregate_id(&self) -> Uuid {
        self.mailbox_id
    }
    fn tenant_id(&self) -> Option<TenantId> {
        None
    }
}
