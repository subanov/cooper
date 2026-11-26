//! What the Asset extras (custom fields, software, licences, inventory, import) need from
//! storage (ADR-0041).

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AuditRecord, EventEnvelope, TenantId, TenantScope};
use thiserror::Error;
use uuid::Uuid;

use crate::custom_fields::{FieldDefinition, FieldSet};
use crate::inventory::{LicenceStatus, SoftwareItem};

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExtrasRepoError {
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

/// A licence as stored (its key is sealed and never read back).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenceRecord {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// Licence type.
    pub licence_type: String,
    /// The last four characters of the key, for display.
    pub key_hint: Option<String>,
    /// Quantity of licences.
    pub quantity: u32,
    /// Renewal date.
    pub renewal_date: Option<NaiveDate>,
    /// Notice period, in days.
    pub notice_period_days: Option<u32>,
    /// Cost in hundredths and its currency.
    pub cost: Option<(i64, String)>,
    /// Status.
    pub status: LicenceStatus,
    /// Who created it.
    pub created_by: Actor,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// An installed application on an Asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    /// The application.
    pub application_id: Uuid,
    /// Its name.
    pub name: String,
    /// Its publisher.
    pub publisher: String,
    /// The installed version.
    pub version: String,
}

/// An expiry that is due for a reminder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueReminder {
    /// The Asset.
    pub asset_id: Uuid,
    /// Its Company.
    pub tenant_id: TenantId,
    /// Its owner (an Analyst).
    pub owner_id: Uuid,
    /// `warranty` or `lease`.
    pub kind: String,
    /// The expiry day.
    pub date: NaiveDate,
    /// The Asset's tag, hostname or ID.
    pub label: String,
}

/// An Asset found by an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundAsset {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
}

/// Storage of the Asset extras.
#[async_trait]
pub trait ExtrasRepository: Send + Sync {
    /// All Field Sets, with their Asset Types and fields (instance-wide).
    async fn field_sets(&self) -> Result<Vec<FieldSet>, ExtrasRepoError>;
    /// One Field Set.
    async fn field_set(&self, id: Uuid) -> Result<Option<FieldSet>, ExtrasRepoError>;
    /// Inserts or updates a Field Set and its Asset Types (audited).
    async fn save_field_set(&self, set: &FieldSet, audit: &AuditRecord) -> Result<(), ExtrasRepoError>;
    /// Inserts or updates a custom field of a Field Set (audited).
    async fn save_field(&self, set_id: Uuid, field: &FieldDefinition, audit: &AuditRecord) -> Result<(), ExtrasRepoError>;

    /// The custom values of an Asset, by field ID.
    async fn custom_values(&self, scope: &TenantScope, asset: Uuid) -> Result<BTreeMap<Uuid, String>, ExtrasRepoError>;
    /// Whether another Asset of the Company already has this value in a field.
    async fn value_taken(&self, tenant: TenantId, field: Uuid, value: &str, except_asset: Uuid) -> Result<bool, ExtrasRepoError>;
    /// Replaces the custom values of an Asset (audited).
    async fn save_custom_values(
        &self,
        tenant: TenantId,
        asset: Uuid,
        values: &BTreeMap<Uuid, String>,
        audit: &AuditRecord,
    ) -> Result<(), ExtrasRepoError>;

    /// The Asset with this service tag, hostname or asset tag (first match in that order),
    /// in the caller's scope.
    async fn find_asset(
        &self,
        scope: &TenantScope,
        service_tag: Option<&str>,
        hostname: Option<&str>,
        asset_tag: Option<&str>,
    ) -> Result<Vec<FoundAsset>, ExtrasRepoError>;
    /// Replaces the hardware snapshot and installed software of an Asset (audited).
    async fn save_inventory(
        &self,
        tenant: TenantId,
        asset: Uuid,
        snapshot: &serde_json::Value,
        software: &[(SoftwareItem, Uuid)],
        at: DateTime<Utc>,
        audit: &AuditRecord,
    ) -> Result<(), ExtrasRepoError>;
    /// Find or create an application by name and publisher; returns its ID.
    async fn application_id(&self, tenant: TenantId, item: &SoftwareItem) -> Result<Uuid, ExtrasRepoError>;
    /// The software installed on an Asset.
    async fn installations(&self, scope: &TenantScope, asset: Uuid) -> Result<Vec<Installation>, ExtrasRepoError>;

    /// Licences in the caller's scope, by type.
    async fn licences(&self, scope: &TenantScope) -> Result<Vec<LicenceRecord>, ExtrasRepoError>;
    /// One licence.
    async fn licence(&self, scope: &TenantScope, id: Uuid) -> Result<Option<LicenceRecord>, ExtrasRepoError>;
    /// Inserts or updates a licence (audited). `sealed_key` is the encrypted key, when given.
    async fn save_licence(
        &self,
        l: &LicenceRecord,
        sealed_key: Option<&str>,
        audit: &AuditRecord,
    ) -> Result<(), ExtrasRepoError>;
    /// Links an application to a licence.
    async fn link_application(&self, tenant: TenantId, licence: Uuid, application: Uuid) -> Result<(), ExtrasRepoError>;
    /// Unlinks an application from a licence.
    async fn unlink_application(&self, licence: Uuid, application: Uuid) -> Result<(), ExtrasRepoError>;
    /// The applications linked to a licence, and the Assets that have any of them.
    async fn compliance_counts(&self, scope: &TenantScope, licence: Uuid) -> Result<(u32, u32), ExtrasRepoError>;
    /// Sets or clears the owner of an Asset (audited).
    async fn save_owner(&self, tenant: TenantId, asset: Uuid, owner: Option<Uuid>, audit: &AuditRecord) -> Result<(), ExtrasRepoError>;
    /// Warranty and lease expiries in `from..=to` of active Assets that have an owner, across
    /// Companies (for the reminder job).
    async fn due_reminders(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<DueReminder>, ExtrasRepoError>;
    /// Records a reminder and appends its event, unless this one was sent already. Returns
    /// whether it was new.
    async fn record_reminder(&self, due: &DueReminder, event: &EventEnvelope) -> Result<bool, ExtrasRepoError>;
    /// Assets of a Company with an Asset Type, that are active (not retired), with their IDs
    /// and identifiers, for the import's missing-asset rule.
    async fn active_assets_of_type(
        &self,
        tenant: TenantId,
        asset_type: Uuid,
    ) -> Result<Vec<(Uuid, Option<String>, Option<String>, Option<String>)>, ExtrasRepoError>;
}
