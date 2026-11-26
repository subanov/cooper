//! Asset extras use cases (FR-AST-003, FR-AST-007, FR-AST-008, FR-AST-013, FR-AST-014;
//! ADR-0041). Writes to Assets go through [`AssetService`], so its Capabilities and audit
//! apply unchanged.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, Clock, Forbidden, Principal, TenantId, TenantScope,
};
use cooper_platform::crypto::Keyring;
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::application::{AppError, AssetService, RetireRequest};
use crate::custom_fields::{self, CustomFieldError, FieldDefinition, FieldInput, FieldSet};
use crate::domain::{AssetDetails, Money};
use crate::extras_ports::{ExtrasRepoError, ExtrasRepository, LicenceRecord};
use crate::import::{self, ImportError, ImportField, MissingPolicy, RowOutcome, WriteMode};
use crate::inventory::{self, Compliance, InventoryReport, LicenceStatus};

/// Capabilities of the extras.
pub mod capabilities {
    /// Push an inventory report (API keys and automation).
    pub const INVENTORY: &str = "assets.inventory";
    /// Import Assets from CSV.
    pub const IMPORT: &str = "assets.import";

    /// Every Capability the extras declare.
    pub const ALL: &[&str] = &[INVENTORY, IMPORT];
}

/// An extras use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExtrasError {
    /// Input is not valid.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: String,
        /// Why.
        reason: String,
    },
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// An Asset use case failed.
    #[error(transparent)]
    Asset(#[from] AppError),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> ExtrasError {
    ExtrasError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}

impl From<CustomFieldError> for ExtrasError {
    fn from(e: CustomFieldError) -> Self {
        match e {
            CustomFieldError::Invalid { field, reason } => invalid(field, reason),
        }
    }
}

impl From<inventory::InventoryError> for ExtrasError {
    fn from(e: inventory::InventoryError) -> Self {
        match e {
            inventory::InventoryError::Invalid { field, reason } => invalid(field, reason),
        }
    }
}

impl From<ImportError> for ExtrasError {
    fn from(e: ImportError) -> Self {
        invalid("import", e.to_string())
    }
}

impl From<ExtrasRepoError> for ExtrasError {
    fn from(e: ExtrasRepoError) -> Self {
        match e {
            ExtrasRepoError::Conflict(c) if c.ends_with("_name_key") => {
                Self::Conflict("a row with this name already exists".into())
            }
            ExtrasRepoError::Conflict(c) if c == "asset_field_definitions_key_key" => {
                Self::Conflict("a custom field with this key already exists".into())
            }
            ExtrasRepoError::Conflict(_) => Self::Conflict("the record conflicts with an existing one".into()),
            ExtrasRepoError::MissingReference(c) => invalid("reference", format!("refers to a record that does not exist ({c})")),
            ExtrasRepoError::NotFound => Self::NotFound("record"),
            ExtrasRepoError::Storage(message) => {
                tracing::error!(error = %message, "asset extras storage error");
                Self::Internal
            }
        }
    }
}

/// Warranty and lease expiries are reminded this many days ahead (FR-AST-012).
pub const REMINDER_DAYS: u64 = 30;

/// The event published when an Asset's warranty or lease is due (FR-AST-012).
#[derive(Debug, Clone, serde::Serialize)]
pub struct AssetReminderDue {
    /// The Asset.
    pub asset_id: Uuid,
    /// Company (envelope only).
    #[serde(skip)]
    pub tenant_id: TenantId,
    /// The owner to remind.
    pub owner_id: Uuid,
    /// `warranty` or `lease`.
    pub kind: String,
    /// The expiry day.
    pub date: chrono::NaiveDate,
    /// The Asset's label.
    pub label: String,
}

impl cooper_kernel::DomainEvent for AssetReminderDue {
    const EVENT_TYPE: &'static str = "asset.reminder_due";
    const AGGREGATE_TYPE: &'static str = "asset";

    fn aggregate_id(&self) -> Uuid {
        self.asset_id
    }

    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct ExtrasPorts {
    /// Assets (checks visibility, Capabilities and audits Asset changes).
    pub assets: AssetService,
    /// Storage.
    pub store: Arc<dyn ExtrasRepository>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Seals licence keys at rest.
    pub keyring: Keyring,
    /// Analysts (owners of Assets).
    pub directory: Arc<dyn crate::ports::Directory>,
}

/// Asset extras use cases.
#[derive(Clone)]
pub struct ExtrasService {
    ports: ExtrasPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), ExtrasError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

fn audit(
    tenant: impl Into<Option<TenantId>>,
    aggregate: (&'static str, Uuid),
    action: AuditAction,
    before: &[(&'static str, Value)],
    after: Vec<(&'static str, Value)>,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    after.into_iter().fold(
        AuditRecord::new(tenant.into(), aggregate.0, aggregate.1, action, actor, now),
        |r, (field, value)| {
            r.change(field, before.iter().find(|(n, _)| *n == field).map(|(_, v)| v), Some(&value))
        },
    )
}

/// The input of a Field Set (name and Asset Types).
#[derive(Debug, Clone)]
pub struct FieldSetInput {
    /// Name.
    pub name: String,
    /// Asset Types it applies to.
    pub asset_type_ids: BTreeSet<Uuid>,
    /// Active.
    pub active: bool,
}

/// What an Administrator may change on a custom field.
#[derive(Debug, Clone)]
pub struct FieldUpdate {
    /// Label.
    pub label: String,
    /// Required.
    pub required: bool,
    /// Unique per Company.
    pub unique: bool,
    /// Searchable.
    pub searchable: bool,
    /// Shown in lists.
    pub show_in_list: bool,
    /// Active.
    pub active: bool,
}

/// A licence as entered.
#[derive(Debug, Clone)]
pub struct LicenceInput {
    /// Type.
    pub licence_type: String,
    /// The key; `None` keeps the stored key on update.
    pub key: Option<String>,
    /// Quantity.
    pub quantity: u32,
    /// Renewal date.
    pub renewal_date: Option<NaiveDate>,
    /// Notice period, in days.
    pub notice_period_days: Option<u32>,
    /// Cost as decimal text, with its currency.
    pub cost: Option<(String, String)>,
    /// Status.
    pub status: LicenceStatus,
}

/// What a licence shows about its use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenceView {
    /// The licence.
    pub licence: LicenceRecord,
    /// Applications linked to it.
    pub linked_applications: u32,
    /// Assets with any of them installed.
    pub installed_on: u32,
    /// Compliance.
    pub compliance: Compliance,
}

/// One row of an import's outcome log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowLog {
    /// Line in the file (the header is line 1).
    pub line: usize,
    /// What happened.
    pub outcome: String,
    /// The Asset concerned, if any.
    pub asset_id: Option<Uuid>,
    /// Why, when it failed or was flagged.
    pub message: Option<String>,
}

/// The result of an import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Whether anything was written.
    pub dry_run: bool,
    /// One entry per row, then one per missing Asset.
    pub log: Vec<RowLog>,
}

/// The parameters of an import.
#[derive(Debug, Clone)]
pub struct ImportRequest {
    /// Company the Assets belong to.
    pub tenant: TenantId,
    /// The CSV text, with a header row.
    pub csv: String,
    /// Column header to field.
    pub mapping: BTreeMap<String, String>,
    /// Asset Type for new Assets, and the scope of the missing-asset rule.
    pub asset_type_id: Option<Uuid>,
    /// How existing Assets change.
    pub write_mode: String,
    /// What happens to Assets the file does not mention.
    pub missing: String,
    /// Only preview.
    pub dry_run: bool,
}

impl ExtrasService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: ExtrasPorts) -> Self {
        Self { ports }
    }

    // --- Custom fields (FR-AST-003) ------------------------------------------------------

    /// Field Sets with their fields (needs `assets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn field_sets(&self, principal: &Principal) -> Result<Vec<FieldSet>, ExtrasError> {
        self.check_read(principal)?;
        Ok(self.ports.store.field_sets().await?)
    }

    fn check_read(&self, principal: &Principal) -> Result<(), ExtrasError> {
        if matches!(principal.actor(), Actor::Requester(_)) {
            return Err(Forbidden("staff".into()).into());
        }
        Ok(principal.require(crate::application::capabilities::READ)?)
    }

    /// Creates a Field Set (needs `assets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_field_set(&self, principal: &Principal, input: FieldSetInput) -> Result<FieldSet, ExtrasError> {
        require(principal, crate::application::capabilities::CONFIGURE)?;
        let mut set = FieldSet::new(Uuid::now_v7(), &input.name, input.asset_type_ids)?;
        set.active = input.active;
        let now = self.ports.clock.now();
        let record = audit(
            None::<TenantId>,
            ("asset_field_set", set.id),
            AuditAction::Created,
            &[],
            set_audit(&set),
            principal.actor(),
            now,
        );
        self.ports.store.save_field_set(&set, &record).await?;
        Ok(set)
    }

    /// Changes a Field Set's name, Asset Types and active flag (needs `assets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_field_set(&self, principal: &Principal, id: Uuid, input: FieldSetInput) -> Result<FieldSet, ExtrasError> {
        require(principal, crate::application::capabilities::CONFIGURE)?;
        let before = self.ports.store.field_set(id).await?.ok_or(ExtrasError::NotFound("Field Set"))?;
        let mut set = before.clone();
        let name = FieldSet::new(set.id, &input.name, input.asset_type_ids.clone())?.name;
        set.name = name;
        set.asset_type_ids = input.asset_type_ids;
        set.active = input.active;
        let now = self.ports.clock.now();
        let record = audit(
            None::<TenantId>,
            ("asset_field_set", set.id),
            AuditAction::Updated,
            &set_audit(&before),
            set_audit(&set),
            principal.actor(),
            now,
        );
        self.ports.store.save_field_set(&set, &record).await?;
        Ok(set)
    }

    /// Adds a custom field to a Field Set (needs `assets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a key that is already used.
    pub async fn add_field(&self, principal: &Principal, set_id: Uuid, input: FieldInput) -> Result<FieldDefinition, ExtrasError> {
        require(principal, crate::application::capabilities::CONFIGURE)?;
        let set = self.ports.store.field_set(set_id).await?.ok_or(ExtrasError::NotFound("Field Set"))?;
        let all = self.ports.store.field_sets().await?;
        if all.iter().any(|s| s.fields.iter().any(|f| f.key == input.key.trim())) {
            return Err(ExtrasError::Conflict("a custom field with this key already exists".into()));
        }
        set.check_new_key(input.key.trim())?;
        let field = FieldDefinition::new(Uuid::now_v7(), input)?;
        let now = self.ports.clock.now();
        let record = audit(
            None::<TenantId>,
            ("asset_field", field.id),
            AuditAction::Created,
            &[],
            field_audit(&field),
            principal.actor(),
            now,
        );
        self.ports.store.save_field(set_id, &field, &record).await?;
        Ok(field)
    }

    /// Changes a custom field's label, flags and active state (needs `assets.configure`).
    /// The key, type and options do not change once the field exists.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid input.
    pub async fn update_field(&self, principal: &Principal, set_id: Uuid, field_id: Uuid, update: FieldUpdate) -> Result<FieldDefinition, ExtrasError> {
        require(principal, crate::application::capabilities::CONFIGURE)?;
        let set = self.ports.store.field_set(set_id).await?.ok_or(ExtrasError::NotFound("Field Set"))?;
        let before = set.fields.iter().find(|f| f.id == field_id).cloned().ok_or(ExtrasError::NotFound("custom field"))?;
        let label = update.label.trim().to_owned();
        if label.is_empty() || label.chars().count() > 255 {
            return Err(invalid("label", "must be 1 to 255 characters"));
        }
        let field = FieldDefinition {
            label,
            required: update.required,
            unique: update.unique,
            searchable: update.searchable,
            show_in_list: update.show_in_list,
            active: update.active,
            ..before.clone()
        };
        let now = self.ports.clock.now();
        let record = audit(
            None::<TenantId>,
            ("asset_field", field.id),
            AuditAction::Updated,
            &field_audit(&before),
            field_audit(&field),
            principal.actor(),
            now,
        );
        self.ports.store.save_field(set_id, &field, &record).await?;
        Ok(field)
    }

    /// The custom values of an Asset, by field key (needs `assets.read` and visibility).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn custom_values(&self, principal: &Principal, asset: Uuid) -> Result<BTreeMap<String, String>, ExtrasError> {
        self.check_read(principal)?;
        let found = self.asset(principal, asset).await?;
        let values = self.ports.store.custom_values(principal.scope(), asset).await?;
        let keys: BTreeMap<Uuid, String> = self
            .applicable(found.details().asset_type_id)
            .await?
            .into_iter()
            .map(|f| (f.id, f.key))
            .collect();
        Ok(values
            .into_iter()
            .filter_map(|(id, v)| keys.get(&id).map(|k| (k.clone(), v)))
            .collect())
    }

    /// Replaces an Asset's custom values (needs `assets.update`). Required fields must be
    /// given; unique fields must not repeat another Asset's value.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid or duplicate values.
    pub async fn set_custom_values(
        &self,
        principal: &Principal,
        asset_id: Uuid,
        values: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, ExtrasError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        let asset = self.asset(principal, asset_id).await?;
        let tenant = asset.tenant_id();
        let applicable = self.applicable(asset.details().asset_type_id).await?;
        let stored = custom_fields::check_values(&applicable, values)?;
        for field in applicable.iter().filter(|f| f.unique) {
            if let Some(value) = stored.get(&field.id)
                && self.ports.store.value_taken(tenant, field.id, value, asset_id).await?
            {
                return Err(ExtrasError::Conflict(format!("{} is already used by another Asset", field.key)));
            }
        }
        let before = self.custom_values(principal, asset_id).await?;
        let now = self.ports.clock.now();
        let record = audit(
            tenant,
            ("asset", asset_id),
            AuditAction::Updated,
            &[("custom_fields", json!(before))],
            vec![("custom_fields", json!(values.iter().filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| (k, v.trim())).collect::<BTreeMap<_, _>>()))],
            principal.actor(),
            now,
        );
        self.ports.store.save_custom_values(tenant, asset_id, &stored, &record).await?;
        self.custom_values(principal, asset_id).await
    }

    /// The fields that apply to an Asset Type: those of active Field Sets assigned to it.
    async fn applicable(&self, asset_type: Option<Uuid>) -> Result<Vec<FieldDefinition>, ExtrasError> {
        let Some(t) = asset_type else { return Ok(Vec::new()) };
        Ok(self
            .ports
            .store
            .field_sets()
            .await?
            .into_iter()
            .filter(|s| s.active && s.asset_type_ids.contains(&t))
            .flat_map(|s| s.fields)
            .collect())
    }

    async fn asset(&self, principal: &Principal, id: Uuid) -> Result<crate::domain::Asset, ExtrasError> {
        Ok(self.ports.assets.get(principal, crate::domain::AssetId::from_uuid(id)).await?)
    }

    /// Sets or clears the owner of an Asset: the Analyst reminded of its expiries (needs
    /// `assets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or an owner that is not an active Analyst.
    pub async fn set_owner(&self, principal: &Principal, asset: Uuid, owner: Option<AnalystId>) -> Result<(), ExtrasError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        let found = self.asset(principal, asset).await?;
        if let Some(o) = owner {
            let active = self.ports.directory.analyst_active(o).await.map_err(|_| ExtrasError::Internal)?;
            if !active {
                return Err(invalid("owner_id", "is not an active Analyst"));
            }
        }
        let now = self.ports.clock.now();
        let rec = audit(
            found.tenant_id(),
            ("asset", asset),
            AuditAction::Updated,
            &[],
            vec![("owner_id", json!(owner.map(|o| o.as_uuid())))],
            principal.actor(),
            now,
        );
        self.ports.store.save_owner(found.tenant_id(), asset, owner.map(|o| o.as_uuid()), &rec).await?;
        Ok(())
    }

    /// Sends the reminders due now: owners of Assets whose warranty or lease ends within
    /// `REMINDER_DAYS`. Each is sent once. Returns how many were sent (system job).
    ///
    /// # Errors
    ///
    /// Storage failure.
    pub async fn send_reminders(&self) -> Result<usize, ExtrasError> {
        let now = self.ports.clock.now();
        let today = now.date_naive();
        let until = today.checked_add_days(chrono::Days::new(REMINDER_DAYS)).unwrap_or(NaiveDate::MAX);
        let due = self.ports.store.due_reminders(today, until).await?;
        let mut sent = 0;
        for d in &due {
            let payload = AssetReminderDue {
                asset_id: d.asset_id,
                tenant_id: d.tenant_id,
                owner_id: d.owner_id,
                kind: d.kind.clone(),
                date: d.date,
                label: d.label.clone(),
            };
            let event = cooper_kernel::EventEnvelope::wrap(&payload, Actor::Automation, now).map_err(|_| ExtrasError::Internal)?;
            if self.ports.store.record_reminder(d, &event).await? {
                sent += 1;
            }
        }
        Ok(sent)
    }

    // --- Inventory (FR-AST-008) and software (FR-AST-013) --------------------------------

    /// Records an inventory report for the Asset it identifies (needs `assets.inventory`).
    /// The Asset must exist already and be active; the report replaces the hardware snapshot
    /// and the installed software.
    ///
    /// # Errors
    ///
    /// Forbidden, no or an ambiguous Asset, or storage failure.
    pub async fn push_inventory(&self, principal: &Principal, report: InventoryReport) -> Result<(Uuid, usize), ExtrasError> {
        require(principal, capabilities::INVENTORY)?;
        let scope: &TenantScope = principal.scope();
        let mut found = Vec::new();
        for (kind, value) in [
            ("service_tag", report.service_tag.as_deref()),
            ("hostname", report.hostname.as_deref()),
            ("asset_tag", report.asset_tag.as_deref()),
        ] {
            if value.is_none() {
                continue;
            }
            let (s, h, a) = match kind {
                "service_tag" => (value, None, None),
                "hostname" => (None, value, None),
                _ => (None, None, value),
            };
            found = self.ports.store.find_asset(scope, s, h, a).await?;
            if !found.is_empty() {
                break;
            }
        }
        let asset = match found.as_slice() {
            [] => return Err(ExtrasError::NotFound("Asset")),
            [one] => one.clone(),
            _ => return Err(ExtrasError::Conflict("more than one Asset matches this inventory report".into())),
        };
        let mut software = Vec::with_capacity(report.software.len());
        for item in &report.software {
            let app = self.ports.store.application_id(asset.tenant_id, item).await?;
            software.push((item.clone(), app));
        }
        let snapshot = json!({
            "hardware": {
                "manufacturer": report.manufacturer, "model": report.model, "os": report.os,
                "cpu": report.cpu, "ram_mb": report.ram_mb,
            },
            "disks": report.disks.iter().map(|(n, gb)| json!({ "name": n, "size_gb": gb })).collect::<Vec<_>>(),
            "network_adapters": report.adapters.iter().map(|(n, mac, ip)| json!({ "name": n, "mac": mac, "ip": ip })).collect::<Vec<_>>(),
        });
        let now = self.ports.clock.now();
        let record = audit(
            asset.tenant_id,
            ("asset", asset.id),
            AuditAction::Updated,
            &[],
            vec![("inventory_software_count", json!(software.len()))],
            principal.actor(),
            now,
        );
        self.ports
            .store
            .save_inventory(asset.tenant_id, asset.id, &snapshot, &software, now, &record)
            .await?;
        Ok((asset.id, software.len()))
    }

    /// The software installed on an Asset (needs `assets.read` and visibility).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or storage failure.
    pub async fn software_of(&self, principal: &Principal, asset: Uuid) -> Result<Vec<crate::extras_ports::Installation>, ExtrasError> {
        self.check_read(principal)?;
        self.asset(principal, asset).await?;
        Ok(self.ports.store.installations(principal.scope(), asset).await?)
    }

    // --- Licences (FR-AST-014) -----------------------------------------------------------

    /// Licences in the caller's Companies (needs `assets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn licences(&self, principal: &Principal) -> Result<Vec<LicenceRecord>, ExtrasError> {
        self.check_read(principal)?;
        Ok(self.ports.store.licences(principal.scope()).await?)
    }

    /// One licence with its compliance (needs `assets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn licence(&self, principal: &Principal, id: Uuid) -> Result<LicenceView, ExtrasError> {
        self.check_read(principal)?;
        let licence = self.ports.store.licence(principal.scope(), id).await?.ok_or(ExtrasError::NotFound("Licence"))?;
        self.view(principal.scope(), licence).await
    }

    async fn view(&self, scope: &TenantScope, licence: LicenceRecord) -> Result<LicenceView, ExtrasError> {
        let (linked, installed) = self.ports.store.compliance_counts(scope, licence.id).await?;
        Ok(LicenceView {
            compliance: inventory::compliance(licence.quantity, installed, linked),
            linked_applications: linked,
            installed_on: installed,
            licence,
        })
    }

    fn validate_licence(input: &LicenceInput) -> Result<Option<(i64, String)>, ExtrasError> {
        let t = input.licence_type.trim();
        if t.is_empty() || t.chars().count() > 100 {
            return Err(invalid("licence_type", "must be 1 to 100 characters"));
        }
        if input.notice_period_days.is_some_and(|d| d > 3650) {
            return Err(invalid("notice_period_days", "must be at most 3650"));
        }
        if input.quantity > 1_000_000 {
            return Err(invalid("quantity", "must be at most 1000000"));
        }
        match &input.cost {
            None => Ok(None),
            Some((amount, currency)) => {
                let money = Money::parse(amount, currency).map_err(|e| invalid("cost", e.to_string()))?;
                Ok(Some((money.amount.hundredths(), money.currency.as_str().to_owned())))
            }
        }
    }

    /// Records a licence (needs `assets.update`). The key is sealed before it is stored.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or storage failure.
    pub async fn create_licence(&self, principal: &Principal, tenant: TenantId, input: LicenceInput) -> Result<LicenceView, ExtrasError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        if !principal.scope().allows(tenant) {
            return Err(ExtrasError::NotFound("Company"));
        }
        let cost = Self::validate_licence(&input)?;
        let now = self.ports.clock.now();
        let id = Uuid::now_v7();
        let (sealed, hint) = self.seal_key(id, input.key.as_deref())?;
        let record = LicenceRecord {
            id,
            tenant_id: tenant,
            licence_type: input.licence_type.trim().to_owned(),
            key_hint: hint,
            quantity: input.quantity,
            renewal_date: input.renewal_date,
            notice_period_days: input.notice_period_days,
            cost: cost.clone(),
            status: input.status,
            created_by: principal.actor(),
            created_at: now,
            updated_at: now,
        };
        let rec = audit(tenant, ("licence", id), AuditAction::Created, &[], licence_audit(&record), principal.actor(), now);
        self.ports.store.save_licence(&record, sealed.as_deref(), &rec).await?;
        self.view(principal.scope(), record).await
    }

    /// Changes a licence (needs `assets.update`). A new key replaces the stored one; no key
    /// keeps it.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or storage failure.
    pub async fn update_licence(&self, principal: &Principal, id: Uuid, input: LicenceInput) -> Result<LicenceView, ExtrasError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        let before = self.ports.store.licence(principal.scope(), id).await?.ok_or(ExtrasError::NotFound("Licence"))?;
        let cost = Self::validate_licence(&input)?;
        let now = self.ports.clock.now();
        let (sealed, hint) = self.seal_key(id, input.key.as_deref())?;
        let mut after = before.clone();
        after.licence_type = input.licence_type.trim().to_owned();
        after.quantity = input.quantity;
        after.renewal_date = input.renewal_date;
        after.notice_period_days = input.notice_period_days;
        after.cost = cost;
        after.status = input.status;
        if hint.is_some() {
            after.key_hint = hint;
        }
        after.updated_at = now;
        let rec = audit(
            before.tenant_id,
            ("licence", id),
            AuditAction::Updated,
            &licence_audit(&before),
            licence_audit(&after),
            principal.actor(),
            now,
        );
        self.ports.store.save_licence(&after, sealed.as_deref(), &rec).await?;
        self.view(principal.scope(), after).await
    }

    fn seal_key(&self, id: Uuid, key: Option<&str>) -> Result<(Option<String>, Option<String>), ExtrasError> {
        let Some(key) = key.map(str::trim).filter(|k| !k.is_empty()) else {
            return Ok((None, None));
        };
        if key.chars().count() > 4000 {
            return Err(invalid("key", "must be at most 4000 characters"));
        }
        let sealed = self
            .ports
            .keyring
            .encrypt(&format!("licences.licence_key:{id}"), key.as_bytes())
            .map_err(|e| {
                tracing::error!(error = %e, "cannot seal a licence key");
                ExtrasError::Internal
            })?;
        Ok((Some(sealed), inventory::key_hint(key)))
    }

    /// Links an application to a licence (needs `assets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or storage failure.
    pub async fn link_application(&self, principal: &Principal, licence: Uuid, application: Uuid) -> Result<LicenceView, ExtrasError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        let record = self.ports.store.licence(principal.scope(), licence).await?.ok_or(ExtrasError::NotFound("Licence"))?;
        self.ports.store.link_application(record.tenant_id, licence, application).await?;
        self.view(principal.scope(), record).await
    }

    /// Unlinks an application from a licence (needs `assets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or storage failure.
    pub async fn unlink_application(&self, principal: &Principal, licence: Uuid, application: Uuid) -> Result<LicenceView, ExtrasError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        let record = self.ports.store.licence(principal.scope(), licence).await?.ok_or(ExtrasError::NotFound("Licence"))?;
        self.ports.store.unlink_application(licence, application).await?;
        self.view(principal.scope(), record).await
    }

    // --- Import (FR-AST-007) -------------------------------------------------------------

    /// Imports Assets from CSV (needs `assets.import`, and the Asset Capabilities it uses).
    /// With `dry_run`, nothing is written: the log says what would happen.
    ///
    /// # Errors
    ///
    /// Forbidden, a file or mapping that cannot be used, or storage failure. Row-level
    /// problems are in the log, not errors.
    pub async fn import(&self, principal: &Principal, req: ImportRequest) -> Result<ImportReport, ExtrasError> {
        require(principal, capabilities::IMPORT)?;
        if !principal.scope().allows(req.tenant) {
            return Err(ExtrasError::NotFound("Company"));
        }
        let (header, rows) = import::parse_csv(&req.csv)?;
        let mut mapping = BTreeMap::new();
        for (column, target) in &req.mapping {
            mapping.insert(column.clone(), ImportField::parse(target)?);
        }
        let mapped = import::check_mapping(&header, &mapping)?;
        let write_mode = WriteMode::parse(&req.write_mode)?;
        let missing = MissingPolicy::parse(&req.missing)?;
        let tenant_scope = TenantScope::single(req.tenant);
        let today = self.ports.clock.now().date_naive();
        let mut log = Vec::with_capacity(rows.len());
        let mut matched: BTreeSet<Uuid> = BTreeSet::new();

        for (i, row) in rows.iter().enumerate() {
            let line = i + 2;
            let values = import::row_values(&mapped, row);
            let outcome = self.import_row(principal, &req, &tenant_scope, &values, write_mode).await;
            match outcome {
                Ok((kind, id)) => {
                    if let Some(id) = id {
                        matched.insert(id);
                    }
                    log.push(RowLog { line, outcome: kind.as_str().to_owned(), asset_id: id, message: None });
                }
                Err((id, message)) => {
                    if let Some(id) = id {
                        matched.insert(id);
                    }
                    log.push(RowLog { line, outcome: RowOutcome::Failed.as_str().to_owned(), asset_id: id, message: Some(message) });
                }
            }
        }

        if let (Some(asset_type), false) = (req.asset_type_id, matches!(missing, MissingPolicy::Ignore)) {
            for (id, service_tag, hostname, asset_tag) in self.ports.store.active_assets_of_type(req.tenant, asset_type).await? {
                if matched.contains(&id) {
                    continue;
                }
                let label = asset_tag.or(service_tag).or(hostname).unwrap_or_else(|| id.to_string());
                match missing {
                    MissingPolicy::Flag => log.push(RowLog {
                        line: 0,
                        outcome: "flagged".into(),
                        asset_id: Some(id),
                        message: Some(format!("{label} is not in the file")),
                    }),
                    MissingPolicy::Deactivate if !req.dry_run => {
                        let retire = RetireRequest {
                            retired_on: Some(today),
                            reason: "Missing from an Asset import".into(),
                            asset_status_id: None,
                        };
                        match self.ports.assets.retire(principal, crate::domain::AssetId::from_uuid(id), retire).await {
                            Ok(_) => log.push(RowLog { line: 0, outcome: "deactivated".into(), asset_id: Some(id), message: None }),
                            Err(e) => log.push(RowLog { line: 0, outcome: RowOutcome::Failed.as_str().into(), asset_id: Some(id), message: Some(e.to_string()) }),
                        }
                    }
                    MissingPolicy::Deactivate => log.push(RowLog { line: 0, outcome: "would_deactivate".into(), asset_id: Some(id), message: None }),
                    MissingPolicy::Ignore => {}
                }
            }
        }
        Ok(ImportReport { dry_run: req.dry_run, log })
    }

    /// One row: find the Asset by tag, then fill or overwrite it, or create it. Returns the
    /// outcome and the Asset, or the Asset (if any) and the reason it failed.
    async fn import_row(
        &self,
        principal: &Principal,
        req: &ImportRequest,
        scope: &TenantScope,
        values: &BTreeMap<ImportField, String>,
        write_mode: WriteMode,
    ) -> Result<(RowOutcome, Option<Uuid>), (Option<Uuid>, String)> {
        let identifiers = [
            (ImportField::AssetTag, values.get(&ImportField::AssetTag)),
            (ImportField::ServiceTag, values.get(&ImportField::ServiceTag)),
            (ImportField::Hostname, values.get(&ImportField::Hostname)),
        ];
        if identifiers.iter().all(|(_, v)| v.is_none()) {
            return Err((None, "the row has no asset tag, service tag or hostname".into()));
        }
        let mut existing = None;
        for (field, value) in identifiers {
            let Some(value) = value else { continue };
            let (s, h, a) = match field {
                ImportField::ServiceTag => (Some(value.as_str()), None, None),
                ImportField::Hostname => (None, Some(value.as_str()), None),
                _ => (None, None, Some(value.as_str())),
            };
            let found = self.ports.store.find_asset(scope, s, h, a).await.map_err(|e| (None, e.to_string()))?;
            if let Some(f) = found.into_iter().next() {
                existing = Some(f.id);
                break;
            }
        }
        let dates = |field: ImportField| -> Result<Option<NaiveDate>, (Option<Uuid>, String)> {
            values
                .get(&field)
                .map(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| (existing, format!("{} is not a date (YYYY-MM-DD)", field.name()))))
                .transpose()
        };
        let text_of = |field: ImportField| values.get(&field).cloned();
        let (purchase_date, warranty, lease_start, lease_end) = (
            dates(ImportField::PurchaseDate)?,
            dates(ImportField::WarrantyExpiresOn)?,
            dates(ImportField::LeaseStartsOn)?,
            dates(ImportField::LeaseExpiresOn)?,
        );
        match existing {
            Some(id) => {
                let asset = self
                    .ports
                    .assets
                    .get(principal, crate::domain::AssetId::from_uuid(id))
                    .await
                    .map_err(|e| (Some(id), e.to_string()))?;
                let mut d = asset.details().clone();
                let before = d.clone();
                let overwrite = matches!(write_mode, WriteMode::Overwrite);
                let set_text = |slot: &mut Option<String>, value: Option<String>| {
                    if let Some(v) = value && (overwrite || slot.is_none()) {
                        *slot = Some(v);
                    }
                };
                set_text(&mut d.hostname, text_of(ImportField::Hostname));
                set_text(&mut d.service_tag, text_of(ImportField::ServiceTag));
                set_text(&mut d.asset_tag, text_of(ImportField::AssetTag));
                let set_date = |slot: &mut Option<NaiveDate>, value: Option<NaiveDate>| {
                    if let Some(v) = value && (overwrite || slot.is_none()) {
                        *slot = Some(v);
                    }
                };
                set_date(&mut d.purchase_date, purchase_date);
                set_date(&mut d.warranty_expires_on, warranty);
                set_date(&mut d.lease_starts_on, lease_start);
                set_date(&mut d.lease_expires_on, lease_end);
                if d == before {
                    return Ok((RowOutcome::Unchanged, Some(id)));
                }
                if !req.dry_run {
                    self.ports
                        .assets
                        .edit(principal, crate::domain::AssetId::from_uuid(id), d.clone())
                        .await
                        .map_err(|e| (Some(id), e.to_string()))?;
                }
                Ok((RowOutcome::Updated, Some(id)))
            }
            None => {
                let Some(asset_type) = req.asset_type_id else {
                    return Err((None, "no Asset Type is set for creating new Assets".into()));
                };
                let details = AssetDetails {
                    hostname: text_of(ImportField::Hostname),
                    service_tag: text_of(ImportField::ServiceTag),
                    asset_tag: text_of(ImportField::AssetTag),
                    asset_type_id: Some(asset_type),
                    asset_status_id: None,
                    location_id: None,
                    supplier_id: None,
                    cost_centre_id: None,
                    purchase_date,
                    purchase_cost: None,
                    warranty_expires_on: warranty,
                    lease_starts_on: lease_start,
                    lease_expires_on: lease_end,
                };
                if req.dry_run {
                    return Ok((RowOutcome::Created, None));
                }
                let created = self
                    .ports
                    .assets
                    .create(principal, req.tenant, details)
                    .await
                    .map_err(|e| (None, e.to_string()))?;
                Ok((RowOutcome::Created, Some(created.id().as_uuid())))
            }
        }
    }
}

fn set_audit(s: &FieldSet) -> Vec<(&'static str, Value)> {
    vec![
        ("name", json!(s.name)),
        ("active", json!(s.active)),
        ("asset_type_ids", json!(s.asset_type_ids.iter().collect::<Vec<_>>())),
    ]
}

fn field_audit(f: &FieldDefinition) -> Vec<(&'static str, Value)> {
    vec![
        ("key", json!(f.key)),
        ("label", json!(f.label)),
        ("kind", json!(f.kind.as_str())),
        ("options", json!(f.options)),
        ("required", json!(f.required)),
        ("unique", json!(f.unique)),
        ("searchable", json!(f.searchable)),
        ("show_in_list", json!(f.show_in_list)),
        ("active", json!(f.active)),
    ]
}

fn licence_audit(l: &LicenceRecord) -> Vec<(&'static str, Value)> {
    vec![
        ("licence_type", json!(l.licence_type)),
        ("quantity", json!(l.quantity)),
        ("renewal_date", json!(l.renewal_date)),
        ("notice_period_days", json!(l.notice_period_days)),
        ("status", json!(l.status.as_str())),
        ("key_hint", json!(l.key_hint)),
    ]
}

