//! Analyst sessions.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::AnalystId;
use uuid::Uuid;

use super::{PgIdentityStore, db_err};
use crate::domain::{RepoError, Session, SessionId, SessionRepository};

struct SessionRow {
    id: Uuid,
    analyst_id: Uuid,
    token_hash: Vec<u8>,
    csrf_token: String,
    created_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    idle_expires_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    ip_address: Option<String>,
    user_agent: Option<String>,
}

#[async_trait]
impl SessionRepository for PgIdentityStore {
    async fn insert(&self, s: &Session) -> Result<(), RepoError> {
        sqlx::query!(
            "INSERT INTO analyst_sessions (id, analyst_id, token_hash, csrf_token, created_at, last_seen_at,
                                           idle_expires_at, expires_at, revoked_at, ip_address, user_agent)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::text::inet, $11)",
            s.id().as_uuid(),
            s.analyst_id().as_uuid(),
            s.token_hash(),
            s.csrf_token(),
            s.created_at(),
            s.last_seen_at(),
            s.idle_expires_at(),
            s.expires_at(),
            s.revoked_at(),
            s.ip_address(),
            s.user_agent(),
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn find_by_token_hash(&self, hash: &[u8]) -> Result<Option<Session>, RepoError> {
        let row = sqlx::query_as!(
            SessionRow,
            "SELECT id, analyst_id, token_hash, csrf_token, created_at, last_seen_at, idle_expires_at,
                    expires_at, revoked_at, host(ip_address) AS ip_address, user_agent
             FROM analyst_sessions WHERE token_hash = $1",
            hash
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(|r| {
            Session::restore(
                SessionId::from_uuid(r.id),
                AnalystId::from_uuid(r.analyst_id),
                r.token_hash,
                r.csrf_token,
                r.created_at,
                r.last_seen_at,
                r.idle_expires_at,
                r.expires_at,
                r.revoked_at,
                r.ip_address,
                r.user_agent,
            )
        }))
    }

    async fn touch(&self, s: &Session) -> Result<(), RepoError> {
        sqlx::query!(
            "UPDATE analyst_sessions SET last_seen_at = $2, idle_expires_at = $3 WHERE id = $1",
            s.id().as_uuid(),
            s.last_seen_at(),
            s.idle_expires_at()
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn revoke(&self, s: &Session) -> Result<(), RepoError> {
        sqlx::query!(
            "UPDATE analyst_sessions SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL",
            s.id().as_uuid(),
            s.revoked_at()
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}
