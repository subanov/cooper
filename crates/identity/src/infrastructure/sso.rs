//! OIDC providers, sign-ins in progress and linked identities (ADR-0025). Client
//! secrets are encrypted with the Keyring, bound to `sso_providers.client_secret:<id>`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AnalystId, AuditRecord};
use uuid::Uuid;

use super::{PgIdentityStore, audit, db_err, stored};
use crate::domain::{
    RepoError, SecretChange, SsoAudience, SsoIdentity, SsoLogin, SsoProvider, SsoProviderInput,
    SsoRepository,
};

fn purpose(id: Uuid) -> String {
    format!("sso_providers.client_secret:{id}")
}

#[async_trait]
impl SsoRepository for PgIdentityStore {
    async fn providers(&self) -> Result<Vec<SsoProvider>, RepoError> {
        let rows = sqlx::query!(
            "SELECT id, display_name, issuer_url, client_id, scopes, audience, link_by_verified_email, is_active,
                    created_at, updated_at
             FROM sso_providers ORDER BY display_name, id"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(SsoProvider::restore(
                    r.id,
                    SsoProviderInput {
                        display_name: r.display_name,
                        issuer_url: r.issuer_url,
                        client_id: r.client_id,
                        audience: stored(r.audience.parse())?,
                        scopes: r.scopes,
                        link_by_verified_email: r.link_by_verified_email,
                        is_active: r.is_active,
                    },
                    r.created_at,
                    r.updated_at,
                ))
            })
            .collect()
    }

    async fn save_provider(
        &self,
        p: &SsoProvider,
        secret: SecretChange<'_>,
        record: &AuditRecord,
    ) -> Result<(), RepoError> {
        let i = p.input();
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO sso_providers (id, display_name, issuer_url, client_id, scopes, audience, link_by_verified_email,
                                        is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET display_name = EXCLUDED.display_name, issuer_url = EXCLUDED.issuer_url,
                 client_id = EXCLUDED.client_id, scopes = EXCLUDED.scopes, audience = EXCLUDED.audience,
                 link_by_verified_email = EXCLUDED.link_by_verified_email, is_active = EXCLUDED.is_active,
                 updated_at = EXCLUDED.updated_at",
            p.id(),
            i.display_name,
            i.issuer_url,
            i.client_id,
            i.scopes,
            i.audience.as_str(),
            i.link_by_verified_email,
            i.is_active,
            p.created_at(),
            p.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        let encrypted = match secret {
            SecretChange::Keep => None,
            SecretChange::Clear => Some(None),
            SecretChange::Set(s) => Some(Some(
                self.keyring
                    .encrypt(&purpose(p.id()), s.as_bytes())
                    .map_err(|e| RepoError::Storage(e.to_string()))?,
            )),
        };
        if let Some(value) = encrypted {
            sqlx::query!(
                "UPDATE sso_providers SET client_secret_enc = $2 WHERE id = $1",
                p.id(),
                value
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn client_secret(&self, provider: Uuid) -> Result<Option<String>, RepoError> {
        let row = sqlx::query_scalar!(
            "SELECT client_secret_enc FROM sso_providers WHERE id = $1",
            provider
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .flatten();
        row.map(|enc| {
            let plain = self
                .keyring
                .decrypt(&purpose(provider), &enc)
                .map_err(|e| RepoError::Storage(e.to_string()))?;
            String::from_utf8(plain).map_err(|e| RepoError::Storage(e.to_string()))
        })
        .transpose()
    }

    async fn insert_login(&self, l: &SsoLogin) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        // Housekeeping on the way: abandoned sign-ins never pile up.
        sqlx::query!("DELETE FROM sso_logins WHERE expires_at < now()")
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO sso_logins (state_hash, provider_id, audience, nonce, pkce_verifier, return_to, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            l.state_hash,
            l.provider_id,
            l.audience.as_str(),
            l.nonce,
            l.pkce_verifier,
            l.return_to,
            l.expires_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn take_login(&self, state_hash: &[u8]) -> Result<Option<SsoLogin>, RepoError> {
        let row = sqlx::query!(
            "DELETE FROM sso_logins WHERE state_hash = $1
             RETURNING state_hash, provider_id, audience, nonce, pkce_verifier, return_to, expires_at",
            state_hash
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|r| {
            Ok(SsoLogin {
                state_hash: r.state_hash,
                provider_id: r.provider_id,
                audience: stored(r.audience.parse())?,
                nonce: r.nonce,
                pkce_verifier: r.pkce_verifier,
                return_to: r.return_to,
                expires_at: r.expires_at,
            })
        })
        .transpose()
    }

    async fn identity(
        &self,
        provider: Uuid,
        audience: SsoAudience,
        subject: &str,
    ) -> Result<Option<SsoIdentity>, RepoError> {
        let row = sqlx::query!(
            "SELECT analyst_id, portal_email FROM sso_identities WHERE provider_id = $1 AND audience = $2 AND subject = $3",
            provider,
            audience.as_str(),
            subject
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.and_then(|r| match (r.analyst_id, r.portal_email) {
            (Some(a), _) => Some(SsoIdentity::Analyst(AnalystId::from_uuid(a))),
            (None, Some(e)) => Some(SsoIdentity::PortalEmail(e)),
            (None, None) => None,
        }))
    }

    async fn link(
        &self,
        provider: Uuid,
        audience: SsoAudience,
        subject: &str,
        identity: &SsoIdentity,
        now: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let (analyst, email) = match identity {
            SsoIdentity::Analyst(a) => (Some(a.as_uuid()), None),
            SsoIdentity::PortalEmail(e) => (None, Some(e.as_str())),
        };
        sqlx::query!(
            "INSERT INTO sso_identities (provider_id, subject, audience, analyst_id, portal_email, linked_at, last_login_at)
             VALUES ($1, $2, $3, $4, $5, $6, $6)
             ON CONFLICT (provider_id, audience, subject) DO UPDATE SET last_login_at = EXCLUDED.last_login_at",
            provider,
            subject,
            audience.as_str(),
            analyst,
            email,
            now
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}
