//! HTTP API of Contracts (FR-CON-001 to FR-CON-004).

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{AnalystId, Cursor, Limit, Page, Principal, RequesterId, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, ContractView, ContractsService};
use crate::domain::{
    Contact, ContactDetails, ContactId, ContractDetails, ContractId, ContractParty, ContractsError,
    DataProtection, Lookup, LookupInput, LookupKind, Money, NoticePeriod, NoticeStatus, NoticeUnit,
    Supplier, SupplierDetails, SupplierId,
};
use crate::ports::{ContractQuery, SupplierQuery};

/// State the handlers need.
#[derive(Clone)]
pub struct ContractsApi {
    /// The service.
    pub service: ContractsService,
}

/// `/api/v1` path prefixes owned by the Contracts Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &[
    "/contracts",
    "/suppliers",
    "/contacts",
    "/supplier-types",
    "/supplier-statuses",
    "/contract-statuses",
    "/payment-schedules",
];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ContractsApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_supplier_types, create_supplier_type))
        .routes(routes!(update_supplier_type))
        .routes(routes!(list_supplier_statuses, create_supplier_status))
        .routes(routes!(update_supplier_status))
        .routes(routes!(list_contract_statuses, create_contract_status))
        .routes(routes!(update_contract_status))
        .routes(routes!(list_payment_schedules, create_payment_schedule))
        .routes(routes!(update_payment_schedule))
        .routes(routes!(list_suppliers, create_supplier))
        .routes(routes!(get_supplier, edit_supplier))
        .routes(routes!(list_supplier_contacts, create_contact))
        .routes(routes!(supplier_history))
        .routes(routes!(get_contact, edit_contact))
        .routes(routes!(list_contracts, create_contract))
        .routes(routes!(get_contract, edit_contract, delete_contract))
        .routes(routes!(contract_history))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<ContractsError> for ApiError {
    fn from(e: ContractsError) -> Self {
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
            AppError::Rule(ContractsError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(ContractsError::InvalidState(_)) => typed(
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

fn limit(v: Option<u32>) -> Result<Limit, ApiError> {
    v.map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| ContractsError::invalid("limit", "must be between 1 and 200").into())
}

fn cursor(v: Option<String>) -> Result<Option<(String, Uuid)>, ApiError> {
    v.map(|c| Cursor::from_client(c).decode::<(String, Uuid)>())
        .transpose()
        .map_err(|_| ContractsError::invalid("cursor", "is not valid").into())
}

fn page<T, D>(
    rows: Vec<T>,
    limit: Limit,
    key: impl Fn(&T) -> (String, Uuid),
    dto: impl Fn(T) -> D,
) -> ApiResult<(Vec<D>, Option<String>)> {
    let page = Page::from_rows(rows, limit, |r| Cursor::encode(&key(r)))
        .map_err(|_| AppError::Internal)?;
    Ok((
        page.items.into_iter().map(dto).collect(),
        page.next_cursor.map(|c| c.as_str().to_owned()),
    ))
}

// ----- Reference data -------------------------------------------------------------------------

/// A Supplier Type, Supplier Status, Contract Status or Payment Schedule.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContractLookupDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Contract Statuses: a Contract in this status is finished (no Calendar Events, no
    /// reminders). Always `false` for the other lookups.
    pub is_closed: bool,
    /// Display order.
    pub sort_order: i32,
    /// Active (only active rows can be chosen).
    pub is_active: bool,
}

impl From<Lookup> for ContractLookupDto {
    fn from(l: Lookup) -> Self {
        Self {
            id: l.id,
            name: l.name,
            is_closed: l.is_closed,
            sort_order: l.sort_order,
            is_active: l.is_active,
        }
    }
}

/// Creates or changes a lookup.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ContractLookupRequest {
    /// Name.
    pub name: String,
    /// Contract Statuses: closed.
    #[serde(default)]
    pub is_closed: bool,
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

impl From<ContractLookupRequest> for LookupInput {
    fn from(r: ContractLookupRequest) -> Self {
        Self {
            name: r.name,
            is_closed: r.is_closed,
            sort_order: r.sort_order,
            is_active: r.is_active,
        }
    }
}

macro_rules! lookup_handlers {
    ($kind:expr, $path:literal, $item:literal, $list:ident, $create:ident, $update:ident) => {
        #[utoipa::path(get, path = $path, tag = "Contracts", responses((status = 200, body = Vec<ContractLookupDto>)))]
        async fn $list(State(api): State<ContractsApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<ContractLookupDto>>> {
            Ok(Json(api.service.lookups(&p, $kind).await?.into_iter().map(Into::into).collect()))
        }

        #[utoipa::path(post, path = $path, tag = "Contracts", request_body = ContractLookupRequest, responses((status = 201, body = ContractLookupDto)))]
        async fn $create(State(api): State<ContractsApi>, Authenticated(p): Authenticated, Json(b): Json<ContractLookupRequest>) -> ApiResult<Response> {
            Ok(created(ContractLookupDto::from(api.service.create_lookup(&p, $kind, b.into()).await?)))
        }

        #[utoipa::path(put, path = $item, tag = "Contracts", params(("id" = Uuid, Path)), request_body = ContractLookupRequest, responses((status = 200, body = ContractLookupDto)))]
        async fn $update(
            State(api): State<ContractsApi>,
            Authenticated(p): Authenticated,
            Path(id): Path<Uuid>,
            Json(b): Json<ContractLookupRequest>,
        ) -> ApiResult<Json<ContractLookupDto>> {
            Ok(Json(api.service.update_lookup(&p, $kind, id, b.into()).await?.into()))
        }
    };
}

lookup_handlers!(
    LookupKind::SupplierType,
    "/supplier-types",
    "/supplier-types/{id}",
    list_supplier_types,
    create_supplier_type,
    update_supplier_type
);
lookup_handlers!(
    LookupKind::SupplierStatus,
    "/supplier-statuses",
    "/supplier-statuses/{id}",
    list_supplier_statuses,
    create_supplier_status,
    update_supplier_status
);
lookup_handlers!(
    LookupKind::ContractStatus,
    "/contract-statuses",
    "/contract-statuses/{id}",
    list_contract_statuses,
    create_contract_status,
    update_contract_status
);
lookup_handlers!(
    LookupKind::PaymentSchedule,
    "/payment-schedules",
    "/payment-schedules/{id}",
    list_payment_schedules,
    create_payment_schedule,
    update_payment_schedule
);

// ----- History --------------------------------------------------------------------------------

/// One change in an audit trail.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContractHistoryDto {
    /// When.
    pub at: DateTime<Utc>,
    /// Who (`{kind, id}`).
    #[schema(value_type = Object)]
    pub actor: serde_json::Value,
    /// `created`, `updated` or `deleted`.
    pub action: String,
    /// Field.
    pub field: Option<String>,
    /// Before.
    #[schema(value_type = Object)]
    pub old_value: Option<serde_json::Value>,
    /// After.
    #[schema(value_type = Object)]
    pub new_value: Option<serde_json::Value>,
}

async fn history(
    api: &ContractsApi,
    p: &Principal,
    kind: &'static str,
    id: Uuid,
) -> ApiResult<Json<Vec<ContractHistoryDto>>> {
    Ok(Json(
        api.service
            .history(p, kind, id)
            .await?
            .into_iter()
            .map(|e| ContractHistoryDto {
                at: e.occurred_at,
                actor: serde_json::json!({ "kind": e.actor.kind().as_str(), "id": e.actor.id() }),
                action: e.action.as_str().to_owned(),
                field: e.field,
                old_value: e.old_value,
                new_value: e.new_value,
            })
            .collect(),
    ))
}

// ----- Suppliers (FR-CON-001) -------------------------------------------------------------------

/// A Supplier.
#[derive(Debug, Serialize, ToSchema)]
pub struct SupplierDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Legal name.
    pub legal_name: String,
    /// Trading name.
    pub trading_name: Option<String>,
    /// Company registration number.
    pub registration_number: Option<String>,
    /// VAT number.
    pub vat_number: Option<String>,
    /// Supplier Type.
    pub supplier_type_id: Option<Uuid>,
    /// Supplier Status.
    pub supplier_status_id: Option<Uuid>,
    /// Notes.
    pub notes: String,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Supplier> for SupplierDto {
    fn from(s: Supplier) -> Self {
        let d = s.details().clone();
        Self {
            id: s.id().as_uuid(),
            company_id: s.tenant_id().as_uuid(),
            legal_name: d.legal_name,
            trading_name: d.trading_name,
            registration_number: d.registration_number,
            vat_number: d.vat_number,
            supplier_type_id: d.supplier_type_id,
            supplier_status_id: d.supplier_status_id,
            notes: d.notes,
            is_active: d.is_active,
            created_at: s.created_at(),
            updated_at: s.updated_at(),
        }
    }
}

/// Creates or edits a Supplier.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SupplierRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Legal name.
    pub legal_name: String,
    /// Trading name.
    pub trading_name: Option<String>,
    /// Company registration number.
    pub registration_number: Option<String>,
    /// VAT number.
    pub vat_number: Option<String>,
    /// Supplier Type.
    pub supplier_type_id: Option<Uuid>,
    /// Supplier Status.
    pub supplier_status_id: Option<Uuid>,
    /// Notes.
    #[serde(default)]
    pub notes: String,
    /// Active.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl SupplierRequest {
    fn details(self) -> SupplierDetails {
        SupplierDetails {
            legal_name: self.legal_name,
            trading_name: self.trading_name,
            registration_number: self.registration_number,
            vat_number: self.vat_number,
            supplier_type_id: self.supplier_type_id,
            supplier_status_id: self.supplier_status_id,
            notes: self.notes,
            is_active: self.is_active,
        }
    }
}

/// A page of Suppliers.
#[derive(Debug, Serialize, ToSchema)]
pub struct SupplierPage {
    /// Suppliers by legal name.
    pub items: Vec<SupplierDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Supplier filters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SupplierListParams {
    /// Company.
    pub company_id: Option<Uuid>,
    /// Text in the legal or trading name.
    pub q: Option<String>,
    /// Supplier Type.
    pub supplier_type_id: Option<Uuid>,
    /// Supplier Status.
    pub supplier_status_id: Option<Uuid>,
    /// Active (`true`) or inactive (`false`).
    pub active: Option<bool>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

/// Suppliers in your Companies by legal name (needs `contracts.read`).
#[utoipa::path(get, path = "/suppliers", tag = "Contracts", params(SupplierListParams), responses((status = 200, body = SupplierPage)))]
async fn list_suppliers(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<SupplierListParams>,
) -> ApiResult<Json<SupplierPage>> {
    let query = SupplierQuery {
        tenant_id: q.company_id.map(TenantId::from_uuid),
        search: q.q,
        supplier_type_id: q.supplier_type_id,
        supplier_status_id: q.supplier_status_id,
        active: q.active,
        limit: limit(q.limit)?,
        after: cursor(q.cursor)?,
    };
    let rows = api.service.list_suppliers(&p, &query).await?;
    let (items, next_cursor) = page(
        rows,
        query.limit,
        |s| (s.details().legal_name.clone(), s.id().as_uuid()),
        SupplierDto::from,
    )?;
    Ok(Json(SupplierPage { items, next_cursor }))
}

/// Creates a Supplier (needs `contracts.write`).
#[utoipa::path(post, path = "/suppliers", tag = "Contracts", request_body = SupplierRequest, responses((status = 201, body = SupplierDto)))]
async fn create_supplier(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<SupplierRequest>,
) -> ApiResult<Response> {
    let tenant = b
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| ContractsError::invalid("company_id", "is required"))?;
    let s = api.service.create_supplier(&p, tenant, b.details()).await?;
    Ok(created(SupplierDto::from(s)))
}

/// One Supplier.
#[utoipa::path(get, path = "/suppliers/{id}", tag = "Contracts", params(("id" = Uuid, Path)), responses((status = 200, body = SupplierDto)))]
async fn get_supplier(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SupplierDto>> {
    Ok(Json(
        api.service
            .get_supplier(&p, SupplierId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Edits a Supplier (needs `contracts.write`).
#[utoipa::path(put, path = "/suppliers/{id}", tag = "Contracts", params(("id" = Uuid, Path)), request_body = SupplierRequest, responses((status = 200, body = SupplierDto)))]
async fn edit_supplier(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<SupplierRequest>,
) -> ApiResult<Json<SupplierDto>> {
    Ok(Json(
        api.service
            .edit_supplier(&p, SupplierId::from_uuid(id), b.details())
            .await?
            .into(),
    ))
}

/// The audit trail of a Supplier.
#[utoipa::path(get, path = "/suppliers/{id}/history", tag = "Contracts", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ContractHistoryDto>)))]
async fn supplier_history(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ContractHistoryDto>>> {
    history(&api, &p, "supplier", id).await
}

// ----- Contacts (FR-CON-001) --------------------------------------------------------------------

/// A Contact: a person at a Supplier.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContactDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Supplier.
    pub supplier_id: Uuid,
    /// Full name.
    pub full_name: String,
    /// Job title.
    pub job_title: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Phone.
    pub phone: Option<String>,
    /// Mobile.
    pub mobile: Option<String>,
    /// Notes.
    pub notes: String,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Contact> for ContactDto {
    fn from(c: Contact) -> Self {
        let d = c.details().clone();
        Self {
            id: c.id().as_uuid(),
            company_id: c.tenant_id().as_uuid(),
            supplier_id: c.supplier_id().as_uuid(),
            full_name: d.full_name,
            job_title: d.job_title,
            email: d.email,
            phone: d.phone,
            mobile: d.mobile,
            notes: d.notes,
            is_active: d.is_active,
            created_at: c.created_at(),
            updated_at: c.updated_at(),
        }
    }
}

/// Creates or edits a Contact.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ContactRequest {
    /// Full name.
    pub full_name: String,
    /// Job title.
    pub job_title: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Phone.
    pub phone: Option<String>,
    /// Mobile.
    pub mobile: Option<String>,
    /// Notes.
    #[serde(default)]
    pub notes: String,
    /// Active.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl From<ContactRequest> for ContactDetails {
    fn from(r: ContactRequest) -> Self {
        Self {
            full_name: r.full_name,
            job_title: r.job_title,
            email: r.email,
            phone: r.phone,
            mobile: r.mobile,
            notes: r.notes,
            is_active: r.is_active,
        }
    }
}

/// The Contacts of a Supplier by name (needs `contracts.read`).
#[utoipa::path(get, path = "/suppliers/{id}/contacts", tag = "Contracts", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ContactDto>)))]
async fn list_supplier_contacts(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ContactDto>>> {
    Ok(Json(
        api.service
            .contacts(&p, SupplierId::from_uuid(id))
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Adds a Contact to a Supplier (needs `contracts.write`).
#[utoipa::path(post, path = "/suppliers/{id}/contacts", tag = "Contracts", params(("id" = Uuid, Path)), request_body = ContactRequest, responses((status = 201, body = ContactDto)))]
async fn create_contact(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ContactRequest>,
) -> ApiResult<Response> {
    let c = api
        .service
        .create_contact(&p, SupplierId::from_uuid(id), b.into())
        .await?;
    Ok(created(ContactDto::from(c)))
}

/// One Contact.
#[utoipa::path(get, path = "/contacts/{id}", tag = "Contracts", params(("id" = Uuid, Path)), responses((status = 200, body = ContactDto)))]
async fn get_contact(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ContactDto>> {
    Ok(Json(
        api.service
            .get_contact(&p, ContactId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Edits a Contact (needs `contracts.write`).
#[utoipa::path(put, path = "/contacts/{id}", tag = "Contracts", params(("id" = Uuid, Path)), request_body = ContactRequest, responses((status = 200, body = ContactDto)))]
async fn edit_contact(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ContactRequest>,
) -> ApiResult<Json<ContactDto>> {
    Ok(Json(
        api.service
            .edit_contact(&p, ContactId::from_uuid(id), b.into())
            .await?
            .into(),
    ))
}

// ----- Contracts (FR-CON-002 to FR-CON-004) ---------------------------------------------------

/// The unit of a notice period.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoticePeriodUnit {
    /// Calendar days.
    Days,
    /// Calendar months (a Contract ending on a month end gives notice on a month end).
    Months,
}

/// How long before the end date notice must be given.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct NoticePeriodDto {
    /// Number of days (0 to 3650) or months (0 to 120).
    pub amount: u32,
    /// Days or months.
    pub unit: NoticePeriodUnit,
}

impl From<NoticePeriod> for NoticePeriodDto {
    fn from(p: NoticePeriod) -> Self {
        Self {
            amount: u32::from(p.amount()),
            unit: match p.unit() {
                NoticeUnit::Days => NoticePeriodUnit::Days,
                NoticeUnit::Months => NoticePeriodUnit::Months,
            },
        }
    }
}

/// Where a Contract's Notice Date stands today.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoticeStatusDto {
    /// No Notice Date, or the Contract is closed.
    None,
    /// Further away than `contracts.reminder_days`.
    Later,
    /// From today up to `contracts.reminder_days` ahead.
    Upcoming,
    /// Passed.
    Passed,
}

impl From<NoticeStatus> for NoticeStatusDto {
    fn from(s: NoticeStatus) -> Self {
        match s {
            NoticeStatus::None => Self::None,
            NoticeStatus::Later => Self::Later,
            NoticeStatus::Upcoming => Self::Upcoming,
            NoticeStatus::Passed => Self::Passed,
        }
    }
}

/// A Contract.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContractDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Contract number (unique per Company).
    pub contract_number: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// `supplier` (with `supplier_id`) or `customer` (the Company, optionally with
    /// `requester_id`).
    pub party: String,
    /// Supplier.
    pub supplier_id: Option<Uuid>,
    /// Customer Requester.
    pub requester_id: Option<Uuid>,
    /// Owner Analyst.
    pub owner_id: Option<Uuid>,
    /// Contract Status.
    pub status_id: Option<Uuid>,
    /// Neither deleted nor in a closed Contract Status.
    pub is_active: bool,
    /// Start date.
    pub start_date: Option<NaiveDate>,
    /// End date.
    pub end_date: Option<NaiveDate>,
    /// Notice period.
    pub notice_period: Option<NoticePeriodDto>,
    /// Notice Date, calculated by Cooper: the end date minus the notice period.
    pub notice_date: Option<NaiveDate>,
    /// Where the Notice Date stands today.
    pub notice_status: NoticeStatusDto,
    /// Value as a decimal string with two places, for example `"1250.00"`.
    pub value: Option<String>,
    /// ISO 4217 currency of the value.
    pub currency: Option<String>,
    /// Payment Schedule.
    pub payment_schedule_id: Option<Uuid>,
    /// Cost Centre.
    pub cost_centre_id: Option<Uuid>,
    /// Link to the document management system.
    pub dms_link: Option<String>,
    /// Personal data is transferred (FR-CON-004).
    pub personal_data_transferred: bool,
    /// A DPIA is required.
    pub dpia_required: bool,
    /// When the DPIA was completed.
    pub dpia_completed_on: Option<NaiveDate>,
    /// Link to the DPIA.
    pub dpia_link: Option<String>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<ContractView> for ContractDto {
    fn from(v: ContractView) -> Self {
        let c = v.contract;
        let notice_date = c.notice_date();
        let is_active = c.is_active();
        let d = c.details().clone();
        let dp = d.data_protection;
        Self {
            id: c.id().as_uuid(),
            company_id: c.tenant_id().as_uuid(),
            contract_number: d.contract_number,
            title: d.title,
            description: d.description,
            party: d.party.kind().to_owned(),
            supplier_id: d.party.supplier_id().map(|s| s.as_uuid()),
            requester_id: d.party.requester_id().map(|r| r.as_uuid()),
            owner_id: d.owner_id.map(|a| a.as_uuid()),
            status_id: d.status_id,
            is_active,
            start_date: d.start_date,
            end_date: d.end_date,
            notice_period: d.notice_period.map(Into::into),
            notice_date,
            notice_status: v.notice_status.into(),
            value: d.value.as_ref().map(Money::amount),
            currency: d.value.as_ref().map(|m| m.currency().as_str().to_owned()),
            payment_schedule_id: d.payment_schedule_id,
            cost_centre_id: d.cost_centre_id,
            dms_link: d.dms_link,
            personal_data_transferred: dp.personal_data_transferred,
            dpia_required: dp.dpia_required,
            dpia_completed_on: dp.dpia_completed_on,
            dpia_link: dp.dpia_link,
            created_at: c.created_at(),
            updated_at: c.updated_at(),
        }
    }
}

/// Creates or edits a Contract.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ContractRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Contract number (unique per Company).
    pub contract_number: String,
    /// Title.
    pub title: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Supplier: the Contract is with this Supplier. Leave empty for a customer Contract.
    pub supplier_id: Option<Uuid>,
    /// Customer Requester (customer Contracts only).
    pub requester_id: Option<Uuid>,
    /// Owner Analyst.
    pub owner_id: Option<Uuid>,
    /// Contract Status.
    pub status_id: Option<Uuid>,
    /// Start date.
    pub start_date: Option<NaiveDate>,
    /// End date.
    pub end_date: Option<NaiveDate>,
    /// Notice period.
    pub notice_period: Option<NoticePeriodDto>,
    /// Value as a decimal string (at most two decimal places).
    pub value: Option<String>,
    /// ISO 4217 currency (required with a value).
    pub currency: Option<String>,
    /// Payment Schedule.
    pub payment_schedule_id: Option<Uuid>,
    /// Cost Centre of the Company.
    pub cost_centre_id: Option<Uuid>,
    /// Link to the document management system (`http(s)://`).
    pub dms_link: Option<String>,
    /// Personal data is transferred.
    #[serde(default)]
    pub personal_data_transferred: bool,
    /// A DPIA is required.
    #[serde(default)]
    pub dpia_required: bool,
    /// When the DPIA was completed (needs `dpia_required`).
    pub dpia_completed_on: Option<NaiveDate>,
    /// Link to the DPIA (needs `dpia_required`).
    pub dpia_link: Option<String>,
}

impl ContractRequest {
    fn details(self) -> Result<ContractDetails, ApiError> {
        let party = match (self.supplier_id, self.requester_id) {
            (Some(_), Some(_)) => {
                return Err(ContractsError::invalid(
                    "requester_id",
                    "a Supplier Contract has no customer Requester",
                )
                .into());
            }
            (Some(s), None) => ContractParty::Supplier(SupplierId::from_uuid(s)),
            (None, r) => ContractParty::Customer(r.map(RequesterId::from_uuid)),
        };
        let notice_period = self
            .notice_period
            .map(|p| {
                NoticePeriod::new(
                    p.amount,
                    match p.unit {
                        NoticePeriodUnit::Days => NoticeUnit::Days,
                        NoticePeriodUnit::Months => NoticeUnit::Months,
                    },
                )
            })
            .transpose()?;
        let value = match (
            self.value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty()),
            self.currency
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty()),
        ) {
            (Some(v), Some(c)) => Some(Money::parse(v, c)?),
            (None, None) => None,
            (Some(_), None) => {
                return Err(ContractsError::invalid("currency", "is required with a value").into());
            }
            (None, Some(_)) => {
                return Err(ContractsError::invalid("value", "is required with a currency").into());
            }
        };
        Ok(ContractDetails {
            contract_number: self.contract_number,
            title: self.title,
            description: self.description,
            party,
            owner_id: self.owner_id.map(AnalystId::from_uuid),
            status_id: self.status_id,
            start_date: self.start_date,
            end_date: self.end_date,
            notice_period,
            value,
            payment_schedule_id: self.payment_schedule_id,
            cost_centre_id: self.cost_centre_id,
            dms_link: self.dms_link,
            data_protection: DataProtection {
                personal_data_transferred: self.personal_data_transferred,
                dpia_required: self.dpia_required,
                dpia_completed_on: self.dpia_completed_on,
                dpia_link: self.dpia_link,
            },
        })
    }
}

/// A page of Contracts.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContractPage {
    /// Contracts by number.
    pub items: Vec<ContractDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Contract filters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ContractListParams {
    /// Company.
    pub company_id: Option<Uuid>,
    /// Text in the number or title.
    pub q: Option<String>,
    /// Contract Status.
    pub status_id: Option<Uuid>,
    /// Supplier.
    pub supplier_id: Option<Uuid>,
    /// Owner (`me` is accepted).
    pub owner: Option<String>,
    /// Active (`true`) or in a closed Contract Status (`false`).
    pub active: Option<bool>,
    /// Only active Contracts whose Notice Date is from today up to
    /// `contracts.reminder_days` ahead.
    pub notice_due: Option<bool>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

fn owner_param(p: &Principal, v: Option<&str>) -> Result<Option<AnalystId>, ApiError> {
    match v {
        None => Ok(None),
        Some("me") => match p.actor() {
            cooper_kernel::Actor::Analyst(a) => Ok(Some(a)),
            _ => Err(ContractsError::invalid("owner", "`me` needs an Analyst").into()),
        },
        Some(id) => Ok(Some(AnalystId::from_uuid(id.parse().map_err(|_| {
            ContractsError::invalid("owner", "must be an ID or `me`")
        })?))),
    }
}

/// Contracts in your Companies by number, with their Notice Date and where it stands
/// (needs `contracts.read`).
#[utoipa::path(get, path = "/contracts", tag = "Contracts", params(ContractListParams), responses((status = 200, body = ContractPage)))]
async fn list_contracts(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ContractListParams>,
) -> ApiResult<Json<ContractPage>> {
    let query = ContractQuery {
        tenant_id: q.company_id.map(TenantId::from_uuid),
        search: q.q,
        status_id: q.status_id,
        supplier_id: q.supplier_id.map(SupplierId::from_uuid),
        owner_id: owner_param(&p, q.owner.as_deref())?,
        active: q.active,
        notice_between: None,
        limit: limit(q.limit)?,
        after: cursor(q.cursor)?,
    };
    let size = query.limit;
    let rows = api
        .service
        .list_contracts(&p, query, q.notice_due.unwrap_or(false))
        .await?;
    let (items, next_cursor) = page(
        rows,
        size,
        |v| {
            (
                v.contract.details().contract_number.clone(),
                v.contract.id().as_uuid(),
            )
        },
        ContractDto::from,
    )?;
    Ok(Json(ContractPage { items, next_cursor }))
}

/// Creates a Contract (needs `contracts.write`). Cooper calculates its Notice Date.
#[utoipa::path(post, path = "/contracts", tag = "Contracts", request_body = ContractRequest, responses((status = 201, body = ContractDto)))]
async fn create_contract(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<ContractRequest>,
) -> ApiResult<Response> {
    let tenant = b
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| ContractsError::invalid("company_id", "is required"))?;
    let v = api
        .service
        .create_contract(&p, tenant, b.details()?)
        .await?;
    Ok(created(ContractDto::from(v)))
}

/// One Contract.
#[utoipa::path(get, path = "/contracts/{id}", tag = "Contracts", params(("id" = Uuid, Path)), responses((status = 200, body = ContractDto)))]
async fn get_contract(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ContractDto>> {
    Ok(Json(
        api.service
            .get_contract(&p, ContractId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Edits a Contract (needs `contracts.write`).
#[utoipa::path(put, path = "/contracts/{id}", tag = "Contracts", params(("id" = Uuid, Path)), request_body = ContractRequest, responses((status = 200, body = ContractDto)))]
async fn edit_contract(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ContractRequest>,
) -> ApiResult<Json<ContractDto>> {
    Ok(Json(
        api.service
            .edit_contract(&p, ContractId::from_uuid(id), b.details()?)
            .await?
            .into(),
    ))
}

/// Deletes a Contract (needs `contracts.write`).
#[utoipa::path(delete, path = "/contracts/{id}", tag = "Contracts", params(("id" = Uuid, Path)), responses((status = 204)))]
async fn delete_contract(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api.service
        .delete_contract(&p, ContractId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The audit trail of a Contract.
#[utoipa::path(get, path = "/contracts/{id}/history", tag = "Contracts", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ContractHistoryDto>)))]
async fn contract_history(
    State(api): State<ContractsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ContractHistoryDto>>> {
    history(&api, &p, "contract", id).await
}
