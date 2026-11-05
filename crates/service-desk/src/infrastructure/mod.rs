//! PostgreSQL storage of the service desk.

mod content;
mod reference;
mod sla;
mod tickets;

pub use sla::SlaMonitor;

use cooper_kernel::{AuditRecord, EventEnvelope};
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
