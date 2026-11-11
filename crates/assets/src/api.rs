//! HTTP API of Assets (FR-AST).

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{AnalystId, Cursor, Limit, RequesterId, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, AssetService, CheckOutRequest, RetireRequest};
use crate::domain::{
    Asset, AssetCheckout, AssetDetails, AssetError, AssetHolder, AssetId, Lookup, LookupInput,
    LookupKind, Money,
};
use crate::ports::{AssetLink, AssetQuery, AssetSummary, LinkKind};

/// State the handlers need.
#[derive(Clone)]
pub struct AssetsApi {
    /// The service.
    pub service: AssetService,
}

/// `/api/v1` path prefixes owned by the Assets Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &[
    "/assets",
    "/asset-types",
    "/asset-statuses",
    "/asset-locations",
];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    AssetsApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_asset_types, create_asset_type))
        .routes(routes!(update_asset_type))
        .routes(routes!(list_asset_statuses, create_asset_status))
        .routes(routes!(update_asset_status))
        .routes(routes!(list_asset_locations, create_asset_location))
        .routes(routes!(update_asset_location))
        .routes(routes!(list_assets, create_asset))
        .routes(routes!(asset_summary))
        .routes(routes!(get_asset, edit_asset))
        .routes(routes!(check_out_asset))
        .routes(routes!(check_in_asset))
        .routes(routes!(list_asset_checkouts))
        .routes(routes!(retire_asset))
        .routes(routes!(list_asset_links, link_asset))
        .routes(routes!(unlink_asset))
        .routes(routes!(asset_history))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<AssetError> for ApiError {
    fn from(e: AssetError) -> Self {
        Self(AppError::Rule(e))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let detail = self.0.to_string();
        let typed = |status: StatusCode, slug: &str, title: &str| {
            ProblemJson::new(status)
                .with_type(format!("https://cooper.dev/problems/{slug}"), title)
                .with_detail(detail.clone())
        };
        match self.0 {
            AppError::Rule(AssetError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(AssetError::InvalidState(_)) => typed(
                StatusCode::CONFLICT,
                "rule-violated",
                "Not allowed in the current state",
            ),
            AppError::Forbidden(f) => forbidden(&f),
            AppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            AppError::Conflict(_) => typed(StatusCode::CONFLICT, "conflict", "Conflict"),
            AppError::Internal => ProblemJson::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn created<T: Serialize>(body: T) -> Response {
    (StatusCode::CREATED, Json(body)).into_response()
}

// ----- Reference data (FR-AST-002) --------------------------------------------------------------

/// An Asset Type, Asset Status or Location.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetLookupDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Locations: the parent Location.
    pub parent_id: Option<Uuid>,
    /// Statuses: the Asset has left the estate (only Retire moves an Asset here).
    pub is_retired: bool,
    /// Statuses: `#rrggbb`.
    pub colour: Option<String>,
    /// Display order.
    pub sort_order: i32,
    /// Active (offered for new choices).
    pub is_active: bool,
}

impl From<Lookup> for AssetLookupDto {
    fn from(l: Lookup) -> Self {
        Self {
            id: l.id,
            name: l.name,
            parent_id: l.parent_id,
            is_retired: l.is_retired,
            colour: l.colour,
            sort_order: l.sort_order,
            is_active: l.is_active,
        }
    }
}

/// Creates or changes an Asset Type, Asset Status or Location.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssetLookupRequest {
    /// Name.
    pub name: String,
    /// Locations: the parent Location.
    pub parent_id: Option<Uuid>,
    /// Statuses: retired.
    #[serde(default)]
    pub is_retired: bool,
    /// Statuses: `#rrggbb`.
    pub colour: Option<String>,
    /// Order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active.
    #[serde(default = "yes")]
    pub is_active: bool,
}

const fn yes() -> bool {
    true
}

impl From<AssetLookupRequest> for LookupInput {
    fn from(r: AssetLookupRequest) -> Self {
        Self {
            name: r.name,
            parent_id: r.parent_id,
            is_retired: r.is_retired,
            colour: r.colour,
            sort_order: r.sort_order,
            is_active: r.is_active,
        }
    }
}

macro_rules! lookup_handlers {
    ($kind:expr, $path:literal, $item:literal, $list:ident, $create:ident, $update:ident) => {
        #[utoipa::path(get, path = $path, tag = "Assets", responses((status = 200, body = Vec<AssetLookupDto>)))]
        async fn $list(State(api): State<AssetsApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<AssetLookupDto>>> {
            Ok(Json(api.service.lookups(&p, $kind).await?.into_iter().map(Into::into).collect()))
        }

        #[utoipa::path(post, path = $path, tag = "Assets", request_body = AssetLookupRequest, responses((status = 201, body = AssetLookupDto)))]
        async fn $create(State(api): State<AssetsApi>, Authenticated(p): Authenticated, Json(b): Json<AssetLookupRequest>) -> ApiResult<Response> {
            Ok(created(AssetLookupDto::from(api.service.create_lookup(&p, $kind, b.into()).await?)))
        }

        #[utoipa::path(put, path = $item, tag = "Assets", params(("id" = Uuid, Path)), request_body = AssetLookupRequest, responses((status = 200, body = AssetLookupDto)))]
        async fn $update(
            State(api): State<AssetsApi>,
            Authenticated(p): Authenticated,
            Path(id): Path<Uuid>,
            Json(b): Json<AssetLookupRequest>,
        ) -> ApiResult<Json<AssetLookupDto>> {
            Ok(Json(api.service.update_lookup(&p, $kind, id, b.into()).await?.into()))
        }
    };
}

lookup_handlers!(
    LookupKind::Type,
    "/asset-types",
    "/asset-types/{id}",
    list_asset_types,
    create_asset_type,
    update_asset_type
);
lookup_handlers!(
    LookupKind::Status,
    "/asset-statuses",
    "/asset-statuses/{id}",
    list_asset_statuses,
    create_asset_status,
    update_asset_status
);
lookup_handlers!(
    LookupKind::Location,
    "/asset-locations",
    "/asset-locations/{id}",
    list_asset_locations,
    create_asset_location,
    update_asset_location
);

// ----- Assets (FR-AST-001) ----------------------------------------------------------------------

/// Who has an Asset.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AssetHolderDto {
    /// `requester` or `analyst`.
    pub kind: String,
    /// The person.
    pub id: Uuid,
}

impl From<AssetHolder> for AssetHolderDto {
    fn from(h: AssetHolder) -> Self {
        Self {
            kind: h.kind().to_owned(),
            id: h.id(),
        }
    }
}

/// An open Check Out.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetCurrentCheckoutDto {
    /// The Check Out.
    pub id: Uuid,
    /// The Holder.
    pub holder: AssetHolderDto,
    /// Since.
    pub checked_out_at: DateTime<Utc>,
    /// Expected return.
    pub due_back_on: Option<NaiveDate>,
}

/// An Asset (FR-AST-001).
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Network name.
    pub hostname: Option<String>,
    /// Manufacturer serial number.
    pub service_tag: Option<String>,
    /// Label, unique per Company.
    pub asset_tag: Option<String>,
    /// Asset Type.
    pub asset_type_id: Option<Uuid>,
    /// Asset Status.
    pub asset_status_id: Option<Uuid>,
    /// Location.
    pub location_id: Option<Uuid>,
    /// Supplier.
    pub supplier_id: Option<Uuid>,
    /// Cost Centre.
    pub cost_centre_id: Option<Uuid>,
    /// Purchase date.
    pub purchase_date: Option<NaiveDate>,
    /// Purchase cost as a decimal string with two places (`"1299.00"`).
    pub purchase_cost: Option<String>,
    /// ISO-4217 currency of the purchase cost.
    pub purchase_currency: Option<String>,
    /// Warranty expiry.
    pub warranty_expires_on: Option<NaiveDate>,
    /// Lease start.
    pub lease_starts_on: Option<NaiveDate>,
    /// Lease expiry.
    pub lease_expires_on: Option<NaiveDate>,
    /// The open Check Out, if the Asset is checked out.
    pub checkout: Option<AssetCurrentCheckoutDto>,
    /// In the estate (not retired).
    pub is_active: bool,
    /// The day it was retired.
    pub retired_on: Option<NaiveDate>,
    /// Why it was retired.
    pub retirement_reason: Option<String>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Asset> for AssetDto {
    fn from(a: Asset) -> Self {
        let d = a.details().clone();
        Self {
            id: a.id().as_uuid(),
            company_id: a.tenant_id().as_uuid(),
            hostname: d.hostname,
            service_tag: d.service_tag,
            asset_tag: d.asset_tag,
            asset_type_id: d.asset_type_id,
            asset_status_id: d.asset_status_id,
            location_id: d.location_id,
            supplier_id: d.supplier_id,
            cost_centre_id: d.cost_centre_id,
            purchase_date: d.purchase_date,
            purchase_cost: d.purchase_cost.as_ref().map(|m| m.amount.to_string()),
            purchase_currency: d.purchase_cost.map(|m| m.currency.to_string()),
            warranty_expires_on: d.warranty_expires_on,
            lease_starts_on: d.lease_starts_on,
            lease_expires_on: d.lease_expires_on,
            checkout: a.checkout().map(|c| AssetCurrentCheckoutDto {
                id: c.id,
                holder: c.holder.into(),
                checked_out_at: c.checked_out_at,
                due_back_on: c.due_back_on,
            }),
            is_active: a.is_active(),
            retired_on: a.retirement().map(|r| r.retired_on),
            retirement_reason: a.retirement().map(|r| r.reason.clone()),
            created_at: a.created_at(),
            updated_at: a.updated_at(),
        }
    }
}

/// Creates or edits an Asset.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssetRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Network name.
    pub hostname: Option<String>,
    /// Manufacturer serial number.
    pub service_tag: Option<String>,
    /// Label, unique per Company.
    pub asset_tag: Option<String>,
    /// Asset Type.
    pub asset_type_id: Option<Uuid>,
    /// Asset Status (not a retired one: use Retire).
    pub asset_status_id: Option<Uuid>,
    /// Location.
    pub location_id: Option<Uuid>,
    /// Supplier.
    pub supplier_id: Option<Uuid>,
    /// Cost Centre of the Asset's Company.
    pub cost_centre_id: Option<Uuid>,
    /// Purchase date.
    pub purchase_date: Option<NaiveDate>,
    /// Purchase cost as a decimal string (`"1299.00"`, at most two decimals).
    pub purchase_cost: Option<String>,
    /// ISO-4217 currency, required with a cost.
    pub purchase_currency: Option<String>,
    /// Warranty expiry.
    pub warranty_expires_on: Option<NaiveDate>,
    /// Lease start.
    pub lease_starts_on: Option<NaiveDate>,
    /// Lease expiry.
    pub lease_expires_on: Option<NaiveDate>,
}

impl AssetRequest {
    fn details(&self) -> Result<AssetDetails, ApiError> {
        let blank = |s: &Option<String>| {
            s.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let purchase_cost = match (blank(&self.purchase_cost), blank(&self.purchase_currency)) {
            (Some(a), Some(c)) => Some(Money::parse(&a, &c)?),
            (None, None) => None,
            (Some(_), None) => {
                return Err(
                    AssetError::invalid("purchase_currency", "is required with a cost").into(),
                );
            }
            (None, Some(_)) => {
                return Err(
                    AssetError::invalid("purchase_cost", "is required with a currency").into(),
                );
            }
        };
        Ok(AssetDetails {
            hostname: self.hostname.clone(),
            service_tag: self.service_tag.clone(),
            asset_tag: self.asset_tag.clone(),
            asset_type_id: self.asset_type_id,
            asset_status_id: self.asset_status_id,
            location_id: self.location_id,
            supplier_id: self.supplier_id,
            cost_centre_id: self.cost_centre_id,
            purchase_date: self.purchase_date,
            purchase_cost,
            warranty_expires_on: self.warranty_expires_on,
            lease_starts_on: self.lease_starts_on,
            lease_expires_on: self.lease_expires_on,
        })
    }
}

/// A page of Assets.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetPage {
    /// Assets, newest first.
    pub items: Vec<AssetDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Asset list filters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AssetListParams {
    /// One Company.
    pub company_id: Option<Uuid>,
    /// Asset Type.
    pub asset_type_id: Option<Uuid>,
    /// Asset Status.
    pub asset_status_id: Option<Uuid>,
    /// Location (and the Locations below it).
    pub location_id: Option<Uuid>,
    /// Holder kind (`requester` or `analyst`), with `holder_id`.
    pub holder_kind: Option<String>,
    /// Holder, with `holder_kind`.
    pub holder_id: Option<Uuid>,
    /// Checked out (`true`) or available (`false`).
    pub checked_out: Option<bool>,
    /// Include retired Assets (left out by default).
    pub include_retired: Option<bool>,
    /// Retired Assets only.
    pub retired_only: Option<bool>,
    /// Hostname, Asset Tag or Service Tag contains.
    pub q: Option<String>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

fn query(q: AssetListParams) -> Result<AssetQuery, ApiError> {
    let limit = q
        .limit
        .map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| AssetError::invalid("limit", "must be between 1 and 200"))?;
    let after = q
        .cursor
        .map(|c| Cursor::from_client(c).decode::<Uuid>())
        .transpose()
        .map_err(|_| AssetError::invalid("cursor", "is not valid"))?;
    let holder = match (q.holder_kind, q.holder_id) {
        (Some(k), Some(id)) => Some(AssetHolder::from_parts(&k, id)?),
        (None, None) => None,
        _ => {
            return Err(AssetError::invalid("holder_id", "goes with holder_kind").into());
        }
    };
    Ok(AssetQuery {
        tenant: q.company_id.map(TenantId::from_uuid),
        asset_type_id: q.asset_type_id,
        asset_status_id: q.asset_status_id,
        locations: q.location_id.map(|l| vec![l]),
        holder,
        checked_out: q.checked_out,
        include_retired: q.include_retired.unwrap_or(false),
        retired_only: q.retired_only.unwrap_or(false),
        search: q.q,
        limit,
        after,
    })
}

/// Assets in your Companies, newest first; retired Assets only when asked for (needs
/// `assets.read`).
#[utoipa::path(get, path = "/assets", tag = "Assets", params(AssetListParams), responses((status = 200, body = AssetPage)))]
async fn list_assets(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<AssetListParams>,
) -> ApiResult<Json<AssetPage>> {
    let query = query(q)?;
    let rows = api.service.list(&p, &query).await?;
    let page =
        cooper_kernel::Page::from_rows(rows, query.limit, |a| Cursor::encode(&a.id().as_uuid()))
            .map_err(|_| AppError::Internal)?;
    Ok(Json(AssetPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Registers an Asset (needs `assets.create`).
#[utoipa::path(post, path = "/assets", tag = "Assets", request_body = AssetRequest, responses((status = 201, body = AssetDto)))]
async fn create_asset(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<AssetRequest>,
) -> ApiResult<Response> {
    let company = b
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| AssetError::invalid("company_id", "is required"))?;
    let details = b.details()?;
    Ok(created(AssetDto::from(
        api.service.create(&p, company, details).await?,
    )))
}

/// Active Assets counted by type and status.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetCountDto {
    /// The Asset Type or Asset Status (`null`: none).
    pub id: Option<Uuid>,
    /// Active Assets.
    pub count: u64,
}

/// Counts of the estate (FR-AST-005).
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetSummaryDto {
    /// Assets in the estate.
    pub active: u64,
    /// Retired Assets (not counted as active).
    pub retired: u64,
    /// Active Assets checked out.
    pub checked_out: u64,
    /// Active Assets per Asset Type.
    pub by_type: Vec<AssetCountDto>,
    /// Active Assets per Asset Status.
    pub by_status: Vec<AssetCountDto>,
}

impl From<AssetSummary> for AssetSummaryDto {
    fn from(s: AssetSummary) -> Self {
        let counts = |v: Vec<(Option<Uuid>, u64)>| {
            v.into_iter()
                .map(|(id, count)| AssetCountDto { id, count })
                .collect()
        };
        Self {
            active: s.active,
            retired: s.retired,
            checked_out: s.checked_out,
            by_type: counts(s.by_type),
            by_status: counts(s.by_status),
        }
    }
}

/// Summary filter.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AssetSummaryParams {
    /// One Company (default: all of yours).
    pub company_id: Option<Uuid>,
}

/// Active and retired Asset counts, active Assets per type and status (needs
/// `assets.read`; FR-AST-005).
#[utoipa::path(get, path = "/assets/summary", tag = "Assets", params(AssetSummaryParams), responses((status = 200, body = AssetSummaryDto)))]
async fn asset_summary(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<AssetSummaryParams>,
) -> ApiResult<Json<AssetSummaryDto>> {
    Ok(Json(
        api.service
            .summary(&p, q.company_id.map(TenantId::from_uuid))
            .await?
            .into(),
    ))
}

/// One Asset.
#[utoipa::path(get, path = "/assets/{id}", tag = "Assets", params(("id" = Uuid, Path)), responses((status = 200, body = AssetDto)))]
async fn get_asset(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AssetDto>> {
    Ok(Json(
        api.service.get(&p, AssetId::from_uuid(id)).await?.into(),
    ))
}

/// Edits an Asset (needs `assets.update`).
#[utoipa::path(put, path = "/assets/{id}", tag = "Assets", params(("id" = Uuid, Path)), request_body = AssetRequest, responses((status = 200, body = AssetDto)))]
async fn edit_asset(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<AssetRequest>,
) -> ApiResult<Json<AssetDto>> {
    let details = b.details()?;
    Ok(Json(
        api.service
            .edit(&p, AssetId::from_uuid(id), details)
            .await?
            .into(),
    ))
}

// ----- Check Out / Check In (FR-AST-004) --------------------------------------------------------

/// One entry of the Check Out log.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetCheckoutDto {
    /// ID.
    pub id: Uuid,
    /// The Holder.
    pub holder: AssetHolderDto,
    /// When it was checked out.
    pub checked_out_at: DateTime<Utc>,
    /// Who checked it out (`{kind, id}`).
    #[schema(value_type = Object)]
    pub checked_out_by: serde_json::Value,
    /// Note at Check Out.
    pub checkout_note: String,
    /// Expected return.
    pub due_back_on: Option<NaiveDate>,
    /// When it was checked in (`null`: still out).
    pub checked_in_at: Option<DateTime<Utc>>,
    /// Who checked it in (`{kind, id}`).
    #[schema(value_type = Object)]
    pub checked_in_by: Option<serde_json::Value>,
    /// Note at Check In.
    pub checkin_note: String,
}

fn actor_json(a: cooper_kernel::Actor) -> serde_json::Value {
    serde_json::json!({ "kind": a.kind().as_str(), "id": a.id() })
}

impl From<AssetCheckout> for AssetCheckoutDto {
    fn from(c: AssetCheckout) -> Self {
        Self {
            id: c.id,
            holder: c.holder.into(),
            checked_out_at: c.checked_out_at,
            checked_out_by: actor_json(c.checked_out_by),
            checkout_note: c.checkout_note,
            due_back_on: c.due_back_on,
            checked_in_at: c.checked_in_at,
            checked_in_by: c.checked_in_by.map(actor_json),
            checkin_note: c.checkin_note,
        }
    }
}

/// Checks an Asset out.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssetCheckOutRequest {
    /// `requester` (of the Asset's Company) or `analyst`.
    pub holder_kind: String,
    /// The person.
    pub holder_id: Uuid,
    /// Note.
    #[serde(default)]
    pub note: String,
    /// Expected return.
    pub due_back_on: Option<NaiveDate>,
}

/// Checks an Asset in.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssetCheckInRequest {
    /// Note.
    #[serde(default)]
    pub note: String,
}

/// Checks an Asset out to a Requester or Analyst (needs `assets.update`). A retired or
/// already checked-out Asset cannot be checked out.
#[utoipa::path(post, path = "/assets/{id}/check-out", tag = "Assets", params(("id" = Uuid, Path)), request_body = AssetCheckOutRequest, responses((status = 200, body = AssetDto)))]
async fn check_out_asset(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<AssetCheckOutRequest>,
) -> ApiResult<Json<AssetDto>> {
    let holder = match b.holder_kind.as_str() {
        "requester" => AssetHolder::Requester(RequesterId::from_uuid(b.holder_id)),
        "analyst" => AssetHolder::Analyst(AnalystId::from_uuid(b.holder_id)),
        _ => {
            return Err(AssetError::invalid("holder_kind", "must be requester or analyst").into());
        }
    };
    let request = CheckOutRequest {
        holder,
        note: b.note,
        due_back_on: b.due_back_on,
    };
    let (asset, _) = api
        .service
        .check_out(&p, AssetId::from_uuid(id), request)
        .await?;
    Ok(Json(asset.into()))
}

/// Checks an Asset back in (needs `assets.update`).
#[utoipa::path(post, path = "/assets/{id}/check-in", tag = "Assets", params(("id" = Uuid, Path)), request_body = AssetCheckInRequest, responses((status = 200, body = AssetDto)))]
async fn check_in_asset(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<AssetCheckInRequest>,
) -> ApiResult<Json<AssetDto>> {
    let (asset, _) = api
        .service
        .check_in(&p, AssetId::from_uuid(id), &b.note)
        .await?;
    Ok(Json(asset.into()))
}

/// The Check Out log of an Asset, newest first.
#[utoipa::path(get, path = "/assets/{id}/checkouts", tag = "Assets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<AssetCheckoutDto>)))]
async fn list_asset_checkouts(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<AssetCheckoutDto>>> {
    Ok(Json(
        api.service
            .checkouts(&p, AssetId::from_uuid(id))
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

// ----- Retire (FR-AST-005) ----------------------------------------------------------------------

/// Retires an Asset.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssetRetireRequest {
    /// The day it left the estate (default today).
    pub retired_on: Option<NaiveDate>,
    /// Why.
    pub reason: String,
    /// A retired Asset Status to move to (otherwise the status is cleared).
    pub asset_status_id: Option<Uuid>,
}

/// Retires an Asset with a date and reason (needs `assets.update`). It must be checked
/// in first; retired Assets are excluded from active counts and lists.
#[utoipa::path(post, path = "/assets/{id}/retire", tag = "Assets", params(("id" = Uuid, Path)), request_body = AssetRetireRequest, responses((status = 200, body = AssetDto)))]
async fn retire_asset(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<AssetRetireRequest>,
) -> ApiResult<Json<AssetDto>> {
    let request = RetireRequest {
        retired_on: b.retired_on,
        reason: b.reason,
        asset_status_id: b.asset_status_id,
    };
    Ok(Json(
        api.service
            .retire(&p, AssetId::from_uuid(id), request)
            .await?
            .into(),
    ))
}

// ----- Links and history (FR-AST-006) -----------------------------------------------------------

/// A linked ticket or Contract.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetLinkDto {
    /// `ticket` or `contract`.
    pub kind: String,
    /// The record.
    pub target_id: Uuid,
    /// Its number.
    pub number: String,
    /// When it was linked.
    pub linked_at: DateTime<Utc>,
}

impl From<(AssetLink, String)> for AssetLinkDto {
    fn from((l, number): (AssetLink, String)) -> Self {
        Self {
            kind: l.kind.as_str().to_owned(),
            target_id: l.target_id,
            number,
            linked_at: l.linked_at,
        }
    }
}

/// Links a record.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssetLinkRequest {
    /// `ticket` or `contract`.
    pub kind: String,
    /// The record (same Company).
    pub target_id: Uuid,
}

/// Linked tickets and Contracts you may see (FR-AST-006).
#[utoipa::path(get, path = "/assets/{id}/links", tag = "Assets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<AssetLinkDto>)))]
async fn list_asset_links(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<AssetLinkDto>>> {
    Ok(Json(
        api.service
            .links(&p, AssetId::from_uuid(id))
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Links a ticket or Contract of the same Company (needs `assets.update`).
#[utoipa::path(post, path = "/assets/{id}/links", tag = "Assets", params(("id" = Uuid, Path)), request_body = AssetLinkRequest, responses((status = 201, body = AssetLinkDto)))]
async fn link_asset(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<AssetLinkRequest>,
) -> ApiResult<Response> {
    let kind = b.kind.parse::<LinkKind>()?;
    Ok(created(AssetLinkDto::from(
        api.service
            .link(&p, AssetId::from_uuid(id), kind, b.target_id)
            .await?,
    )))
}

/// Removes a link (needs `assets.update`).
#[utoipa::path(delete, path = "/assets/{id}/links/{kind}/{target_id}", tag = "Assets", params(("id" = Uuid, Path), ("kind" = String, Path), ("target_id" = Uuid, Path)), responses((status = 204)))]
async fn unlink_asset(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path((id, kind, target)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service
        .unlink(
            &p,
            AssetId::from_uuid(id),
            kind.parse::<LinkKind>()?,
            target,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// One change in an Asset's history.
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetHistoryDto {
    /// Audit entry ID.
    pub id: Uuid,
    /// When.
    pub at: DateTime<Utc>,
    /// Who (`{kind, id}`).
    #[schema(value_type = Object)]
    pub actor: serde_json::Value,
    /// `created` or `updated`.
    pub action: String,
    /// Field (`holder` for Check Out / Check In, `retired_on` and `retirement_reason`
    /// for Retire, `ticket_linked` and the like for links).
    pub field: Option<String>,
    /// Before.
    #[schema(value_type = Object)]
    pub old_value: Option<serde_json::Value>,
    /// After.
    #[schema(value_type = Object)]
    pub new_value: Option<serde_json::Value>,
}

/// The full history of an Asset: edits, Check Outs and Check Ins, retirement and links
/// (FR-AST-006).
#[utoipa::path(get, path = "/assets/{id}/history", tag = "Assets", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<AssetHistoryDto>)))]
async fn asset_history(
    State(api): State<AssetsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<AssetHistoryDto>>> {
    Ok(Json(
        api.service
            .history(&p, AssetId::from_uuid(id))
            .await?
            .into_iter()
            .map(|e| AssetHistoryDto {
                id: e.id,
                at: e.occurred_at,
                actor: actor_json(e.actor),
                action: e.action.as_str().to_owned(),
                field: e.field,
                old_value: e.old_value,
                new_value: e.new_value,
            })
            .collect(),
    ))
}
