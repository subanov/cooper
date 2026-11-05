//! Email Templates (FR-EML-009): automated emails sent on ticket events, matched by
//! the recipient's address or domain.

use chrono::{DateTime, Utc};
use cooper_kernel::TenantId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::email::{check_placeholders, domain_of, valid_address};
use super::{Audited, Direction, Placeholders, Ticket, TicketEmail, TicketError, sdref_subject};

/// The ticket event an Email Template is sent on (fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailTemplateEvent {
    /// A ticket was created: sent to the Requester.
    TicketCreated,
    /// A ticket was assigned to an Analyst: sent to the new Assignee.
    TicketAssigned,
    /// The Requester replied by email: an acknowledgement to the Requester.
    TicketReplied,
    /// A ticket was closed: sent to the Requester.
    TicketClosed,
}

impl EmailTemplateEvent {
    /// Every event.
    pub const ALL: [Self; 4] = [
        Self::TicketCreated,
        Self::TicketAssigned,
        Self::TicketReplied,
        Self::TicketClosed,
    ];

    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TicketCreated => "ticket_created",
            Self::TicketAssigned => "ticket_assigned",
            Self::TicketReplied => "ticket_replied",
            Self::TicketClosed => "ticket_closed",
        }
    }

    /// Parses a storage name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == s)
    }

    /// Whether the email goes to the Assignee (otherwise to the Requester).
    #[must_use]
    pub const fn to_assignee(self) -> bool {
        matches!(self, Self::TicketAssigned)
    }
}

/// Editable fields of an Email Template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailTemplateInput {
    /// Name.
    pub name: String,
    /// Event it is sent on.
    pub event: EmailTemplateEvent,
    /// Recipient address (`sam@acme.example`) or domain (`acme.example`) this template
    /// is for; `None` for everyone else.
    pub match_pattern: Option<String>,
    /// Subject with placeholders (the SDREF tag is added).
    pub subject: String,
    /// Body with placeholders.
    pub body: String,
    /// Sent when active.
    pub is_active: bool,
}

/// An automated email sent on a ticket event (FR-EML-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailTemplate {
    id: Uuid,
    input: EmailTemplateInput,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn normalise(mut i: EmailTemplateInput) -> Result<EmailTemplateInput, TicketError> {
    i.name = i.name.trim().to_owned();
    if i.name.is_empty() || i.name.chars().count() > 100 {
        return Err(TicketError::invalid("name", "must be 1 to 100 characters"));
    }
    i.subject = i.subject.trim().to_owned();
    if i.subject.is_empty() || i.subject.chars().count() > 200 {
        return Err(TicketError::invalid(
            "subject",
            "must be 1 to 200 characters",
        ));
    }
    if i.body.trim().is_empty() || i.body.chars().count() > 20_000 {
        return Err(TicketError::invalid(
            "body",
            "must be 1 to 20000 characters",
        ));
    }
    check_placeholders("subject", &i.subject)?;
    check_placeholders("body", &i.body)?;
    i.match_pattern = match i.match_pattern.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(p) => {
            let p = p.trim_start_matches('@').to_lowercase();
            let is_domain = !p.contains('@')
                && p.contains('.')
                && !p.starts_with('.')
                && !p.contains(char::is_whitespace);
            if !is_domain && !valid_address(&p) {
                return Err(TicketError::invalid(
                    "match_pattern",
                    "must be an email address or a domain",
                ));
            }
            Some(p)
        }
    };
    Ok(i)
}

impl EmailTemplate {
    /// Creates a template.
    ///
    /// # Errors
    ///
    /// Invalid fields or an unknown placeholder.
    pub fn create(input: EmailTemplateInput, now: DateTime<Utc>) -> Result<Self, TicketError> {
        Ok(Self {
            id: Uuid::now_v7(),
            input: normalise(input)?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a template from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        input: EmailTemplateInput,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            input,
            created_at,
            updated_at,
        }
    }

    /// Replaces the template.
    ///
    /// # Errors
    ///
    /// Invalid fields or an unknown placeholder.
    pub fn update(
        &mut self,
        input: EmailTemplateInput,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        self.input = normalise(input)?;
        self.updated_at = now;
        Ok(())
    }

    /// How well this template matches `recipient`: 2 for its address, 1 for its domain
    /// (or a subdomain, as in the Mailbox Whitelist),
    /// 0 for a template without a pattern, `None` when it does not apply.
    #[must_use]
    pub fn specificity(&self, recipient: &str) -> Option<u8> {
        let recipient = recipient.trim().to_lowercase();
        match &self.input.match_pattern {
            None => Some(0),
            Some(p) if p.contains('@') => (*p == recipient).then_some(2),
            Some(p) => {
                let domain = recipient.rsplit('@').next().unwrap_or_default();
                (domain == p || domain.ends_with(&format!(".{p}"))).then_some(1)
            }
        }
    }

    /// Composes the outbound email for `ticket` to `to`. The `Message-ID` is derived
    /// from `source_event`, so a redelivered event never sends twice.
    ///
    /// # Errors
    ///
    /// A deleted ticket or an invalid recipient address.
    #[allow(clippy::too_many_arguments)]
    pub fn compose(
        &self,
        ticket: &Ticket,
        to: &str,
        from: &str,
        values: &Placeholders,
        source_event: Uuid,
        in_reply_to: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<TicketEmail, TicketError> {
        if ticket.deleted_at().is_some() {
            return Err(TicketError::Deleted);
        }
        if !valid_address(to) {
            return Err(TicketError::invalid(
                "to",
                "the recipient has no valid email address",
            ));
        }
        Ok(TicketEmail {
            id: Uuid::now_v7(),
            tenant_id: ticket.tenant_id(),
            ticket_id: ticket.id(),
            direction: Direction::Outbound,
            from: from.to_owned(),
            to: vec![to.trim().to_owned()],
            cc: Vec::new(),
            subject: sdref_subject(ticket.number(), &values.fill(&self.input.subject)),
            text_body: values.fill(&self.input.body),
            message_id: format!("<auto-{source_event}@{}>", domain_of(from)),
            author_id: None,
            occurred_at: now,
            mailbox_id: None,
            requester_id: None,
            in_reply_to,
        })
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Fields.
    #[must_use]
    pub const fn input(&self) -> &EmailTemplateInput {
        &self.input
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

/// The active template for `event` that best matches `recipient`: its address first,
/// then its domain, then a template without a pattern.
#[must_use]
pub fn select_template<'a>(
    templates: &'a [EmailTemplate],
    event: EmailTemplateEvent,
    recipient: &str,
) -> Option<&'a EmailTemplate> {
    templates
        .iter()
        .filter(|t| t.input.is_active && t.input.event == event)
        .filter_map(|t| t.specificity(recipient).map(|s| (s, t)))
        .max_by_key(|(s, t)| (*s, std::cmp::Reverse(t.created_at)))
        .map(|(_, t)| t)
}

impl Audited for EmailTemplate {
    fn audit_type(&self) -> &'static str {
        "email_template"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let i = &self.input;
        vec![
            ("name", json!(i.name)),
            ("event", json!(i.event)),
            ("match_pattern", json!(i.match_pattern)),
            ("subject", json!(i.subject)),
            ("body", json!(i.body)),
            ("is_active", json!(i.is_active)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(event: EmailTemplateEvent, pattern: Option<&str>) -> EmailTemplateInput {
        EmailTemplateInput {
            name: format!("{event:?} {pattern:?}"),
            event,
            match_pattern: pattern.map(str::to_owned),
            subject: "We received {ticket_subject}".into(),
            body: "Hi {requester_name}, your ticket is {ticket_number}.".into(),
            is_active: true,
        }
    }

    #[test]
    fn patterns_are_normalised_and_validated() {
        let t = EmailTemplate::create(
            input(EmailTemplateEvent::TicketCreated, Some(" @ACME.example ")),
            Utc::now(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(t.input().match_pattern.as_deref(), Some("acme.example"));
        let blank = EmailTemplate::create(
            input(EmailTemplateEvent::TicketCreated, Some("  ")),
            Utc::now(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(blank.input().match_pattern, None);
        for bad in ["acme", "sam@", "a b.example"] {
            assert!(
                EmailTemplate::create(
                    input(EmailTemplateEvent::TicketCreated, Some(bad)),
                    Utc::now()
                )
                .is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn unknown_placeholders_are_rejected_in_subject_and_body() {
        let mut i = input(EmailTemplateEvent::TicketClosed, None);
        i.subject = "Closed {ticket_nmuber}".into();
        assert!(matches!(
            EmailTemplate::create(i, Utc::now()),
            Err(TicketError::Invalid {
                field: "subject",
                ..
            })
        ));
    }

    #[test]
    fn address_beats_domain_beats_default() {
        let now = Utc::now();
        let make = |e, p| EmailTemplate::create(input(e, p), now).unwrap_or_else(|e| panic!("{e}"));
        let default = make(EmailTemplateEvent::TicketCreated, None);
        let domain = make(EmailTemplateEvent::TicketCreated, Some("acme.example"));
        let address = make(EmailTemplateEvent::TicketCreated, Some("sam@acme.example"));
        let closed = make(EmailTemplateEvent::TicketClosed, Some("sam@acme.example"));
        let all = vec![default.clone(), domain.clone(), address.clone(), closed];
        let pick = |r| select_template(&all, EmailTemplateEvent::TicketCreated, r).map(|t| t.id());
        assert_eq!(pick("Sam@Acme.example"), Some(address.id()));
        assert_eq!(pick("lee@acme.example"), Some(domain.id()));
        assert_eq!(pick("lee@eu.acme.example"), Some(domain.id()));
        assert_eq!(pick("lee@notacme.example"), Some(default.id()));
        assert_eq!(pick("lee@other.example"), Some(default.id()));
        assert_eq!(
            select_template(&all, EmailTemplateEvent::TicketAssigned, "sam@acme.example"),
            None
        );
    }

    #[test]
    fn inactive_templates_are_never_selected() {
        let mut i = input(EmailTemplateEvent::TicketCreated, None);
        i.is_active = false;
        let t = EmailTemplate::create(i, Utc::now()).unwrap_or_else(|e| panic!("{e}"));
        assert!(select_template(&[t], EmailTemplateEvent::TicketCreated, "a@b.example").is_none());
    }

    #[test]
    fn events_round_trip() {
        for e in EmailTemplateEvent::ALL {
            assert_eq!(EmailTemplateEvent::parse(e.as_str()), Some(e));
        }
        assert_eq!(EmailTemplateEvent::parse("ticket_deleted"), None);
    }
}
