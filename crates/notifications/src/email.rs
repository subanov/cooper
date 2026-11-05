//! Outbound email (FR-EML-008): a queue with a send log and retries.
//!
//! Contexts never talk SMTP. Replies arrive as `ticket.reply_sent` events (queued by
//! [`ReplyMailer`]); other mail (password reset) is queued through [`EmailOutbox`]. The
//! `email-sender` job claims due emails with `SKIP LOCKED`, sends them through a
//! [`MailTransport`], logs every attempt and retries failures with backoff until they are
//! dead.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{EventEnvelope, TenantId, TenantScope};
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};
use cooper_platform::outbox::{EventHandler, HandlerError, retry_at};
use cooper_platform::tenancy;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Attempts before an email is given up.
pub const MAX_ATTEMPTS: i32 = 8;

/// An email to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundEmail {
    /// ID.
    pub id: Uuid,
    /// Company, when the email concerns one.
    pub tenant_id: Option<TenantId>,
    /// What it is (`ticket_reply`, `password_reset`, …).
    pub kind: String,
    /// `From` header, for example `Service Desk <desk@example.com>`.
    pub from: String,
    /// Recipients.
    pub to: Vec<String>,
    /// Copy recipients.
    pub cc: Vec<String>,
    /// Subject.
    pub subject: String,
    /// Plain-text body.
    pub text_body: String,
    /// `Message-ID`, including angle brackets. Unique: queuing the same email twice is
    /// ignored.
    pub message_id: String,
    /// `In-Reply-To`.
    pub in_reply_to: Option<String>,
    /// `References`.
    pub references: Vec<String>,
}

/// Sends email.
#[async_trait]
pub trait MailTransport: Send + Sync {
    /// Sends one email.
    async fn send(&self, email: &OutboundEmail) -> Result<(), String>;
}

/// SMTP (STARTTLS or TLS via the URL scheme), for example
/// `smtp://user:pass@mail.example.com:587?tls=required` or `smtp://localhost:1025`.
pub struct SmtpTransport {
    inner: lettre::AsyncSmtpTransport<lettre::Tokio1Executor>,
}

impl SmtpTransport {
    /// Builds a transport from a URL (see `lettre` connection URLs).
    ///
    /// # Errors
    ///
    /// An invalid URL.
    pub fn from_url(url: &str) -> Result<Self, String> {
        let inner = lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::from_url(url)
            .map_err(|e| format!("invalid SMTP URL: {e}"))?
            .build();
        Ok(Self { inner })
    }
}

#[async_trait]
impl MailTransport for SmtpTransport {
    async fn send(&self, email: &OutboundEmail) -> Result<(), String> {
        use lettre::AsyncTransport as _;
        use lettre::message::header::ContentType;
        let parse = |a: &str| {
            a.parse::<lettre::message::Mailbox>()
                .map_err(|e| format!("invalid address {a:?}: {e}"))
        };
        let mut builder = lettre::Message::builder()
            .from(parse(&email.from)?)
            .subject(email.subject.clone())
            .message_id(Some(email.message_id.clone()));
        for to in &email.to {
            builder = builder.to(parse(to)?);
        }
        for cc in &email.cc {
            builder = builder.cc(parse(cc)?);
        }
        if let Some(r) = &email.in_reply_to {
            builder = builder.in_reply_to(r.clone());
        }
        if !email.references.is_empty() {
            builder = builder.references(email.references.join(" "));
        }
        let message = builder
            .header(ContentType::TEXT_PLAIN)
            .body(email.text_body.clone())
            .map_err(|e| e.to_string())?;
        self.inner
            .send(message)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Keeps sent emails in memory (tests and local experiments).
#[derive(Default)]
pub struct MemoryTransport {
    sent: Mutex<Vec<OutboundEmail>>,
    /// When set, every send fails with this message.
    pub fail_with: Mutex<Option<String>>,
}

impl MemoryTransport {
    /// Emails sent so far.
    #[must_use]
    pub fn sent(&self) -> Vec<OutboundEmail> {
        self.sent.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[async_trait]
impl MailTransport for MemoryTransport {
    async fn send(&self, email: &OutboundEmail) -> Result<(), String> {
        if let Some(err) = self.fail_with.lock().map_err(|e| e.to_string())?.clone() {
            return Err(err);
        }
        self.sent
            .lock()
            .map_err(|e| e.to_string())?
            .push(email.clone());
        Ok(())
    }
}

/// Queues outbound email.
#[derive(Clone)]
pub struct EmailOutbox {
    pool: PgPool,
}

impl EmailOutbox {
    /// Creates the queue.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Queues an email; one with the same `Message-ID` is ignored. Returns whether it
    /// was new.
    ///
    /// # Errors
    ///
    /// Database errors.
    pub async fn queue(&self, e: &OutboundEmail, now: DateTime<Utc>) -> Result<bool, sqlx::Error> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All).await?;
        let inserted = sqlx::query!(
            "INSERT INTO outbound_emails (id, tenant_id, kind, from_address, to_addresses, cc_addresses, subject, text_body,
                                          message_id, in_reply_to, references_ids, status, next_attempt_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'queued', $12, $12)
             ON CONFLICT (message_id) DO NOTHING",
            e.id,
            e.tenant_id.map(|t| t.as_uuid()),
            e.kind,
            e.from,
            &e.to,
            &e.cc,
            e.subject,
            e.text_body,
            e.message_id,
            e.in_reply_to,
            &e.references,
            now
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(inserted == 1)
    }
}

/// Sends due emails (FR-EML-008). Every 10 seconds by default.
pub struct EmailSender {
    transport: Option<Arc<dyn MailTransport>>,
}

impl EmailSender {
    /// A sender using `transport`; with `None` (SMTP not configured) emails stay queued.
    #[must_use]
    pub fn new(transport: Option<Arc<dyn MailTransport>>) -> Self {
        Self { transport }
    }
}

#[async_trait]
impl Job for EmailSender {
    fn name(&self) -> &'static str {
        "email-sender"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(10)
    }

    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError> {
        let Some(transport) = &self.transport else {
            return Ok(Some(
                "no mail transport configured (COOPER_SMTP_URL)".into(),
            ));
        };
        let now = ctx.clock.now();
        let mut tx = tenancy::begin(&ctx.pool, &TenantScope::All).await?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id, kind, from_address, to_addresses, cc_addresses, subject, text_body, message_id,
                    in_reply_to, references_ids, attempts
             FROM outbound_emails WHERE status = 'queued' AND next_attempt_at <= $1
             ORDER BY next_attempt_at LIMIT 20 FOR UPDATE SKIP LOCKED",
            now
        )
        .fetch_all(&mut *tx)
        .await?;
        let (mut sent, mut failed) = (0, 0);
        for r in rows {
            let email = OutboundEmail {
                id: r.id,
                tenant_id: r.tenant_id.map(TenantId::from_uuid),
                kind: r.kind,
                from: r.from_address,
                to: r.to_addresses,
                cc: r.cc_addresses,
                subject: r.subject,
                text_body: r.text_body,
                message_id: r.message_id,
                in_reply_to: r.in_reply_to,
                references: r.references_ids,
            };
            let attempts = r.attempts + 1;
            let result = transport.send(&email).await;
            sqlx::query!(
                "INSERT INTO outbound_email_attempts (id, email_id, attempted_at, succeeded, error) VALUES ($1, $2, $3, $4, $5)",
                Uuid::now_v7(),
                email.id,
                now,
                result.is_ok(),
                result.as_ref().err()
            )
            .execute(&mut *tx)
            .await?;
            match result {
                Ok(()) => {
                    sent += 1;
                    sqlx::query!(
                        "UPDATE outbound_emails SET status = 'sent', attempts = $2, sent_at = $3, last_error = NULL WHERE id = $1",
                        email.id,
                        attempts,
                        now
                    )
                    .execute(&mut *tx)
                    .await?;
                }
                Err(error) => {
                    failed += 1;
                    let dead = attempts >= MAX_ATTEMPTS;
                    tracing::warn!(email_id = %email.id, attempts, dead, %error, "outbound email failed");
                    sqlx::query!(
                        "UPDATE outbound_emails SET status = CASE WHEN $4 THEN 'dead' ELSE 'queued' END, attempts = $2,
                                last_error = $3, next_attempt_at = $5 WHERE id = $1",
                        email.id,
                        attempts,
                        error,
                        dead,
                        retry_at(now, attempts)
                    )
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        tx.commit().await?;
        Ok(Some(format!("sent {sent}, failed {failed}")))
    }
}

/// Queues the email of a `ticket.reply_sent` event (FR-TKT-021).
pub struct ReplyMailer {
    /// The queue.
    pub outbox: EmailOutbox,
}

#[async_trait]
impl EventHandler for ReplyMailer {
    fn name(&self) -> &'static str {
        "reply-mailer"
    }

    fn handles(&self, event_type: &str) -> bool {
        event_type == "ticket.reply_sent"
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError> {
        let email: OutboundEmail =
            serde_json::from_value(event.payload.get("email").cloned().unwrap_or_default())?;
        self.outbox.queue(&email, event.occurred_at).await?;
        Ok(())
    }
}

/// Settings for outbound email.
pub mod settings {
    use cooper_kernel::{SettingDefinition, SettingKind};

    /// `From` of Cooper's email until Mailboxes send their own (FR-TKT-021).
    pub const FROM_ADDRESS: SettingDefinition = SettingDefinition {
        key: "email.from_address",
        description: "From address of email Cooper sends, for example `Service Desk <desk@example.com>`.",
        kind: SettingKind::Text { max_chars: 200 },
        default: "\"Cooper <cooper@localhost>\"",
    };

    /// Every setting declared by the notifications context.
    pub const ALL: &[SettingDefinition] = &[FROM_ADDRESS];
}
