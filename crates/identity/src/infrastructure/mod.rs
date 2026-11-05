//! Infrastructure: PostgreSQL repositories (sqlx), Argon2id password hashing and
//! random token generation.

mod companies;
mod people;
mod requesters;
mod security;
mod sessions;

use std::sync::Arc;

use cooper_kernel::{AuditRecord, Clock};
use cooper_platform::db::Tx;
use sqlx::PgPool;

pub use security::{Argon2Hasher, OsTokenGenerator};

use crate::application::IdentityPorts;
use crate::domain::RepoError;

/// The sqlx implementation of every identity repository.
#[derive(Clone)]
pub struct PgIdentityStore {
    pool: PgPool,
}

impl PgIdentityStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Wires every identity port to PostgreSQL, Argon2id and the OS random generator.
    ///
    /// # Errors
    ///
    /// Fails if the dummy password hash cannot be computed or the OS has no randomness.
    pub fn ports(pool: PgPool, clock: Arc<dyn Clock>) -> Result<IdentityPorts, RepoError> {
        let store = Arc::new(Self::new(pool));
        Ok(IdentityPorts {
            companies: store.clone(),
            analysts: store.clone(),
            sessions: store.clone(),
            authorization: store.clone(),
            teams: store.clone(),
            departments: store.clone(),
            roles: store.clone(),
            requesters: store.clone(),
            cost_centres: store.clone(),
            setup: store,
            hasher: Arc::new(Argon2Hasher::new()?),
            tokens: Arc::new(OsTokenGenerator),
            clock,
        })
    }
}

/// Maps a sqlx error to a repository error, keeping constraint names for conflicts.
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

/// Writes an audit record inside `tx` if it carries anything.
pub(crate) async fn audit(tx: &mut Tx, record: &AuditRecord) -> Result<(), RepoError> {
    if record.is_meaningful() {
        cooper_platform::audit::record(tx, record)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

/// Turns a stored value back into a domain value; a failure means corrupt data.
pub(crate) fn stored<T, E: std::fmt::Display>(value: Result<T, E>) -> Result<T, RepoError> {
    value.map_err(|e| RepoError::Storage(format!("invalid stored value: {e}")))
}

/// SQL `LIKE` pattern for a case-insensitive substring search.
pub(crate) fn like_pattern(search: Option<&String>) -> Option<String> {
    search.map(|s| {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        format!("%{}%", escaped.to_lowercase())
    })
}
