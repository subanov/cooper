//! Ticket emails, Replies with SDREF (FR-TKT-021) and Reply Templates (FR-TKT-022).

use chrono::{DateTime, Utc};
use cooper_kernel::{AnalystId, TenantId, TicketId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Audited, Ticket, TicketError};

/// `[SDREF:<number>] <subject>`: the tag that threads replies back onto the ticket.
#[must_use]
pub fn sdref_subject(number: &str, subject: &str) -> String {
    let tag = format!("[SDREF:{number}]");
    if subject.contains(&tag) {
        subject.to_owned()
    } else {
        format!("{tag} {subject}")
    }
}

/// The Ticket Number in an `[SDREF:…]` tag, if the subject has one (FR-EML-003).
#[must_use]
pub fn parse_sdref(subject: &str) -> Option<String> {
    let start = subject.find("[SDREF:")? + "[SDREF:".len();
    let end = subject[start..].find(']')? + start;
    let number = subject[start..end].trim();
    (!number.is_empty() && number.len() <= 50).then(|| number.to_owned())
}

/// Direction of an email (fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Received.
    Inbound,
    /// Sent.
    Outbound,
}

impl Direction {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}

/// An email on a ticket's timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketEmail {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// Ticket.
    pub ticket_id: TicketId,
    /// Direction.
    pub direction: Direction,
    /// `From`.
    pub from: String,
    /// `To`.
    pub to: Vec<String>,
    /// `Cc`.
    pub cc: Vec<String>,
    /// Subject.
    pub subject: String,
    /// Plain-text body.
    pub text_body: String,
    /// `Message-ID` with angle brackets.
    pub message_id: String,
    /// The Analyst who wrote an outbound email.
    pub author_id: Option<AnalystId>,
    /// Sent or received.
    pub occurred_at: DateTime<Utc>,
    /// The Mailbox an inbound email arrived in.
    pub mailbox_id: Option<Uuid>,
    /// The Requester who sent an inbound email.
    pub requester_id: Option<cooper_kernel::RequesterId>,
    /// `In-Reply-To`.
    pub in_reply_to: Option<String>,
}

/// What a Reply needs besides the ticket.
#[derive(Debug, Clone)]
pub struct ReplyDraft {
    /// The Requester's address.
    pub to: String,
    /// Copy recipients.
    pub cc: Vec<String>,
    /// The text the Analyst wrote.
    pub body: String,
    /// The Analyst's Signature (appended when not empty).
    pub signature: String,
    /// `From`.
    pub from: String,
    /// The Analyst.
    pub author_id: Option<AnalystId>,
    /// `Message-ID` of the last email on the ticket, for threading.
    pub in_reply_to: Option<String>,
}

pub(super) fn domain_of(from: &str) -> &str {
    let addr = from
        .rsplit('<')
        .next()
        .unwrap_or(from)
        .trim_end_matches('>');
    addr.rsplit('@')
        .next()
        .filter(|d| !d.is_empty() && !d.contains(' '))
        .unwrap_or("localhost")
}

pub(super) fn valid_address(a: &str) -> bool {
    let a = a.trim();
    a.len() <= 254
        && a.split_once('@')
            .is_some_and(|(l, d)| !l.is_empty() && d.contains('.'))
        && !a.contains(char::is_whitespace)
}

impl TicketEmail {
    /// Composes an outbound Reply to the Requester (FR-TKT-021).
    ///
    /// # Errors
    ///
    /// Deleted ticket, an empty body, or invalid addresses.
    pub fn reply(
        ticket: &Ticket,
        draft: ReplyDraft,
        now: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        if ticket.deleted_at().is_some() {
            return Err(TicketError::Deleted);
        }
        let body = draft.body.trim();
        if body.is_empty() || body.chars().count() > 100_000 {
            return Err(TicketError::invalid(
                "body",
                "must be 1 to 100000 characters",
            ));
        }
        if !valid_address(&draft.to) {
            return Err(TicketError::invalid(
                "to",
                "the Requester has no valid email address",
            ));
        }
        if draft.cc.len() > 20 || draft.cc.iter().any(|c| !valid_address(c)) {
            return Err(TicketError::invalid(
                "cc",
                "must be at most 20 valid email addresses",
            ));
        }
        let signature = draft.signature.trim();
        let text_body = if signature.is_empty() {
            body.to_owned()
        } else {
            format!("{body}\n\n-- \n{signature}")
        };
        let id = Uuid::now_v7();
        Ok(Self {
            id,
            tenant_id: ticket.tenant_id(),
            ticket_id: ticket.id(),
            direction: Direction::Outbound,
            message_id: format!("<{id}@{}>", domain_of(&draft.from)),
            from: draft.from,
            to: vec![draft.to.trim().to_owned()],
            cc: draft.cc.iter().map(|c| c.trim().to_owned()).collect(),
            subject: sdref_subject(ticket.number(), &ticket.details().subject),
            text_body,
            author_id: draft.author_id,
            occurred_at: now,
            mailbox_id: None,
            requester_id: None,
            in_reply_to: draft.in_reply_to,
        })
    }
}

/// A reply a Requester writes on the Self-Service Portal (FR-PRT-003).
#[derive(Debug, Clone)]
pub struct PortalReplyDraft {
    /// The signed-in Requester.
    pub requester_id: cooper_kernel::RequesterId,
    /// The Requester's email address.
    pub from: String,
    /// Text.
    pub body: String,
    /// The service desk's From address (the reply is addressed to it).
    pub service_desk: String,
    /// Message-ID of the newest message on the ticket.
    pub in_reply_to: Option<String>,
}

impl TicketEmail {
    /// Records a Portal reply as an inbound message, so it threads, notifies and reopens
    /// exactly like an emailed reply (FR-PRT-003).
    ///
    /// # Errors
    ///
    /// Deleted ticket or a body that is empty or longer than 100000 characters.
    pub fn portal_reply(
        ticket: &Ticket,
        draft: PortalReplyDraft,
        now: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        if ticket.deleted_at().is_some() {
            return Err(TicketError::Deleted);
        }
        let body = draft.body.trim();
        if body.is_empty() || body.chars().count() > 100_000 {
            return Err(TicketError::invalid(
                "body",
                "must be 1 to 100000 characters",
            ));
        }
        let id = Uuid::now_v7();
        Ok(Self {
            id,
            tenant_id: ticket.tenant_id(),
            ticket_id: ticket.id(),
            direction: Direction::Inbound,
            message_id: format!("<portal-{id}@{}>", domain_of(&draft.service_desk)),
            from: draft.from,
            to: vec![draft.service_desk],
            cc: Vec::new(),
            subject: sdref_subject(ticket.number(), &ticket.details().subject),
            text_body: body.to_owned(),
            author_id: None,
            occurred_at: now,
            mailbox_id: None,
            requester_id: Some(draft.requester_id),
            in_reply_to: draft.in_reply_to,
        })
    }
}

impl Audited for TicketEmail {
    fn audit_type(&self) -> &'static str {
        "ticket_email"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("ticket_id", json!(self.ticket_id)),
            ("direction", json!(self.direction)),
            ("to", json!(self.to)),
            ("cc", json!(self.cc)),
            ("subject", json!(self.subject)),
        ]
    }
}

/// Values for Reply Template placeholders.
#[derive(Debug, Clone, Default)]
pub struct Placeholders {
    /// `{requester_name}`.
    pub requester_name: String,
    /// `{ticket_number}`.
    pub ticket_number: String,
    /// `{ticket_subject}`.
    pub ticket_subject: String,
    /// `{analyst_name}`.
    pub analyst_name: String,
}

impl Placeholders {
    /// `text` with every placeholder replaced by its value.
    #[must_use]
    pub fn fill(&self, text: &str) -> String {
        text.replace("{requester_name}", &self.requester_name)
            .replace("{ticket_number}", &self.ticket_number)
            .replace("{ticket_subject}", &self.ticket_subject)
            .replace("{analyst_name}", &self.analyst_name)
    }
}

/// A canned reply (FR-TKT-022).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTemplate {
    id: Uuid,
    name: String,
    body: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Placeholders a template may use.
pub const PLACEHOLDERS: &[&str] = &[
    "{requester_name}",
    "{ticket_number}",
    "{ticket_subject}",
    "{analyst_name}",
];

fn check_template(name: &str, body: &str) -> Result<(String, String), TicketError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(TicketError::invalid("name", "must be 1 to 100 characters"));
    }
    if body.trim().is_empty() || body.chars().count() > 20_000 {
        return Err(TicketError::invalid(
            "body",
            "must be 1 to 20000 characters",
        ));
    }
    check_placeholders("body", body)?;
    Ok((name.to_owned(), body.to_owned()))
}

/// Rejects `{unknown_placeholder}` tokens in a template text.
pub(super) fn check_placeholders(field: &'static str, text: &str) -> Result<(), TicketError> {
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        let token = &rest[start..=start + end];
        if token
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c == '{' || c == '}')
            && !PLACEHOLDERS.contains(&token)
        {
            return Err(TicketError::invalid(
                field,
                format!(
                    "unknown placeholder {token}; use {}",
                    PLACEHOLDERS.join(", ")
                ),
            ));
        }
        rest = &rest[start + end + 1..];
    }
    Ok(())
}

impl ReplyTemplate {
    /// Creates a template.
    ///
    /// # Errors
    ///
    /// Invalid name or body, or an unknown placeholder.
    pub fn create(name: &str, body: &str, now: DateTime<Utc>) -> Result<Self, TicketError> {
        let (name, body) = check_template(name, body)?;
        Ok(Self {
            id: Uuid::now_v7(),
            name,
            body,
            is_active: true,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a template from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        name: String,
        body: String,
        is_active: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            body,
            is_active,
            created_at,
            updated_at,
        }
    }

    /// Replaces the template.
    ///
    /// # Errors
    ///
    /// Invalid name or body, or an unknown placeholder.
    pub fn update(
        &mut self,
        name: &str,
        body: &str,
        is_active: bool,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        let (name, body) = check_template(name, body)?;
        self.name = name;
        self.body = body;
        self.is_active = is_active;
        self.updated_at = now;
        Ok(())
    }

    /// The body with placeholders filled in.
    #[must_use]
    pub fn render(&self, values: &Placeholders) -> String {
        values.fill(&self.body)
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Body with placeholders.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
    /// Active flag.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
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

impl Audited for ReplyTemplate {
    fn audit_type(&self) -> &'static str {
        "reply_template"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("body", json!(self.body)),
            ("is_active", json!(self.is_active)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdref_round_trip() {
        let s = sdref_subject("ACME-000042", "Printer jammed");
        assert_eq!(s, "[SDREF:ACME-000042] Printer jammed");
        assert_eq!(sdref_subject("ACME-000042", &s), s, "tag is not repeated");
        assert_eq!(
            parse_sdref(&format!("RE: {s}")).as_deref(),
            Some("ACME-000042")
        );
        assert_eq!(parse_sdref("no tag"), None);
        assert_eq!(parse_sdref("[SDREF:]"), None);
    }

    #[test]
    fn templates_render_known_placeholders_only() {
        let t = ReplyTemplate::create(
            "Thanks",
            "Hi {requester_name}, about {ticket_number}: done. {analyst_name}",
            Utc::now(),
        )
        .unwrap();
        let text = t.render(&Placeholders {
            requester_name: "Sam".into(),
            ticket_number: "D-1".into(),
            analyst_name: "Dee".into(),
            ..Default::default()
        });
        assert_eq!(text, "Hi Sam, about D-1: done. Dee");
        assert!(ReplyTemplate::create("Bad", "Hi {requester_nmae}", Utc::now()).is_err());
        assert!(ReplyTemplate::create("JSON ok", "Use {\"a\": 1} as the body", Utc::now()).is_ok());
    }

    #[test]
    fn portal_replies_are_inbound_threaded_messages() {
        let ticket = Ticket::open(
            crate::domain::NewTicket {
                tenant_id: TenantId::new(),
                number: "ACME-000007".into(),
                details: crate::domain::TicketDetails {
                    subject: "VPN down".into(),
                    priority_id: Uuid::now_v7(),
                    ..Default::default()
                },
                status_id: Uuid::now_v7(),
                created_by: cooper_kernel::Actor::Automation,
            },
            Utc::now(),
        )
        .unwrap();
        let requester = cooper_kernel::RequesterId::new();
        let draft = |body: &str| PortalReplyDraft {
            requester_id: requester,
            from: "sam@acme.example".into(),
            body: body.into(),
            service_desk: "Service Desk <desk@acme.example>".into(),
            in_reply_to: Some("<prev@acme.example>".into()),
        };
        let reply =
            TicketEmail::portal_reply(&ticket, draft("  Still broken  "), Utc::now()).unwrap();
        assert_eq!(reply.direction, Direction::Inbound);
        assert_eq!(reply.text_body, "Still broken");
        assert_eq!(reply.subject, "[SDREF:ACME-000007] VPN down");
        assert_eq!(reply.requester_id, Some(requester));
        assert!(reply.message_id.starts_with("<portal-"));
        assert!(reply.message_id.ends_with("@acme.example>"));
        assert!(TicketEmail::portal_reply(&ticket, draft("   "), Utc::now()).is_err());
    }

    #[test]
    fn message_ids_use_the_sender_domain() {
        assert_eq!(
            domain_of("Service Desk <desk@acme.example>"),
            "acme.example"
        );
        assert_eq!(domain_of("desk@acme.example"), "acme.example");
        assert_eq!(domain_of("nonsense"), "nonsense");
        assert!(valid_address("sam@acme.example"));
        assert!(!valid_address("sam@localhost"));
    }
}
