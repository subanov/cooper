//! Analyst browser sessions (FR-IAM-002, ADR-0011).

use std::fmt;

use chrono::{DateTime, Duration, Utc};
use cooper_kernel::AnalystId;
use sha2::{Digest, Sha256};

cooper_kernel::define_id!(
    /// Identifies a session.
    SessionId
);

/// How long sessions live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    /// A session expires after this long without a request.
    pub idle_timeout: Duration,
    /// A session expires this long after sign-in, whatever the activity.
    pub absolute_lifetime: Duration,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::hours(8),
            absolute_lifetime: Duration::days(7),
        }
    }
}

/// The secret session token sent to the browser in the session cookie. Only its hash is
/// stored. Never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Wraps a token: freshly generated (hex of 32 random bytes) or read from a cookie.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self(value)
    }

    /// The token as sent to the browser.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// SHA-256 of the token, as stored.
    #[must_use]
    pub fn hash(&self) -> Vec<u8> {
        Sha256::digest(self.0.as_bytes()).to_vec()
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

/// Compares two secrets without leaking where they differ.
pub(super) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A signed-in browser session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    id: SessionId,
    analyst_id: AnalystId,
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

/// Sessions are refreshed at most this often, to avoid a database write per request.
const TOUCH_INTERVAL_SECS: i64 = 60;

impl Session {
    /// Starts a session.
    #[must_use]
    pub fn start(
        analyst_id: AnalystId,
        token: &SessionToken,
        csrf_token: String,
        policy: SessionPolicy,
        now: DateTime<Utc>,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> Self {
        let expires_at = now + policy.absolute_lifetime;
        Self {
            id: SessionId::new(),
            analyst_id,
            token_hash: token.hash(),
            csrf_token,
            created_at: now,
            last_seen_at: now,
            idle_expires_at: (now + policy.idle_timeout).min(expires_at),
            expires_at,
            revoked_at: None,
            ip_address,
            user_agent,
        }
    }

    /// Rebuilds a session from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: SessionId,
        analyst_id: AnalystId,
        token_hash: Vec<u8>,
        csrf_token: String,
        created_at: DateTime<Utc>,
        last_seen_at: DateTime<Utc>,
        idle_expires_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> Self {
        Self {
            id,
            analyst_id,
            token_hash,
            csrf_token,
            created_at,
            last_seen_at,
            idle_expires_at,
            expires_at,
            revoked_at,
            ip_address,
            user_agent,
        }
    }

    /// Whether the session can still authenticate requests.
    #[must_use]
    pub fn is_valid(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && now < self.idle_expires_at && now < self.expires_at
    }

    /// Records activity, extending the idle expiry (never past the absolute expiry).
    /// Returns `true` if the change is worth storing.
    pub fn touch(&mut self, policy: SessionPolicy, now: DateTime<Utc>) -> bool {
        if now - self.last_seen_at < Duration::seconds(TOUCH_INTERVAL_SECS) {
            return false;
        }
        self.last_seen_at = now;
        self.idle_expires_at = (now + policy.idle_timeout).min(self.expires_at);
        true
    }

    /// Ends the session.
    pub fn revoke(&mut self, now: DateTime<Utc>) {
        if self.revoked_at.is_none() {
            self.revoked_at = Some(now);
        }
    }

    /// Whether `candidate` is this session's CSRF token (constant-time comparison).
    #[must_use]
    pub fn csrf_matches(&self, candidate: &str) -> bool {
        constant_time_eq(self.csrf_token.as_bytes(), candidate.as_bytes())
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> SessionId {
        self.id
    }
    /// The signed-in Analyst.
    #[must_use]
    pub const fn analyst_id(&self) -> AnalystId {
        self.analyst_id
    }
    /// SHA-256 of the token.
    #[must_use]
    pub fn token_hash(&self) -> &[u8] {
        &self.token_hash
    }
    /// The CSRF token.
    #[must_use]
    pub fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
    /// When the session started.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// The last recorded activity.
    #[must_use]
    pub const fn last_seen_at(&self) -> DateTime<Utc> {
        self.last_seen_at
    }
    /// When the session expires without activity.
    #[must_use]
    pub const fn idle_expires_at(&self) -> DateTime<Utc> {
        self.idle_expires_at
    }
    /// When the session expires regardless of activity.
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    /// When the session was revoked.
    #[must_use]
    pub const fn revoked_at(&self) -> Option<DateTime<Utc>> {
        self.revoked_at
    }
    /// The client IP at sign-in.
    #[must_use]
    pub fn ip_address(&self) -> Option<&str> {
        self.ip_address.as_deref()
    }
    /// The client user agent at sign-in.
    #[must_use]
    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap()
    }

    fn session(policy: SessionPolicy) -> Session {
        Session::start(
            AnalystId::new(),
            &SessionToken::new("tok".into()),
            "csrf".into(),
            policy,
            t0(),
            None,
            None,
        )
    }

    #[test]
    fn idle_session_expires() {
        let policy = SessionPolicy {
            idle_timeout: Duration::minutes(30),
            absolute_lifetime: Duration::days(1),
        };
        let mut s = session(policy);
        assert!(s.is_valid(t0() + Duration::minutes(29)));
        assert!(!s.is_valid(t0() + Duration::minutes(30)));
        assert!(s.touch(policy, t0() + Duration::minutes(20)));
        assert!(s.is_valid(t0() + Duration::minutes(45)));
    }

    #[test]
    fn activity_never_extends_past_absolute_lifetime() {
        let policy = SessionPolicy {
            idle_timeout: Duration::hours(8),
            absolute_lifetime: Duration::hours(10),
        };
        let mut s = session(policy);
        s.touch(policy, t0() + Duration::hours(9));
        assert_eq!(s.idle_expires_at(), t0() + Duration::hours(10));
        assert!(!s.is_valid(t0() + Duration::hours(10)));
    }

    #[test]
    fn frequent_requests_do_not_write() {
        let policy = SessionPolicy::default();
        let mut s = session(policy);
        assert!(!s.touch(policy, t0() + Duration::seconds(30)));
        assert!(s.touch(policy, t0() + Duration::seconds(61)));
    }

    #[test]
    fn revoked_session_is_invalid_and_token_is_hashed() {
        let mut s = session(SessionPolicy::default());
        s.revoke(t0());
        assert!(!s.is_valid(t0()));
        assert_eq!(s.token_hash().len(), 32);
        assert_ne!(s.token_hash(), b"tok");
        assert!(s.csrf_matches("csrf"));
        assert!(!s.csrf_matches("csrX"));
        assert!(!s.csrf_matches("csr"));
        assert_eq!(
            format!("{:?}", SessionToken::new("x".into())),
            "SessionToken(<redacted>)"
        );
    }
}
