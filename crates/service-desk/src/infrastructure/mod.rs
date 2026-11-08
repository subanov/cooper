//! PostgreSQL storage of the service desk.

mod content;
#[cfg(feature = "google")]
mod google;
#[cfg(feature = "graph")]
mod graph;
mod imap;
mod mailboxes;
#[cfg(any(feature = "graph", feature = "google"))]
mod oauth;
mod reference;
mod sla;
mod template_mailer;
mod tickets;

#[cfg(feature = "google")]
pub use google::GmailClient;
#[cfg(feature = "graph")]
pub use graph::GraphClient;
pub use imap::{
    Disposition, FetchedMessage, ImapClient, MailboxClient, MailboxPoller, MailboxSession,
    parse_email,
};
pub use mailboxes::{PgMailboxStore, ReencryptMailboxSecrets};
pub use sla::SlaMonitor;
pub use template_mailer::EmailTemplateMailer;

use cooper_kernel::{AuditRecord, EventEnvelope, TenantId, TicketId};
use cooper_platform::db::Tx;
use sqlx::PgPool;

use crate::domain::RepoError;

/// sqlx implementation of the service desk repositories.
#[derive(Clone)]
pub struct PgServiceDeskStore {
    pool: PgPool,
}

impl PgServiceDeskStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub(crate) fn db_err(err: sqlx::Error) -> RepoError {
    if let sqlx::Error::Database(db) = &err {
        let constraint = db.constraint().unwrap_or_default().to_owned();
        match db.code().as_deref() {
            Some("23505") => return RepoError::Conflict(constraint),
            Some("23503") => return RepoError::MissingReference(constraint),
            _ => {}
        }
    }
    RepoError::Storage(err.to_string())
}

/// Writes the audit record (if it changed anything) and the events in `tx`.
pub(crate) async fn record(
    tx: &mut Tx,
    audit: &AuditRecord,
    events: &[EventEnvelope],
) -> Result<(), RepoError> {
    if audit.is_meaningful() {
        cooper_platform::audit::record(tx, audit)
            .await
            .map_err(db_err)?;
    }
    for event in events {
        cooper_platform::outbox::append(tx, event)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

/// Announces a Live Update for a ticket when `tx` commits (ADR-0022). Needed for writes
/// that raise no Domain Event; [`record`] already announces those that do, and duplicates
/// within one transaction are delivered once.
pub(crate) async fn ticket_changed(
    tx: &mut Tx,
    tenant: TenantId,
    ticket: TicketId,
) -> Result<(), RepoError> {
    let change = cooper_platform::live::LiveChange::new(tenant, "ticket", ticket.as_uuid());
    cooper_platform::live::publish(tx, &change)
        .await
        .map_err(db_err)
}
