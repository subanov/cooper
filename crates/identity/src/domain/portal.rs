//! Portal Accounts, Portal sessions and set-password links (FR-PRT-001, ADR-0021).
//!
//! A Portal Account belongs to an email address and acts for every active Requester
//! with that address. Passwords are only ever set through an emailed one-time link, so
//! every account's address is confirmed.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use cooper_kernel::{AuditRecord, RequesterId, TenantId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit::Audited;
use super::session::constant_time_eq;
use super::{EmailAddress, Name, PasswordHash, RepoError, SessionPolicy, SessionToken};

cooper_kernel::define_id!(
    /// Identifies a Portal Account.
    PortalAccountId
);

cooper_kernel::define_id!(
    /// Identifies a Portal session.
    PortalSessionId
);

/// The Portal sign-in of one email address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalAccount {
    id: PortalAccountId,
    email: EmailAddress,
    password_hash: PasswordHash,
    password_changed_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl PortalAccount {
    /// Creates the account when its first password is set through a link.
    #[must_use]
    pub fn create(email: EmailAddress, password_hash: PasswordHash, now: DateTime<Utc>) -> Self {
        Self {
            id: PortalAccountId::new(),
            email,
            password_hash,
            password_changed_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    /// Rebuilds an account from storage.
    #[must_use]
    pub const fn restore(
        id: PortalAccountId,
        email: EmailAddress,
        password_hash: PasswordHash,
        password_changed_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            email,
            password_hash,
            password_changed_at,
            created_at,
            updated_at,
        }
    }

    /// Replaces the password.
    pub fn set_password(&mut self, hash: PasswordHash, now: DateTime<Utc>) {
        self.password_hash = hash;
        self.password_changed_at = now;
        self.updated_at = now;
    }

    /// The ID.
    #[must_use]
    pub const fn id(&self) -> PortalAccountId {
        self.id
    }
    /// The email address.
    #[must_use]
    pub const fn email(&self) -> &EmailAddress {
        &self.email
    }
    /// The password hash.
    #[must_use]
    pub const fn password_hash(&self) -> &PasswordHash {
        &self.password_hash
    }
    /// When the password was last set.
    #[must_use]
    pub const fn password_changed_at(&self) -> DateTime<Utc> {
        self.password_changed_at
    }
    /// When the account was created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// When the account last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl Audited for PortalAccount {
    const AGGREGATE_TYPE: &'static str = "portal_account";
    fn audit_id(&self) -> Uuid {
        self.id.as_uuid()
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        // Never the hash; the change time shows that the password changed.
        vec![
            ("email", json!(self.email)),
            ("password_changed_at", json!(self.password_changed_at)),
        ]
    }
}

/// Why a set-password link was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalLinkPurpose {
    /// An Analyst invited the Requester (Portal Invitation).
    Invitation,
    /// The person asked to reset (or get) a password.
    Reset,
    /// Self-Registration: the link also creates the Requester.
    Registration,
}

impl PortalLinkPurpose {
    /// Stable name used in storage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Invitation => "invitation",
            Self::Reset => "reset",
            Self::Registration => "registration",
        }
    }

    /// Parses a stored name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "invitation" => Some(Self::Invitation),
            "reset" => Some(Self::Reset),
            "registration" => Some(Self::Registration),
            _ => None,
        }
    }

    /// How long a link of this purpose works.
    #[must_use]
    pub fn lifetime(self) -> Duration {
        match self {
            Self::Invitation => Duration::days(7),
            Self::Reset => Duration::minutes(60),
            Self::Registration => Duration::hours(24),
        }
    }
}

/// A one-time link that sets a Portal password. Only the hash of its token is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalLink {
    token_hash: Vec<u8>,
    purpose: PortalLinkPurpose,
    email: EmailAddress,
    full_name: Option<Name>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl PortalLink {
    /// A Portal Invitation or reset link.
    #[must_use]
    pub fn new(
        purpose: PortalLinkPurpose,
        token: &SessionToken,
        email: EmailAddress,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            token_hash: token.hash(),
            purpose,
            email,
            full_name: None,
            created_at: now,
            expires_at: now + purpose.lifetime(),
        }
    }

    /// A Self-Registration link, carrying the name for the new Requester.
    #[must_use]
    pub fn registration(
        token: &SessionToken,
        email: EmailAddress,
        full_name: Name,
        now: DateTime<Utc>,
    ) -> Self {
        let mut link = Self::new(PortalLinkPurpose::Registration, token, email, now);
        link.full_name = Some(full_name);
        link
    }

    /// Rebuilds a link from storage.
    #[must_use]
    pub const fn restore(
        token_hash: Vec<u8>,
        purpose: PortalLinkPurpose,
        email: EmailAddress,
        full_name: Option<Name>,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            token_hash,
            purpose,
            email,
            full_name,
            created_at,
            expires_at,
        }
    }

    /// SHA-256 of the token.
    #[must_use]
    pub fn token_hash(&self) -> &[u8] {
        &self.token_hash
    }
    /// Why it was sent.
    #[must_use]
    pub const fn purpose(&self) -> PortalLinkPurpose {
        self.purpose
    }
    /// The address it was sent to.
    #[must_use]
    pub const fn email(&self) -> &EmailAddress {
        &self.email
    }
    /// The name given at Self-Registration.
    #[must_use]
    pub const fn full_name(&self) -> Option<&Name> {
        self.full_name.as_ref()
    }
    /// When it was sent.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// When it stops working.
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

/// A signed-in Portal session, acting as one of the account's Requesters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalSession {
    id: PortalSessionId,
    account_id: PortalAccountId,
    requester_id: RequesterId,
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

/// Sessions are refreshed at most this often.
const TOUCH_INTERVAL_SECS: i64 = 60;

/// Fields of a new Portal session.
#[derive(Debug, Clone)]
pub struct NewPortalSession<'a> {
    /// The signed-in account.
    pub account_id: PortalAccountId,
    /// The Requester the session acts as first.
    pub requester_id: RequesterId,
    /// The cookie token.
    pub token: &'a SessionToken,
    /// The CSRF token.
    pub csrf_token: String,
    /// Client IP.
    pub ip_address: Option<String>,
    /// Client user agent.
    pub user_agent: Option<String>,
}

impl PortalSession {
    /// Starts a session.
    #[must_use]
    pub fn start(new: NewPortalSession<'_>, policy: SessionPolicy, now: DateTime<Utc>) -> Self {
        let expires_at = now + policy.absolute_lifetime;
        Self {
            id: PortalSessionId::new(),
            account_id: new.account_id,
            requester_id: new.requester_id,
            token_hash: new.token.hash(),
            csrf_token: new.csrf_token,
            created_at: now,
            last_seen_at: now,
            idle_expires_at: (now + policy.idle_timeout).min(expires_at),
            expires_at,
            revoked_at: None,
            ip_address: new.ip_address,
            user_agent: new.user_agent,
        }
    }

    /// Rebuilds a session from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: PortalSessionId,
        account_id: PortalAccountId,
        requester_id: RequesterId,
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
            account_id,
            requester_id,
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

    /// Records activity; returns `true` if the change is worth storing.
    pub fn touch(&mut self, policy: SessionPolicy, now: DateTime<Utc>) -> bool {
        if now - self.last_seen_at < Duration::seconds(TOUCH_INTERVAL_SECS) {
            return false;
        }
        self.last_seen_at = now;
        self.idle_expires_at = (now + policy.idle_timeout).min(self.expires_at);
        true
    }

    /// Acts as another of the account's Requesters. The application checks that the
    /// Requester is active and has the account's address.
    pub const fn switch_requester(&mut self, requester_id: RequesterId) {
        self.requester_id = requester_id;
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
    pub const fn id(&self) -> PortalSessionId {
        self.id
    }
    /// The signed-in account.
    #[must_use]
    pub const fn account_id(&self) -> PortalAccountId {
        self.account_id
    }
    /// The Requester the session acts as.
    #[must_use]
    pub const fn requester_id(&self) -> RequesterId {
        self.requester_id
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

/// Portal Accounts, sessions and links.
#[async_trait]
pub trait PortalRepository: Send + Sync {
    /// The account of an email address (case-insensitive).
    async fn account_by_email(&self, email: &str) -> Result<Option<PortalAccount>, RepoError>;
    /// An account by ID.
    async fn account(&self, id: PortalAccountId) -> Result<Option<PortalAccount>, RepoError>;
    /// Inserts or updates an account and ends all of its sessions (the password changed).
    async fn save_account(
        &self,
        account: &PortalAccount,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Stores a link, replacing the unused links of the same address and purpose.
    async fn issue_link(&self, link: &PortalLink) -> Result<(), RepoError>;
    /// Marks a link used and returns it, if it exists, is unused and has not expired.
    async fn consume_link(
        &self,
        token_hash: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Option<PortalLink>, RepoError>;
    /// Stores a new session.
    async fn insert_session(&self, session: &PortalSession) -> Result<(), RepoError>;
    /// Finds a session by the hash of its token.
    async fn session_by_token_hash(&self, hash: &[u8]) -> Result<Option<PortalSession>, RepoError>;
    /// Stores activity, the current Requester and revocation of a session.
    async fn update_session(&self, session: &PortalSession) -> Result<(), RepoError>;
}

/// An email with a set-password link.
#[derive(Debug, Clone)]
pub struct PortalLinkEmail {
    /// Why it is sent.
    pub purpose: PortalLinkPurpose,
    /// Recipient address.
    pub to: String,
    /// Recipient name.
    pub name: String,
    /// The raw token for the link.
    pub token: String,
    /// When the link stops working.
    pub expires_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap()
    }

    #[test]
    fn links_expire_by_purpose_and_registration_carries_a_name() {
        let token = SessionToken::new("t".into());
        let email = EmailAddress::try_from("sam@acme.example").unwrap();
        let invite = PortalLink::new(PortalLinkPurpose::Invitation, &token, email.clone(), t0());
        assert_eq!(invite.expires_at(), t0() + Duration::days(7));
        assert_eq!(invite.full_name(), None);
        let reset = PortalLink::new(PortalLinkPurpose::Reset, &token, email.clone(), t0());
        assert_eq!(reset.expires_at(), t0() + Duration::minutes(60));
        let reg = PortalLink::registration(&token, email, Name::try_from("Sam").unwrap(), t0());
        assert_eq!(reg.purpose(), PortalLinkPurpose::Registration);
        assert_eq!(reg.full_name().map(Name::as_str), Some("Sam"));
        assert_ne!(reg.token_hash(), b"t");
        for p in [
            PortalLinkPurpose::Invitation,
            PortalLinkPurpose::Reset,
            PortalLinkPurpose::Registration,
        ] {
            assert_eq!(PortalLinkPurpose::parse(p.as_str()), Some(p));
        }
    }

    #[test]
    fn portal_sessions_expire_switch_and_check_csrf() {
        let policy = SessionPolicy {
            idle_timeout: Duration::minutes(30),
            absolute_lifetime: Duration::hours(1),
        };
        let first = RequesterId::new();
        let mut s = PortalSession::start(
            NewPortalSession {
                account_id: PortalAccountId::new(),
                requester_id: first,
                token: &SessionToken::new("tok".into()),
                csrf_token: "csrf".into(),
                ip_address: None,
                user_agent: None,
            },
            policy,
            t0(),
        );
        assert!(s.is_valid(t0() + Duration::minutes(29)));
        assert!(!s.is_valid(t0() + Duration::minutes(30)));
        assert!(s.touch(policy, t0() + Duration::minutes(20)));
        assert!(!s.touch(policy, t0() + Duration::minutes(20)));
        assert_eq!(s.idle_expires_at(), t0() + Duration::minutes(50));
        assert!(s.touch(policy, t0() + Duration::minutes(45)));
        assert_eq!(s.idle_expires_at(), t0() + Duration::hours(1));
        let other = RequesterId::new();
        s.switch_requester(other);
        assert_eq!(s.requester_id(), other);
        assert!(s.csrf_matches("csrf") && !s.csrf_matches("csrX"));
        s.revoke(t0());
        assert!(!s.is_valid(t0()));
    }

    #[test]
    fn account_audit_never_contains_the_hash() {
        let account = PortalAccount::create(
            EmailAddress::try_from("sam@acme.example").unwrap(),
            PasswordHash::from_phc("$argon2id$secret".into()),
            t0(),
        );
        let fields = format!("{:?}", account.audit_fields());
        assert!(!fields.contains("argon2"));
    }
}
