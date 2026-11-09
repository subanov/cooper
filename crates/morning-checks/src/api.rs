//! HTTP API of Morning Checks (FR-MCK).

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, Utc};
use cooper_http::auth::forbidden;
use cooper_http::csv::{CsvWriter, download};
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{Actor, AnalystId, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, CheckView, Day, HistoryGrid, LinkView, MorningChecksService};
use crate::domain::{
    CheckAssignee, CheckDetails, CheckGroup, CheckGroupId, CheckId, CheckResult, CheckResultId,
    CheckStatus, CheckStatusInput, GroupDetails, LinkKind, MorningChecksError, Routing,
    RoutingSource,
};

/// State the handlers need.
#[derive(Clone)]
pub struct MorningChecksApi {
    /// The service.
    pub service: MorningChecksService,
}

/// `/api/v1` path prefixes owned by the Morning Checks Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &["/morning-checks"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    MorningChecksApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_check_statuses, create_check_status))
        .routes(routes!(update_check_status))
        .routes(routes!(list_check_groups, create_check_group))
        .routes(routes!(update_check_group))
        .routes(routes!(list_checks, create_check))
        .routes(routes!(get_check, update_check))
        .routes(routes!(check_day, record_check_result))
        .routes(routes!(get_check_result, clear_check_result))
        .routes(routes!(check_result_history))
        .routes(routes!(list_result_links, link_check_result))
        .routes(routes!(unlink_check_result))
        .routes(routes!(create_follow_up_task))
        .routes(routes!(check_history))
        .routes(routes!(export_check_history))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<MorningChecksError> for ApiError {
    fn from(e: MorningChecksError) -> Self {
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
            AppError::Rule(MorningChecksError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(MorningChecksError::InvalidState(_)) => typed(
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

/// Who did something (`{kind, id}`).
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckActorDto {
    /// `analyst`, `api_key`, `requester` or `automation`.
    pub kind: String,
    /// Their ID.
    pub id: Option<Uuid>,
}

impl From<Actor> for CheckActorDto {
    fn from(a: Actor) -> Self {
        Self {
            kind: a.kind().as_str().to_owned(),
            id: a.id(),
        }
    }
}

// ----- Check Statuses (FR-MCK-002) --------------------------------------------------------

/// A Check Status.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckStatusDto {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows (`green`, `amber`, `red`).
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// `#rrggbb`, used to colour-code results.
    pub colour: String,
    /// Results with this status need notes.
    pub requires_notes: bool,
    /// Order.
    pub sort_order: i32,
    /// Selectable for new results.
    pub is_active: bool,
}

impl From<CheckStatus> for CheckStatusDto {
    fn from(s: CheckStatus) -> Self {
        Self {
            id: s.id,
            key: s.key,
            name: s.name,
            colour: s.colour,
            requires_notes: s.requires_notes,
            sort_order: s.sort_order,
            is_active: s.is_active,
        }
    }
}

/// Creates or changes a Check Status.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckStatusRequest {
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: String,
    /// Results with this status need notes.
    #[serde(default)]
    pub requires_notes: bool,
    /// Order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active (deactivate instead of deleting).
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl From<CheckStatusRequest> for CheckStatusInput {
    fn from(r: CheckStatusRequest) -> Self {
        Self {
            name: r.name,
            colour: r.colour,
            requires_notes: r.requires_notes,
            sort_order: r.sort_order,
            is_active: r.is_active,
        }
    }
}

/// Check Statuses in order (needs `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/statuses", tag = "Morning Checks", responses((status = 200, body = Vec<CheckStatusDto>)))]
async fn list_check_statuses(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<CheckStatusDto>>> {
    Ok(Json(
        api.service
            .statuses(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Creates a Check Status (needs `morning-checks.configure`).
#[utoipa::path(post, path = "/morning-checks/statuses", tag = "Morning Checks", request_body = CheckStatusRequest, responses((status = 201, body = CheckStatusDto)))]
async fn create_check_status(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<CheckStatusRequest>,
) -> ApiResult<Response> {
    Ok(created(CheckStatusDto::from(
        api.service.create_status(&p, b.into()).await?,
    )))
}

/// Changes or deactivates a Check Status (needs `morning-checks.configure`).
#[utoipa::path(put, path = "/morning-checks/statuses/{id}", tag = "Morning Checks", params(("id" = Uuid, Path)), request_body = CheckStatusRequest, responses((status = 200, body = CheckStatusDto)))]
async fn update_check_status(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<CheckStatusRequest>,
) -> ApiResult<Json<CheckStatusDto>> {
    Ok(Json(
        api.service.update_status(&p, id, b.into()).await?.into(),
    ))
}

// ----- Check Groups (FR-MCK-001) ----------------------------------------------------------

/// A Check Group.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckGroupDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Routed to this Team.
    pub assigned_team_id: Option<Uuid>,
    /// Routed to this Analyst.
    pub assigned_analyst_id: Option<Uuid>,
    /// Order.
    pub sort_order: i32,
    /// Its Checks are in the daily round.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<CheckGroup> for CheckGroupDto {
    fn from(g: CheckGroup) -> Self {
        let d = g.details().clone();
        let (team, analyst) = match d.assignee {
            Some(CheckAssignee::Team(t)) => (Some(t.as_uuid()), None),
            Some(CheckAssignee::Analyst(a)) => (None, Some(a.as_uuid())),
            None => (None, None),
        };
        Self {
            id: g.id().as_uuid(),
            company_id: g.tenant_id().as_uuid(),
            name: d.name,
            description: d.description,
            assigned_team_id: team,
            assigned_analyst_id: analyst,
            sort_order: d.sort_order,
            is_active: d.is_active,
            created_at: g.created_at(),
            updated_at: g.updated_at(),
        }
    }
}

/// Creates or changes a Check Group.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckGroupRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Route to this Team (not with an Analyst).
    pub assigned_team_id: Option<Uuid>,
    /// Route to this Analyst (not with a Team).
    pub assigned_analyst_id: Option<Uuid>,
    /// Order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active (deactivate instead of deleting).
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl CheckGroupRequest {
    fn details(&self) -> Result<GroupDetails, ApiError> {
        let assignee = match (self.assigned_team_id, self.assigned_analyst_id) {
            (Some(_), Some(_)) => {
                return Err(MorningChecksError::invalid(
                    "assigned_analyst_id",
                    "a Check Group is routed to a Team or an Analyst, not both",
                )
                .into());
            }
            (Some(t), None) => Some(CheckAssignee::Team(TeamId::from_uuid(t))),
            (None, Some(a)) => Some(CheckAssignee::Analyst(AnalystId::from_uuid(a))),
            (None, None) => None,
        };
        Ok(GroupDetails {
            name: self.name.clone(),
            description: self.description.clone(),
            assignee,
            sort_order: self.sort_order,
            is_active: self.is_active,
        })
    }
}

/// Filters by Company.
#[derive(Debug, Deserialize, IntoParams)]
pub struct CompanyParams {
    /// Only this Company.
    pub company_id: Option<Uuid>,
}

/// Check Groups in your Companies, in order (needs `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/groups", tag = "Morning Checks", params(CompanyParams), responses((status = 200, body = Vec<CheckGroupDto>)))]
async fn list_check_groups(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<CompanyParams>,
) -> ApiResult<Json<Vec<CheckGroupDto>>> {
    Ok(Json(
        api.service
            .groups(&p, q.company_id.map(TenantId::from_uuid))
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Creates a Check Group (needs `morning-checks.configure`).
#[utoipa::path(post, path = "/morning-checks/groups", tag = "Morning Checks", request_body = CheckGroupRequest, responses((status = 201, body = CheckGroupDto)))]
async fn create_check_group(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<CheckGroupRequest>,
) -> ApiResult<Response> {
    let tenant = b
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| MorningChecksError::invalid("company_id", "is required"))?;
    let details = b.details()?;
    Ok(created(CheckGroupDto::from(
        api.service.create_group(&p, tenant, details).await?,
    )))
}

/// Changes or deactivates a Check Group (needs `morning-checks.configure`).
#[utoipa::path(put, path = "/morning-checks/groups/{id}", tag = "Morning Checks", params(("id" = Uuid, Path)), request_body = CheckGroupRequest, responses((status = 200, body = CheckGroupDto)))]
async fn update_check_group(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<CheckGroupRequest>,
) -> ApiResult<Json<CheckGroupDto>> {
    let details = b.details()?;
    Ok(Json(
        api.service
            .update_group(&p, CheckGroupId::from_uuid(id), details)
            .await?
            .into(),
    ))
}

// ----- Checks (FR-MCK-001) ----------------------------------------------------------------

/// Who a Check is routed to: its own Analyst, else its group's Analyst, else its group's
/// Team.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckRoutingDto {
    /// `analyst`, `team` or `nobody`.
    pub kind: String,
    /// The Analyst.
    pub analyst_id: Option<Uuid>,
    /// The Team.
    pub team_id: Option<Uuid>,
    /// `check` or `group` (where the routing came from).
    pub source: Option<String>,
}

impl From<Routing> for CheckRoutingDto {
    fn from(r: Routing) -> Self {
        match r {
            Routing::Analyst(a, source) => Self {
                kind: "analyst".into(),
                analyst_id: Some(a.as_uuid()),
                team_id: None,
                source: Some(
                    match source {
                        RoutingSource::Check => "check",
                        RoutingSource::Group => "group",
                    }
                    .into(),
                ),
            },
            Routing::Team(t) => Self {
                kind: "team".into(),
                analyst_id: None,
                team_id: Some(t.as_uuid()),
                source: Some("group".into()),
            },
            Routing::Nobody => Self {
                kind: "nobody".into(),
                analyst_id: None,
                team_id: None,
                source: None,
            },
        }
    }
}

/// A Check.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Name.
    pub name: String,
    /// What to verify.
    pub description: String,
    /// Check Group.
    pub check_group_id: Option<Uuid>,
    /// Its name.
    pub check_group_name: Option<String>,
    /// The Check's own Analyst.
    pub assigned_analyst_id: Option<Uuid>,
    /// Who it is routed to.
    pub routing: CheckRoutingDto,
    /// Order within its group.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
    /// In the daily round (active, in an active group or none).
    pub in_round: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<CheckView> for CheckDto {
    fn from(v: CheckView) -> Self {
        let in_round = v.check.in_round(v.group.as_ref());
        let d = v.check.details().clone();
        Self {
            id: v.check.id().as_uuid(),
            company_id: v.check.tenant_id().as_uuid(),
            name: d.name,
            description: d.description,
            check_group_id: d.group_id.map(|g| g.as_uuid()),
            check_group_name: v.group.map(|g| g.details().name.clone()),
            assigned_analyst_id: d.assigned_analyst.map(|a| a.as_uuid()),
            routing: v.routing.into(),
            sort_order: d.sort_order,
            is_active: d.is_active,
            in_round,
            created_at: v.check.created_at(),
            updated_at: v.check.updated_at(),
        }
    }
}

/// Creates or changes a Check.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// What to verify.
    #[serde(default)]
    pub description: String,
    /// Check Group (same Company).
    pub check_group_id: Option<Uuid>,
    /// The Check's own Analyst.
    pub assigned_analyst_id: Option<Uuid>,
    /// Order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active (deactivate instead of deleting).
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl CheckRequest {
    fn details(&self) -> CheckDetails {
        CheckDetails {
            name: self.name.clone(),
            description: self.description.clone(),
            group_id: self.check_group_id.map(CheckGroupId::from_uuid),
            assigned_analyst: self.assigned_analyst_id.map(AnalystId::from_uuid),
            sort_order: self.sort_order,
            is_active: self.is_active,
        }
    }
}

/// Filters Checks.
#[derive(Debug, Deserialize, IntoParams)]
pub struct CheckListParams {
    /// Only this Company.
    pub company_id: Option<Uuid>,
    /// Only this Check Group.
    pub check_group_id: Option<Uuid>,
}

/// Checks in your Companies, active or not, in round order (needs `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/checks", tag = "Morning Checks", params(CheckListParams), responses((status = 200, body = Vec<CheckDto>)))]
async fn list_checks(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<CheckListParams>,
) -> ApiResult<Json<Vec<CheckDto>>> {
    Ok(Json(
        api.service
            .checks(
                &p,
                q.company_id.map(TenantId::from_uuid),
                q.check_group_id.map(CheckGroupId::from_uuid),
            )
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Creates a Check (needs `morning-checks.configure`).
#[utoipa::path(post, path = "/morning-checks/checks", tag = "Morning Checks", request_body = CheckRequest, responses((status = 201, body = CheckDto)))]
async fn create_check(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<CheckRequest>,
) -> ApiResult<Response> {
    let tenant = b
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| MorningChecksError::invalid("company_id", "is required"))?;
    Ok(created(CheckDto::from(
        api.service.create_check(&p, tenant, b.details()).await?,
    )))
}

/// One Check (needs `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/checks/{id}", tag = "Morning Checks", params(("id" = Uuid, Path)), responses((status = 200, body = CheckDto)))]
async fn get_check(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CheckDto>> {
    Ok(Json(
        api.service.check(&p, CheckId::from_uuid(id)).await?.into(),
    ))
}

/// Changes or deactivates a Check (needs `morning-checks.configure`).
#[utoipa::path(put, path = "/morning-checks/checks/{id}", tag = "Morning Checks", params(("id" = Uuid, Path)), request_body = CheckRequest, responses((status = 200, body = CheckDto)))]
async fn update_check(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<CheckRequest>,
) -> ApiResult<Json<CheckDto>> {
    Ok(Json(
        api.service
            .update_check(&p, CheckId::from_uuid(id), b.details())
            .await?
            .into(),
    ))
}

// ----- Check Results (FR-MCK-003) ---------------------------------------------------------

/// A Check Result.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckResultDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Check.
    pub check_id: Uuid,
    /// Check Date.
    pub check_date: NaiveDate,
    /// Check Status.
    pub status_id: Uuid,
    /// Notes.
    pub notes: String,
    /// Who first recorded it.
    pub recorded_by: CheckActorDto,
    /// When.
    pub recorded_at: DateTime<Utc>,
    /// Who last changed it.
    pub modified_by: CheckActorDto,
    /// When.
    pub updated_at: DateTime<Utc>,
}

impl From<CheckResult> for CheckResultDto {
    fn from(r: CheckResult) -> Self {
        Self {
            id: r.id().as_uuid(),
            company_id: r.tenant_id().as_uuid(),
            check_id: r.check_id().as_uuid(),
            check_date: r.check_date(),
            status_id: r.status_id(),
            notes: r.notes().to_owned(),
            recorded_by: r.recorded_by().into(),
            recorded_at: r.recorded_at(),
            modified_by: r.modified_by().into(),
            updated_at: r.updated_at(),
        }
    }
}

/// A Check in a day's round, with its result.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckDayItemDto {
    /// The Check.
    pub check: CheckDto,
    /// Its result (`null`: unrecorded).
    pub result: Option<CheckResultDto>,
}

/// The round of a Check Date.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckDayDto {
    /// The Check Date shown.
    pub check_date: NaiveDate,
    /// Today's Check Date (in the instance time zone).
    pub today: NaiveDate,
    /// Checks in the round, in order.
    pub items: Vec<CheckDayItemDto>,
}

impl From<Day> for CheckDayDto {
    fn from(d: Day) -> Self {
        Self {
            check_date: d.check_date,
            today: d.today,
            items: d
                .items
                .into_iter()
                .map(|i| CheckDayItemDto {
                    check: i.view.into(),
                    result: i.result.map(Into::into),
                })
                .collect(),
        }
    }
}

/// Picks the day.
#[derive(Debug, Deserialize, IntoParams)]
pub struct CheckDayParams {
    /// Check Date (default today).
    pub date: Option<NaiveDate>,
    /// Only this Company.
    pub company_id: Option<Uuid>,
    /// Only Checks routed to you or one of your Teams.
    pub mine: Option<bool>,
}

/// The round of a Check Date: every Check in the round with its result (needs
/// `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/results", tag = "Morning Checks", params(CheckDayParams), responses((status = 200, body = CheckDayDto)))]
async fn check_day(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<CheckDayParams>,
) -> ApiResult<Json<CheckDayDto>> {
    Ok(Json(
        api.service
            .day(
                &p,
                q.date,
                q.company_id.map(TenantId::from_uuid),
                q.mine.unwrap_or(false),
            )
            .await?
            .into(),
    ))
}

/// Records a Check Result.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RecordCheckResultRequest {
    /// The Check.
    pub check_id: Uuid,
    /// Check Date (default today; never after today).
    pub check_date: Option<NaiveDate>,
    /// Check Status (active).
    pub status_id: Uuid,
    /// Notes (required when the status requires them).
    #[serde(default)]
    pub notes: String,
}

/// Records the result of a Check for a date, or changes the one already recorded (needs
/// `morning-checks.record`). 201 when new, 200 when changed.
#[utoipa::path(post, path = "/morning-checks/results", tag = "Morning Checks", request_body = RecordCheckResultRequest, responses((status = 201, body = CheckResultDto), (status = 200, body = CheckResultDto)))]
async fn record_check_result(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<RecordCheckResultRequest>,
) -> ApiResult<Response> {
    let (r, is_new) = api
        .service
        .record(
            &p,
            CheckId::from_uuid(b.check_id),
            b.check_date,
            b.status_id,
            &b.notes,
        )
        .await?;
    let status = if is_new {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(CheckResultDto::from(r))).into_response())
}

/// One Check Result (needs `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/results/{id}", tag = "Morning Checks", params(("id" = Uuid, Path)), responses((status = 200, body = CheckResultDto)))]
async fn get_check_result(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CheckResultDto>> {
    Ok(Json(
        api.service
            .result(&p, CheckResultId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Clears a Check Result: the Check is unrecorded again for that date, and the result's
/// Result Links go (needs `morning-checks.record`).
#[utoipa::path(delete, path = "/morning-checks/results/{id}", tag = "Morning Checks", params(("id" = Uuid, Path)), responses((status = 204)))]
async fn clear_check_result(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api.service.clear(&p, CheckResultId::from_uuid(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// One audited change of a Check Result.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckResultHistoryDto {
    /// When.
    pub at: DateTime<Utc>,
    /// Who.
    pub actor: CheckActorDto,
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

/// Who recorded a Check Result and every change since (needs `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/results/{id}/history", tag = "Morning Checks", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<CheckResultHistoryDto>)))]
async fn check_result_history(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<CheckResultHistoryDto>>> {
    Ok(Json(
        api.service
            .result_history(&p, CheckResultId::from_uuid(id))
            .await?
            .into_iter()
            .map(|e| CheckResultHistoryDto {
                at: e.occurred_at,
                actor: e.actor.into(),
                action: e.action.as_str().to_owned(),
                field: e.field,
                old_value: e.old_value,
                new_value: e.new_value,
            })
            .collect(),
    ))
}

// ----- Result Links (FR-MCK-004) ----------------------------------------------------------

/// A Result Link.
#[derive(Debug, Serialize, ToSchema)]
pub struct ResultLinkDto {
    /// `ticket` or `task`.
    pub kind: String,
    /// The ticket or Task.
    pub target_id: Uuid,
    /// Its Ticket Number or Task title.
    pub reference: String,
    /// Who linked it.
    pub linked_by: CheckActorDto,
    /// When.
    pub linked_at: DateTime<Utc>,
}

impl From<LinkView> for ResultLinkDto {
    fn from(v: LinkView) -> Self {
        Self {
            kind: v.link.kind.as_str().to_owned(),
            target_id: v.link.target_id,
            reference: v.reference,
            linked_by: v.link.linked_by.into(),
            linked_at: v.link.linked_at,
        }
    }
}

/// Links a follow-up record.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ResultLinkRequest {
    /// `ticket` or `task`.
    pub kind: String,
    /// The ticket or Task (same Company).
    pub target_id: Uuid,
}

/// Follow-up tickets and Tasks of a result that you may see (needs
/// `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/results/{id}/links", tag = "Morning Checks", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ResultLinkDto>)))]
async fn list_result_links(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ResultLinkDto>>> {
    Ok(Json(
        api.service
            .links(&p, CheckResultId::from_uuid(id))
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Links a follow-up ticket or Task of the same Company (needs `morning-checks.record`).
#[utoipa::path(post, path = "/morning-checks/results/{id}/links", tag = "Morning Checks", params(("id" = Uuid, Path)), request_body = ResultLinkRequest, responses((status = 201, body = ResultLinkDto)))]
async fn link_check_result(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ResultLinkRequest>,
) -> ApiResult<Response> {
    let kind = b.kind.parse::<LinkKind>()?;
    Ok(created(ResultLinkDto::from(
        api.service
            .link(&p, CheckResultId::from_uuid(id), kind, b.target_id)
            .await?,
    )))
}

/// Removes a Result Link; the ticket or Task stays (needs `morning-checks.record`).
#[utoipa::path(delete, path = "/morning-checks/results/{id}/links/{kind}/{target_id}", tag = "Morning Checks", params(("id" = Uuid, Path), ("kind" = String, Path), ("target_id" = Uuid, Path)), responses((status = 204)))]
async fn unlink_check_result(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path((id, kind, target)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service
        .unlink(
            &p,
            CheckResultId::from_uuid(id),
            kind.parse::<LinkKind>()?,
            target,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Creates a follow-up Task.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct FollowUpTaskRequest {
    /// Title (default: the Check, its status and date).
    pub title: Option<String>,
}

/// Creates a follow-up Task from a result, routed like the Check and carrying its notes,
/// and links it (needs `morning-checks.record` and `tasks.create`).
#[utoipa::path(post, path = "/morning-checks/results/{id}/follow-up-task", tag = "Morning Checks", params(("id" = Uuid, Path)), request_body = FollowUpTaskRequest, responses((status = 201, body = ResultLinkDto)))]
async fn create_follow_up_task(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<FollowUpTaskRequest>,
) -> ApiResult<Response> {
    Ok(created(ResultLinkDto::from(
        api.service
            .follow_up_task(&p, CheckResultId::from_uuid(id), b.title)
            .await?,
    )))
}

// ----- History and export (FR-MCK-005) ----------------------------------------------------

/// One cell of the history grid.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckHistoryCellDto {
    /// Check Date.
    pub check_date: NaiveDate,
    /// The result (`null`: unrecorded).
    pub result_id: Option<Uuid>,
    /// Its Check Status (colour-code with the status's `colour`).
    pub status_id: Option<Uuid>,
    /// Whether it has notes.
    pub has_notes: bool,
}

/// One row of the history grid.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckHistoryRowDto {
    /// The Check.
    pub check_id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Its name.
    pub name: String,
    /// Its Check Group's name.
    pub check_group_name: Option<String>,
    /// One cell per date, oldest first.
    pub cells: Vec<CheckHistoryCellDto>,
}

/// The history grid: Checks × dates, colour-coded by Check Status.
#[derive(Debug, Serialize, ToSchema)]
pub struct CheckHistoryDto {
    /// First date.
    pub from: NaiveDate,
    /// Last date.
    pub to: NaiveDate,
    /// The dates, oldest first.
    pub dates: Vec<NaiveDate>,
    /// Check Statuses, with their colours.
    pub statuses: Vec<CheckStatusDto>,
    /// One row per Check in the round or with results in the range.
    pub rows: Vec<CheckHistoryRowDto>,
}

impl From<HistoryGrid> for CheckHistoryDto {
    fn from(g: HistoryGrid) -> Self {
        let dates = g.range.days();
        Self {
            from: g.range.from(),
            to: g.range.to(),
            statuses: g.statuses.into_iter().map(Into::into).collect(),
            rows: g
                .rows
                .into_iter()
                .map(|r| CheckHistoryRowDto {
                    check_id: r.check.id().as_uuid(),
                    company_id: r.check.tenant_id().as_uuid(),
                    name: r.check.details().name.clone(),
                    check_group_name: r.group.map(|g| g.details().name.clone()),
                    cells: dates
                        .iter()
                        .zip(r.cells)
                        .map(|(d, c)| CheckHistoryCellDto {
                            check_date: *d,
                            result_id: c.as_ref().map(|r| r.id().as_uuid()),
                            status_id: c.as_ref().map(CheckResult::status_id),
                            has_notes: c.as_ref().is_some_and(|r| !r.notes().is_empty()),
                        })
                        .collect(),
                })
                .collect(),
            dates,
        }
    }
}

/// Picks the range.
#[derive(Debug, Deserialize, IntoParams)]
pub struct CheckHistoryParams {
    /// First date (default: 30 days before `to`).
    pub from: Option<NaiveDate>,
    /// Last date (default today).
    pub to: Option<NaiveDate>,
    /// Only this Company.
    pub company_id: Option<Uuid>,
}

/// The history grid of Checks × dates, at most 92 days (needs `morning-checks.read`).
#[utoipa::path(get, path = "/morning-checks/history", tag = "Morning Checks", params(CheckHistoryParams), responses((status = 200, body = CheckHistoryDto)))]
async fn check_history(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<CheckHistoryParams>,
) -> ApiResult<Json<CheckHistoryDto>> {
    Ok(Json(
        api.service
            .history(&p, q.from, q.to, q.company_id.map(TenantId::from_uuid))
            .await?
            .into(),
    ))
}

/// Recorded Check Results of a range (at most 366 days) as CSV, one row per result
/// (needs `morning-checks.read`). Cells a spreadsheet would run as formulas are
/// prefixed with `'`.
#[utoipa::path(get, path = "/morning-checks/history/export", tag = "Morning Checks", params(CheckHistoryParams), responses((status = 200, content_type = "text/csv", body = String)))]
async fn export_check_history(
    State(api): State<MorningChecksApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<CheckHistoryParams>,
) -> ApiResult<Response> {
    let rows = api
        .service
        .export(&p, q.from, q.to, q.company_id.map(TenantId::from_uuid))
        .await?;
    let mut csv = CsvWriter::new();
    csv.row([
        "Check Date",
        "Company ID",
        "Check Group",
        "Check",
        "Check Status",
        "Notes",
        "Recorded By",
        "Recorded At",
        "Modified By",
        "Modified At",
    ]);
    let file = match (rows.first(), rows.last()) {
        (Some(a), Some(b)) => format!("morning-checks-{}-{}.csv", a.check_date, b.check_date),
        _ => "morning-checks.csv".to_owned(),
    };
    for r in rows {
        csv.row([
            r.check_date.to_string(),
            r.company_id.to_string(),
            r.group,
            r.check,
            r.status,
            r.notes,
            r.recorded_by,
            r.recorded_at.to_rfc3339(),
            r.modified_by,
            r.updated_at.to_rfc3339(),
        ]);
    }
    Ok(download(&file, csv.finish()))
}
