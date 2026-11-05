//! Portal Accounts, Portal sessions and set-password links (ADR-0021). Not
//! tenant-scoped: they are read before any Company is known.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AuditRecord, RequesterId};
use uuid::Uuid;

use super::{PgIdentityStore, audit, db_err, stored};
use crate::domain::{
    EmailAddress, Name, PasswordHash, PortalAccount, PortalAccountId, PortalLink,
    PortalLinkPurpose, PortalRepository, PortalSession, PortalSessionId, RepoError,
};

struct AccountRow {
    id: Uuid,
    email: String,
    password_hash: String,
    password_changed_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<AccountRow> for PortalAccount {
    type Error = RepoError;

    fn try_from(r: AccountRow) -> Result<Self, RepoError> {
        Ok(Self::restore(
            PortalAccountId::from_uuid(r.id),
            stored(EmailAddress::try_from(r.email))?,
            PasswordHash::from_phc(r.password_hash),
            r.password_changed_at,
            r.created_at,
            r.updated_at,
        ))
    }
}

struct SessionRow {
    id: Uuid,
    portal_account_id: Uuid,
    requester_id: Uuid,
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
impl PortalRepository for PgIdentityStore {
    async fn account_by_email(&self, email: &str) -> Result<Option<PortalAccount>, RepoError> {
        sqlx::query_as!(
            AccountRow,
            "SELECT id, email, password_hash, password_changed_at, created_at, updated_at
             FROM portal_accounts WHERE lower(email) = lower($1)",
            email
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(PortalAccount::try_from)
        .transpose()
    }

    async fn account(&self, id: PortalAccountId) -> Result<Option<PortalAccount>, RepoError> {
        sqlx::query_as!(
            AccountRow,
            "SELECT id, email, password_hash, password_changed_at, created_at, updated_at
             FROM portal_accounts WHERE id = $1",
            id.as_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(PortalAccount::try_from)
        .transpose()
    }

    async fn save_account(&self, a: &PortalAccount, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO portal_accounts (id, email, password_hash, password_changed_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE
             SET password_hash = EXCLUDED.password_hash,
                 password_changed_at = EXCLUDED.password_changed_at,
                 updated_at = EXCLUDED.updated_at",
            a.id().as_uuid(),
            a.email().as_str(),
            a.password_hash().as_str(),
            a.password_changed_at(),
            a.created_at(),
            a.updated_at(),
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "UPDATE requester_sessions SET revoked_at = $2
             WHERE portal_account_id = $1 AND revoked_at IS NULL",
            a.id().as_uuid(),
            a.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn issue_link(&self, link: &PortalLink) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "DELETE FROM portal_links
             WHERE lower(email) = lower($1) AND purpose = $2 AND used_at IS NULL",
            link.email().as_str(),
            link.purpose().as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO portal_links (token_hash, purpose, email, full_name, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            link.token_hash(),
            link.purpose().as_str(),
            link.email().as_str(),
            link.full_name().map(Name::as_str),
            link.created_at(),
            link.expires_at(),
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn consume_link(
        &self,
        token_hash: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Option<PortalLink>, RepoError> {
        // One statement, so a link cannot be used twice by concurrent requests.
        let Some(r) = sqlx::query!(
            "UPDATE portal_links SET used_at = $2
             WHERE token_hash = $1 AND used_at IS NULL AND expires_at > $2
             RETURNING token_hash, purpose, email, full_name, created_at, expires_at",
            token_hash,
            now
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        else {
            return Ok(None);
        };
        let purpose = PortalLinkPurpose::parse(&r.purpose)
            .ok_or_else(|| RepoError::Storage(format!("unknown link purpose {}", r.purpose)))?;
        Ok(Some(PortalLink::restore(
            r.token_hash,
            purpose,
            stored(EmailAddress::try_from(r.email))?,
            r.full_name
                .map(Name::try_from)
                .transpose()
                .map_err(|e| RepoError::Storage(format!("invalid stored value: {e}")))?,
            r.created_at,
            r.expires_at,
        )))
    }

    async fn insert_session(&self, s: &PortalSession) -> Result<(), RepoError> {
        sqlx::query!(
            "INSERT INTO requester_sessions (id, portal_account_id, requester_id, token_hash, csrf_token,
                                             created_at, last_seen_at, idle_expires_at, expires_at,
                                             revoked_at, ip_address, user_agent)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::text::inet, $12)",
            s.id().as_uuid(),
            s.account_id().as_uuid(),
            s.requester_id().as_uuid(),
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

    async fn session_by_token_hash(&self, hash: &[u8]) -> Result<Option<PortalSession>, RepoError> {
        let row = sqlx::query_as!(
            SessionRow,
            "SELECT id, portal_account_id, requester_id, token_hash, csrf_token, created_at, last_seen_at,
                    idle_expires_at, expires_at, revoked_at, host(ip_address) AS ip_address, user_agent
             FROM requester_sessions WHERE token_hash = $1",
            hash
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(|r| {
            PortalSession::restore(
                PortalSessionId::from_uuid(r.id),
                PortalAccountId::from_uuid(r.portal_account_id),
                RequesterId::from_uuid(r.requester_id),
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

    async fn update_session(&self, s: &PortalSession) -> Result<(), RepoError> {
        sqlx::query!(
            "UPDATE requester_sessions
             SET requester_id = $2, last_seen_at = $3, idle_expires_at = $4,
                 revoked_at = COALESCE(revoked_at, $5)
             WHERE id = $1",
            s.id().as_uuid(),
            s.requester_id().as_uuid(),
            s.last_seen_at(),
            s.idle_expires_at(),
            s.revoked_at(),
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}
