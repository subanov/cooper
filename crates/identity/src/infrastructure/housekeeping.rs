//! Background Job that removes expired identity records.

use async_trait::async_trait;
use chrono::Duration;
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};

/// Deletes expired sessions and trusted devices, old sign-in failures, expired IP bans
/// and API key usage counters. Runs hourly by default.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityHousekeeping;

#[async_trait]
impl Job for IdentityHousekeeping {
    fn name(&self) -> &'static str {
        "identity-housekeeping"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(3600)
    }

    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError> {
        let now = ctx.clock.now();
        let mut tx = ctx.pool.begin().await?;
        let sessions = sqlx::query!(
            "DELETE FROM analyst_sessions WHERE expires_at < $1 OR revoked_at < $1 - interval '1 day' OR idle_expires_at < $1 - interval '1 day'",
            now
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let devices = sqlx::query!(
            "DELETE FROM analyst_trusted_devices WHERE expires_at < $1",
            now
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let failures = sqlx::query!(
            "DELETE FROM sign_in_failures WHERE occurred_at < $1",
            now - Duration::days(1)
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query!("DELETE FROM ip_bans WHERE banned_until < $1", now)
            .execute(&mut *tx)
            .await?;
        sqlx::query!(
            "DELETE FROM api_key_usage WHERE window_start < $1",
            now - Duration::hours(1)
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(format!(
            "removed {sessions} sessions, {devices} trusted devices, {failures} sign-in failures"
        )))
    }
}

/// Re-encrypts two-factor secrets written with an older key after a key rotation
/// (ADR-0013). Runs every 10 minutes by default; idle when every value uses the active
/// key.
#[derive(Debug, Clone)]
pub struct ReencryptIdentitySecrets {
    keyring: cooper_platform::crypto::Keyring,
}

impl ReencryptIdentitySecrets {
    /// A job using `keyring` (active key first).
    #[must_use]
    pub const fn new(keyring: cooper_platform::crypto::Keyring) -> Self {
        Self { keyring }
    }
}

#[async_trait]
impl Job for ReencryptIdentitySecrets {
    fn name(&self) -> &'static str {
        "identity-secrets-reencrypt"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(600)
    }

    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError> {
        let active = format!("v{}.%", self.keyring.active_key_id());
        let rows = sqlx::query!(
            "SELECT id, totp_secret_encrypted, totp_pending_encrypted FROM analysts
             WHERE totp_secret_encrypted NOT LIKE $1 OR totp_pending_encrypted NOT LIKE $1
             LIMIT 500",
            active
        )
        .fetch_all(&ctx.pool)
        .await?;
        let mut updated = 0_u32;
        for row in rows {
            let rewrap = |kind: &str, value: Option<String>| -> Result<Option<String>, JobError> {
                let purpose = format!("analyst.{kind}:{}", row.id);
                value
                    .map(|v| {
                        let plain = self.keyring.decrypt(&purpose, &v)?;
                        Ok(self.keyring.encrypt(&purpose, &plain)?)
                    })
                    .transpose()
            };
            let secret = rewrap("totp", row.totp_secret_encrypted)?;
            let pending = rewrap("totp_pending", row.totp_pending_encrypted)?;
            sqlx::query!(
                "UPDATE analysts SET totp_secret_encrypted = $2, totp_pending_encrypted = $3 WHERE id = $1",
                row.id,
                secret,
                pending
            )
            .execute(&ctx.pool)
            .await?;
            updated += 1;
        }
        Ok(Some(format!("re-encrypted secrets of {updated} Analysts")))
    }
}
