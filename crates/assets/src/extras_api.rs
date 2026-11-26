//! HTTP API of the Asset extras (FR-AST-003, FR-AST-007, FR-AST-008, FR-AST-013,
//! FR-AST-014; ADR-0041).

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::NaiveDate;
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::TenantId;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::custom_fields::{CustomKind, FieldDefinition, FieldInput, FieldSet};
use crate::extras::{
    ExtrasError, ExtrasService, FieldSetInput, FieldUpdate, ImportReport, ImportRequest, LicenceInput, LicenceView,
};
use crate::extras_ports::{Installation, LicenceRecord};
use crate::import::{ImportError, MissingPolicy};
use crate::inventory::{self, InventoryError, InventoryReport, LicenceStatus, SoftwareItem};

/// State the handlers need.
#[derive(Clone)]
pub struct AssetExtrasApi {
    /// The service.
    pub service: ExtrasService,
}

/// `/api/v1` path prefixes the extras add to the Assets and Software Modules (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &["/asset-field-sets", "/inventory", "/licences", "/software-applications"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    AssetExtrasApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_field_sets, create_field_set))
        .routes(routes!(update_field_set))
        .routes(routes!(add_field))
        .routes(routes!(update_asset_field))
        .routes(routes!(get_custom_values, set_custom_values))
        .routes(routes!(asset_software))
        .routes(routes!(push_inventory))
        .routes(routes!(list_licences, create_licence))
        .routes(routes!(get_licence, update_licence))
        .routes(routes!(link_application, unlink_application))
        .routes(routes!(import_assets))
}

/// An error as problem+json.
pub struct ApiError(pub ExtrasError);

impl From<ExtrasError> for ApiError {
    fn from(e: ExtrasError) -> Self {
        Self(e)
    }
}

impl From<InventoryError> for ApiError {
    fn from(e: InventoryError) -> Self {
        Self(ExtrasError::from(e))
    }
}

impl From<ImportError> for ApiError {
    fn from(e: ImportError) -> Self {
        Self(ExtrasError::from(e))
    }
}

impl From<CustomFieldsError> for ApiError {
    fn from(e: CustomFieldsError) -> Self {
        Self(ExtrasError::from(e))
    }
}

type CustomFieldsError = crate::custom_fields::CustomFieldError;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let detail = self.0.to_string();
        let typed = |status: StatusCode, slug: &str, title: &str| {
            ProblemJson::new(status)
                .with_type(format!("https://cooper.dev/problems/{slug}"), title)
                .with_detail(detail.clone())
        };
        match self.0 {
            ExtrasError::Invalid { field, .. } => typed(StatusCode::UNPROCESSABLE_ENTITY, "validation", "Invalid input")
                .with_extension("field", field),
            ExtrasError::Forbidden(f) => forbidden(&f),
            ExtrasError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            ExtrasError::Conflict(_) => typed(StatusCode::CONFLICT, "conflict", "Conflict"),
            ExtrasError::Asset(_) => typed(StatusCode::CONFLICT, "rule-violated", "Not allowed in the current state"),
            ExtrasError::Internal => ProblemJson::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ----- DTOs -------------------------------------------------------------------------------

/// A custom field.
#[derive(Debug, Serialize, ToSchema)]
pub struct CustomFieldDto {
    /// ID.
    pub id: Uuid,
    /// Key.
    pub key: String,
    /// Label.
    pub label: String,
    /// `text`, `number`, `date`, `boolean` or `choice`.
    pub kind: String,
    /// Options of a choice field.
    pub options: Vec<String>,
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

impl From<&FieldDefinition> for CustomFieldDto {
    fn from(f: &FieldDefinition) -> Self {
        Self {
            id: f.id,
            key: f.key.clone(),
            label: f.label.clone(),
            kind: f.kind.as_str().to_owned(),
            options: f.options.clone(),
            required: f.required,
            unique: f.unique,
            searchable: f.searchable,
            show_in_list: f.show_in_list,
            active: f.active,
        }
    }
}

/// A Field Set.
#[derive(Debug, Serialize, ToSchema)]
pub struct FieldSetDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Asset Types it applies to.
    pub asset_type_ids: Vec<Uuid>,
    /// Active.
    pub active: bool,
    /// Fields.
    pub fields: Vec<CustomFieldDto>,
}

impl From<&FieldSet> for FieldSetDto {
    fn from(s: &FieldSet) -> Self {
        Self {
            id: s.id,
            name: s.name.clone(),
            asset_type_ids: s.asset_type_ids.iter().copied().collect(),
            active: s.active,
            fields: s.fields.iter().map(CustomFieldDto::from).collect(),
        }
    }
}

/// A Field Set to create or change.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FieldSetRequest {
    /// Name.
    pub name: String,
    /// Asset Types it applies to.
    #[serde(default)]
    pub asset_type_ids: Vec<Uuid>,
    /// Active (default true).
    #[serde(default = "yes")]
    pub active: bool,
}

fn yes() -> bool {
    true
}

/// A custom field to add.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FieldRequest {
    /// Key.
    pub key: String,
    /// Label.
    pub label: String,
    /// `text`, `number`, `date`, `boolean` or `choice`.
    pub kind: String,
    /// Options of a choice field.
    #[serde(default)]
    pub options: Vec<String>,
    /// Required.
    #[serde(default)]
    pub required: bool,
    /// Unique per Company.
    #[serde(default)]
    pub unique: bool,
    /// Searchable.
    #[serde(default)]
    pub searchable: bool,
    /// Shown in lists.
    #[serde(default)]
    pub show_in_list: bool,
}

/// What may change on a custom field.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FieldUpdateRequest {
    /// Label.
    pub label: String,
    /// Required.
    #[serde(default)]
    pub required: bool,
    /// Unique per Company.
    #[serde(default)]
    pub unique: bool,
    /// Searchable.
    #[serde(default)]
    pub searchable: bool,
    /// Shown in lists.
    #[serde(default)]
    pub show_in_list: bool,
    /// Active.
    #[serde(default = "yes")]
    pub active: bool,
}

/// Custom values of an Asset, by key.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CustomValuesDto {
    /// Values by field key.
    pub values: BTreeMap<String, String>,
}

/// An installed application.
#[derive(Debug, Serialize, ToSchema)]
pub struct InstallationDto {
    /// The application.
    pub application_id: Uuid,
    /// Name.
    pub name: String,
    /// Publisher.
    pub publisher: String,
    /// Installed version.
    pub version: String,
}

impl From<Installation> for InstallationDto {
    fn from(i: Installation) -> Self {
        Self {
            application_id: i.application_id,
            name: i.name,
            publisher: i.publisher,
            version: i.version,
        }
    }
}

/// Hardware of an inventory report.
#[derive(Debug, Deserialize, ToSchema, Default)]
pub struct HardwareDto {
    /// Manufacturer.
    pub manufacturer: Option<String>,
    /// Model.
    pub model: Option<String>,
    /// Operating system.
    pub os: Option<String>,
    /// Processor.
    pub cpu: Option<String>,
    /// Memory in MB.
    pub ram_mb: Option<u32>,
}

/// A disk.
#[derive(Debug, Deserialize, ToSchema)]
pub struct DiskDto {
    /// Name.
    pub name: String,
    /// Size in GB.
    pub size_gb: u32,
}

/// A network adapter.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AdapterDto {
    /// Name.
    pub name: String,
    /// MAC address.
    pub mac: Option<String>,
    /// IP address.
    pub ip: Option<String>,
}

/// An installed application in an inventory report.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SoftwareDto {
    /// Name.
    pub name: String,
    /// Publisher.
    #[serde(default)]
    pub publisher: String,
    /// Version.
    #[serde(default)]
    pub version: String,
}

/// An inventory report, pushed by an agent or script.
#[derive(Debug, Deserialize, ToSchema)]
pub struct InventoryRequest {
    /// Service tag (finds the Asset).
    pub service_tag: Option<String>,
    /// Hostname (finds the Asset).
    pub hostname: Option<String>,
    /// Asset tag (finds the Asset).
    pub asset_tag: Option<String>,
    /// Hardware.
    #[serde(default)]
    pub hardware: HardwareDto,
    /// Disks.
    #[serde(default)]
    pub disks: Vec<DiskDto>,
    /// Network adapters.
    #[serde(default)]
    pub network_adapters: Vec<AdapterDto>,
    /// Installed software.
    #[serde(default)]
    pub software: Vec<SoftwareDto>,
}

/// Where an inventory report was recorded.
#[derive(Debug, Serialize, ToSchema)]
pub struct InventoryReceiptDto {
    /// The Asset.
    pub asset_id: Uuid,
    /// Applications recorded as installed.
    pub installed_software: usize,
}

/// A licence to create or change.
#[derive(Debug, Deserialize, ToSchema)]
pub struct LicenceRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Type.
    pub licence_type: String,
    /// The key (sealed at rest; never shown again). Omit to keep the stored key.
    pub key: Option<String>,
    /// Quantity.
    pub quantity: u32,
    /// Renewal date.
    pub renewal_date: Option<NaiveDate>,
    /// Notice period, in days.
    pub notice_period_days: Option<u32>,
    /// Cost as decimal text, e.g. `"1299.00`".
    pub cost: Option<String>,
    /// Currency (ISO 4217), with `cost`.
    pub currency: Option<String>,
    /// `active`, `expired` or `cancelled` (default active).
    pub status: Option<String>,
}

/// A licence with its use and compliance.
#[derive(Debug, Serialize, ToSchema)]
pub struct LicenceDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Type.
    pub licence_type: String,
    /// Last four characters of the key.
    pub key_hint: Option<String>,
    /// Quantity.
    pub quantity: u32,
    /// Renewal date.
    pub renewal_date: Option<NaiveDate>,
    /// Notice period, in days.
    pub notice_period_days: Option<u32>,
    /// Cost as decimal text.
    pub cost: Option<String>,
    /// Currency.
    pub currency: Option<String>,
    /// Status.
    pub status: String,
    /// Applications linked.
    pub linked_applications: u32,
    /// Assets with any of them installed.
    pub installed_on: u32,
    /// `compliant`, `over_used` or `no_applications`.
    pub compliance: String,
}

fn cost_text(minor: i64) -> String {
    format!("{}.{:02}", minor / 100, (minor % 100).abs())
}

impl From<LicenceView> for LicenceDto {
    fn from(v: LicenceView) -> Self {
        let l: LicenceRecord = v.licence;
        Self {
            id: l.id,
            company_id: l.tenant_id.as_uuid(),
            licence_type: l.licence_type,
            key_hint: l.key_hint,
            quantity: l.quantity,
            renewal_date: l.renewal_date,
            notice_period_days: l.notice_period_days,
            cost: l.cost.as_ref().map(|(m, _)| cost_text(*m)),
            currency: l.cost.map(|(_, c)| c),
            status: l.status.as_str().to_owned(),
            linked_applications: v.linked_applications,
            installed_on: v.installed_on,
            compliance: match v.compliance {
                inventory::Compliance::Compliant => "compliant",
                inventory::Compliance::OverUsed => "over_used",
                inventory::Compliance::NoApplications => "no_applications",
            }
            .to_owned(),
        }
    }
}

/// Links an application to a licence.
#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkRequest {
    /// The application.
    pub application_id: Uuid,
}

/// A CSV import.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ImportAssetsRequest {
    /// Company.
    pub company_id: Uuid,
    /// The CSV text, with a header row.
    pub csv: String,
    /// Column header to field: `hostname`, `service_tag`, `asset_tag`, `purchase_date`,
    /// `warranty_expires_on`, `lease_starts_on` or `lease_expires_on`.
    pub mapping: BTreeMap<String, String>,
    /// Asset Type for new Assets, and the scope of the missing-asset rule.
    pub asset_type_id: Option<Uuid>,
    /// `fill` (default) or `overwrite`.
    pub write_mode: Option<String>,
    /// `ignore` (default), `flag` or `deactivate`.
    pub missing: Option<String>,
    /// Only preview: nothing is written.
    #[serde(default)]
    pub dry_run: bool,
}

/// One row of the outcome log.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportRowDto {
    /// Line in the file (the header is line 1; 0 for Assets not in the file).
    pub line: usize,
    /// `created`, `updated`, `unchanged`, `failed`, `flagged`, `deactivated` or `would_deactivate`.
    pub outcome: String,
    /// The Asset concerned.
    pub asset_id: Option<Uuid>,
    /// Why, when it failed or was flagged.
    pub message: Option<String>,
}

/// The outcome of an import.
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportResultDto {
    /// Whether this was only a preview.
    pub dry_run: bool,
    /// One entry per row, then per Asset not in the file.
    pub log: Vec<ImportRowDto>,
}

impl From<ImportReport> for ImportResultDto {
    fn from(r: ImportReport) -> Self {
        Self {
            dry_run: r.dry_run,
            log: r
                .log
                .into_iter()
                .map(|l| ImportRowDto {
                    line: l.line,
                    outcome: l.outcome,
                    asset_id: l.asset_id,
                    message: l.message,
                })
                .collect(),
        }
    }
}

fn parse_kind(s: &str) -> Result<CustomKind, ExtrasError> {
    CustomKind::parse(s).map_err(ExtrasError::from)
}

fn field_input(f: FieldRequest) -> Result<FieldInput, ExtrasError> {
    Ok(FieldInput {
        key: f.key,
        label: f.label,
        kind: parse_kind(&f.kind)?,
        options: f.options,
        required: f.required,
        unique: f.unique,
        searchable: f.searchable,
        show_in_list: f.show_in_list,
    })
}

fn licence_input(b: LicenceRequest) -> Result<(Option<TenantId>, LicenceInput), ExtrasError> {
    let cost = match (b.cost, b.currency) {
        (Some(a), Some(c)) => Some((a, c)),
        (None, None) => None,
        _ => return Err(ExtrasError::Invalid { field: "cost".into(), reason: "a cost needs a currency, and the other way round".into() }),
    };
    Ok((
        b.company_id.map(TenantId::from_uuid),
        LicenceInput {
            licence_type: b.licence_type,
            key: b.key,
            quantity: b.quantity,
            renewal_date: b.renewal_date,
            notice_period_days: b.notice_period_days,
            cost,
            status: b.status.as_deref().map_or(Ok(LicenceStatus::Active), LicenceStatus::parse).map_err(|e| ExtrasError::Invalid {
                field: "status".into(),
                reason: e.to_string(),
            })?,
        },
    ))
}

// ----- Handlers ---------------------------------------------------------------------------

/// Field Sets with their custom fields (needs `assets.read`).
#[utoipa::path(get, path = "/asset-field-sets", tag = "Assets", responses((status = 200, body = [FieldSetDto])))]
async fn list_field_sets(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<FieldSetDto>>> {
    let sets = api.service.field_sets(&p).await?;
    Ok(Json(sets.iter().map(FieldSetDto::from).collect()))
}

/// Creates a Field Set (needs `assets.configure`).
#[utoipa::path(post, path = "/asset-field-sets", tag = "Assets", request_body = FieldSetRequest, responses((status = 201, body = FieldSetDto)))]
async fn create_field_set(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Json(b): Json<FieldSetRequest>) -> ApiResult<Response> {
    let set = api
        .service
        .create_field_set(&p, FieldSetInput { name: b.name, asset_type_ids: b.asset_type_ids.into_iter().collect::<BTreeSet<_>>(), active: b.active })
        .await?;
    Ok((StatusCode::CREATED, Json(FieldSetDto::from(&set))).into_response())
}

/// Changes a Field Set (needs `assets.configure`).
#[utoipa::path(put, path = "/asset-field-sets/{id}", tag = "Assets", params(("id" = Uuid, Path)), request_body = FieldSetRequest, responses((status = 200, body = FieldSetDto)))]
async fn update_field_set(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<FieldSetRequest>) -> ApiResult<Json<FieldSetDto>> {
    let set = api
        .service
        .update_field_set(&p, id, FieldSetInput { name: b.name, asset_type_ids: b.asset_type_ids.into_iter().collect::<BTreeSet<_>>(), active: b.active })
        .await?;
    Ok(Json(FieldSetDto::from(&set)))
}

/// Adds a custom field to a Field Set (needs `assets.configure`).
#[utoipa::path(post, path = "/asset-field-sets/{id}/fields", tag = "Assets", params(("id" = Uuid, Path)), request_body = FieldRequest, responses((status = 201, body = CustomFieldDto)))]
async fn add_field(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<FieldRequest>) -> ApiResult<Response> {
    let f = api.service.add_field(&p, id, field_input(b)?).await?;
    Ok((StatusCode::CREATED, Json(CustomFieldDto::from(&f))).into_response())
}

/// Changes a custom field (needs `assets.configure`). Key, type and options are fixed.
#[utoipa::path(put, path = "/asset-field-sets/{id}/fields/{field_id}", tag = "Assets", params(("id" = Uuid, Path), ("field_id" = Uuid, Path)), request_body = FieldUpdateRequest, responses((status = 200, body = CustomFieldDto)))]
async fn update_asset_field(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path((id, field_id)): Path<(Uuid, Uuid)>, Json(b): Json<FieldUpdateRequest>) -> ApiResult<Json<CustomFieldDto>> {
    let f = api
        .service
        .update_field(&p, id, field_id, FieldUpdate {
            label: b.label,
            required: b.required,
            unique: b.unique,
            searchable: b.searchable,
            show_in_list: b.show_in_list,
            active: b.active,
        })
        .await?;
    Ok(Json(CustomFieldDto::from(&f)))
}

/// The custom values of an Asset (needs `assets.read`).
#[utoipa::path(get, path = "/assets/{id}/custom-fields", tag = "Assets", params(("id" = Uuid, Path)), responses((status = 200, body = CustomValuesDto)))]
async fn get_custom_values(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<CustomValuesDto>> {
    Ok(Json(CustomValuesDto { values: api.service.custom_values(&p, id).await? }))
}

/// Replaces the custom values of an Asset (needs `assets.update`).
#[utoipa::path(put, path = "/assets/{id}/custom-fields", tag = "Assets", params(("id" = Uuid, Path)), request_body = CustomValuesDto, responses((status = 200, body = CustomValuesDto)))]
async fn set_custom_values(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<CustomValuesDto>) -> ApiResult<Json<CustomValuesDto>> {
    Ok(Json(CustomValuesDto { values: api.service.set_custom_values(&p, id, &b.values).await? }))
}

/// The software installed on an Asset (needs `assets.read`).
#[utoipa::path(get, path = "/assets/{id}/software", tag = "Assets", params(("id" = Uuid, Path)), responses((status = 200, body = [InstallationDto])))]
async fn asset_software(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<InstallationDto>>> {
    let rows = api.service.software_of(&p, id).await?;
    Ok(Json(rows.into_iter().map(InstallationDto::from).collect()))
}

/// Records an inventory report for the Asset it identifies (needs `assets.inventory`).
#[utoipa::path(post, path = "/inventory/push", tag = "Assets", request_body = InventoryRequest, responses((status = 200, body = InventoryReceiptDto)))]
async fn push_inventory(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Json(b): Json<InventoryRequest>) -> ApiResult<Json<InventoryReceiptDto>> {
    let software = b
        .software
        .iter()
        .map(|s| SoftwareItem::new(&s.name, &s.publisher, &s.version))
        .collect::<Result<Vec<_>, _>>()?;
    let report = InventoryReport::new(
        (b.service_tag, b.hostname, b.asset_tag),
        (b.hardware.manufacturer, b.hardware.model, b.hardware.os, b.hardware.cpu, b.hardware.ram_mb),
        b.disks.into_iter().map(|d| (d.name, d.size_gb)).collect(),
        b.network_adapters.into_iter().map(|a| (a.name, a.mac, a.ip)).collect(),
        software,
    )?;
    let (asset_id, installed_software) = api.service.push_inventory(&p, report).await?;
    Ok(Json(InventoryReceiptDto { asset_id, installed_software }))
}

/// Licences in the caller's Companies (needs `assets.read`).
#[utoipa::path(get, path = "/licences", tag = "Assets", responses((status = 200, body = [LicenceDto])))]
async fn list_licences(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<LicenceDto>>> {
    let mut out = Vec::new();
    for l in api.service.licences(&p).await? {
        out.push(LicenceDto::from(api.service.licence(&p, l.id).await?));
    }
    Ok(Json(out))
}

/// Records a licence (needs `assets.update`).
#[utoipa::path(post, path = "/licences", tag = "Assets", request_body = LicenceRequest, responses((status = 201, body = LicenceDto)))]
async fn create_licence(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Json(b): Json<LicenceRequest>) -> ApiResult<Response> {
    let (tenant, input) = licence_input(b)?;
    let tenant = tenant.ok_or_else(|| ExtrasError::Invalid { field: "company_id".into(), reason: "is required".into() })?;
    let view = api.service.create_licence(&p, tenant, input).await?;
    Ok((StatusCode::CREATED, Json(LicenceDto::from(view))).into_response())
}

/// One licence with its compliance (needs `assets.read`).
#[utoipa::path(get, path = "/licences/{id}", tag = "Assets", params(("id" = Uuid, Path)), responses((status = 200, body = LicenceDto)))]
async fn get_licence(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<LicenceDto>> {
    Ok(Json(LicenceDto::from(api.service.licence(&p, id).await?)))
}

/// Changes a licence (needs `assets.update`).
#[utoipa::path(put, path = "/licences/{id}", tag = "Assets", params(("id" = Uuid, Path)), request_body = LicenceRequest, responses((status = 200, body = LicenceDto)))]
async fn update_licence(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<LicenceRequest>) -> ApiResult<Json<LicenceDto>> {
    let (_, input) = licence_input(b)?;
    Ok(Json(LicenceDto::from(api.service.update_licence(&p, id, input).await?)))
}

/// Links an application to a licence (needs `assets.update`).
#[utoipa::path(post, path = "/licences/{id}/applications", tag = "Assets", params(("id" = Uuid, Path)), request_body = LinkRequest, responses((status = 200, body = LicenceDto)))]
async fn link_application(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<LinkRequest>) -> ApiResult<Json<LicenceDto>> {
    Ok(Json(LicenceDto::from(api.service.link_application(&p, id, b.application_id).await?)))
}

/// Unlinks an application from a licence (needs `assets.update`).
#[utoipa::path(delete, path = "/licences/{id}/applications/{application_id}", tag = "Assets", params(("id" = Uuid, Path), ("application_id" = Uuid, Path)), responses((status = 200, body = LicenceDto)))]
async fn unlink_application(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Path((id, application_id)): Path<(Uuid, Uuid)>) -> ApiResult<Json<LicenceDto>> {
    Ok(Json(LicenceDto::from(api.service.unlink_application(&p, id, application_id).await?)))
}

/// Imports Assets from CSV: a preview with `dry_run`, otherwise the Assets are written
/// (needs `assets.import` and the Asset Capabilities it uses).
#[utoipa::path(post, path = "/assets/imports", tag = "Assets", request_body = ImportAssetsRequest, responses((status = 200, body = ImportResultDto)))]
async fn import_assets(State(api): State<AssetExtrasApi>, Authenticated(p): Authenticated, Json(b): Json<ImportAssetsRequest>) -> ApiResult<Json<ImportResultDto>> {
    let missing = b.missing.clone().unwrap_or_else(|| "ignore".into());
    MissingPolicy::parse(&missing)?;
    let report = api
        .service
        .import(&p, ImportRequest {
            tenant: TenantId::from_uuid(b.company_id),
            csv: b.csv,
            mapping: b.mapping,
            asset_type_id: b.asset_type_id,
            write_mode: b.write_mode.unwrap_or_else(|| "fill".into()),
            missing,
            dry_run: b.dry_run,
        })
        .await?;
    Ok(Json(ImportResultDto::from(report)))
}

