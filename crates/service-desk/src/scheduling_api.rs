//! HTTP API of scheduled work (FR-TKT-031) and the rota (FR-SLA-008; ADR-0042).

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{AnalystId, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::scheduling::{RotaEntry, RotaLocation, ScheduledWork, SchedulingError, Shift, Span};
use crate::scheduling_service::{RotaPlacement, RotaService, ScheduledWorkService, SchedulingAppError, WorkQuery};

/// State the handlers need.
#[derive(Clone)]
pub struct SchedulingApi {
    /// Scheduled work.
    pub work: ScheduledWorkService,
    /// The rota.
    pub rota: RotaService,
}

/// `/api/v1` path prefixes of scheduling (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &["/scheduled-work", "/rota"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    SchedulingApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_ticket_work, schedule_work))
        .routes(routes!(list_work))
        .routes(routes!(reschedule_work))
        .routes(routes!(cancel_work))
        .routes(routes!(list_shifts, create_shift))
        .routes(routes!(list_locations, create_location))
        .routes(routes!(rota_day, schedule_rota))
}

/// An error as problem+json.
pub struct ApiError(pub SchedulingAppError);

impl From<SchedulingAppError> for ApiError {
    fn from(e: SchedulingAppError) -> Self {
        Self(e)
    }
}

impl From<SchedulingError> for ApiError {
    fn from(e: SchedulingError) -> Self {
        Self(SchedulingAppError::Invalid(e))
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
            SchedulingAppError::Invalid(SchedulingError::Invalid { field, .. }) => {
                typed(StatusCode::UNPROCESSABLE_ENTITY, "validation", "Invalid input").with_extension("field", field)
            }
            SchedulingAppError::Forbidden(f) => forbidden(&f),
            SchedulingAppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            SchedulingAppError::Conflict(_) => typed(StatusCode::CONFLICT, "conflict", "Conflict"),
            SchedulingAppError::Desk(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            SchedulingAppError::Internal => ProblemJson::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ----- DTOs -------------------------------------------------------------------------------

/// Scheduled work on a ticket.
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// The ticket.
    pub ticket_id: Uuid,
    /// The Owner (an Analyst).
    pub owner_id: Uuid,
    /// Whole days (true) or a time range.
    pub all_day: bool,
    /// First day.
    pub starts_on: NaiveDate,
    /// Last day.
    pub ends_on: NaiveDate,
    /// Start instant, for timed work.
    pub starts_at: Option<DateTime<Utc>>,
    /// End instant, for timed work.
    pub ends_at: Option<DateTime<Utc>>,
    /// Notes.
    pub notes: String,
    /// False once cancelled.
    pub active: bool,
}

impl From<&ScheduledWork> for WorkDto {
    fn from(w: &ScheduledWork) -> Self {
        let (starts_at, ends_at) = match w.span {
            Span::Timed { starts_at, ends_at } => (Some(starts_at), Some(ends_at)),
            Span::AllDay { .. } => (None, None),
        };
        Self {
            id: w.id,
            company_id: w.tenant_id,
            ticket_id: w.ticket_id,
            owner_id: w.owner.as_uuid(),
            all_day: w.span.all_day(),
            starts_on: w.span.first_day(),
            ends_on: w.span.last_day(),
            starts_at,
            ends_at,
            notes: w.notes.clone(),
            active: w.active,
        }
    }
}

/// Scheduled work to make or change.
#[derive(Debug, Deserialize, ToSchema)]
pub struct WorkRequest {
    /// The Owner (an active Analyst).
    pub owner_id: Uuid,
    /// Whole days.
    #[serde(default)]
    pub all_day: bool,
    /// First day (whole days).
    pub starts_on: Option<NaiveDate>,
    /// Last day (whole days; defaults to the first).
    pub ends_on: Option<NaiveDate>,
    /// Start (timed work).
    pub starts_at: Option<DateTime<Utc>>,
    /// End (timed work).
    pub ends_at: Option<DateTime<Utc>>,
    /// Notes.
    #[serde(default)]
    pub notes: String,
}

fn span_of(r: &WorkRequest) -> Result<Span, SchedulingError> {
    if r.all_day {
        let first = r.starts_on.ok_or_else(|| SchedulingError::Invalid { field: "starts_on", reason: "is required for whole days".into() })?;
        let last = r.ends_on.unwrap_or(first);
        let at = |d: NaiveDate| d.and_hms_opt(0, 0, 0).map(|t| t.and_utc()).unwrap_or_default();
        Span::new(true, at(first), at(last))
    } else {
        let (Some(a), Some(b)) = (r.starts_at, r.ends_at) else {
            return Err(SchedulingError::Invalid { field: "starts_at", reason: "and ends_at are required for timed work".into() });
        };
        Span::new(false, a, b)
    }
}

/// A list of scheduled work.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct WorkListQuery {
    /// Only this Company.
    pub company_id: Option<Uuid>,
    /// Only this Owner.
    pub owner_id: Option<Uuid>,
    /// Work that ends on or after this day.
    pub from: Option<NaiveDate>,
    /// Work that starts on or before this day.
    pub to: Option<NaiveDate>,
}

/// A shift.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ShiftDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Start time (HH:MM:SS).
    pub starts_at: NaiveTime,
    /// End time (HH:MM:SS; may be earlier than the start for a night shift).
    pub ends_at: NaiveTime,
    /// Active.
    pub active: bool,
}

impl From<&Shift> for ShiftDto {
    fn from(s: &Shift) -> Self {
        Self { id: s.id, name: s.name.clone(), starts_at: s.starts_at, ends_at: s.ends_at, active: s.active }
    }
}

/// A shift to define.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ShiftRequest {
    /// Company.
    pub company_id: Uuid,
    /// Name.
    pub name: String,
    /// Start time.
    pub starts_at: NaiveTime,
    /// End time.
    pub ends_at: NaiveTime,
}

/// A rota location.
#[derive(Debug, Serialize, ToSchema)]
pub struct LocationDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Active.
    pub active: bool,
}

impl From<&RotaLocation> for LocationDto {
    fn from(l: &RotaLocation) -> Self {
        Self { id: l.id, name: l.name.clone(), active: l.active }
    }
}

/// A location to define.
#[derive(Debug, Deserialize, ToSchema)]
pub struct LocationRequest {
    /// Company.
    pub company_id: Uuid,
    /// Name, such as Office or WFH.
    pub name: String,
}

/// A rota entry.
#[derive(Debug, Serialize, ToSchema)]
pub struct RotaEntryDto {
    /// ID.
    pub id: Uuid,
    /// The Analyst.
    pub analyst_id: Uuid,
    /// The day.
    pub work_date: NaiveDate,
    /// The shift.
    pub shift_id: Uuid,
    /// The location.
    pub location_id: Uuid,
    /// On call.
    pub on_call: bool,
}

impl From<&RotaEntry> for RotaEntryDto {
    fn from(e: &RotaEntry) -> Self {
        Self { id: e.id, analyst_id: e.analyst.as_uuid(), work_date: e.work_date, shift_id: e.shift_id, location_id: e.location_id, on_call: e.on_call }
    }
}

/// The rota for a range of days.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RotaQuery {
    /// Company.
    pub company_id: Uuid,
    /// First day (default today, UTC).
    pub from: Option<NaiveDate>,
    /// Last day (default the first day).
    pub to: Option<NaiveDate>,
}

/// An Analyst to put on the rota.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RotaEntryRequest {
    /// Company.
    pub company_id: Uuid,
    /// The Analyst.
    pub analyst_id: Uuid,
    /// The day.
    pub work_date: NaiveDate,
    /// The shift (of the Company).
    pub shift_id: Uuid,
    /// The location (of the Company).
    pub location_id: Uuid,
    /// On call (default false).
    #[serde(default)]
    pub on_call: bool,
}

// ----- Handlers: scheduled work -----------------------------------------------------------

/// Scheduled work on a ticket (needs visibility of the ticket).
#[utoipa::path(get, path = "/tickets/{id}/scheduled-work", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = [WorkDto])))]
async fn list_ticket_work(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<WorkDto>>> {
    let rows = api.work.for_ticket(&p, id).await?;
    Ok(Json(rows.iter().map(WorkDto::from).collect()))
}

/// Schedules work on a ticket for an Owner (needs `tickets.update`).
#[utoipa::path(post, path = "/tickets/{id}/scheduled-work", tag = "Tickets", params(("id" = Uuid, Path)), request_body = WorkRequest, responses((status = 201, body = WorkDto)))]
async fn schedule_work(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<WorkRequest>) -> ApiResult<Response> {
    let span = span_of(&b)?;
    let w = api.work.schedule(&p, id, AnalystId::from_uuid(b.owner_id), span, &b.notes).await?;
    Ok((StatusCode::CREATED, Json(WorkDto::from(&w))).into_response())
}

/// Scheduled work, by Owner and day range (needs `tickets.read`).
#[utoipa::path(get, path = "/scheduled-work", tag = "Tickets", params(WorkListQuery), responses((status = 200, body = [WorkDto])))]
async fn list_work(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Query(q): Query<WorkListQuery>) -> ApiResult<Json<Vec<WorkDto>>> {
    let rows = api
        .work
        .list(&p, WorkQuery {
            tenant_id: q.company_id.map(TenantId::from_uuid),
            owner: q.owner_id.map(AnalystId::from_uuid),
            ticket_id: None,
            from: q.from,
            to: q.to,
        })
        .await?;
    Ok(Json(rows.iter().map(WorkDto::from).collect()))
}

/// Moves or changes scheduled work (needs `tickets.update`).
#[utoipa::path(put, path = "/scheduled-work/{id}", tag = "Tickets", params(("id" = Uuid, Path)), request_body = WorkRequest, responses((status = 200, body = WorkDto)))]
async fn reschedule_work(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<WorkRequest>) -> ApiResult<Json<WorkDto>> {
    let span = span_of(&b)?;
    let w = api.work.reschedule(&p, id, AnalystId::from_uuid(b.owner_id), span, &b.notes).await?;
    Ok(Json(WorkDto::from(&w)))
}

/// Cancels scheduled work; it stays in the history (needs `tickets.update`).
#[utoipa::path(post, path = "/scheduled-work/{id}/cancel", tag = "Tickets", params(("id" = Uuid, Path)), responses((status = 200, body = WorkDto)))]
async fn cancel_work(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<WorkDto>> {
    let w = api.work.cancel(&p, id).await?;
    Ok(Json(WorkDto::from(&w)))
}

// ----- Handlers: rota ---------------------------------------------------------------------

/// Shifts of a Company (needs `rota.read`).
#[utoipa::path(get, path = "/rota/shifts", tag = "Rota", params(("company_id" = Uuid, Query)), responses((status = 200, body = [ShiftDto])))]
async fn list_shifts(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Query(q): Query<CompanyQuery>) -> ApiResult<Json<Vec<ShiftDto>>> {
    let rows = api.rota.shifts(&p, TenantId::from_uuid(q.company_id)).await?;
    Ok(Json(rows.iter().map(ShiftDto::from).collect()))
}

/// Defines a shift (needs `rota.configure`).
#[utoipa::path(post, path = "/rota/shifts", tag = "Rota", request_body = ShiftRequest, responses((status = 201, body = ShiftDto)))]
async fn create_shift(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Json(b): Json<ShiftRequest>) -> ApiResult<Response> {
    let s = api.rota.create_shift(&p, TenantId::from_uuid(b.company_id), &b.name, b.starts_at, b.ends_at).await?;
    Ok((StatusCode::CREATED, Json(ShiftDto::from(&s))).into_response())
}

/// Locations of a Company (needs `rota.read`).
#[utoipa::path(get, path = "/rota/locations", tag = "Rota", params(("company_id" = Uuid, Query)), responses((status = 200, body = [LocationDto])))]
async fn list_locations(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Query(q): Query<CompanyQuery>) -> ApiResult<Json<Vec<LocationDto>>> {
    let rows = api.rota.locations(&p, TenantId::from_uuid(q.company_id)).await?;
    Ok(Json(rows.iter().map(LocationDto::from).collect()))
}

/// Defines a location, such as Office or WFH (needs `rota.configure`).
#[utoipa::path(post, path = "/rota/locations", tag = "Rota", request_body = LocationRequest, responses((status = 201, body = LocationDto)))]
async fn create_location(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Json(b): Json<LocationRequest>) -> ApiResult<Response> {
    let l = api.rota.create_location(&p, TenantId::from_uuid(b.company_id), &b.name).await?;
    Ok((StatusCode::CREATED, Json(LocationDto::from(&l))).into_response())
}

/// Who is on the rota on a day, or in a range (needs `rota.read`).
#[utoipa::path(get, path = "/rota", tag = "Rota", params(RotaQuery), responses((status = 200, body = [RotaEntryDto])))]
async fn rota_day(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Query(q): Query<RotaQuery>) -> ApiResult<Json<Vec<RotaEntryDto>>> {
    let from = q.from.unwrap_or_else(|| Utc::now().date_naive());
    let to = q.to.unwrap_or(from);
    let rows = api.rota.entries(&p, TenantId::from_uuid(q.company_id), from, to).await?;
    Ok(Json(rows.iter().map(RotaEntryDto::from).collect()))
}

/// Puts an Analyst on the rota (needs `rota.manage`).
#[utoipa::path(post, path = "/rota", tag = "Rota", request_body = RotaEntryRequest, responses((status = 201, body = RotaEntryDto)))]
async fn schedule_rota(State(api): State<SchedulingApi>, Authenticated(p): Authenticated, Json(b): Json<RotaEntryRequest>) -> ApiResult<Response> {
    let placement = RotaPlacement {
        analyst: AnalystId::from_uuid(b.analyst_id),
        date: b.work_date,
        shift_id: b.shift_id,
        location_id: b.location_id,
        on_call: b.on_call,
    };
    let e = api.rota.schedule(&p, TenantId::from_uuid(b.company_id), placement).await?;
    Ok((StatusCode::CREATED, Json(RotaEntryDto::from(&e))).into_response())
}

/// A Company, as a query parameter.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CompanyQuery {
    /// Company.
    pub company_id: Uuid,
}
