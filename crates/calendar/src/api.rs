//! HTTP API of the Calendar (FR-CAL).

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::TenantId;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, CalendarService, EventList, EventView, ListEvents};
use crate::domain::{
    CalendarCategory, CalendarCategoryId, CalendarError, CalendarEventId, CategoryInput, Colour,
    DateRange, EventDetails, EventTiming,
};

/// State the handlers need.
#[derive(Clone)]
pub struct CalendarApi {
    /// The service.
    pub service: CalendarService,
}

/// `/api/v1` path prefixes owned by the Calendar Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &["/calendar"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    CalendarApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_calendar_categories, create_calendar_category))
        .routes(routes!(update_calendar_category))
        .routes(routes!(list_calendar_events, create_calendar_event))
        .routes(routes!(
            get_calendar_event,
            edit_calendar_event,
            delete_calendar_event
        ))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<CalendarError> for ApiError {
    fn from(e: CalendarError) -> Self {
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
            AppError::Rule(CalendarError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(CalendarError::InvalidState(_)) => typed(
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

// ----- Calendar Categories ----------------------------------------------------------------

/// A Calendar Category.
#[derive(Debug, Serialize, ToSchema)]
pub struct CalendarCategoryDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: String,
    /// Offered for new events.
    pub is_active: bool,
    /// Made by Cooper for generated events (`contracts`, `assets`); not offered for
    /// manual events and never inactive.
    pub system_key: Option<String>,
}

impl From<&CalendarCategory> for CalendarCategoryDto {
    fn from(c: &CalendarCategory) -> Self {
        Self {
            id: c.id().as_uuid(),
            name: c.name().to_owned(),
            colour: c.colour().as_str().to_owned(),
            is_active: c.is_active(),
            system_key: c.key().map(str::to_owned),
        }
    }
}

/// Creates or changes a Calendar Category.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CalendarCategoryRequest {
    /// Name (1 to 50 characters, unique).
    pub name: String,
    /// `#rrggbb`.
    pub colour: String,
    /// Offered for new events.
    #[serde(default = "yes")]
    pub is_active: bool,
}

const fn yes() -> bool {
    true
}

impl TryFrom<CalendarCategoryRequest> for CategoryInput {
    type Error = CalendarError;
    fn try_from(r: CalendarCategoryRequest) -> Result<Self, CalendarError> {
        Ok(Self {
            name: r.name,
            colour: Colour::try_from(r.colour)?,
            is_active: r.is_active,
        })
    }
}

/// Calendar Categories, Administrators' first, by name (FR-CAL-001).
#[utoipa::path(get, path = "/calendar/categories", tag = "Calendar", responses((status = 200, body = Vec<CalendarCategoryDto>)))]
async fn list_calendar_categories(
    State(api): State<CalendarApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<CalendarCategoryDto>>> {
    Ok(Json(
        api.service
            .categories(&p)
            .await?
            .iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Adds a Calendar Category (needs `calendar.configure`).
#[utoipa::path(post, path = "/calendar/categories", tag = "Calendar", request_body = CalendarCategoryRequest, responses((status = 201, body = CalendarCategoryDto)))]
async fn create_calendar_category(
    State(api): State<CalendarApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<CalendarCategoryRequest>,
) -> ApiResult<Response> {
    let c = api.service.create_category(&p, b.try_into()?).await?;
    Ok((StatusCode::CREATED, Json(CalendarCategoryDto::from(&c))).into_response())
}

/// Changes a Calendar Category (needs `calendar.configure`). System categories may be
/// renamed and recoloured but not deactivated.
#[utoipa::path(put, path = "/calendar/categories/{id}", tag = "Calendar", params(("id" = Uuid, Path)), request_body = CalendarCategoryRequest, responses((status = 200, body = CalendarCategoryDto)))]
async fn update_calendar_category(
    State(api): State<CalendarApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<CalendarCategoryRequest>,
) -> ApiResult<Json<CalendarCategoryDto>> {
    let c = api
        .service
        .update_category(&p, CalendarCategoryId::from_uuid(id), b.try_into()?)
        .await?;
    Ok(Json((&c).into()))
}

// ----- Calendar Events --------------------------------------------------------------------

/// The record a generated Calendar Event comes from.
#[derive(Debug, Serialize, ToSchema)]
pub struct CalendarEventSourceDto {
    /// `contract` or `asset`.
    pub kind: String,
    /// The Contract or Asset.
    pub id: Uuid,
    /// Which date: `notice_date`, `end_date`, `warranty_expires_on` or `lease_expires_on`.
    pub date: String,
}

/// A Calendar Event.
#[derive(Debug, Serialize, ToSchema)]
pub struct CalendarEventDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Category.
    pub category_id: Uuid,
    /// Category name.
    pub category_name: String,
    /// Category colour, `#rrggbb`.
    pub colour: String,
    /// Whole days (`start_date`..=`end_date`) rather than times (`starts_at`..`ends_at`).
    pub all_day: bool,
    /// All-day: first day.
    pub start_date: Option<NaiveDate>,
    /// All-day: last day (inclusive).
    pub end_date: Option<NaiveDate>,
    /// Timed: start (UTC).
    pub starts_at: Option<DateTime<Utc>>,
    /// Timed: end (UTC).
    pub ends_at: Option<DateTime<Utc>>,
    /// Where.
    pub location: Option<String>,
    /// Source of a generated event; generated events are read-only.
    pub source: Option<CalendarEventSourceDto>,
    /// Whether the event can be edited and deleted (manual events).
    pub editable: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<EventView> for CalendarEventDto {
    fn from(v: EventView) -> Self {
        let e = v.event;
        let d = e.details().clone();
        let (start_date, end_date, starts_at, ends_at) = match d.timing {
            EventTiming::AllDay { start, end } => (Some(start), Some(end), None, None),
            EventTiming::Timed { start, end } => (None, None, Some(start), Some(end)),
        };
        Self {
            id: e.id().as_uuid(),
            company_id: e.tenant_id().as_uuid(),
            title: d.title,
            description: d.description,
            category_id: d.category_id.as_uuid(),
            category_name: v.category.name().to_owned(),
            colour: v.category.colour().as_str().to_owned(),
            all_day: d.timing.is_all_day(),
            start_date,
            end_date,
            starts_at,
            ends_at,
            location: d.location,
            source: e.source().map(|s| CalendarEventSourceDto {
                kind: s.kind.as_str().to_owned(),
                id: s.id,
                date: s.date.as_str().to_owned(),
            }),
            editable: !e.is_generated(),
            created_at: e.created_at(),
            updated_at: e.updated_at(),
        }
    }
}

/// Events of a range.
#[derive(Debug, Serialize, ToSchema)]
pub struct CalendarEventList {
    /// Events by start (all-day events first on a day).
    pub items: Vec<CalendarEventDto>,
    /// More events matched than returned (2000); narrow the range or filter.
    pub truncated: bool,
}

impl From<EventList> for CalendarEventList {
    fn from(l: EventList) -> Self {
        Self {
            items: l.items.into_iter().map(Into::into).collect(),
            truncated: l.truncated,
        }
    }
}

/// Which events to list.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CalendarEventParams {
    /// First day (required).
    pub from: NaiveDate,
    /// Last day, inclusive (required; at most 366 days after `from`). Timed events are
    /// matched on their UTC days.
    pub to: NaiveDate,
    /// Only this Company.
    pub company_id: Option<Uuid>,
    /// Only this Calendar Category.
    pub category_id: Option<Uuid>,
}

/// Creates or changes a manual Calendar Event.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CalendarEventRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Title (1 to 255 characters).
    pub title: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// An active Calendar Category that is not a system one.
    pub category_id: Uuid,
    /// Whole days (`start_date`, `end_date`) or times (`starts_at`, `ends_at`).
    #[serde(default)]
    pub all_day: bool,
    /// All-day: first day.
    pub start_date: Option<NaiveDate>,
    /// All-day: last day (default `start_date`).
    pub end_date: Option<NaiveDate>,
    /// Timed: start.
    pub starts_at: Option<DateTime<Utc>>,
    /// Timed: end (at or after the start).
    pub ends_at: Option<DateTime<Utc>>,
    /// Where.
    pub location: Option<String>,
}

impl TryFrom<CalendarEventRequest> for EventDetails {
    type Error = CalendarError;
    fn try_from(r: CalendarEventRequest) -> Result<Self, CalendarError> {
        let timing = if r.all_day {
            let start = r
                .start_date
                .ok_or_else(|| CalendarError::invalid("start_date", "is required"))?;
            EventTiming::all_day(start, r.end_date.unwrap_or(start))?
        } else {
            let start = r
                .starts_at
                .ok_or_else(|| CalendarError::invalid("starts_at", "is required"))?;
            let end = r
                .ends_at
                .ok_or_else(|| CalendarError::invalid("ends_at", "is required"))?;
            EventTiming::timed(start, end)?
        };
        Ok(Self {
            title: r.title,
            description: r.description,
            category_id: CalendarCategoryId::from_uuid(r.category_id),
            timing,
            location: r.location,
        })
    }
}

/// Calendar Events of the Companies you can see touching a range of days, manual and
/// generated (FR-CAL-001, FR-CAL-002).
#[utoipa::path(get, path = "/calendar/events", tag = "Calendar", params(CalendarEventParams), responses((status = 200, body = CalendarEventList)))]
async fn list_calendar_events(
    State(api): State<CalendarApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<CalendarEventParams>,
) -> ApiResult<Json<CalendarEventList>> {
    let list = api
        .service
        .events(
            &p,
            ListEvents {
                range: DateRange::new(q.from, q.to)?,
                tenant_id: q.company_id.map(TenantId::from_uuid),
                category_id: q.category_id.map(CalendarCategoryId::from_uuid),
            },
        )
        .await?;
    Ok(Json(list.into()))
}

/// Adds a manual Calendar Event (needs `calendar.create`).
#[utoipa::path(post, path = "/calendar/events", tag = "Calendar", request_body = CalendarEventRequest, responses((status = 201, body = CalendarEventDto)))]
async fn create_calendar_event(
    State(api): State<CalendarApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<CalendarEventRequest>,
) -> ApiResult<Response> {
    let tenant = b
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| CalendarError::invalid("company_id", "is required"))?;
    let e = api.service.create_event(&p, tenant, b.try_into()?).await?;
    Ok((StatusCode::CREATED, Json(CalendarEventDto::from(e))).into_response())
}

/// One Calendar Event.
#[utoipa::path(get, path = "/calendar/events/{id}", tag = "Calendar", params(("id" = Uuid, Path)), responses((status = 200, body = CalendarEventDto)))]
async fn get_calendar_event(
    State(api): State<CalendarApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CalendarEventDto>> {
    Ok(Json(
        api.service
            .event(&p, CalendarEventId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Changes a manual Calendar Event (needs `calendar.update`; generated events: `409`).
#[utoipa::path(put, path = "/calendar/events/{id}", tag = "Calendar", params(("id" = Uuid, Path)), request_body = CalendarEventRequest, responses((status = 200, body = CalendarEventDto)))]
async fn edit_calendar_event(
    State(api): State<CalendarApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<CalendarEventRequest>,
) -> ApiResult<Json<CalendarEventDto>> {
    let e = api
        .service
        .edit_event(&p, CalendarEventId::from_uuid(id), b.try_into()?)
        .await?;
    Ok(Json(e.into()))
}

/// Deletes a manual Calendar Event (needs `calendar.update`; generated events: `409`).
#[utoipa::path(delete, path = "/calendar/events/{id}", tag = "Calendar", params(("id" = Uuid, Path)), responses((status = 204)))]
async fn delete_calendar_event(
    State(api): State<CalendarApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api.service
        .delete_event(&p, CalendarEventId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
