//! What Contracts needs from storage and from the rest of Cooper.

use async_trait::async_trait;
use chrono::NaiveDate;
use cooper_kernel::{
    AnalystId, AuditRecord, EventEnvelope, Limit, RequesterId, TenantId, TenantScope,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Contact, ContactId, Contract, ContractId, Lookup, LookupKind, ReminderKind, Supplier,
    SupplierId,
};

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A uniqueness constraint was violated.
    #[error("conflict: {0}")]
    Conflict(String),
    /// A referenced row does not exist.
    #[error("missing reference: {0}")]
    MissingReference(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("storage: {0}")]
    Storage(String),
}

/// A Supplier list request, ordered by legal name.
#[derive(Debug, Clone, Default)]
pub struct SupplierQuery {
    /// Only this Company.
    pub tenant_id: Option<TenantId>,
    /// Text in the legal or trading name.
    pub search: Option<String>,
    /// Supplier Type.
    pub supplier_type_id: Option<Uuid>,
    /// Supplier Status.
    pub supplier_status_id: Option<Uuid>,
    /// Active (`true`) or inactive (`false`).
    pub active: Option<bool>,
    /// Page size.
    pub limit: Limit,
    /// After `(lower(legal_name), id)`.
    pub after: Option<(String, Uuid)>,
}

/// A Contract list request, ordered by Contract number.
#[derive(Debug, Clone, Default)]
pub struct ContractQuery {
    /// Only this Company.
    pub tenant_id: Option<TenantId>,
    /// Text in the number or title.
    pub search: Option<String>,
    /// Contract Status.
    pub status_id: Option<Uuid>,
    /// With this Supplier.
    pub supplier_id: Option<SupplierId>,
    /// Owned by.
    pub owner_id: Option<AnalystId>,
    /// Active (`true`: not in a closed Contract Status) or closed (`false`).
    pub active: Option<bool>,
    /// Notice Date within these dates (inclusive).
    pub notice_between: Option<(NaiveDate, NaiveDate)>,
    /// Page size.
    pub limit: Limit,
    /// After `(lower(contract_number), id)`.
    pub after: Option<(String, Uuid)>,
}

/// Contracts storage. Lookups are instance-wide; everything else is tenant-scoped.
#[async_trait]
pub trait ContractsRepository: Send + Sync {
    /// Lookups of a kind.
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError>;
    /// Inserts or updates a lookup, with events (atomically).
    async fn save_lookup(
        &self,
        lookup: &Lookup,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Contracts (not deleted) in a Contract Status, in every Company.
    async fn contracts_with_status(&self, status: Uuid) -> Result<Vec<Contract>, RepoError>;

    /// One Supplier.
    async fn supplier(
        &self,
        scope: &TenantScope,
        id: SupplierId,
    ) -> Result<Option<Supplier>, RepoError>;
    /// Suppliers (fetches `limit + 1`).
    async fn suppliers(
        &self,
        scope: &TenantScope,
        q: &SupplierQuery,
    ) -> Result<Vec<Supplier>, RepoError>;
    /// Inserts or updates a Supplier.
    async fn save_supplier(&self, s: &Supplier, audit: &AuditRecord) -> Result<(), RepoError>;

    /// One Contact.
    async fn contact(
        &self,
        scope: &TenantScope,
        id: ContactId,
    ) -> Result<Option<Contact>, RepoError>;
    /// The Contacts of a Supplier, by name.
    async fn contacts(
        &self,
        scope: &TenantScope,
        supplier: SupplierId,
    ) -> Result<Vec<Contact>, RepoError>;
    /// Inserts or updates a Contact.
    async fn save_contact(&self, c: &Contact, audit: &AuditRecord) -> Result<(), RepoError>;

    /// One Contract that is not deleted.
    async fn contract(
        &self,
        scope: &TenantScope,
        id: ContractId,
    ) -> Result<Option<Contract>, RepoError>;
    /// Contracts that are not deleted (fetches `limit + 1`).
    async fn contracts(
        &self,
        scope: &TenantScope,
        q: &ContractQuery,
    ) -> Result<Vec<Contract>, RepoError>;
    /// Inserts a Contract with its events.
    async fn insert_contract(
        &self,
        c: &Contract,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Updates (or soft-deletes) a Contract with its events.
    async fn update_contract(
        &self,
        c: &Contract,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;

    /// Active Contracts with an owner whose Notice Date or end date falls in
    /// `from..=to` and has not been reminded yet, in every Company.
    async fn reminder_candidates(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<Contract>, RepoError>;
    /// Records that a reminder was sent and appends its event, atomically. Returns
    /// `false` (and appends nothing) when it was already recorded.
    async fn record_reminder(
        &self,
        c: &Contract,
        kind: ReminderKind,
        date: NaiveDate,
        event: &EventEnvelope,
    ) -> Result<bool, RepoError>;

    /// Audit history of a Supplier, Contact or Contract, oldest first.
    async fn history(
        &self,
        scope: &TenantScope,
        aggregate_type: &str,
        id: Uuid,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError>;
}

/// The rest of Cooper, through application APIs.
#[async_trait]
pub trait Directory: Send + Sync {
    /// Whether an Analyst exists and is active.
    async fn analyst_active(&self, analyst: AnalystId) -> Result<bool, RepoError>;
    /// The Company of a Requester.
    async fn requester_company(
        &self,
        requester: RequesterId,
    ) -> Result<Option<TenantId>, RepoError>;
    /// The Company of a Cost Centre.
    async fn cost_centre_company(&self, cost_centre: Uuid) -> Result<Option<TenantId>, RepoError>;
}

/// System Settings Contracts reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractSettings {
    /// Days ahead of a Notice Date or end date to remind the owner (0: off).
    pub reminder_days: u32,
    /// IANA time zone in which dates are calendar days (`general.time_zone`).
    pub time_zone: String,
}

impl Default for ContractSettings {
    fn default() -> Self {
        Self {
            reminder_days: 30,
            time_zone: "UTC".into(),
        }
    }
}

/// Reads [`ContractSettings`].
#[async_trait]
pub trait ContractSettingsProvider: Send + Sync {
    /// Current settings.
    async fn settings(&self) -> ContractSettings;
}
