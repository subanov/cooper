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
}
