//! Repository and service ports. Implemented with sqlx in `infrastructure`, and with
//! in-memory fakes in application tests.
//!
//! Every write takes the aggregate together with its audit record and stores both in one
//! transaction (ADR-0011), so "audit in the same transaction" (FR-GEN-001) holds.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    AnalystId, ApiKeyId, AuditRecord, Capability, DepartmentId, Limit, RequesterId, TeamId,
    TenantId, TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use super::{
    Analyst, ApiKey, Company, CompanyAccess, CostCentre, CostCentreId, Department, Password,
    PasswordHash, Requester, Role, RoleId, Session, Team,
};

/// A repository operation failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A unique constraint was violated; carries the constraint name.
    #[error("conflict on {0}")]
    Conflict(String),
    /// A referenced record does not exist; carries the constraint name.
    #[error("missing reference {0}")]
    MissingReference(String),
    /// The record to update or delete does not exist (or is not visible).
    #[error("not found")]
    NotFound,
    /// Any other storage failure. The message is for logs only.
    #[error("storage error: {0}")]
    Storage(String),
}

/// Parameters for listing records sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListQuery {
    /// Case-insensitive substring filter on name (and email where there is one).
    pub search: Option<String>,
    /// Include deactivated records.
    pub include_inactive: bool,
    /// Restrict to one Company (tenant-scoped records only).
    pub tenant_id: Option<TenantId>,
    /// Only these records, when not empty (Requesters only).
    pub ids: Vec<Uuid>,
    /// Page size. Repositories fetch `limit + 1` rows.
    pub limit: Limit,
    /// Continue after this `(lower(name), id)` sort key.
    pub after: Option<(String, Uuid)>,
}

/// Capabilities and Company access of an Analyst, including what their Teams give them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Authorization {
    /// Union of the Capabilities of every Role assigned directly or through a Team.
    pub capabilities: BTreeSet<Capability>,
    /// Union of Company access granted directly or through a Team.
    pub company_access: CompanyAccess,
}

/// Companies.
#[async_trait]
pub trait CompanyRepository: Send + Sync {
    /// Loads a Company visible in `scope`.
    async fn get(&self, scope: &TenantScope, id: TenantId) -> Result<Option<Company>, RepoError>;
    /// Lists Companies visible in `scope`.
    async fn list(&self, scope: &TenantScope, query: &ListQuery)
    -> Result<Vec<Company>, RepoError>;
    /// The Freemail Domains.
    async fn freemail_domains(&self) -> Result<BTreeSet<String>, RepoError>;
    /// The active Company with this Sender Domain, if any.
    async fn find_by_sender_domain(&self, domain: &str) -> Result<Option<TenantId>, RepoError>;
    /// Inserts a Company.
    async fn insert(&self, company: &Company, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Updates a Company.
    async fn update(&self, company: &Company, audit: &AuditRecord) -> Result<(), RepoError>;
}

/// Analysts.
#[async_trait]
pub trait AnalystRepository: Send + Sync {
    /// Loads an Analyst.
    async fn get(&self, id: AnalystId) -> Result<Option<Analyst>, RepoError>;
    /// Finds an Analyst by username or email (case-insensitive).
    async fn find_by_login(&self, login: &str) -> Result<Option<Analyst>, RepoError>;
    /// Lists Analysts.
    async fn list(&self, query: &ListQuery) -> Result<Vec<Analyst>, RepoError>;
    /// Number of active Administrators.
    async fn count_active_admins(&self) -> Result<i64, RepoError>;
    /// Inserts an Analyst.
    async fn insert(&self, analyst: &Analyst, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Updates an Analyst, revoking sessions and trusted devices in the same transaction
    /// as requested (deactivation, password change, two-factor reset).
    async fn update(
        &self,
        analyst: &Analyst,
        audit: &AuditRecord,
        revoke: Revocations,
    ) -> Result<(), RepoError>;
}

/// What to revoke together with an Analyst update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Revocations {
    /// End every open session.
    pub sessions: bool,
    /// Forget every trusted device.
    pub trusted_devices: bool,
}

impl Revocations {
    /// Revoke nothing.
    pub const NONE: Self = Self {
        sessions: false,
        trusted_devices: false,
    };
    /// Revoke sessions and trusted devices.
    pub const ALL: Self = Self {
        sessions: true,
        trusted_devices: true,
    };
}

/// Browser sessions.
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Stores a new session.
    async fn insert(&self, session: &Session) -> Result<(), RepoError>;
    /// Finds a session by the hash of its token.
    async fn find_by_token_hash(&self, hash: &[u8]) -> Result<Option<Session>, RepoError>;
    /// Stores activity (last seen, idle expiry).
    async fn touch(&self, session: &Session) -> Result<(), RepoError>;
    /// Stores revocation.
    async fn revoke(&self, session: &Session) -> Result<(), RepoError>;
}

/// Effective authorization of Analysts.
#[async_trait]
pub trait AuthorizationRepository: Send + Sync {
    /// Loads what an Analyst may do and see, including through Teams.
    async fn load(&self, analyst_id: AnalystId) -> Result<Authorization, RepoError>;
}

/// Teams.
#[async_trait]
pub trait TeamRepository: Send + Sync {
    /// Loads a Team.
    async fn get(&self, id: TeamId) -> Result<Option<Team>, RepoError>;
    /// Lists Teams.
    async fn list(&self, query: &ListQuery) -> Result<Vec<Team>, RepoError>;
    /// Inserts a Team.
    async fn insert(&self, team: &Team, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Updates a Team, its members, Roles and Company access.
    async fn update(&self, team: &Team, audit: &AuditRecord) -> Result<(), RepoError>;
}

/// Departments.
#[async_trait]
pub trait DepartmentRepository: Send + Sync {
    /// Loads a Department.
    async fn get(&self, id: DepartmentId) -> Result<Option<Department>, RepoError>;
    /// Lists Departments.
    async fn list(&self, query: &ListQuery) -> Result<Vec<Department>, RepoError>;
    /// Inserts a Department.
    async fn insert(&self, department: &Department, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Updates a Department.
    async fn update(&self, department: &Department, audit: &AuditRecord) -> Result<(), RepoError>;
}

/// RBAC Roles.
#[async_trait]
pub trait RoleRepository: Send + Sync {
    /// Loads a Role.
    async fn get(&self, id: RoleId) -> Result<Option<Role>, RepoError>;
    /// Lists Roles.
    async fn list(&self, query: &ListQuery) -> Result<Vec<Role>, RepoError>;
    /// Inserts a Role.
    async fn insert(&self, role: &Role, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Updates a Role.
    async fn update(&self, role: &Role, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Deletes a Role and its assignments.
    async fn delete(&self, role: &Role, audit: &AuditRecord) -> Result<(), RepoError>;
}

/// Requesters (tenant-scoped).
#[async_trait]
pub trait RequesterRepository: Send + Sync {
    /// Loads a Requester visible in `scope`.
    async fn get(
        &self,
        scope: &TenantScope,
        id: RequesterId,
    ) -> Result<Option<Requester>, RepoError>;
    /// Lists Requesters visible in `scope`.
    async fn list(
        &self,
        scope: &TenantScope,
        query: &ListQuery,
    ) -> Result<Vec<Requester>, RepoError>;
    /// Requesters with this email address (case-insensitive) visible in `scope`.
    async fn find_by_email(
        &self,
        scope: &TenantScope,
        email: &str,
    ) -> Result<Vec<Requester>, RepoError>;
    /// The Manager chain above `id`, nearest first, stopping after `max` steps.
    async fn manager_chain(
        &self,
        scope: &TenantScope,
        id: RequesterId,
        max: u32,
    ) -> Result<Vec<RequesterId>, RepoError>;
    /// Inserts a Requester.
    async fn insert(
        &self,
        scope: &TenantScope,
        requester: &Requester,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Updates a Requester.
    async fn update(
        &self,
        scope: &TenantScope,
        requester: &Requester,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
}

/// Cost Centres (tenant-scoped).
#[async_trait]
pub trait CostCentreRepository: Send + Sync {
    /// Loads a Cost Centre visible in `scope`.
    async fn get(
        &self,
        scope: &TenantScope,
        id: CostCentreId,
    ) -> Result<Option<CostCentre>, RepoError>;
    /// Lists Cost Centres visible in `scope`.
    async fn list(
        &self,
        scope: &TenantScope,
        query: &ListQuery,
    ) -> Result<Vec<CostCentre>, RepoError>;
    /// Inserts a Cost Centre.
    async fn insert(
        &self,
        scope: &TenantScope,
        cost_centre: &CostCentre,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Updates a Cost Centre.
    async fn update(
        &self,
        scope: &TenantScope,
        cost_centre: &CostCentre,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
}

/// First-run setup (FR-SYS-004).
#[async_trait]
pub trait SetupRepository: Send + Sync {
    /// Whether setup has not been completed (no Analyst exists).
    async fn is_required(&self) -> Result<bool, RepoError>;
    /// Stores the default Company and the first Administrator atomically. Fails with
    /// [`RepoError::Conflict`] (`setup`) if an Analyst already exists.
    async fn complete(
        &self,
        company: &Company,
        company_audit: &AuditRecord,
        admin: &Analyst,
        admin_audit: &AuditRecord,
    ) -> Result<(), RepoError>;
}

/// API Keys (FR-API-002).
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    /// Loads a key.
    async fn get(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepoError>;
    /// Finds a key by its public prefix.
    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, RepoError>;
    /// Lists keys by name.
    async fn list(&self, query: &ListQuery) -> Result<Vec<ApiKey>, RepoError>;
    /// Inserts a key.
    async fn insert(&self, key: &ApiKey, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Updates a key.
    async fn update(&self, key: &ApiKey, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Counts a request in the current minute and returns the count so far; also
    /// refreshes `last_used_at` at most once a minute.
    async fn record_use(&self, id: ApiKeyId, now: DateTime<Utc>) -> Result<u32, RepoError>;
}

/// Password hashing (Argon2id in production).
#[async_trait]
pub trait PasswordHasher: Send + Sync {
    /// Hashes a new password.
    async fn hash(&self, password: &Password) -> Result<PasswordHash, RepoError>;
    /// Verifies `candidate` against `hash`. With no hash, verifies against a dummy hash
    /// and returns `false`, so unknown users take as long as known ones.
    async fn verify(&self, candidate: &str, hash: Option<&PasswordHash>) -> bool;
}

/// Random secrets (session and CSRF tokens).
pub trait TokenGenerator: Send + Sync {
    /// A new random token: hex of 32 random bytes.
    fn token(&self) -> Result<String, RepoError>;
}

/// One-time password reset tokens (FR-IAM-004). Only hashes are stored.
#[async_trait]
pub trait PasswordResetRepository: Send + Sync {
    /// Stores a token for an Analyst, replacing their unused ones.
    async fn issue(
        &self,
        analyst: AnalystId,
        token_hash: &[u8],
        now: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    /// Marks a token used and returns its Analyst, if it exists, is unused and has not
    /// expired.
    async fn consume(
        &self,
        token_hash: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Option<AnalystId>, RepoError>;
}

/// Sends password reset emails (through the notifications context).
#[async_trait]
pub trait ResetMailer: Send + Sync {
    /// Emails a reset link with `token` to the Analyst.
    async fn send_reset(
        &self,
        to: &str,
        name: &str,
        token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    /// Emails a Portal set-password link (Portal Invitation, reset or Self-Registration).
    async fn send_portal_link(&self, email: &super::PortalLinkEmail) -> Result<(), RepoError>;
}

/// User Groups and Manager Grants (ADR-0025).
#[async_trait]
pub trait GroupRepository: Send + Sync {
    /// Every User Group.
    async fn groups(&self) -> Result<Vec<super::UserGroup>, RepoError>;
    /// Inserts or updates a group.
    async fn save_group(
        &self,
        group: &super::UserGroup,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// The members of a group (expired ones included).
    async fn members(&self, group: Uuid) -> Result<Vec<super::Membership>, RepoError>;
    /// Replaces the members of a group.
    async fn set_members(
        &self,
        group: Uuid,
        members: &[super::Membership],
        audit: &AuditRecord,
        now: DateTime<Utc>,
    ) -> Result<(), RepoError>;
    /// Current Requester members of every active group.
    async fn requester_members(
        &self,
        now: DateTime<Utc>,
    ) -> Result<
        std::collections::BTreeMap<Uuid, std::collections::BTreeSet<cooper_kernel::RequesterId>>,
        RepoError,
    >;
    /// The active User Groups `member` currently belongs to (unexpired membership).
    async fn groups_of(
        &self,
        member: super::Member,
        now: DateTime<Utc>,
    ) -> Result<std::collections::BTreeSet<Uuid>, RepoError>;
    /// The Manager Grants of a Company.
    async fn grants(&self, tenant: TenantId) -> Result<Vec<super::ManagerGrant>, RepoError>;
    /// Stores a grant.
    async fn add_grant(
        &self,
        grant: &super::ManagerGrant,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Removes a grant.
    async fn remove_grant(
        &self,
        grant: &super::ManagerGrant,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Every Requester of a Company, as the Manager rules need them.
    async fn company_requesters(
        &self,
        tenant: TenantId,
    ) -> Result<Vec<super::RequesterSummary>, RepoError>;
}

/// What to do with a provider's stored client secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretChange<'a> {
    /// Leave it as it is.
    Keep,
    /// Replace it.
    Set(&'a str),
    /// Remove it (public client with PKCE only).
    Clear,
}

/// OIDC providers, sign-ins in progress and linked identities (ADR-0025).
#[async_trait]
pub trait SsoRepository: Send + Sync {
    /// Every provider.
    async fn providers(&self) -> Result<Vec<super::SsoProvider>, RepoError>;
    /// Inserts or updates a provider; the secret is encrypted at rest.
    async fn save_provider(
        &self,
        provider: &super::SsoProvider,
        secret: SecretChange<'_>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// The decrypted client secret, if any.
    async fn client_secret(&self, provider: Uuid) -> Result<Option<String>, RepoError>;
    /// Stores a sign-in in progress.
    async fn insert_login(&self, login: &super::SsoLogin) -> Result<(), RepoError>;
    /// Removes and returns a sign-in in progress (single use).
    async fn take_login(&self, state_hash: &[u8]) -> Result<Option<super::SsoLogin>, RepoError>;
    /// Who a subject is, if linked.
    async fn identity(
        &self,
        provider: Uuid,
        audience: super::SsoAudience,
        subject: &str,
    ) -> Result<Option<super::SsoIdentity>, RepoError>;
    /// Links a subject (or records another sign-in of a linked one).
    async fn link(
        &self,
        provider: Uuid,
        audience: super::SsoAudience,
        subject: &str,
        identity: &super::SsoIdentity,
        now: DateTime<Utc>,
    ) -> Result<(), RepoError>;
}

/// Talks to OpenID Connect providers: discovery, the authorization URL, the code
/// exchange and ID token signature verification (ADR-0025).
#[async_trait]
pub trait OidcClient: Send + Sync {
    /// The URL to send the browser to.
    async fn authorization_url(
        &self,
        provider: &super::SsoProvider,
        redirect_uri: &str,
        state: &str,
        nonce: &str,
        code_challenge: &str,
    ) -> Result<String, String>;
    /// Exchanges the code for tokens and returns the claims of the ID token, whose
    /// signature has been verified against the provider's keys.
    async fn exchange(
        &self,
        provider: &super::SsoProvider,
        client_secret: Option<&str>,
        redirect_uri: &str,
        code: &str,
        pkce_verifier: &str,
    ) -> Result<super::IdClaims, String>;
}

/// Settings the identity context reads from System Settings.
#[async_trait]
pub trait IdentitySettings: Send + Sync {
    /// The Manager rules.
    async fn managers(&self) -> super::ManagerSettings;
    /// Public address of Cooper (`general.base_url`), for the OIDC redirect URI.
    async fn base_url(&self) -> String;
}
