//! What Assets needs from storage and from the rest of Cooper.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, EventEnvelope, Limit, Principal, RequesterId, TenantId,
    TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{Asset, AssetCheckout, AssetError, AssetHolder, AssetId, Lookup, LookupKind};

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A uniqueness constraint was violated.
    #[error("conflict: {0}")]
    Conflict(String),
    /// A referenced row does not exist.
    #[error("missing reference: {0}")]
    MissingReference(String),
    /// The row changed underneath the caller (for example, a second Check Out).
    #[error("concurrent change: {0}")]
    Concurrent(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("storage: {0}")]
    Storage(String),
}

/// What an Asset links to (FR-AST-006). CIs follow with the CMDB (FR-CMD-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkKind {
    /// A ticket.
    Ticket,
    /// A Contract.
    Contract,
}

impl LinkKind {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Contract => "contract",
        }
    }

    /// Display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Contract => "Contract",
        }
    }
}

impl std::str::FromStr for LinkKind {
    type Err = AssetError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ticket" => Ok(Self::Ticket),
            "contract" => Ok(Self::Contract),
            _ => Err(AssetError::invalid("kind", "must be ticket or contract")),
        }
    }
}

/// A link of an Asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetLink {
    /// Kind.
    pub kind: LinkKind,
    /// Target.
    pub target_id: Uuid,
    /// Who.
    pub linked_by: Actor,
    /// When.
    pub linked_at: DateTime<Utc>,
}

/// An Asset list request.
#[derive(Debug, Clone, Default)]
pub struct AssetQuery {
    /// One Company.
    pub tenant: Option<TenantId>,
    /// Asset Type.
    pub asset_type_id: Option<Uuid>,
    /// Asset Status.
    pub asset_status_id: Option<Uuid>,
    /// Any of these Locations (a Location and the ones below it).
    pub locations: Option<Vec<Uuid>>,
    /// Current Holder.
    pub holder: Option<AssetHolder>,
    /// Checked out (`true`) or available (`false`).
    pub checked_out: Option<bool>,
    /// Include retired Assets (excluded by default, FR-AST-005).
    pub include_retired: bool,
    /// Retired Assets only.
    pub retired_only: bool,
    /// Hostname, Asset Tag or Service Tag contains.
    pub search: Option<String>,
    /// Page size.
    pub limit: Limit,
    /// After this Asset (newest first).
    pub after: Option<Uuid>,
}

/// Counts of the estate (FR-AST-005): retired Assets are not counted as active.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetSummary {
    /// Assets in the estate.
    pub active: u64,
    /// Retired Assets.
    pub retired: u64,
    /// Active Assets checked out.
    pub checked_out: u64,
    /// Active Assets per Asset Type (`None`: no type).
    pub by_type: Vec<(Option<Uuid>, u64)>,
    /// Active Assets per Asset Status (`None`: no status).
    pub by_status: Vec<(Option<Uuid>, u64)>,
}

/// Asset storage.
#[async_trait]
pub trait AssetRepository: Send + Sync {
    /// Reference data of a kind.
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError>;
    /// Inserts or updates reference data.
    async fn save_lookup(&self, lookup: &Lookup, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Inserts an Asset.
    async fn insert(
        &self,
        asset: &Asset,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Updates an Asset's details.
    async fn update(
        &self,
        asset: &Asset,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Records a Check Out, unless the Asset was retired or checked out meanwhile
    /// ([`RepoError::Concurrent`]).
    async fn check_out(
        &self,
        asset: &Asset,
        checkout: &AssetCheckout,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Closes a Check Out, unless it was closed meanwhile ([`RepoError::Concurrent`]).
    async fn check_in(
        &self,
        asset: &Asset,
        checkout: &AssetCheckout,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Records the retirement, unless the Asset was checked out or retired meanwhile
    /// ([`RepoError::Concurrent`]).
    async fn retire(
        &self,
        asset: &Asset,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// One Asset.
    async fn get(&self, scope: &TenantScope, id: AssetId) -> Result<Option<Asset>, RepoError>;
    /// Assets, newest first.
    async fn list(&self, scope: &TenantScope, q: &AssetQuery) -> Result<Vec<Asset>, RepoError>;
    /// Counts in `scope` (optionally one Company).
    async fn summary(
        &self,
        scope: &TenantScope,
        tenant: Option<TenantId>,
    ) -> Result<AssetSummary, RepoError>;
    /// Every Check Out of an Asset, newest first.
    async fn checkouts(
        &self,
        scope: &TenantScope,
        id: AssetId,
    ) -> Result<Vec<AssetCheckout>, RepoError>;
    /// Links.
    async fn links(&self, scope: &TenantScope, id: AssetId) -> Result<Vec<AssetLink>, RepoError>;
    /// Adds a link.
    async fn add_link(
        &self,
        asset: &Asset,
        link: &AssetLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Removes a link.
    async fn remove_link(
        &self,
        asset: &Asset,
        kind: LinkKind,
        target: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Audit history of an Asset, oldest first.
    async fn history(
        &self,
        scope: &TenantScope,
        id: AssetId,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError>;
}

/// The rest of Cooper, through application APIs.
#[async_trait]
pub trait Directory: Send + Sync {
    /// Whether an active Requester of `tenant` exists.
    async fn requester_in(&self, tenant: TenantId, id: RequesterId) -> Result<bool, RepoError>;
    /// Whether an Analyst exists and is active.
    async fn analyst_active(&self, id: AnalystId) -> Result<bool, RepoError>;
    /// Whether a Cost Centre of `tenant` exists.
    async fn cost_centre_in(&self, tenant: TenantId, id: Uuid) -> Result<bool, RepoError>;
    /// Whether a Supplier exists for `tenant` (Contracts context).
    async fn supplier_exists(&self, tenant: TenantId, id: Uuid) -> Result<bool, RepoError>;
    /// The display label (ticket or Contract number) of a link target of `tenant` the
    /// caller may see, or `None`.
    async fn link_target(
        &self,
        principal: &Principal,
        tenant: TenantId,
        kind: LinkKind,
        id: Uuid,
    ) -> Result<Option<String>, RepoError>;
}
