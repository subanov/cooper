//! Mailbox storage (credentials encrypted at rest, FR-EML-012) and the Rejected Email log.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AuditRecord, DepartmentId, EventEnvelope, TeamId, TenantId};
use cooper_platform::crypto::Keyring;
use sqlx::PgPool;
use uuid::Uuid;

use super::{db_err, record};
use crate::domain::{
    AfterImport, AfterReject, ImapSettings, InboundEmail, Mailbox, MailboxHealth,
    MailboxRepository, MailboxSettings, Provider, RepoError, Security, WhitelistEntry,
};

/// sqlx implementation of [`MailboxRepository`].
#[derive(Clone)]
pub struct PgMailboxStore {
    pool: PgPool,
    keyring: Keyring,
}

impl PgMailboxStore {
    /// Creates the store; `keyring` encrypts Mailbox secrets.
    #[must_use]
    pub const fn new(pool: PgPool, keyring: Keyring) -> Self {
        Self { pool, keyring }
    }
}

fn purpose(id: Uuid) -> String {
    format!("mailbox.secret:{id}")
}

fn corrupt(e: impl std::fmt::Display) -> RepoError {
    RepoError::Storage(format!("invalid stored mailbox: {e}"))
}

#[async_trait]
impl MailboxRepository for PgMailboxStore {
    async fn mailboxes(&self) -> Result<Vec<Mailbox>, RepoError> {
        let rows = sqlx::query!(
            r#"SELECT m.id, m.name, m.address, m.provider, m.auth_mode, m.imap_host, m.imap_port, m.imap_security,
                      m.imap_username, m.folder, m.tenant_id, m.department_id, m.origin_id, m.team_id, m.post_import_action,
                      m.move_to_folder, m.unknown_sender_action, m.rejected_action, m.is_active, m.last_polled_at,
                      m.last_success_at, m.last_error, m.last_error_at, m.consecutive_failures, m.created_at, m.updated_at,
                      ARRAY(SELECT w.pattern FROM mailbox_whitelist w WHERE w.mailbox_id = m.id ORDER BY w.pattern) AS "patterns!",
                      ARRAY(SELECT w.kind = 'allow' FROM mailbox_whitelist w WHERE w.mailbox_id = m.id ORDER BY w.pattern) AS "allows!"
               FROM mailboxes m ORDER BY lower(m.name)"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                let provider = match r.provider.as_str() {
                    "imap" => Provider::Imap,
                    "microsoft" => Provider::Microsoft,
                    "google" => Provider::Google,
                    other => return Err(corrupt(other)),
                };
                let imap = match (
                    r.imap_host,
                    r.imap_port,
                    r.imap_security.as_deref(),
                    r.imap_username,
                ) {
                    (Some(host), Some(port), Some(sec), Some(username)) => Some(ImapSettings {
                        host,
                        port: u16::try_from(port).map_err(corrupt)?,
                        security: match sec {
                            "tls" => Security::Tls,
                            "starttls" => Security::Starttls,
                            _ => Security::None,
                        },
                        username,
                    }),
                    _ => None,
                };
                let folder = r.move_to_folder.unwrap_or_default();
                let after_import = match r.post_import_action.as_str() {
                    "delete" => AfterImport::Delete,
                    "move" => AfterImport::Move(folder.clone()),
                    _ => AfterImport::MarkRead,
                };
                let after_reject = match r.rejected_action.as_str() {
                    "delete" => AfterReject::Delete,
                    "move" => AfterReject::Move(folder),
                    _ => AfterReject::Keep,
                };
                let whitelist = r
                    .patterns
                    .into_iter()
                    .zip(r.allows)
                    .map(|(pattern, allow)| WhitelistEntry { pattern, allow })
                    .collect();
                Ok(Mailbox::restore(
                    r.id,
                    MailboxSettings {
                        name: r.name,
                        address: r.address,
                        provider,
                        auth_mode: r.auth_mode,
                        imap,
                        folder: r.folder,
                        tenant_id: TenantId::from_uuid(r.tenant_id),
                        department_id: r.department_id.map(DepartmentId::from_uuid),
                        origin_id: r.origin_id,
                        team_id: r.team_id.map(TeamId::from_uuid),
                        after_import,
                        create_unknown_requesters: r.unknown_sender_action == "create_requester",
                        after_reject,
                        whitelist,
                        is_active: r.is_active,
                    },
                    MailboxHealth {
                        last_polled_at: r.last_polled_at,
                        last_success_at: r.last_success_at,
                        last_error: r.last_error,
                        last_error_at: r.last_error_at,
                        consecutive_failures: u32::try_from(r.consecutive_failures).unwrap_or(0),
                    },
                    r.created_at,
                    r.updated_at,
                ))
            })
            .collect()
    }

    async fn save(
        &self,
        m: &Mailbox,
        secret: Option<&str>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let s = m.settings();
        let imap = s.imap.as_ref();
        let (action, folder) = match &s.after_import {
            AfterImport::Delete => ("delete", None),
            AfterImport::Move(f) => ("move", Some(f.clone())),
            AfterImport::MarkRead => ("mark_read", None),
        };
        let (rejected, reject_folder) = match &s.after_reject {
            AfterReject::Delete => ("delete", None),
            AfterReject::Move(f) => ("move", Some(f.clone())),
            AfterReject::Keep => ("keep", None),
        };
        let encrypted = secret
            .map(|v| self.keyring.encrypt(&purpose(m.id()), v.as_bytes()))
            .transpose()
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO mailboxes (id, name, address, provider, auth_mode, imap_host, imap_port, imap_security, imap_username,
                                    folder, secret_encrypted, tenant_id, department_id, origin_id, team_id, post_import_action,
                                    move_to_folder, unknown_sender_action, rejected_action, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, address = EXCLUDED.address, provider = EXCLUDED.provider,
                 auth_mode = EXCLUDED.auth_mode, imap_host = EXCLUDED.imap_host, imap_port = EXCLUDED.imap_port,
                 imap_security = EXCLUDED.imap_security, imap_username = EXCLUDED.imap_username, folder = EXCLUDED.folder,
                 secret_encrypted = COALESCE(EXCLUDED.secret_encrypted, mailboxes.secret_encrypted),
                 tenant_id = EXCLUDED.tenant_id, department_id = EXCLUDED.department_id, origin_id = EXCLUDED.origin_id,
                 team_id = EXCLUDED.team_id, post_import_action = EXCLUDED.post_import_action,
                 move_to_folder = EXCLUDED.move_to_folder, unknown_sender_action = EXCLUDED.unknown_sender_action,
                 rejected_action = EXCLUDED.rejected_action, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            m.id(),
            s.name,
            s.address,
            s.provider.as_str(),
            s.auth_mode,
            imap.map(|i| i.host.clone()),
            imap.map(|i| i32::from(i.port)),
            imap.map(|i| match i.security {
                Security::Tls => "tls",
                Security::Starttls => "starttls",
                Security::None => "none",
            }),
            imap.map(|i| i.username.clone()),
            s.folder,
            encrypted,
            s.tenant_id.as_uuid(),
            s.department_id.map(|d| d.as_uuid()),
            s.origin_id,
            s.team_id.map(|t| t.as_uuid()),
            action,
            folder.or(reject_folder),
            if s.create_unknown_requesters { "create_requester" } else { "reject" },
            rejected,
            s.is_active,
            m.created_at(),
            m.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "DELETE FROM mailbox_whitelist WHERE mailbox_id = $1",
            m.id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for e in &s.whitelist {
            sqlx::query!(
                "INSERT INTO mailbox_whitelist (id, mailbox_id, pattern, kind, created_at) VALUES ($1, $2, $3, $4, $5)",
                Uuid::now_v7(),
                m.id(),
                e.pattern,
                if e.allow { "allow" } else { "block" },
                m.updated_at()
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn secret(&self, mailbox: Uuid) -> Result<Option<String>, RepoError> {
        let stored = sqlx::query_scalar!(
            "SELECT secret_encrypted FROM mailboxes WHERE id = $1",
            mailbox
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .flatten();
        stored
            .map(|v| {
                let bytes = self
                    .keyring
                    .decrypt(&purpose(mailbox), &v)
                    .map_err(|e| RepoError::Storage(e.to_string()))?;
                String::from_utf8(bytes).map_err(|e| RepoError::Storage(e.to_string()))
            })
            .transpose()
    }

    async fn record_health(&self, m: &Mailbox, events: &[EventEnvelope]) -> Result<(), RepoError> {
        let h = m.health();
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "UPDATE mailboxes SET last_polled_at = $2, last_success_at = $3, last_error = $4, last_error_at = $5,
                    consecutive_failures = $6 WHERE id = $1",
            m.id(),
            h.last_polled_at,
            h.last_success_at,
            h.last_error,
            h.last_error_at,
            i32::try_from(h.consecutive_failures).unwrap_or(i32::MAX)
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for e in events {
            cooper_platform::outbox::append(&mut tx, e)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    async fn log_rejected(
        &self,
        mailbox: Uuid,
        email: &InboundEmail,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        sqlx::query!(
            "INSERT INTO rejected_emails (id, mailbox_id, from_address, subject, message_id, reason, received_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            Uuid::now_v7(),
            mailbox,
            email.from_address,
            email.subject.chars().take(500).collect::<String>(),
            email.message_id,
            reason,
            at
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn rejected(
        &self,
        mailbox: Uuid,
        limit: i64,
    ) -> Result<Vec<(String, String, String, DateTime<Utc>)>, RepoError> {
        let rows = sqlx::query!(
            "SELECT from_address, subject, reason, received_at FROM rejected_emails WHERE mailbox_id = $1
             ORDER BY received_at DESC LIMIT $2",
            mailbox,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| (r.from_address, r.subject, r.reason, r.received_at))
            .collect())
    }
}

/// Re-encrypts Mailbox secrets written with an older key after a key rotation
/// (ADR-0013). Every 10 minutes by default.
#[derive(Debug, Clone)]
pub struct ReencryptMailboxSecrets {
    keyring: Keyring,
}

impl ReencryptMailboxSecrets {
    /// A job using `keyring` (active key first).
    #[must_use]
    pub const fn new(keyring: Keyring) -> Self {
        Self { keyring }
    }
}

#[async_trait]
impl cooper_platform::jobs::Job for ReencryptMailboxSecrets {
    fn name(&self) -> &'static str {
        "mailbox-secrets-reencrypt"
    }

    fn default_schedule(&self) -> cooper_platform::jobs::Schedule {
        cooper_platform::jobs::Schedule::every_seconds(600)
    }

    async fn run(
        &self,
        ctx: &cooper_platform::jobs::JobContext,
    ) -> Result<Option<String>, cooper_platform::jobs::JobError> {
        let active = format!("v{}.%", self.keyring.active_key_id());
        let rows = sqlx::query!("SELECT id, secret_encrypted AS \"secret!\" FROM mailboxes WHERE secret_encrypted NOT LIKE $1", active)
            .fetch_all(&ctx.pool)
            .await?;
        for r in &rows {
            let plain = self.keyring.decrypt(&purpose(r.id), &r.secret)?;
            let again = self.keyring.encrypt(&purpose(r.id), &plain)?;
            sqlx::query!(
                "UPDATE mailboxes SET secret_encrypted = $2 WHERE id = $1",
                r.id,
                again
            )
            .execute(&ctx.pool)
            .await?;
        }
        Ok(Some(format!("re-encrypted {} Mailbox secrets", rows.len())))
    }
}
