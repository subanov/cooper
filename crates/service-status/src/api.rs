//! HTTP API of Service Status (FR-SST, FR-PRT-006).

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{Cursor, Limit, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{
    AppError, IncidentView, OpenIncident, PlanMaintenance, PostUpdate, ServiceStatusService,
    ServiceWithStatus, StatusPage,
};
use crate::domain::{
    ImpactLevel, ImpactLevelInput, Impacts, IncidentStatusInput, MaintenanceState,
    PlannedMaintenance, PlannedMaintenanceId, ServiceDetails, ServiceIncidentStatus,
    ServiceStatusError, StatusIncident, StatusIncidentId, StatusIncidentUpdate, StatusService,
    StatusServiceId,
};
use crate::ports::{IncidentQuery, MaintenanceQuery};

/// State the handlers need.
#[derive(Clone)]
pub struct ServiceStatusApi {
    /// The service.
    pub service: ServiceStatusService,
}

/// `/api/v1` path prefixes owned by the Service Status Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &[
    "/service-status",
    "/portal/service-status",
    "/public/service-status",
];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ServiceStatusApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(get_service_status_board))
        .routes(routes!(list_impact_levels, create_impact_level))
        .routes(routes!(update_impact_level))
        .routes(routes!(
            list_service_incident_statuses,
            create_service_incident_status
        ))
        .routes(routes!(update_service_incident_status))
        .routes(routes!(list_status_services, create_status_service))
        .routes(routes!(get_status_service, update_status_service))
        .routes(routes!(list_service_incidents, open_service_incident))
        .routes(routes!(get_service_incident, retitle_service_incident))
        .routes(routes!(post_incident_update))
        .routes(routes!(correct_incident_update))
        .routes(routes!(
            list_planned_maintenance,
            schedule_planned_maintenance
        ))
        .routes(routes!(get_planned_maintenance, edit_planned_maintenance))
        .routes(routes!(start_planned_maintenance))
        .routes(routes!(finish_planned_maintenance))
        .routes(routes!(cancel_planned_maintenance))
        .routes(routes!(portal_service_status))
        .routes(routes!(public_service_status))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<ServiceStatusError> for ApiError {
    fn from(e: ServiceStatusError) -> Self {
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
            AppError::Rule(ServiceStatusError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(ServiceStatusError::InvalidState(_)) => typed(
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

const fn yes() -> bool {
    true
}

fn limit(v: Option<u32>) -> Result<Limit, ApiError> {
    v.map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| ServiceStatusError::invalid("limit", "must be between 1 and 200").into())
}

fn cursor(v: Option<String>) -> Result<Option<(DateTime<Utc>, Uuid)>, ApiError> {
    v.map(|c| Cursor::from_client(c).decode::<(DateTime<Utc>, Uuid)>())
        .transpose()
        .map_err(|_| ServiceStatusError::invalid("cursor", "is not valid").into())
}

// ----- Reference data ---------------------------------------------------------------------------

/// An Impact Level (FR-SST-002).
#[derive(Debug, Serialize, ToSchema)]
pub struct ImpactLevelDto {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// 1 is the worst.
    pub severity_order: i32,
    /// Counts towards downtime.
    pub counts_as_downtime: bool,
    /// The all-clear.
    pub is_default: bool,
    /// Active.
    pub is_active: bool,
}

impl From<ImpactLevel> for ImpactLevelDto {
    fn from(l: ImpactLevel) -> Self {
        Self {
            id: l.id,
            key: l.key,
            name: l.name,
            colour: l.colour,
            severity_order: l.severity_order,
            counts_as_downtime: l.counts_as_downtime,
            is_default: l.is_default,
            is_active: l.is_active,
        }
    }
}

/// Creates or changes an Impact Level.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ImpactLevelRequest {
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// 1 is the worst; unique.
    pub severity_order: i32,
    /// Counts towards downtime.
    #[serde(default)]
    pub counts_as_downtime: bool,
    /// The all-clear.
    #[serde(default)]
    pub is_default: bool,
    /// Active.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl From<ImpactLevelRequest> for ImpactLevelInput {
    fn from(r: ImpactLevelRequest) -> Self {
        Self {
            name: r.name,
            colour: r.colour,
            severity_order: r.severity_order,
            counts_as_downtime: r.counts_as_downtime,
            is_default: r.is_default,
            is_active: r.is_active,
        }
    }
}

/// Impact Levels, worst first.
#[utoipa::path(get, path = "/service-status/impact-levels", tag = "Service Status",
    responses((status = 200, body = Vec<ImpactLevelDto>)))]
async fn list_impact_levels(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<ImpactLevelDto>>> {
    Ok(Json(
        api.service
            .impact_levels(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Creates an Impact Level (needs `service-status.configure`).
#[utoipa::path(post, path = "/service-status/impact-levels", tag = "Service Status",
    request_body = ImpactLevelRequest, responses((status = 201, body = ImpactLevelDto)))]
async fn create_impact_level(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<ImpactLevelRequest>,
) -> ApiResult<Response> {
    Ok(created(ImpactLevelDto::from(
        api.service.create_impact_level(&p, b.into()).await?,
    )))
}

/// Changes an Impact Level (needs `service-status.configure`).
#[utoipa::path(put, path = "/service-status/impact-levels/{id}", tag = "Service Status",
    params(("id" = Uuid, Path)), request_body = ImpactLevelRequest,
    responses((status = 200, body = ImpactLevelDto)))]
async fn update_impact_level(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ImpactLevelRequest>,
) -> ApiResult<Json<ImpactLevelDto>> {
    Ok(Json(
        api.service
            .update_impact_level(&p, id, b.into())
            .await?
            .into(),
    ))
}

/// An Incident Status (FR-SST-003).
#[derive(Debug, Serialize, ToSchema)]
pub struct ServiceIncidentStatusDto {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// The Incident is over.
    pub is_resolved: bool,
    /// Status of new Incidents.
    pub is_default: bool,
    /// Order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl From<ServiceIncidentStatus> for ServiceIncidentStatusDto {
    fn from(s: ServiceIncidentStatus) -> Self {
        Self {
            id: s.id,
            key: s.key,
            name: s.name,
            colour: s.colour,
            is_resolved: s.is_resolved,
            is_default: s.is_default,
            sort_order: s.sort_order,
            is_active: s.is_active,
        }
    }
}

/// Creates or changes an Incident Status.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ServiceIncidentStatusRequest {
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// Resolved.
    #[serde(default)]
    pub is_resolved: bool,
    /// Default.
    #[serde(default)]
    pub is_default: bool,
    /// Order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl From<ServiceIncidentStatusRequest> for IncidentStatusInput {
    fn from(r: ServiceIncidentStatusRequest) -> Self {
        Self {
            name: r.name,
            colour: r.colour,
            is_resolved: r.is_resolved,
            is_default: r.is_default,
            sort_order: r.sort_order,
            is_active: r.is_active,
        }
    }
}

/// Incident Statuses in order.
#[utoipa::path(get, path = "/service-status/incident-statuses", tag = "Service Status",
    responses((status = 200, body = Vec<ServiceIncidentStatusDto>)))]
async fn list_service_incident_statuses(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<ServiceIncidentStatusDto>>> {
    Ok(Json(
        api.service
            .incident_statuses(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Creates an Incident Status (needs `service-status.configure`).
#[utoipa::path(post, path = "/service-status/incident-statuses", tag = "Service Status",
    request_body = ServiceIncidentStatusRequest,
    responses((status = 201, body = ServiceIncidentStatusDto)))]
async fn create_service_incident_status(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<ServiceIncidentStatusRequest>,
) -> ApiResult<Response> {
    Ok(created(ServiceIncidentStatusDto::from(
        api.service.create_incident_status(&p, b.into()).await?,
    )))
}

/// Changes an Incident Status (needs `service-status.configure`).
#[utoipa::path(put, path = "/service-status/incident-statuses/{id}", tag = "Service Status",
    params(("id" = Uuid, Path)), request_body = ServiceIncidentStatusRequest,
    responses((status = 200, body = ServiceIncidentStatusDto)))]
async fn update_service_incident_status(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ServiceIncidentStatusRequest>,
) -> ApiResult<Json<ServiceIncidentStatusDto>> {
    Ok(Json(
        api.service
            .update_incident_status(&p, id, b.into())
            .await?
            .into(),
    ))
}

// ----- Services ---------------------------------------------------------------------------------

/// A Service (FR-SST-001).
#[derive(Debug, Serialize, ToSchema)]
pub struct StatusServiceDto {
    /// ID.
    pub id: Uuid,
    /// Company (`null`: shared with every Company).
    pub company_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Display order.
    pub sort_order: i32,
    /// Shown and selectable.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<StatusService> for StatusServiceDto {
    fn from(s: StatusService) -> Self {
        let d = s.details().clone();
        Self {
            id: s.id().as_uuid(),
            company_id: s.tenant_id().map(|t| t.as_uuid()),
            name: d.name,
            description: d.description,
            sort_order: d.sort_order,
            is_active: d.is_active,
            created_at: s.created_at(),
            updated_at: s.updated_at(),
        }
    }
}

/// Creates or changes a Service.
#[derive(Debug, Deserialize, ToSchema)]
pub struct StatusServiceRequest {
    /// Company (create only; absent: shared with every Company).
    pub company_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Display order.
    #[serde(default)]
    pub sort_order: i32,
    /// Shown and selectable (deactivate instead of deleting).
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl StatusServiceRequest {
    fn details(&self) -> ServiceDetails {
        ServiceDetails {
            name: self.name.clone(),
            description: self.description.clone(),
            sort_order: self.sort_order,
            is_active: self.is_active,
        }
    }
}

/// A Service with its Current Status.
#[derive(Debug, Serialize, ToSchema)]
pub struct ServiceStatusRowDto {
    /// The Service.
    pub service: StatusServiceDto,
    /// Its Current Status: the worst unresolved impact, or the default (FR-SST-006).
    pub status: ImpactLevelDto,
}

impl From<ServiceWithStatus> for ServiceStatusRowDto {
    fn from(s: ServiceWithStatus) -> Self {
        Self {
            service: s.service.into(),
            status: s.status.into(),
        }
    }
}

/// The board: active Services with their Current Status (FR-SST-006).
#[utoipa::path(get, path = "/service-status", tag = "Service Status",
    responses((status = 200, body = Vec<ServiceStatusRowDto>)))]
async fn get_service_status_board(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<ServiceStatusRowDto>>> {
    Ok(Json(
        api.service
            .board(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Services you can see, active or not.
#[utoipa::path(get, path = "/service-status/services", tag = "Service Status",
    responses((status = 200, body = Vec<StatusServiceDto>)))]
async fn list_status_services(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<StatusServiceDto>>> {
    Ok(Json(
        api.service
            .services(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Creates a Service (needs `service-status.configure`; shared ones need every Company).
#[utoipa::path(post, path = "/service-status/services", tag = "Service Status",
    request_body = StatusServiceRequest, responses((status = 201, body = StatusServiceDto)))]
async fn create_status_service(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<StatusServiceRequest>,
) -> ApiResult<Response> {
    let s = api
        .service
        .create_service(&p, b.company_id.map(TenantId::from_uuid), b.details())
        .await?;
    Ok(created(StatusServiceDto::from(s)))
}

/// One Service.
#[utoipa::path(get, path = "/service-status/services/{id}", tag = "Service Status",
    params(("id" = Uuid, Path)), responses((status = 200, body = StatusServiceDto)))]
async fn get_status_service(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StatusServiceDto>> {
    Ok(Json(
        api.service
            .service(&p, StatusServiceId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Changes or deactivates a Service (needs `service-status.configure`).
#[utoipa::path(put, path = "/service-status/services/{id}", tag = "Service Status",
    params(("id" = Uuid, Path)), request_body = StatusServiceRequest,
    responses((status = 200, body = StatusServiceDto)))]
async fn update_status_service(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<StatusServiceRequest>,
) -> ApiResult<Json<StatusServiceDto>> {
    Ok(Json(
        api.service
            .update_service(&p, StatusServiceId::from_uuid(id), b.details())
            .await?
            .into(),
    ))
}

// ----- Service Incidents ------------------------------------------------------------------------

/// The Impact Level on one Service.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ServiceImpactDto {
    /// The Service.
    pub service_id: Uuid,
    /// Its Impact Level.
    pub impact_level_id: Uuid,
}

fn impacts_dto(i: &Impacts) -> Vec<ServiceImpactDto> {
    i.iter()
        .map(|(s, l)| ServiceImpactDto {
            service_id: s.as_uuid(),
            impact_level_id: *l,
        })
        .collect()
}

fn impacts_in(v: &[ServiceImpactDto]) -> Vec<(StatusServiceId, Uuid)> {
    v.iter()
        .map(|i| (StatusServiceId::from_uuid(i.service_id), i.impact_level_id))
        .collect()
}

/// A Service Incident (FR-SST-004).
#[derive(Debug, Serialize, ToSchema)]
pub struct ServiceIncidentDto {
    /// ID.
    pub id: Uuid,
    /// Company of its Services (`null`: shared).
    pub company_id: Option<Uuid>,
    /// Title.
    pub title: String,
    /// Incident Status.
    pub status_id: Uuid,
    /// Whether the status is resolved.
    pub is_resolved: bool,
    /// The Impact Level on each affected Service now.
    pub services: Vec<ServiceImpactDto>,
    /// When the disruption began.
    pub started_at: DateTime<Utc>,
    /// When it was resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// The Planned Maintenance it came from.
    pub planned_maintenance_id: Option<Uuid>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<StatusIncident> for ServiceIncidentDto {
    fn from(i: StatusIncident) -> Self {
        Self {
            id: i.id().as_uuid(),
            company_id: i.tenant_id().map(|t| t.as_uuid()),
            title: i.title().to_owned(),
            status_id: i.status_id(),
            is_resolved: i.is_resolved(),
            services: impacts_dto(i.impacts()),
            started_at: i.started_at(),
            resolved_at: i.resolved_at(),
            planned_maintenance_id: i.planned_maintenance_id().map(|m| m.as_uuid()),
            created_at: i.created_at(),
            updated_at: i.updated_at(),
        }
    }
}

/// An Incident Update (FR-SST-005).
#[derive(Debug, Serialize, ToSchema)]
pub struct IncidentUpdateDto {
    /// ID.
    pub id: Uuid,
    /// Incident Status at the time.
    pub status_id: Uuid,
    /// The Impact Level on each Service at the time.
    pub services: Vec<ServiceImpactDto>,
    /// Text.
    pub body: String,
    /// Shown to Requesters and on the Public Status Page.
    pub is_public: bool,
    /// Who posted it (`{kind, id}`).
    #[schema(value_type = Object)]
    pub author: serde_json::Value,
    /// When it took effect.
    pub posted_at: DateTime<Utc>,
    /// When it was last corrected.
    pub edited_at: Option<DateTime<Utc>>,
}

impl From<StatusIncidentUpdate> for IncidentUpdateDto {
    fn from(u: StatusIncidentUpdate) -> Self {
        Self {
            id: u.id,
            status_id: u.status_id,
            services: impacts_dto(&u.impacts),
            author: serde_json::json!({ "kind": u.author.kind().as_str(), "id": u.author.id() }),
            body: u.body,
            is_public: u.is_public,
            posted_at: u.posted_at,
            edited_at: u.edited_at,
        }
    }
}

/// A Service Incident with its updates, oldest first.
#[derive(Debug, Serialize, ToSchema)]
pub struct ServiceIncidentDetailDto {
    /// The Incident.
    pub incident: ServiceIncidentDto,
    /// Its updates.
    pub updates: Vec<IncidentUpdateDto>,
}

impl From<IncidentView> for ServiceIncidentDetailDto {
    fn from(v: IncidentView) -> Self {
        Self {
            incident: v.incident.into(),
            updates: v.updates.into_iter().map(Into::into).collect(),
        }
    }
}

/// A page of Service Incidents.
#[derive(Debug, Serialize, ToSchema)]
pub struct ServiceIncidentPage {
    /// Incidents, newest first.
    pub items: Vec<ServiceIncidentDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Incident filters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ServiceIncidentListParams {
    /// Unresolved (`true`) or resolved (`false`).
    pub open: Option<bool>,
    /// Affecting this Service.
    pub service_id: Option<Uuid>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

/// Service Incidents you can see, newest first.
#[utoipa::path(get, path = "/service-status/incidents", tag = "Service Status",
    params(ServiceIncidentListParams), responses((status = 200, body = ServiceIncidentPage)))]
async fn list_service_incidents(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ServiceIncidentListParams>,
) -> ApiResult<Json<ServiceIncidentPage>> {
    let query = IncidentQuery {
        open: q.open,
        service_id: q.service_id.map(StatusServiceId::from_uuid),
        limit: limit(q.limit)?,
        after: cursor(q.cursor)?,
    };
    let rows = api.service.incidents(&p, &query).await?;
    let page = cooper_kernel::Page::from_rows(rows, query.limit, |i| {
        Cursor::encode(&(i.started_at(), i.id().as_uuid()))
    })
    .map_err(|_| AppError::Internal)?;
    Ok(Json(ServiceIncidentPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Opens a Service Incident.
#[derive(Debug, Deserialize, ToSchema)]
pub struct OpenServiceIncidentRequest {
    /// Title.
    pub title: String,
    /// Incident Status (default when absent).
    pub status_id: Option<Uuid>,
    /// Affected Services with Impact Levels (all of one Company, or all shared).
    pub services: Vec<ServiceImpactDto>,
    /// Text of the opening update.
    #[serde(default)]
    pub body: String,
    /// Whether the opening update is public (internal by default).
    #[serde(default)]
    pub is_public: bool,
    /// When the disruption began (now when absent; never in the future).
    pub started_at: Option<DateTime<Utc>>,
}

/// Opens a Service Incident (needs `service-status.update`).
#[utoipa::path(post, path = "/service-status/incidents", tag = "Service Status",
    request_body = OpenServiceIncidentRequest,
    responses((status = 201, body = ServiceIncidentDetailDto)))]
async fn open_service_incident(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<OpenServiceIncidentRequest>,
) -> ApiResult<Response> {
    let v = api
        .service
        .open_incident(
            &p,
            OpenIncident {
                services: impacts_in(&b.services),
                title: b.title,
                status_id: b.status_id,
                body: b.body,
                is_public: b.is_public,
                started_at: b.started_at,
            },
        )
        .await?;
    Ok(created(ServiceIncidentDetailDto::from(v)))
}

/// One Service Incident with every update.
#[utoipa::path(get, path = "/service-status/incidents/{id}", tag = "Service Status",
    params(("id" = Uuid, Path)), responses((status = 200, body = ServiceIncidentDetailDto)))]
async fn get_service_incident(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ServiceIncidentDetailDto>> {
    Ok(Json(
        api.service
            .incident(&p, StatusIncidentId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Renames a Service Incident.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RetitleServiceIncidentRequest {
    /// Title.
    pub title: String,
}

/// Changes the title of a Service Incident (needs `service-status.update`).
#[utoipa::path(put, path = "/service-status/incidents/{id}", tag = "Service Status",
    params(("id" = Uuid, Path)), request_body = RetitleServiceIncidentRequest,
    responses((status = 200, body = ServiceIncidentDto)))]
async fn retitle_service_incident(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<RetitleServiceIncidentRequest>,
) -> ApiResult<Json<ServiceIncidentDto>> {
    Ok(Json(
        api.service
            .retitle(&p, StatusIncidentId::from_uuid(id), &b.title)
            .await?
            .into(),
    ))
}

/// Posts an Incident Update.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PostIncidentUpdateRequest {
    /// New Incident Status.
    pub status_id: Option<Uuid>,
    /// New impacts, replacing all (absent: unchanged).
    pub services: Option<Vec<ServiceImpactDto>>,
    /// Text.
    #[serde(default)]
    pub body: String,
    /// Public (internal by default).
    #[serde(default)]
    pub is_public: bool,
}

/// The Incident after an update, and the update.
#[derive(Debug, Serialize, ToSchema)]
pub struct PostedIncidentUpdateDto {
    /// The Incident now.
    pub incident: ServiceIncidentDto,
    /// The update.
    pub update: IncidentUpdateDto,
}

/// Posts an Incident Update (needs `service-status.update`); it must say something new.
#[utoipa::path(post, path = "/service-status/incidents/{id}/updates", tag = "Service Status",
    params(("id" = Uuid, Path)), request_body = PostIncidentUpdateRequest,
    responses((status = 201, body = PostedIncidentUpdateDto)))]
async fn post_incident_update(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<PostIncidentUpdateRequest>,
) -> ApiResult<Response> {
    let (incident, update) = api
        .service
        .post_update(
            &p,
            StatusIncidentId::from_uuid(id),
            PostUpdate {
                status_id: b.status_id,
                services: b.services.as_deref().map(impacts_in),
                body: b.body,
                is_public: b.is_public,
            },
        )
        .await?;
    Ok(created(PostedIncidentUpdateDto {
        incident: incident.into(),
        update: update.into(),
    }))
}

/// Corrects a posted update.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CorrectIncidentUpdateRequest {
    /// Text.
    pub body: String,
    /// Public.
    pub is_public: bool,
}

/// Corrects the wording and visibility of an Incident Update (needs
/// `service-status.update`); its status and impact snapshot stay as they were.
#[utoipa::path(put, path = "/service-status/incidents/{id}/updates/{update_id}", tag = "Service Status",
    params(("id" = Uuid, Path), ("update_id" = Uuid, Path)), request_body = CorrectIncidentUpdateRequest,
    responses((status = 200, body = IncidentUpdateDto)))]
async fn correct_incident_update(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path((id, update_id)): Path<(Uuid, Uuid)>,
    Json(b): Json<CorrectIncidentUpdateRequest>,
) -> ApiResult<Json<IncidentUpdateDto>> {
    Ok(Json(
        api.service
            .correct_update(
                &p,
                StatusIncidentId::from_uuid(id),
                update_id,
                &b.body,
                b.is_public,
            )
            .await?
            .into(),
    ))
}

// ----- Planned Maintenance ----------------------------------------------------------------------

/// A Planned Maintenance (FR-SST-007).
#[derive(Debug, Serialize, ToSchema)]
pub struct PlannedMaintenanceDto {
    /// ID.
    pub id: Uuid,
    /// Company of its Services (`null`: shared).
    pub company_id: Option<Uuid>,
    /// Title.
    pub title: String,
    /// Public description, posted when it starts.
    pub description: String,
    /// Planned start.
    pub planned_start: DateTime<Utc>,
    /// Planned end.
    pub planned_end: DateTime<Utc>,
    /// `scheduled`, `started`, `finished` or `cancelled`.
    pub state: String,
    /// The Service Incident it became.
    pub incident_id: Option<Uuid>,
    /// Affected Services with Impact Levels.
    pub services: Vec<ServiceImpactDto>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<PlannedMaintenance> for PlannedMaintenanceDto {
    fn from(m: PlannedMaintenance) -> Self {
        let d = m.details().clone();
        Self {
            id: m.id().as_uuid(),
            company_id: m.tenant_id().map(|t| t.as_uuid()),
            title: d.title,
            description: d.description,
            planned_start: d.planned_start,
            planned_end: d.planned_end,
            state: m.state().as_str().to_owned(),
            incident_id: m.incident_id().map(|i| i.as_uuid()),
            services: impacts_dto(&d.impacts),
            created_at: m.created_at(),
            updated_at: m.updated_at(),
        }
    }
}

/// Schedules or changes Planned Maintenance.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PlannedMaintenanceRequest {
    /// Title.
    pub title: String,
    /// Public description.
    #[serde(default)]
    pub description: String,
    /// Planned start (a start already passed starts it at once).
    pub planned_start: DateTime<Utc>,
    /// Planned end (after the start, in the future).
    pub planned_end: DateTime<Utc>,
    /// Affected Services with Impact Levels (all of one Company, or all shared).
    pub services: Vec<ServiceImpactDto>,
}

impl From<PlannedMaintenanceRequest> for PlanMaintenance {
    fn from(b: PlannedMaintenanceRequest) -> Self {
        Self {
            services: impacts_in(&b.services),
            title: b.title,
            description: b.description,
            planned_start: b.planned_start,
            planned_end: b.planned_end,
        }
    }
}

/// A page of Planned Maintenance.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlannedMaintenancePage {
    /// Planned Maintenance, soonest first.
    pub items: Vec<PlannedMaintenanceDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Planned Maintenance filters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PlannedMaintenanceListParams {
    /// Comma-separated states (default: `scheduled,started`).
    pub state: Option<String>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

/// Planned Maintenance you can see, soonest first.
#[utoipa::path(get, path = "/service-status/planned-maintenance", tag = "Service Status",
    params(PlannedMaintenanceListParams), responses((status = 200, body = PlannedMaintenancePage)))]
async fn list_planned_maintenance(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<PlannedMaintenanceListParams>,
) -> ApiResult<Json<PlannedMaintenancePage>> {
    let states = match q.state.as_deref() {
        None => vec![MaintenanceState::Scheduled, MaintenanceState::Started],
        Some(s) => s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::parse)
            .collect::<Result<Vec<_>, _>>()?,
    };
    let query = MaintenanceQuery {
        states,
        limit: limit(q.limit)?,
        after: cursor(q.cursor)?,
    };
    let rows = api.service.maintenances(&p, &query).await?;
    let page = cooper_kernel::Page::from_rows(rows, query.limit, |m| {
        Cursor::encode(&(m.details().planned_start, m.id().as_uuid()))
    })
    .map_err(|_| AppError::Internal)?;
    Ok(Json(PlannedMaintenancePage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Schedules Planned Maintenance (needs `service-status.update`).
#[utoipa::path(post, path = "/service-status/planned-maintenance", tag = "Service Status",
    request_body = PlannedMaintenanceRequest, responses((status = 201, body = PlannedMaintenanceDto)))]
async fn schedule_planned_maintenance(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<PlannedMaintenanceRequest>,
) -> ApiResult<Response> {
    Ok(created(PlannedMaintenanceDto::from(
        api.service.schedule(&p, b.into()).await?,
    )))
}

/// One Planned Maintenance.
#[utoipa::path(get, path = "/service-status/planned-maintenance/{id}", tag = "Service Status",
    params(("id" = Uuid, Path)), responses((status = 200, body = PlannedMaintenanceDto)))]
async fn get_planned_maintenance(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PlannedMaintenanceDto>> {
    Ok(Json(
        api.service
            .maintenance(&p, PlannedMaintenanceId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Changes scheduled Planned Maintenance (needs `service-status.update`).
#[utoipa::path(put, path = "/service-status/planned-maintenance/{id}", tag = "Service Status",
    params(("id" = Uuid, Path)), request_body = PlannedMaintenanceRequest,
    responses((status = 200, body = PlannedMaintenanceDto)))]
async fn edit_planned_maintenance(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<PlannedMaintenanceRequest>,
) -> ApiResult<Json<PlannedMaintenanceDto>> {
    Ok(Json(
        api.service
            .edit_maintenance(&p, PlannedMaintenanceId::from_uuid(id), b.into())
            .await?
            .into(),
    ))
}

/// Starts scheduled Planned Maintenance now, opening its Service Incident (needs
/// `service-status.update`).
#[utoipa::path(post, path = "/service-status/planned-maintenance/{id}/start", tag = "Service Status",
    params(("id" = Uuid, Path)), responses((status = 200, body = PlannedMaintenanceDto)))]
async fn start_planned_maintenance(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PlannedMaintenanceDto>> {
    Ok(Json(
        api.service
            .start_maintenance(&p, PlannedMaintenanceId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Finishes started Planned Maintenance now, resolving its Service Incident unless
/// someone already did (needs `service-status.update`).
#[utoipa::path(post, path = "/service-status/planned-maintenance/{id}/finish", tag = "Service Status",
    params(("id" = Uuid, Path)), responses((status = 200, body = PlannedMaintenanceDto)))]
async fn finish_planned_maintenance(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PlannedMaintenanceDto>> {
    Ok(Json(
        api.service
            .finish_maintenance(&p, PlannedMaintenanceId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Cancels scheduled Planned Maintenance (needs `service-status.update`).
#[utoipa::path(post, path = "/service-status/planned-maintenance/{id}/cancel", tag = "Service Status",
    params(("id" = Uuid, Path)), responses((status = 200, body = PlannedMaintenanceDto)))]
async fn cancel_planned_maintenance(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PlannedMaintenanceDto>> {
    Ok(Json(
        api.service
            .cancel_maintenance(&p, PlannedMaintenanceId::from_uuid(id))
            .await?
            .into(),
    ))
}

// ----- Portal and Public Status Page ------------------------------------------------------------

/// An Impact Level as Requesters and the public see it.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicImpactDto {
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// 1 is the worst.
    pub severity_order: i32,
}

/// A Service with its Current Status, as Requesters and the public see it.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicServiceDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Current Status.
    pub status: PublicImpactDto,
}

/// The impact on one Service, by name.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicServiceImpactDto {
    /// Service name.
    pub service: String,
    /// Impact Level.
    pub impact: PublicImpactDto,
}

/// A public Incident Update: no author, no internal text.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicIncidentUpdateDto {
    /// When.
    pub posted_at: DateTime<Utc>,
    /// Incident Status name at the time.
    pub status: String,
    /// Text.
    pub body: String,
}

/// A Service Incident with its public updates, oldest first.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicIncidentDto {
    /// ID.
    pub id: Uuid,
    /// Title.
    pub title: String,
    /// Incident Status name.
    pub status: String,
    /// Whether it is over.
    pub is_resolved: bool,
    /// Started.
    pub started_at: DateTime<Utc>,
    /// Resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// Affected Services.
    pub services: Vec<PublicServiceImpactDto>,
    /// Public updates.
    pub updates: Vec<PublicIncidentUpdateDto>,
}

/// Scheduled or started Planned Maintenance.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublicMaintenanceDto {
    /// ID.
    pub id: Uuid,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Planned start.
    pub planned_start: DateTime<Utc>,
    /// Planned end.
    pub planned_end: DateTime<Utc>,
    /// `scheduled` or `started`.
    pub state: String,
    /// Affected Services.
    pub services: Vec<PublicServiceImpactDto>,
}

/// A status page (Portal or public).
#[derive(Debug, Serialize, ToSchema)]
pub struct StatusPageDto {
    /// Active Services with their Current Status, in order.
    pub services: Vec<PublicServiceDto>,
    /// Incidents with public updates (only when shown; FR-PRT-006).
    pub incidents: Vec<PublicIncidentDto>,
    /// Scheduled and started Planned Maintenance, soonest first.
    pub planned_maintenance: Vec<PublicMaintenanceDto>,
}

fn public_impact(l: &ImpactLevel) -> PublicImpactDto {
    PublicImpactDto {
        name: l.name.clone(),
        colour: l.colour.clone(),
        severity_order: l.severity_order,
    }
}

impl From<StatusPage> for StatusPageDto {
    fn from(p: StatusPage) -> Self {
        let levels: BTreeMap<Uuid, &ImpactLevel> = p.levels.iter().map(|l| (l.id, l)).collect();
        let status_name = |id: Uuid| {
            p.statuses
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.name.clone())
                .unwrap_or_default()
        };
        let named = |i: &Impacts| -> Vec<PublicServiceImpactDto> {
            i.iter()
                .filter_map(|(s, l)| {
                    Some(PublicServiceImpactDto {
                        service: p.service_names.get(s)?.clone(),
                        impact: public_impact(levels.get(l)?),
                    })
                })
                .collect()
        };
        Self {
            services: p
                .services
                .iter()
                .map(|s| PublicServiceDto {
                    id: s.service.id().as_uuid(),
                    name: s.service.details().name.clone(),
                    description: s.service.details().description.clone(),
                    status: public_impact(&s.status),
                })
                .collect(),
            incidents: p
                .incidents
                .iter()
                .map(|v| PublicIncidentDto {
                    id: v.incident.id().as_uuid(),
                    title: v.incident.title().to_owned(),
                    status: status_name(v.incident.status_id()),
                    is_resolved: v.incident.is_resolved(),
                    started_at: v.incident.started_at(),
                    resolved_at: v.incident.resolved_at(),
                    services: named(v.incident.impacts()),
                    updates: v
                        .updates
                        .iter()
                        .map(|u| PublicIncidentUpdateDto {
                            posted_at: u.posted_at,
                            status: status_name(u.status_id),
                            body: u.body.clone(),
                        })
                        .collect(),
                })
                .collect(),
            planned_maintenance: p
                .maintenance
                .iter()
                .map(|m| PublicMaintenanceDto {
                    id: m.id().as_uuid(),
                    title: m.details().title.clone(),
                    description: m.details().description.clone(),
                    planned_start: m.details().planned_start,
                    planned_end: m.details().planned_end,
                    state: m.state().as_str().to_owned(),
                    services: named(&m.details().impacts),
                })
                .collect(),
        }
    }
}

/// Service Status on the Portal (FR-PRT-006): shared Services and those of your Company,
/// Planned Maintenance and, when shown, Incidents with their public updates.
#[utoipa::path(get, path = "/portal/service-status", tag = "Portal",
    responses((status = 200, body = StatusPageDto)))]
async fn portal_service_status(
    State(api): State<ServiceStatusApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<StatusPageDto>> {
    Ok(Json(api.service.portal(&p).await?.into()))
}

/// The Public Status Page (FR-SST-009): no sign-in, shared records only, `404` unless
/// `service_status.public_page` is on.
#[utoipa::path(get, path = "/public/service-status", tag = "Service Status",
    responses((status = 200, body = StatusPageDto), (status = 404, description = "The page is off")))]
async fn public_service_status(
    State(api): State<ServiceStatusApi>,
) -> ApiResult<Json<StatusPageDto>> {
    Ok(Json(api.service.public_page().await?.into()))
}
