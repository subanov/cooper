//! Infrastructure: PostgreSQL repositories (sqlx), Argon2id password hashing and
//! random token generation.

mod api_keys;
mod companies;
mod housekeeping;
mod people;
mod protection;
mod requesters;
mod security;
mod sessions;

use std::sync::Arc;

use cooper_kernel::{AuditRecord, Clock};
use cooper_platform::crypto::Keyring;
use cooper_platform::db::Tx;
use sqlx::PgPool;

pub use housekeeping::{IdentityHousekeeping, ReencryptIdentitySecrets};
pub use security::{Argon2Hasher, OsTokenGenerator};

use crate::application::IdentityPorts;
use crate::domain::{RepoError, SecurityPolicyProvider};

/// The sqlx implementation of every identity repository.
#[derive(Clone)]
pub struct PgIdentityStore {
    pool: PgPool,
    keyring: Keyring,
}

impl PgIdentityStore {
    /// Creates the store. `keyring` encrypts two-factor secrets at rest.
    #[must_use]
    pub const fn new(pool: PgPool, keyring: Keyring) -> Self {
        Self { pool, keyring }
    }

    /// Wires every identity port to PostgreSQL, Argon2id and the OS random generator.
    /// `policy` supplies the security settings Administrators control.
    ///
    /// # Errors
    ///
    /// Fails if the dummy password hash cannot be computed or the OS has no randomness.
    pub fn ports(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        keyring: Keyring,
        policy: Arc<dyn SecurityPolicyProvider>,
        reset_mailer: Arc<dyn crate::domain::ResetMailer>,
    ) -> Result<IdentityPorts, RepoError> {
        let store = Arc::new(Self::new(pool, keyring));
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
            setup: store.clone(),
            trusted_devices: store.clone(),
            throttle: store.clone(),
            api_keys: store.clone(),
            resets: store,
            reset_mailer,
            policy,
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

/// A [`SecurityPolicyProvider`] that always returns the same policy (tests, tools).
#[derive(Debug, Clone, Copy, Default)]
pub struct FixedSecurityPolicy(pub crate::domain::SecurityPolicy);

#[async_trait::async_trait]
impl SecurityPolicyProvider for FixedSecurityPolicy {
    async fn policy(&self) -> crate::domain::SecurityPolicy {
        self.0
    }
}

/// A [`crate::domain::ResetMailer`] that only logs (tests, or when email is not set up).
#[derive(Debug, Clone, Copy, Default)]
pub struct LoggingResetMailer;

#[async_trait::async_trait]
impl crate::domain::ResetMailer for LoggingResetMailer {
    async fn send_reset(
        &self,
        _to: &str,
        _name: &str,
        _token: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), RepoError> {
        tracing::warn!(%expires_at, "password reset requested but no mailer is configured");
        Ok(())
    }
}
