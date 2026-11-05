//! Sign-in protection: trusted devices (FR-IAM-003), IP bans (FR-IAM-005) and the
//! security policy Administrators set through System Settings.

use std::net::IpAddr;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use cooper_kernel::AnalystId;

use super::{RepoError, SessionToken};

cooper_kernel::define_id!(
    /// Identifies a trusted device.
    TrustedDeviceId
);

/// How long a remembered device skips the two-factor code.
pub const TRUSTED_DEVICE_DAYS: i64 = 30;

/// A browser an Analyst chose to remember after a two-factor sign-in. Only the hash of
/// its token is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedDevice {
    id: TrustedDeviceId,
    analyst_id: AnalystId,
    token_hash: Vec<u8>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    user_agent: Option<String>,
}

impl TrustedDevice {
    /// Remembers a device for [`TRUSTED_DEVICE_DAYS`].
    #[must_use]
    pub fn remember(
        analyst_id: AnalystId,
        token: &SessionToken,
        user_agent: Option<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: TrustedDeviceId::new(),
            analyst_id,
            token_hash: token.hash(),
            created_at: now,
            expires_at: now + Duration::days(TRUSTED_DEVICE_DAYS),
            user_agent,
        }
    }

    /// Rebuilds a device from storage.
    #[must_use]
    pub const fn restore(
        id: TrustedDeviceId,
        analyst_id: AnalystId,
        token_hash: Vec<u8>,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        user_agent: Option<String>,
    ) -> Self {
        Self {
            id,
            analyst_id,
            token_hash,
            created_at,
            expires_at,
            user_agent,
        }
    }

    /// Whether the device still skips the code for `analyst`.
    #[must_use]
    pub fn trusts(&self, analyst: AnalystId, now: DateTime<Utc>) -> bool {
        self.analyst_id == analyst && now < self.expires_at
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> TrustedDeviceId {
        self.id
    }
    /// The Analyst.
    #[must_use]
    pub const fn analyst_id(&self) -> AnalystId {
        self.analyst_id
    }
    /// SHA-256 of the device token.
    #[must_use]
    pub fn token_hash(&self) -> &[u8] {
        &self.token_hash
    }
    /// When it was remembered.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// When it stops being trusted.
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    /// The browser at the time.
    #[must_use]
    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }
}

/// Security settings Administrators control (System Settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityPolicy {
    /// Every Analyst must use two-factor sign-in.
    pub require_totp: bool,
    /// Failed sign-ins from one IP address within the window that cause a ban.
    pub max_failed_sign_ins: u32,
    /// Length of the window and of the ban.
    pub ban_duration: Duration,
    /// People at a Company's Sender Domain may create a Portal Account themselves.
    pub portal_self_registration: bool,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            require_totp: false,
            max_failed_sign_ins: 10,
            ban_duration: Duration::minutes(15),
            portal_self_registration: false,
        }
    }
}

/// Reads the current [`SecurityPolicy`].
#[async_trait]
pub trait SecurityPolicyProvider: Send + Sync {
    /// The policy now in force.
    async fn policy(&self) -> SecurityPolicy;
}

/// Trusted devices.
#[async_trait]
pub trait TrustedDeviceRepository: Send + Sync {
    /// Stores a device.
    async fn insert(&self, device: &TrustedDevice) -> Result<(), RepoError>;
    /// Finds a device by the hash of its token.
    async fn find_by_token_hash(&self, hash: &[u8]) -> Result<Option<TrustedDevice>, RepoError>;
}

/// Failed sign-ins and temporary IP bans (FR-IAM-005).
#[async_trait]
pub trait SignInThrottle: Send + Sync {
    /// When the ban on `ip` ends, if it is banned now.
    async fn banned_until(
        &self,
        ip: IpAddr,
        now: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>, RepoError>;
    /// Records a failure. Bans the address when `policy.max_failed_sign_ins` failures
    /// happened within `policy.ban_duration`; returns the end of the ban if one starts.
    async fn record_failure(
        &self,
        ip: IpAddr,
        login: &str,
        now: DateTime<Utc>,
        policy: &SecurityPolicy,
    ) -> Result<Option<DateTime<Utc>>, RepoError>;
    /// Forgets the failures of `ip` after a successful sign-in.
    async fn clear(&self, ip: IpAddr) -> Result<(), RepoError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn devices_are_trusted_for_their_analyst_until_expiry() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let me = AnalystId::new();
        let d = TrustedDevice::remember(me, &SessionToken::new("t".into()), None, now);
        assert!(d.trusts(me, now + Duration::days(29)));
        assert!(!d.trusts(me, now + Duration::days(30)));
        assert!(!d.trusts(AnalystId::new(), now));
    }
}
