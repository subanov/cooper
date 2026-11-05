//! Trusted devices and sign-in throttling (IP bans).

use std::net::IpAddr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::AnalystId;
use uuid::Uuid;

use super::{PgIdentityStore, db_err};
use crate::domain::{
    RepoError, SecurityPolicy, SignInThrottle, TrustedDevice, TrustedDeviceId,
    TrustedDeviceRepository,
};

#[async_trait]
impl TrustedDeviceRepository for PgIdentityStore {
    async fn insert(&self, d: &TrustedDevice) -> Result<(), RepoError> {
        sqlx::query!(
            "INSERT INTO analyst_trusted_devices (id, analyst_id, token_hash, created_at, expires_at, user_agent)
             VALUES ($1, $2, $3, $4, $5, $6)",
            d.id().as_uuid(),
            d.analyst_id().as_uuid(),
            d.token_hash(),
            d.created_at(),
            d.expires_at(),
            d.user_agent()
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn find_by_token_hash(&self, hash: &[u8]) -> Result<Option<TrustedDevice>, RepoError> {
        let row = sqlx::query!(
            "SELECT id, analyst_id, token_hash, created_at, expires_at, user_agent
             FROM analyst_trusted_devices WHERE token_hash = $1",
            hash
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(|r| {
            TrustedDevice::restore(
                TrustedDeviceId::from_uuid(r.id),
                AnalystId::from_uuid(r.analyst_id),
                r.token_hash,
                r.created_at,
                r.expires_at,
                r.user_agent,
            )
        }))
    }
}

#[async_trait]
impl SignInThrottle for PgIdentityStore {
    async fn banned_until(
        &self,
        ip: IpAddr,
        now: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>, RepoError> {
        sqlx::query_scalar!(
            "SELECT banned_until FROM ip_bans WHERE ip_address = $1::text::inet AND banned_until > $2",
            ip.to_string(),
            now
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)
    }

    async fn record_failure(
        &self,
        ip: IpAddr,
        login: &str,
        now: DateTime<Utc>,
        policy: &SecurityPolicy,
    ) -> Result<Option<DateTime<Utc>>, RepoError> {
        let ip = ip.to_string();
        let login: String = login.chars().take(200).collect();
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO sign_in_failures (id, ip_address, login, occurred_at) VALUES ($1, $2::text::inet, $3, $4)",
            Uuid::now_v7(),
            ip,
            login,
            now
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        let since = now - policy.ban_duration;
        let failures = sqlx::query_scalar!(
            r#"SELECT count(*) AS "count!" FROM sign_in_failures WHERE ip_address = $1::text::inet AND occurred_at > $2"#,
            ip,
            since
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        let ban = if failures >= i64::from(policy.max_failed_sign_ins) {
            let until = now + policy.ban_duration;
            sqlx::query!(
                "INSERT INTO ip_bans (ip_address, banned_until, created_at) VALUES ($1::text::inet, $2, $3)
                 ON CONFLICT (ip_address) DO UPDATE SET banned_until = EXCLUDED.banned_until, created_at = EXCLUDED.created_at",
                ip,
                until,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            sqlx::query!(
                "DELETE FROM sign_in_failures WHERE ip_address = $1::text::inet",
                ip
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            Some(until)
        } else {
            None
        };
        tx.commit().await.map_err(db_err)?;
        Ok(ban)
    }

    async fn clear(&self, ip: IpAddr) -> Result<(), RepoError> {
        sqlx::query!(
            "DELETE FROM sign_in_failures WHERE ip_address = $1::text::inet",
            ip.to_string()
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

#[async_trait]
impl crate::domain::PasswordResetRepository for PgIdentityStore {
    async fn issue(
        &self,
        analyst: AnalystId,
        token_hash: &[u8],
        now: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "DELETE FROM password_reset_tokens WHERE analyst_id = $1 AND used_at IS NULL",
            analyst.as_uuid()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO password_reset_tokens (token_hash, analyst_id, created_at, expires_at) VALUES ($1, $2, $3, $4)",
            token_hash,
            analyst.as_uuid(),
            now,
            expires_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn consume(
        &self,
        token_hash: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Option<AnalystId>, RepoError> {
        let id = sqlx::query_scalar!(
            "UPDATE password_reset_tokens SET used_at = $2
             WHERE token_hash = $1 AND used_at IS NULL AND expires_at > $2
             RETURNING analyst_id",
            token_hash,
            now
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(id.map(AnalystId::from_uuid))
    }
}
