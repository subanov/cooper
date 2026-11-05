//! Mailboxes and inbound email (FR-EML-001 to FR-EML-006, FR-EML-010, FR-EML-011).

use chrono::{DateTime, Utc};
use cooper_kernel::{DepartmentId, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Audited, TicketError};

/// Where a Mailbox's mail comes from (fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// IMAP (with SMTP for sending).
    Imap,
    /// Microsoft 365 through Graph.
    Microsoft,
    /// Google Workspace.
    Google,
}

impl Provider {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Imap => "imap",
            Self::Microsoft => "microsoft",
            Self::Google => "google",
        }
    }
}

/// IMAP transport security (fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    /// TLS from the first byte (port 993).
    Tls,
    /// Plain connection upgraded with STARTTLS (port 143).
    Starttls,
    /// No encryption (test servers only).
    None,
}

/// What happens to a message after import (FR-EML-006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "folder", rename_all = "snake_case")]
pub enum AfterImport {
    /// Delete it from the mailbox.
    Delete,
    /// Move it to a folder.
    Move(String),
    /// Leave it, marked read.
    MarkRead,
}

/// What happens to a rejected message (FR-EML-005).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "folder", rename_all = "snake_case")]
pub enum AfterReject {
    /// Delete it.
    Delete,
    /// Move it to a folder (the Mailbox's move folder).
    Move(String),
    /// Leave it, marked read.
    Keep,
}

/// IMAP connection settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImapSettings {
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// Security.
    pub security: Security,
    /// Login.
    pub username: String,
}

/// A Whitelist entry: an address (`sam@acme.example`) or a domain (`acme.example`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhitelistEntry {
    /// Lower-case address or domain.
    pub pattern: String,
    /// Allow (`true`) or block.
    pub allow: bool,
}

/// Editable settings of a Mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxSettings {
    /// Name.
    pub name: String,
    /// The address mail arrives at (and Replies come from, with SMTP).
    pub address: String,
    /// Provider.
    pub provider: Provider,
    /// Graph auth mode (`delegated`/`app_only`); Microsoft only.
    pub auth_mode: Option<String>,
    /// IMAP settings; IMAP only.
    pub imap: Option<ImapSettings>,
    /// Folder to read.
    pub folder: String,
    /// Default Company for new tickets.
    pub tenant_id: TenantId,
    /// Default Department.
    pub department_id: Option<DepartmentId>,
    /// Origin stamped on new tickets.
    pub origin_id: Option<Uuid>,
    /// Default Queue.
    pub team_id: Option<TeamId>,
    /// After import.
    pub after_import: AfterImport,
    /// Create Requesters for unknown senders (else reject).
    pub create_unknown_requesters: bool,
    /// After rejecting.
    pub after_reject: AfterReject,
    /// Whitelist.
    pub whitelist: Vec<WhitelistEntry>,
    /// Polled when active.
    pub is_active: bool,
}

/// Poll health of a Mailbox (FR-EML-010).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailboxHealth {
    /// Last poll attempt.
    pub last_polled_at: Option<DateTime<Utc>>,
    /// Last successful poll.
    pub last_success_at: Option<DateTime<Utc>>,
    /// Last error.
    pub last_error: Option<String>,
    /// When it happened.
    pub last_error_at: Option<DateTime<Utc>>,
    /// Failures in a row.
    pub consecutive_failures: u32,
}

/// Failures in a row after which Administrators are notified.
pub const FAILURES_BEFORE_ALERT: u32 = 3;

/// An email account Cooper polls to create and update tickets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailbox {
    id: Uuid,
    settings: MailboxSettings,
    health: MailboxHealth,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn normalise(mut s: MailboxSettings) -> Result<MailboxSettings, TicketError> {
    s.name = s.name.trim().to_owned();
    if s.name.is_empty() || s.name.chars().count() > 100 {
        return Err(TicketError::invalid("name", "must be 1 to 100 characters"));
    }
    s.address = s.address.trim().to_lowercase();
    if !s
        .address
        .split_once('@')
        .is_some_and(|(l, d)| !l.is_empty() && d.contains('.') || d == "localhost")
    {
        return Err(TicketError::invalid("address", "is not an email address"));
    }
    match (s.provider, &s.imap) {
        (Provider::Imap, None) => {
            return Err(TicketError::invalid(
                "imap",
                "IMAP Mailboxes need host, port, security and username",
            ));
        }
        (Provider::Imap, Some(i))
            if i.host.trim().is_empty() || i.username.trim().is_empty() || i.port == 0 =>
        {
            return Err(TicketError::invalid(
                "imap",
                "host, port and username are required",
            ));
        }
        (Provider::Microsoft, _)
            if !matches!(s.auth_mode.as_deref(), Some("delegated" | "app_only")) =>
        {
            return Err(TicketError::invalid(
                "auth_mode",
                "must be delegated or app_only for Microsoft",
            ));
        }
        (Provider::Imap | Provider::Google, _) => s.auth_mode = None,
        _ => {}
    }
    if s.provider != Provider::Imap {
        s.imap = None;
    }
    let empty_folder = |f: &String| f.trim().is_empty();
    if matches!(&s.after_import, AfterImport::Move(f) if empty_folder(f))
        || matches!(&s.after_reject, AfterReject::Move(f) if empty_folder(f))
    {
        return Err(TicketError::invalid(
            "move_to_folder",
            "is required to move messages",
        ));
    }
    s.folder = if s.folder.trim().is_empty() {
        "INBOX".to_owned()
    } else {
        s.folder.trim().to_owned()
    };
    let mut whitelist = Vec::new();
    for mut e in s.whitelist {
        e.pattern = e.pattern.trim().trim_start_matches('@').to_lowercase();
        if e.pattern.len() < 3 || !e.pattern.contains('.') && !e.pattern.contains('@') {
            return Err(TicketError::invalid(
                "whitelist",
                format!("{:?} is not an address or domain", e.pattern),
            ));
        }
        if !whitelist
            .iter()
            .any(|w: &WhitelistEntry| w.pattern == e.pattern)
        {
            whitelist.push(e);
        }
    }
    s.whitelist = whitelist;
    Ok(s)
}

impl Mailbox {
    /// Creates a Mailbox.
    ///
    /// # Errors
    ///
    /// Invalid settings.
    pub fn create(settings: MailboxSettings, now: DateTime<Utc>) -> Result<Self, TicketError> {
        Ok(Self {
            id: Uuid::now_v7(),
            settings: normalise(settings)?,
            health: MailboxHealth::default(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a Mailbox from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        settings: MailboxSettings,
        health: MailboxHealth,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            settings,
            health,
            created_at,
            updated_at,
        }
    }

    /// Replaces the settings.
    ///
    /// # Errors
    ///
    /// Invalid settings.
    pub fn update(
        &mut self,
        settings: MailboxSettings,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        self.settings = normalise(settings)?;
        self.updated_at = now;
        Ok(())
    }

    /// Records a successful poll.
    pub fn poll_succeeded(&mut self, now: DateTime<Utc>) {
        self.health.last_polled_at = Some(now);
        self.health.last_success_at = Some(now);
        self.health.consecutive_failures = 0;
    }

    /// Records a failed poll. Returns `true` when this failure should alert
    /// Administrators (the [`FAILURES_BEFORE_ALERT`]th in a row).
    pub fn poll_failed(&mut self, error: &str, now: DateTime<Utc>) -> bool {
        self.health.last_polled_at = Some(now);
        self.health.last_error = Some(error.chars().take(1000).collect());
        self.health.last_error_at = Some(now);
        self.health.consecutive_failures += 1;
        self.health.consecutive_failures == FAILURES_BEFORE_ALERT
    }

    /// Whether the Whitelist lets `sender` in: blocked entries always reject; when
    /// there are allow entries, the sender must match one.
    #[must_use]
    pub fn admits(&self, sender: &str) -> bool {
        let sender = sender.trim().to_lowercase();
        let domain = sender.rsplit('@').next().unwrap_or_default();
        let matches = |p: &str| p == sender || p == domain || domain.ends_with(&format!(".{p}"));
        let list = &self.settings.whitelist;
        if list.iter().any(|e| !e.allow && matches(&e.pattern)) {
            return false;
        }
        let allows: Vec<_> = list.iter().filter(|e| e.allow).collect();
        allows.is_empty() || allows.iter().any(|e| matches(&e.pattern))
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Settings.
    #[must_use]
    pub const fn settings(&self) -> &MailboxSettings {
        &self.settings
    }
    /// Health.
    #[must_use]
    pub const fn health(&self) -> &MailboxHealth {
        &self.health
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

impl Audited for Mailbox {
    fn audit_type(&self) -> &'static str {
        "mailbox"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let s = &self.settings;
        vec![
            ("name", json!(s.name)),
            ("address", json!(s.address)),
            ("provider", json!(s.provider)),
            ("imap", json!(s.imap)),
            ("folder", json!(s.folder)),
            ("tenant_id", json!(s.tenant_id)),
            ("department_id", json!(s.department_id)),
            ("origin_id", json!(s.origin_id)),
            ("team_id", json!(s.team_id)),
            ("after_import", json!(s.after_import)),
            (
                "create_unknown_requesters",
                json!(s.create_unknown_requesters),
            ),
            ("after_reject", json!(s.after_reject)),
            ("whitelist", json!(s.whitelist)),
            ("is_active", json!(s.is_active)),
        ]
    }
}

/// An inbound email, parsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboundEmail {
    /// `Message-ID` with angle brackets (synthesised from the content when missing).
    pub message_id: String,
    /// `In-Reply-To`.
    pub in_reply_to: Option<String>,
    /// `References`.
    pub references: Vec<String>,
    /// Sender address (lower case).
    pub from_address: String,
    /// Sender display name.
    pub from_name: Option<String>,
    /// `To`.
    pub to: Vec<String>,
    /// `Cc`.
    pub cc: Vec<String>,
    /// Subject.
    pub subject: String,
    /// Plain-text body (from the HTML part when there is no text part).
    pub text_body: String,
    /// Headers relevant to loop protection, lower-case names.
    pub headers: Vec<(String, String)>,
    /// When it was sent, if known.
    pub date: Option<DateTime<Utc>>,
}

impl InboundEmail {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Why this email must not create or update a ticket (FR-EML-011): auto-replies,
    /// bounces, mailing lists and Cooper's own mail. `own` are Cooper's own addresses.
    #[must_use]
    pub fn automated_reason(&self, own: &[String]) -> Option<&'static str> {
        let local = self.from_address.split('@').next().unwrap_or_default();
        if own
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&self.from_address))
        {
            return Some("sent by Cooper itself");
        }
        if matches!(local, "mailer-daemon" | "postmaster") || self.from_address.is_empty() {
            return Some("bounce");
        }
        if self
            .header("auto-submitted")
            .is_some_and(|v| !v.trim().eq_ignore_ascii_case("no"))
        {
            return Some("auto-submitted");
        }
        if self.header("precedence").is_some_and(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "bulk" | "junk" | "list" | "auto_reply"
            )
        }) {
            return Some("bulk or list mail");
        }
        if ["x-autoreply", "x-autorespond", "x-auto-response-suppress"]
            .iter()
            .any(|h| self.header(h).is_some())
        {
            return Some("auto-reply");
        }
        if self
            .header("content-type")
            .is_some_and(|v| v.to_lowercase().contains("multipart/report"))
        {
            return Some("delivery report");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> MailboxSettings {
        MailboxSettings {
            name: "Service Desk".into(),
            address: "Desk@Acme.example".into(),
            provider: Provider::Imap,
            auth_mode: Some("app_only".into()),
            imap: Some(ImapSettings {
                host: "imap.acme.example".into(),
                port: 993,
                security: Security::Tls,
                username: "desk".into(),
            }),
            folder: String::new(),
            tenant_id: TenantId::new(),
            department_id: None,
            origin_id: None,
            team_id: None,
            after_import: AfterImport::MarkRead,
            create_unknown_requesters: true,
            after_reject: AfterReject::Delete,
            whitelist: vec![],
            is_active: true,
        }
    }

    #[test]
    fn settings_are_normalised_and_validated() {
        let m = Mailbox::create(settings(), Utc::now()).unwrap();
        assert_eq!(m.settings().address, "desk@acme.example");
        assert_eq!(m.settings().folder, "INBOX");
        assert_eq!(
            m.settings().auth_mode,
            None,
            "auth mode only applies to Microsoft"
        );
        assert!(
            Mailbox::create(
                MailboxSettings {
                    imap: None,
                    ..settings()
                },
                Utc::now()
            )
            .is_err()
        );
        let ms = MailboxSettings {
            provider: Provider::Microsoft,
            auth_mode: None,
            ..settings()
        };
        assert!(Mailbox::create(ms, Utc::now()).is_err());
        let mv = MailboxSettings {
            after_import: AfterImport::Move(" ".into()),
            ..settings()
        };
        assert!(Mailbox::create(mv, Utc::now()).is_err());
    }

    #[test]
    fn whitelist_blocks_and_allows() {
        let list = vec![
            WhitelistEntry {
                pattern: "acme.example".into(),
                allow: true,
            },
            WhitelistEntry {
                pattern: "@spam.acme.example".into(),
                allow: false,
            },
            WhitelistEntry {
                pattern: "vip@partner.example".into(),
                allow: true,
            },
        ];
        let m = Mailbox::create(
            MailboxSettings {
                whitelist: list,
                ..settings()
            },
            Utc::now(),
        )
        .unwrap();
        assert!(m.admits("Sam@acme.example"));
        assert!(m.admits("x@eu.acme.example"), "subdomains match");
        assert!(!m.admits("bot@spam.acme.example"));
        assert!(m.admits("vip@partner.example"));
        assert!(!m.admits("other@partner.example"));
        let open = Mailbox::create(settings(), Utc::now()).unwrap();
        assert!(open.admits("anyone@anywhere.example"));
    }

    #[test]
    fn automated_mail_is_recognised() {
        let mail = |from: &str, headers: &[(&str, &str)]| InboundEmail {
            from_address: from.into(),
            headers: headers
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect(),
            ..InboundEmail::default()
        };
        let own = vec!["desk@acme.example".to_owned()];
        assert_eq!(
            mail("desk@acme.example", &[]).automated_reason(&own),
            Some("sent by Cooper itself")
        );
        assert_eq!(
            mail("MAILER-DAEMON@x.example".to_lowercase().as_str(), &[]).automated_reason(&own),
            Some("bounce")
        );
        assert_eq!(
            mail("sam@x.example", &[("auto-submitted", "auto-replied")]).automated_reason(&own),
            Some("auto-submitted")
        );
        assert_eq!(
            mail("sam@x.example", &[("auto-submitted", "no")]).automated_reason(&own),
            None
        );
        assert_eq!(
            mail("sam@x.example", &[("precedence", "Bulk")]).automated_reason(&own),
            Some("bulk or list mail")
        );
        assert_eq!(
            mail("sam@x.example", &[("x-autoreply", "yes")]).automated_reason(&own),
            Some("auto-reply")
        );
        assert_eq!(
            mail(
                "sam@x.example",
                &[(
                    "content-type",
                    "multipart/report; report-type=delivery-status"
                )]
            )
            .automated_reason(&own),
            Some("delivery report")
        );
    }

    #[test]
    fn alert_after_three_failures_in_a_row() {
        let mut m = Mailbox::create(settings(), Utc::now()).unwrap();
        assert!(!m.poll_failed("timeout", Utc::now()));
        assert!(!m.poll_failed("timeout", Utc::now()));
        assert!(m.poll_failed("timeout", Utc::now()));
        assert!(
            !m.poll_failed("timeout", Utc::now()),
            "alert once per streak"
        );
        m.poll_succeeded(Utc::now());
        assert_eq!(m.health().consecutive_failures, 0);
    }
}
