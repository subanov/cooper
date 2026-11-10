//! HTTP API of Change Management (FR-CHG).

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{AnalystId, Cursor, Limit, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, ChangeService, NewChange};
use crate::domain::{
    ApprovalRoute, CabApprovalType, CabMember, Change, ChangeDetails, ChangeError, ChangeStage,
    ChangeTemplate, Decision, DiffLine, Lookup, LookupInput, LookupKind, Pir, RiskLevel,
    TemplateContent, Vote, risk_level,
};
use crate::ports::{
    ChangeAttachment, ChangeLink, ChangeQuery, ChecklistItem, Comment, LinkKind, TextRevision,
};

/// State the handlers need.
#[derive(Clone)]
pub struct ChangesApi {
    /// The service.
    pub service: ChangeService,
    /// Where uploaded files are stored.
    pub blobs: cooper_platform::attachments::BlobStore,
    /// Largest accepted upload.
    pub upload_limit: std::sync::Arc<dyn crate::ports::UploadLimit>,
}

/// `/api/v1` path prefixes owned by the Changes Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &[
    "/changes",
    "/change-calendar",
    "/change-types",
    "/change-statuses",
    "/change-priorities",
    "/change-impacts",
    "/change-categories",
    "/change-risk-levels",
    "/change-templates",
];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ChangesApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_change_types, create_change_type))
        .routes(routes!(update_change_type))
        .routes(routes!(list_change_statuses, create_change_status))
        .routes(routes!(update_change_status))
        .routes(routes!(list_change_priorities, create_change_priority))
        .routes(routes!(update_change_priority))
        .routes(routes!(list_change_impacts, create_change_impact))
        .routes(routes!(update_change_impact))
        .routes(routes!(list_change_categories, create_change_category))
        .routes(routes!(update_change_category))
        .routes(routes!(list_change_risk_levels, replace_change_risk_levels))
        .routes(routes!(list_change_templates, create_change_template))
        .routes(routes!(update_change_template))
        .routes(routes!(list_changes, create_change))
        .routes(routes!(change_calendar))
        .routes(routes!(get_change, edit_change))
        .routes(routes!(move_change))
        .routes(routes!(get_change_cab, set_change_cab))
        .routes(routes!(list_change_votes, vote_on_change))
        .routes(routes!(list_change_checklist, add_change_checklist_item))
        .routes(routes!(
            update_change_checklist_item,
            delete_change_checklist_item
        ))
        .routes(routes!(list_change_comments, add_change_comment))
        .routes(routes!(list_change_links, link_change))
        .routes(routes!(unlink_change))
        .routes(routes!(list_change_revisions))
        .routes(routes!(compare_change_revisions))
        .routes(routes!(record_change_pir))
        .routes(routes!(list_change_attachments, upload_change_attachment))
        .routes(routes!(download_change_attachment))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<ChangeError> for ApiError {
    fn from(e: ChangeError) -> Self {
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
            AppError::Rule(ChangeError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(ChangeError::InvalidState(_)) => typed(
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

fn parse<T: std::str::FromStr>(
    field: &'static str,
    v: &str,
    expected: &str,
) -> Result<T, ApiError> {
    v.parse()
        .map_err(|_| ApiError::from(ChangeError::invalid(field, format!("must be {expected}"))))
}

fn created<T: Serialize>(body: T) -> Response {
    (StatusCode::CREATED, Json(body)).into_response()
}

// ----- Reference data ---------------------------------------------------------------------

/// A Change Type, Status, Priority, Impact or Category.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeLookupDto {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// Stage (Statuses): `draft`, `awaiting`, `approved`, `implementing`, `done`, `stopped`.
    pub stage: Option<String>,
    /// Closing status (Statuses).
    pub is_closed: bool,
    /// Approval route (Types): `review` or `emergency`.
    pub approval_route: Option<String>,
    /// Standard Change: pre-approved, skips approval (Types).
    pub is_standard: bool,
    /// Default for new Changes.
    pub is_default: bool,
    /// Display order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl From<Lookup> for ChangeLookupDto {
    fn from(l: Lookup) -> Self {
        Self {
            id: l.id,
            key: l.key,
            name: l.name,
            stage: (l.kind == LookupKind::Status).then(|| l.stage.as_str().to_owned()),
            is_closed: l.is_closed,
            approval_route: (l.kind == LookupKind::Type).then(|| l.route.as_str().to_owned()),
            is_standard: l.is_standard,
            is_default: l.is_default,
            sort_order: l.sort_order,
            is_active: l.is_active,
        }
    }
}

/// Creates or changes a Change Type, Status, Priority, Impact or Category.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangeLookupRequest {
    /// Name.
    pub name: String,
    /// Stage (Statuses; fixed once created).
    pub stage: Option<String>,
    /// Closing status (Statuses).
    #[serde(default)]
    pub is_closed: bool,
    /// Approval route (Types): `review` (default) or `emergency`.
    pub approval_route: Option<String>,
    /// Standard Change (Types).
    #[serde(default)]
    pub is_standard: bool,
    /// Make it the default.
    #[serde(default)]
    pub is_default: bool,
    /// Display order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active.
    #[serde(default = "yes")]
    pub is_active: bool,
}

const fn yes() -> bool {
    true
}

impl ChangeLookupRequest {
    fn input(self) -> Result<LookupInput, ApiError> {
        let stage = match self.stage.as_deref() {
            Some(s) => parse::<ChangeStage>(
                "stage",
                s,
                "draft, awaiting, approved, implementing, done or stopped",
            )?,
            None => ChangeStage::Draft,
        };
        let route = match self.approval_route.as_deref() {
            Some(r) => parse::<ApprovalRoute>("approval_route", r, "review or emergency")?,
            None => ApprovalRoute::Review,
        };
        Ok(LookupInput {
            name: self.name,
            stage,
            is_closed: self.is_closed,
            route,
            is_standard: self.is_standard,
            is_default: self.is_default,
            sort_order: self.sort_order,
            is_active: self.is_active,
        })
    }
}

macro_rules! lookup_handlers {
    ($kind:expr, $path:literal, $item:literal, $list:ident, $create:ident, $update:ident) => {
        #[utoipa::path(get, path = $path, tag = "Changes", responses((status = 200, body = Vec<ChangeLookupDto>)))]
        async fn $list(State(api): State<ChangesApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<ChangeLookupDto>>> {
            Ok(Json(api.service.lookups(&p, $kind).await?.into_iter().map(Into::into).collect()))
        }

        #[utoipa::path(post, path = $path, tag = "Changes", request_body = ChangeLookupRequest, responses((status = 201, body = ChangeLookupDto)))]
        async fn $create(
            State(api): State<ChangesApi>,
            Authenticated(p): Authenticated,
            Json(b): Json<ChangeLookupRequest>,
        ) -> ApiResult<Response> {
            let l = api.service.create_lookup(&p, $kind, b.input()?).await?;
            Ok(created(ChangeLookupDto::from(l)))
        }

        #[utoipa::path(put, path = $item, tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangeLookupRequest, responses((status = 200, body = ChangeLookupDto)))]
        async fn $update(
            State(api): State<ChangesApi>,
            Authenticated(p): Authenticated,
            Path(id): Path<Uuid>,
            Json(b): Json<ChangeLookupRequest>,
        ) -> ApiResult<Json<ChangeLookupDto>> {
            Ok(Json(api.service.update_lookup(&p, $kind, id, b.input()?).await?.into()))
        }
    };
}

lookup_handlers!(
    LookupKind::Type,
    "/change-types",
    "/change-types/{id}",
    list_change_types,
    create_change_type,
    update_change_type
);
lookup_handlers!(
    LookupKind::Status,
    "/change-statuses",
    "/change-statuses/{id}",
    list_change_statuses,
    create_change_status,
    update_change_status
);
lookup_handlers!(
    LookupKind::Priority,
    "/change-priorities",
    "/change-priorities/{id}",
    list_change_priorities,
    create_change_priority,
    update_change_priority
);
lookup_handlers!(
    LookupKind::Impact,
    "/change-impacts",
    "/change-impacts/{id}",
    list_change_impacts,
    create_change_impact,
    update_change_impact
);
lookup_handlers!(
    LookupKind::Category,
    "/change-categories",
    "/change-categories/{id}",
    list_change_categories,
    create_change_category,
    update_change_category
);

/// A Risk Level: a named band of risk scores (likelihood × impact, 1–25).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ChangeRiskLevelDto {
    /// ID (output; ignored on input).
    #[serde(default)]
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Lowest score (inclusive).
    pub min_score: u8,
    /// Highest score (inclusive).
    pub max_score: u8,
    /// Display order.
    #[serde(default)]
    pub sort_order: i32,
}

impl From<RiskLevel> for ChangeRiskLevelDto {
    fn from(b: RiskLevel) -> Self {
        Self {
            id: b.id,
            name: b.name,
            min_score: b.min_score,
            max_score: b.max_score,
            sort_order: b.sort_order,
        }
    }
}

/// Risk Levels by score (FR-CHG-005).
#[utoipa::path(get, path = "/change-risk-levels", tag = "Changes", responses((status = 200, body = Vec<ChangeRiskLevelDto>)))]
async fn list_change_risk_levels(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<ChangeRiskLevelDto>>> {
    Ok(Json(
        api.service
            .risk_bands(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Replaces the Risk Levels; together they must cover scores 1–25 exactly once (needs
/// `changes.configure`).
#[utoipa::path(put, path = "/change-risk-levels", tag = "Changes", request_body = Vec<ChangeRiskLevelDto>, responses((status = 200, body = Vec<ChangeRiskLevelDto>)))]
async fn replace_change_risk_levels(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<Vec<ChangeRiskLevelDto>>,
) -> ApiResult<Json<Vec<ChangeRiskLevelDto>>> {
    let bands = b
        .into_iter()
        .map(|d| RiskLevel {
            id: Uuid::now_v7(),
            name: d.name,
            min_score: d.min_score,
            max_score: d.max_score,
            sort_order: d.sort_order,
        })
        .collect();
    Ok(Json(
        api.service
            .set_risk_bands(&p, bands)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

// ----- Templates ----------------------------------------------------------------------------

/// The fields a Change Template pre-fills.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ChangeTemplateContentDto {
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Change Type.
    pub type_id: Option<Uuid>,
    /// Priority.
    pub priority_id: Option<Uuid>,
    /// Impact.
    pub impact_id: Option<Uuid>,
    /// Category.
    pub category_id: Option<Uuid>,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Reason for Change.
    #[serde(default)]
    pub reason_for_change: String,
    /// Risk Evaluation.
    #[serde(default)]
    pub risk_evaluation: String,
    /// Likelihood (1–5).
    pub risk_likelihood: Option<u8>,
    /// Impact score (1–5).
    pub risk_impact_score: Option<u8>,
    /// Test Plan.
    #[serde(default)]
    pub test_plan: String,
    /// Rollback Plan.
    #[serde(default)]
    pub rollback_plan: String,
    /// CAB approval required.
    #[serde(default)]
    pub cab_required: bool,
    /// `all` or `majority`.
    pub cab_approval_type: Option<String>,
}

impl From<TemplateContent> for ChangeTemplateContentDto {
    fn from(c: TemplateContent) -> Self {
        Self {
            title: c.title,
            type_id: c.type_id,
            priority_id: c.priority_id,
            impact_id: c.impact_id,
            category_id: c.category_id,
            description: c.description,
            reason_for_change: c.reason_for_change,
            risk_evaluation: c.risk_evaluation,
            risk_likelihood: c.risk_likelihood,
            risk_impact_score: c.risk_impact_score,
            test_plan: c.test_plan,
            rollback_plan: c.rollback_plan,
            cab_required: c.cab_required,
            cab_approval_type: c.cab_approval_type.map(|t| t.as_str().to_owned()),
        }
    }
}

impl ChangeTemplateContentDto {
    fn content(self) -> Result<TemplateContent, ApiError> {
        Ok(TemplateContent {
            title: self.title,
            type_id: self.type_id,
            priority_id: self.priority_id,
            impact_id: self.impact_id,
            category_id: self.category_id,
            description: self.description,
            reason_for_change: self.reason_for_change,
            risk_evaluation: self.risk_evaluation,
            risk_likelihood: self.risk_likelihood,
            risk_impact_score: self.risk_impact_score,
            test_plan: self.test_plan,
            rollback_plan: self.rollback_plan,
            cab_required: self.cab_required,
            cab_approval_type: cab_type(self.cab_approval_type.as_deref())?,
        })
    }
}

fn cab_type(v: Option<&str>) -> Result<Option<CabApprovalType>, ApiError> {
    v.map(|t| parse::<CabApprovalType>("cab_approval_type", t, "all or majority"))
        .transpose()
}

/// A Change Template (FR-CHG-010).
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeTemplateDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// What it is for.
    pub description: String,
    /// Teams that may use it (empty: everyone).
    pub team_ids: Vec<Uuid>,
    /// Current version (grows when the content changes).
    pub version: i32,
    /// Content of the current version.
    pub content: ChangeTemplateContentDto,
    /// Active.
    pub is_active: bool,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<ChangeTemplate> for ChangeTemplateDto {
    fn from(t: ChangeTemplate) -> Self {
        Self {
            id: t.id,
            name: t.name,
            description: t.description,
            team_ids: t.team_ids.iter().map(TeamId::as_uuid).collect(),
            version: t.version,
            content: t.content.into(),
            is_active: t.is_active,
            updated_at: t.updated_at,
        }
    }
}

/// Creates or changes a Change Template.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangeTemplateRequest {
    /// Name.
    pub name: String,
    /// What it is for.
    #[serde(default)]
    pub description: String,
    /// Teams that may use it (empty: everyone).
    #[serde(default)]
    pub team_ids: Vec<Uuid>,
    /// Content.
    pub content: ChangeTemplateContentDto,
    /// Active.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl ChangeTemplateRequest {
    fn template(self) -> Result<ChangeTemplate, ApiError> {
        let now = Utc::now();
        Ok(ChangeTemplate {
            id: Uuid::nil(),
            name: self.name,
            description: self.description,
            team_ids: self.team_ids.into_iter().map(TeamId::from_uuid).collect(),
            version: 0,
            content: self.content.content()?,
            is_active: self.is_active,
            created_at: now,
            updated_at: now,
        })
    }
}

/// Change Templates; Analysts see those usable by one of their Teams.
#[utoipa::path(get, path = "/change-templates", tag = "Changes", responses((status = 200, body = Vec<ChangeTemplateDto>)))]
async fn list_change_templates(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<ChangeTemplateDto>>> {
    Ok(Json(
        api.service
            .templates(&p)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Creates a Change Template (needs `changes.configure`).
#[utoipa::path(post, path = "/change-templates", tag = "Changes", request_body = ChangeTemplateRequest, responses((status = 201, body = ChangeTemplateDto)))]
async fn create_change_template(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<ChangeTemplateRequest>,
) -> ApiResult<Response> {
    let t = api.service.save_template(&p, None, b.template()?).await?;
    Ok(created(ChangeTemplateDto::from(t)))
}

/// Changes a Change Template; new content makes a new version, Changes keep the version
/// they were created from (needs `changes.configure`).
#[utoipa::path(put, path = "/change-templates/{id}", tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangeTemplateRequest, responses((status = 200, body = ChangeTemplateDto)))]
async fn update_change_template(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeTemplateRequest>,
) -> ApiResult<Json<ChangeTemplateDto>> {
    Ok(Json(
        api.service
            .save_template(&p, Some(id), b.template()?)
            .await?
            .into(),
    ))
}

// ----- Changes ------------------------------------------------------------------------------

/// A Change (FR-CHG-001).
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeDto {
    /// ID.
    pub id: Uuid,
    /// Change Number (`CHG-00001`).
    pub number: String,
    /// Company.
    pub company_id: Uuid,
    /// Title.
    pub title: String,
    /// Change Type.
    pub type_id: Uuid,
    /// Status.
    pub status_id: Uuid,
    /// Stage of the status.
    pub stage: String,
    /// Priority.
    pub priority_id: Uuid,
    /// Impact.
    pub impact_id: Uuid,
    /// Category.
    pub category_id: Option<Uuid>,
    /// Requested By (an Analyst).
    pub requested_by_id: Option<Uuid>,
    /// Assigned To.
    pub assigned_to_id: Option<Uuid>,
    /// Approver (when no CAB is required).
    pub approver_id: Option<Uuid>,
    /// Work Window start.
    pub work_start: Option<DateTime<Utc>>,
    /// Work Window end.
    pub work_end: Option<DateTime<Utc>>,
    /// Outage Window start.
    pub outage_start: Option<DateTime<Utc>>,
    /// Outage Window end.
    pub outage_end: Option<DateTime<Utc>>,
    /// Description.
    pub description: String,
    /// Reason for Change.
    pub reason_for_change: String,
    /// Risk Evaluation.
    pub risk_evaluation: String,
    /// Likelihood (1–5).
    pub risk_likelihood: Option<u8>,
    /// Impact score (1–5).
    pub risk_impact_score: Option<u8>,
    /// Risk score (likelihood × impact).
    pub risk_score: Option<u8>,
    /// Risk Level name.
    pub risk_level: Option<String>,
    /// Test Plan.
    pub test_plan: String,
    /// Rollback Plan.
    pub rollback_plan: String,
    /// CAB approval required.
    pub cab_required: bool,
    /// `all` or `majority`.
    pub cab_approval_type: Option<String>,
    /// Current Approval Round (0 before the first submission).
    pub approval_round: i32,
    /// When the current round approved it.
    pub approved_at: Option<DateTime<Utc>>,
    /// Template it was created from.
    pub template_id: Option<Uuid>,
    /// Template version it was created from.
    pub template_version: Option<i32>,
    /// Post-Implementation Review.
    pub pir: Option<ChangePirDto>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
    /// Closed.
    pub closed_at: Option<DateTime<Utc>>,
}

impl ChangeDto {
    fn new(c: Change, bands: &[RiskLevel]) -> Self {
        let d = c.details().clone();
        let score = d.risk_score();
        Self {
            id: c.id(),
            number: c.number().to_owned(),
            company_id: c.tenant_id().as_uuid(),
            title: d.title,
            type_id: d.type_id,
            status_id: c.status_id(),
            stage: c.stage().as_str().to_owned(),
            priority_id: d.priority_id,
            impact_id: d.impact_id,
            category_id: d.category_id,
            requested_by_id: d.requested_by.map(|a| a.as_uuid()),
            assigned_to_id: d.assigned_to.map(|a| a.as_uuid()),
            approver_id: d.approver.map(|a| a.as_uuid()),
            work_start: d.work_start,
            work_end: d.work_end,
            outage_start: d.outage_start,
            outage_end: d.outage_end,
            description: d.description,
            reason_for_change: d.reason_for_change,
            risk_evaluation: d.risk_evaluation,
            risk_likelihood: d.risk_likelihood,
            risk_impact_score: d.risk_impact_score,
            risk_score: score,
            risk_level: score
                .and_then(|s| risk_level(s, bands))
                .map(|b| b.name.clone()),
            test_plan: d.test_plan,
            rollback_plan: d.rollback_plan,
            cab_required: d.cab_required,
            cab_approval_type: d.cab_approval_type.map(|t| t.as_str().to_owned()),
            approval_round: c.approval_round(),
            approved_at: c.approved_at(),
            template_id: c.template().map(|t| t.0),
            template_version: c.template().map(|t| t.1),
            pir: c.pir().cloned().map(Into::into),
            created_at: c.created_at(),
            updated_at: c.updated_at(),
            closed_at: c.closed_at(),
        }
    }
}

/// Creates or edits a Change. Absent Type, Priority and Impact take the defaults (or the
/// template's values).
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangeRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Change Template to pre-fill empty fields from (create only).
    pub template_id: Option<Uuid>,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Change Type.
    pub type_id: Option<Uuid>,
    /// Priority.
    pub priority_id: Option<Uuid>,
    /// Impact.
    pub impact_id: Option<Uuid>,
    /// Category.
    pub category_id: Option<Uuid>,
    /// Requested By (an Analyst).
    pub requested_by_id: Option<Uuid>,
    /// Assigned To.
    pub assigned_to_id: Option<Uuid>,
    /// Approver (when no CAB is required).
    pub approver_id: Option<Uuid>,
    /// Work Window start.
    pub work_start: Option<DateTime<Utc>>,
    /// Work Window end.
    pub work_end: Option<DateTime<Utc>>,
    /// Outage Window start.
    pub outage_start: Option<DateTime<Utc>>,
    /// Outage Window end.
    pub outage_end: Option<DateTime<Utc>>,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Reason for Change.
    #[serde(default)]
    pub reason_for_change: String,
    /// Risk Evaluation.
    #[serde(default)]
    pub risk_evaluation: String,
    /// Likelihood (1–5).
    pub risk_likelihood: Option<u8>,
    /// Impact score (1–5).
    pub risk_impact_score: Option<u8>,
    /// Test Plan.
    #[serde(default)]
    pub test_plan: String,
    /// Rollback Plan.
    #[serde(default)]
    pub rollback_plan: String,
    /// CAB approval required.
    #[serde(default)]
    pub cab_required: bool,
    /// `all` (default) or `majority`.
    pub cab_approval_type: Option<String>,
}

impl ChangeRequest {
    fn details(&self) -> Result<ChangeDetails, ApiError> {
        Ok(ChangeDetails {
            title: self.title.clone(),
            type_id: self.type_id.unwrap_or_else(Uuid::nil),
            priority_id: self.priority_id.unwrap_or_else(Uuid::nil),
            impact_id: self.impact_id.unwrap_or_else(Uuid::nil),
            category_id: self.category_id,
            requested_by: self.requested_by_id.map(AnalystId::from_uuid),
            assigned_to: self.assigned_to_id.map(AnalystId::from_uuid),
            approver: self.approver_id.map(AnalystId::from_uuid),
            work_start: self.work_start,
            work_end: self.work_end,
            outage_start: self.outage_start,
            outage_end: self.outage_end,
            description: self.description.clone(),
            reason_for_change: self.reason_for_change.clone(),
            risk_evaluation: self.risk_evaluation.clone(),
            risk_likelihood: self.risk_likelihood,
            risk_impact_score: self.risk_impact_score,
            test_plan: self.test_plan.clone(),
            rollback_plan: self.rollback_plan.clone(),
            cab_required: self.cab_required,
            cab_approval_type: cab_type(self.cab_approval_type.as_deref())?,
        })
    }
}

/// A page of Changes.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangePage {
    /// Changes, newest activity first.
    pub items: Vec<ChangeDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Filters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ChangeListParams {
    /// Comma-separated stages (`draft,awaiting`).
    pub stage: Option<String>,
    /// Assigned To.
    pub assigned_to_id: Option<Uuid>,
    /// Waiting for my vote (Approver or CAB member of the open round).
    pub awaiting_my_vote: Option<bool>,
    /// Linked to this ticket.
    pub ticket_id: Option<Uuid>,
    /// Linked to this Problem.
    pub problem_id: Option<Uuid>,
    /// Number or title text.
    pub q: Option<String>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

async fn bands(api: &ChangesApi, p: &cooper_kernel::Principal) -> ApiResult<Vec<RiskLevel>> {
    Ok(api.service.risk_bands(p).await?)
}

/// Changes in your Companies, newest activity first (needs `changes.read`).
#[utoipa::path(get, path = "/changes", tag = "Changes", params(ChangeListParams), responses((status = 200, body = ChangePage)))]
async fn list_changes(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ChangeListParams>,
) -> ApiResult<Json<ChangePage>> {
    let limit = q
        .limit
        .map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| ChangeError::invalid("limit", "must be between 1 and 200"))?;
    let after = q
        .cursor
        .map(|c| Cursor::from_client(c).decode::<(DateTime<Utc>, Uuid)>())
        .transpose()
        .map_err(|_| ChangeError::invalid("cursor", "is not valid"))?;
    let stages = q
        .stage
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            parse::<ChangeStage>(
                "stage",
                s,
                "draft, awaiting, approved, implementing, done or stopped",
            )
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let awaiting_vote_of = if q.awaiting_my_vote == Some(true) {
        match p.actor() {
            cooper_kernel::Actor::Analyst(a) => Some(a),
            _ => return Err(ChangeError::invalid("awaiting_my_vote", "needs an Analyst").into()),
        }
    } else {
        None
    };
    let linked = match (q.ticket_id, q.problem_id) {
        (Some(t), None) => Some((LinkKind::Ticket, t)),
        (None, Some(pr)) => Some((LinkKind::Problem, pr)),
        (None, None) => None,
        (Some(_), Some(_)) => {
            return Err(
                ChangeError::invalid("problem_id", "cannot be combined with ticket_id").into(),
            );
        }
    };
    let query = ChangeQuery {
        stages,
        assigned_to: q.assigned_to_id.map(AnalystId::from_uuid),
        awaiting_vote_of,
        linked,
        search: q.q,
        limit,
        after,
    };
    let rows = api.service.list(&p, &query).await?;
    let bands = bands(&api, &p).await?;
    let page =
        cooper_kernel::Page::from_rows(rows, limit, |c| Cursor::encode(&(c.updated_at(), c.id())))
            .map_err(|_| AppError::Internal)?;
    Ok(Json(ChangePage {
        items: page
            .items
            .into_iter()
            .map(|c| ChangeDto::new(c, &bands))
            .collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Creates a Change in Draft (needs `changes.create`).
#[utoipa::path(post, path = "/changes", tag = "Changes", request_body = ChangeRequest, responses((status = 201, body = ChangeDto)))]
async fn create_change(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<ChangeRequest>,
) -> ApiResult<Response> {
    let new = NewChange {
        tenant_id: b.company_id.map(TenantId::from_uuid),
        details: b.details()?,
        template_id: b.template_id,
    };
    let change = api.service.create(&p, new).await?;
    Ok(created(ChangeDto::new(change, &bands(&api, &p).await?)))
}

/// One Change.
#[utoipa::path(get, path = "/changes/{id}", tag = "Changes", params(("id" = Uuid, Path)), responses((status = 200, body = ChangeDto)))]
async fn get_change(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ChangeDto>> {
    let change = api.service.get(&p, id).await?;
    Ok(Json(ChangeDto::new(change, &bands(&api, &p).await?)))
}

/// Edits a Change. A material edit while awaiting approval or approved returns it to
/// Awaiting Approval with a new Approval Round (needs `changes.update`).
#[utoipa::path(put, path = "/changes/{id}", tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangeRequest, responses((status = 200, body = ChangeDto)))]
async fn edit_change(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeRequest>,
) -> ApiResult<Json<ChangeDto>> {
    let change = api.service.edit(&p, id, b.details()?).await?;
    Ok(Json(ChangeDto::new(change, &bands(&api, &p).await?)))
}

/// A status move.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangeStatusRequest {
    /// The status.
    pub status_id: Uuid,
}

/// Moves a Change to another status, following the stage order (needs `changes.update`).
#[utoipa::path(post, path = "/changes/{id}/status", tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangeStatusRequest, responses((status = 200, body = ChangeDto)))]
async fn move_change(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeStatusRequest>,
) -> ApiResult<Json<ChangeDto>> {
    let change = api.service.move_to(&p, id, b.status_id).await?;
    Ok(Json(ChangeDto::new(change, &bands(&api, &p).await?)))
}

// ----- Calendar -----------------------------------------------------------------------------

/// The period.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ChangeCalendarParams {
    /// Start (inclusive).
    pub from: DateTime<Utc>,
    /// End (exclusive; at most 366 days after `from`).
    pub to: DateTime<Utc>,
}

/// Two Changes whose windows overlap.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeOverlapDto {
    /// One Change.
    pub change_id: Uuid,
    /// The other.
    pub other_change_id: Uuid,
}

/// The Change Calendar.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeCalendarDto {
    /// Changes with a Work or Outage Window in the period.
    pub changes: Vec<ChangeDto>,
    /// Conflicts (overlapping windows of open Changes).
    pub overlaps: Vec<ChangeOverlapDto>,
}

/// Changes scheduled in a period, with conflicts (FR-CHG-012).
#[utoipa::path(get, path = "/change-calendar", tag = "Changes", params(ChangeCalendarParams), responses((status = 200, body = ChangeCalendarDto)))]
async fn change_calendar(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ChangeCalendarParams>,
) -> ApiResult<Json<ChangeCalendarDto>> {
    let cal = api.service.calendar(&p, q.from, q.to).await?;
    let bands = bands(&api, &p).await?;
    Ok(Json(ChangeCalendarDto {
        changes: cal
            .changes
            .into_iter()
            .map(|c| ChangeDto::new(c, &bands))
            .collect(),
        overlaps: cal
            .overlaps
            .into_iter()
            .map(|(a, b)| ChangeOverlapDto {
                change_id: a,
                other_change_id: b,
            })
            .collect(),
    }))
}

// ----- CAB and votes ------------------------------------------------------------------------

/// A CAB member.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ChangeCabMemberDto {
    /// The Analyst.
    pub analyst_id: Uuid,
    /// Their vote counts towards the outcome (optional members only advise).
    #[serde(default = "yes")]
    pub is_required: bool,
}

impl From<CabMember> for ChangeCabMemberDto {
    fn from(m: CabMember) -> Self {
        Self {
            analyst_id: m.analyst_id.as_uuid(),
            is_required: m.is_required,
        }
    }
}

/// The CAB of a Change.
#[utoipa::path(get, path = "/changes/{id}/cab", tag = "Changes", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ChangeCabMemberDto>)))]
async fn get_change_cab(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ChangeCabMemberDto>>> {
    Ok(Json(
        api.service
            .cab(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Replaces the CAB while the Change is in Draft (needs `changes.update`).
#[utoipa::path(put, path = "/changes/{id}/cab", tag = "Changes", params(("id" = Uuid, Path)), request_body = Vec<ChangeCabMemberDto>, responses((status = 200, body = Vec<ChangeCabMemberDto>)))]
async fn set_change_cab(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<Vec<ChangeCabMemberDto>>,
) -> ApiResult<Json<Vec<ChangeCabMemberDto>>> {
    let members = b
        .into_iter()
        .map(|m| CabMember {
            analyst_id: AnalystId::from_uuid(m.analyst_id),
            is_required: m.is_required,
        })
        .collect();
    Ok(Json(
        api.service
            .set_cab(&p, id, members)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// A vote.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeVoteDto {
    /// ID.
    pub id: Uuid,
    /// Approval Round.
    pub round_no: i32,
    /// The Analyst.
    pub analyst_id: Uuid,
    /// `approve` or `reject`.
    pub decision: String,
    /// Comment.
    pub comment: String,
    /// When.
    pub voted_at: DateTime<Utc>,
}

impl From<Vote> for ChangeVoteDto {
    fn from(v: Vote) -> Self {
        Self {
            id: v.id,
            round_no: v.round_no,
            analyst_id: v.analyst_id.as_uuid(),
            decision: v.decision.as_str().to_owned(),
            comment: v.comment,
            voted_at: v.voted_at,
        }
    }
}

/// Which round.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ChangeVoteParams {
    /// Approval Round (default: the current one).
    pub round: Option<i32>,
}

/// Votes of an Approval Round.
#[utoipa::path(get, path = "/changes/{id}/votes", tag = "Changes", params(("id" = Uuid, Path), ChangeVoteParams), responses((status = 200, body = Vec<ChangeVoteDto>)))]
async fn list_change_votes(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Query(q): Query<ChangeVoteParams>,
) -> ApiResult<Json<Vec<ChangeVoteDto>>> {
    Ok(Json(
        api.service
            .votes(&p, id, q.round)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Casts a vote.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangeVoteRequest {
    /// `approve` or `reject`.
    pub decision: String,
    /// Comment (required to reject).
    #[serde(default)]
    pub comment: String,
}

/// Votes in the current Approval Round as Approver or CAB member; the last deciding
/// vote approves or rejects the Change (needs `changes.approve`).
#[utoipa::path(post, path = "/changes/{id}/votes", tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangeVoteRequest, responses((status = 200, body = ChangeDto)))]
async fn vote_on_change(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeVoteRequest>,
) -> ApiResult<Json<ChangeDto>> {
    let decision = parse::<Decision>("decision", &b.decision, "approve or reject")?;
    let change = api.service.vote(&p, id, decision, &b.comment).await?;
    Ok(Json(ChangeDto::new(change, &bands(&api, &p).await?)))
}

// ----- Checklist, comments, links -----------------------------------------------------------

/// A checklist item.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeChecklistItemDto {
    /// ID.
    pub id: Uuid,
    /// Text.
    pub text: String,
    /// Done.
    pub is_done: bool,
    /// Who ticked it.
    pub done_by_id: Option<Uuid>,
    /// When.
    pub done_at: Option<DateTime<Utc>>,
    /// Display order.
    pub sort_order: i32,
}

impl From<ChecklistItem> for ChangeChecklistItemDto {
    fn from(i: ChecklistItem) -> Self {
        Self {
            id: i.id,
            text: i.text,
            is_done: i.is_done,
            done_by_id: i.done_by.map(|a| a.as_uuid()),
            done_at: i.done_at,
            sort_order: i.sort_order,
        }
    }
}

/// Adds or changes a checklist item.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangeChecklistItemRequest {
    /// Text.
    pub text: String,
    /// Done.
    #[serde(default)]
    pub is_done: bool,
    /// Display order.
    #[serde(default)]
    pub sort_order: i32,
}

/// The implementation checklist.
#[utoipa::path(get, path = "/changes/{id}/checklist", tag = "Changes", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ChangeChecklistItemDto>)))]
async fn list_change_checklist(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ChangeChecklistItemDto>>> {
    Ok(Json(
        api.service
            .checklist(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Adds a checklist item (needs `changes.update`).
#[utoipa::path(post, path = "/changes/{id}/checklist", tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangeChecklistItemRequest, responses((status = 201, body = ChangeChecklistItemDto)))]
async fn add_change_checklist_item(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeChecklistItemRequest>,
) -> ApiResult<Response> {
    let item = api
        .service
        .save_item(&p, id, None, &b.text, b.is_done, b.sort_order)
        .await?;
    Ok(created(ChangeChecklistItemDto::from(item)))
}

/// Changes or ticks a checklist item (needs `changes.update`).
#[utoipa::path(put, path = "/changes/{id}/checklist/{item_id}", tag = "Changes", params(("id" = Uuid, Path), ("item_id" = Uuid, Path)), request_body = ChangeChecklistItemRequest, responses((status = 200, body = ChangeChecklistItemDto)))]
async fn update_change_checklist_item(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path((id, item)): Path<(Uuid, Uuid)>,
    Json(b): Json<ChangeChecklistItemRequest>,
) -> ApiResult<Json<ChangeChecklistItemDto>> {
    Ok(Json(
        api.service
            .save_item(&p, id, Some(item), &b.text, b.is_done, b.sort_order)
            .await?
            .into(),
    ))
}

/// Removes a checklist item (needs `changes.update`).
#[utoipa::path(delete, path = "/changes/{id}/checklist/{item_id}", tag = "Changes", params(("id" = Uuid, Path), ("item_id" = Uuid, Path)), responses((status = 204)))]
async fn delete_change_checklist_item(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path((id, item)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service.delete_item(&p, id, item).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A comment.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeCommentDto {
    /// ID.
    pub id: Uuid,
    /// Author.
    pub author_id: Option<Uuid>,
    /// Text.
    pub body: String,
    /// When.
    pub created_at: DateTime<Utc>,
}

impl From<Comment> for ChangeCommentDto {
    fn from(c: Comment) -> Self {
        Self {
            id: c.id,
            author_id: c.author_id.map(|a| a.as_uuid()),
            body: c.body,
            created_at: c.created_at,
        }
    }
}

/// Adds a comment.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangeCommentRequest {
    /// Text.
    pub body: String,
}

/// Comments, oldest first.
#[utoipa::path(get, path = "/changes/{id}/comments", tag = "Changes", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ChangeCommentDto>)))]
async fn list_change_comments(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ChangeCommentDto>>> {
    Ok(Json(
        api.service
            .comments(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Adds a comment (needs `changes.update`).
#[utoipa::path(post, path = "/changes/{id}/comments", tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangeCommentRequest, responses((status = 201, body = ChangeCommentDto)))]
async fn add_change_comment(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeCommentRequest>,
) -> ApiResult<Response> {
    Ok(created(ChangeCommentDto::from(
        api.service.add_comment(&p, id, &b.body).await?,
    )))
}

/// A linked ticket, Problem or Change.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeLinkDto {
    /// `ticket`, `problem` or `change`.
    pub kind: String,
    /// The linked record.
    pub target_id: Uuid,
    /// Its number (Ticket Number, Problem Number or Change Number).
    pub number: String,
    /// When it was linked.
    pub linked_at: DateTime<Utc>,
}

impl From<(ChangeLink, String)> for ChangeLinkDto {
    fn from((l, number): (ChangeLink, String)) -> Self {
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
pub struct ChangeLinkRequest {
    /// `ticket`, `problem` or `change`.
    pub kind: String,
    /// The record (in the same Company).
    pub target_id: Uuid,
}

/// Linked tickets, Problems and Changes (FR-CHG-008).
#[utoipa::path(get, path = "/changes/{id}/links", tag = "Changes", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ChangeLinkDto>)))]
async fn list_change_links(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ChangeLinkDto>>> {
    Ok(Json(
        api.service
            .links(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Links a ticket, Problem or Change of the same Company (needs `changes.update`).
#[utoipa::path(post, path = "/changes/{id}/links", tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangeLinkRequest, responses((status = 201, body = ChangeLinkDto)))]
async fn link_change(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangeLinkRequest>,
) -> ApiResult<Response> {
    let kind = parse::<LinkKind>("kind", &b.kind, "ticket, problem or change")?;
    Ok(created(ChangeLinkDto::from(
        api.service.link(&p, id, kind, b.target_id).await?,
    )))
}

/// Removes a link (needs `changes.update`).
#[utoipa::path(delete, path = "/changes/{id}/links/{kind}/{target_id}", tag = "Changes", params(("id" = Uuid, Path), ("kind" = String, Path), ("target_id" = Uuid, Path)), responses((status = 204)))]
async fn unlink_change(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path((id, kind, target)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<StatusCode> {
    let kind = parse::<LinkKind>("kind", &kind, "ticket, problem or change")?;
    api.service.unlink(&p, id, kind, target).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- Text revisions -----------------------------------------------------------------------

/// A saved version of a long text field.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeTextRevisionDto {
    /// ID.
    pub id: Uuid,
    /// Field.
    pub field: String,
    /// Text.
    pub body: String,
    /// Who (`{kind, id}`).
    #[schema(value_type = Object)]
    pub author: serde_json::Value,
    /// When.
    pub created_at: DateTime<Utc>,
}

impl From<TextRevision> for ChangeTextRevisionDto {
    fn from(r: TextRevision) -> Self {
        Self {
            id: r.id,
            field: r.field,
            body: r.body,
            author: serde_json::json!({ "kind": r.author.kind().as_str(), "id": r.author.id() }),
            created_at: r.created_at,
        }
    }
}

/// Revisions of one long text field (`description`, `reason_for_change`,
/// `risk_evaluation`, `test_plan`, `rollback_plan`), oldest first (FR-CHG-009).
#[utoipa::path(get, path = "/changes/{id}/revisions/{field}", tag = "Changes", params(("id" = Uuid, Path), ("field" = String, Path)), responses((status = 200, body = Vec<ChangeTextRevisionDto>)))]
async fn list_change_revisions(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path((id, field)): Path<(Uuid, String)>,
) -> ApiResult<Json<Vec<ChangeTextRevisionDto>>> {
    Ok(Json(
        api.service
            .revisions(&p, id, &field)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Two revisions to compare.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ChangeCompareParams {
    /// Older revision.
    pub from: Uuid,
    /// Newer revision.
    pub to: Uuid,
}

/// One line of a comparison.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeDiffLineDto {
    /// `same`, `added` or `removed`.
    pub kind: String,
    /// The line.
    pub text: String,
}

impl From<DiffLine> for ChangeDiffLineDto {
    fn from(l: DiffLine) -> Self {
        let (kind, text) = match l {
            DiffLine::Same(t) => ("same", t),
            DiffLine::Added(t) => ("added", t),
            DiffLine::Removed(t) => ("removed", t),
        };
        Self {
            kind: kind.to_owned(),
            text,
        }
    }
}

/// Line comparison of two revisions of a field.
#[utoipa::path(get, path = "/changes/{id}/revisions/{field}/compare", tag = "Changes", params(("id" = Uuid, Path), ("field" = String, Path), ChangeCompareParams), responses((status = 200, body = Vec<ChangeDiffLineDto>)))]
async fn compare_change_revisions(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path((id, field)): Path<(Uuid, String)>,
    Query(q): Query<ChangeCompareParams>,
) -> ApiResult<Json<Vec<ChangeDiffLineDto>>> {
    Ok(Json(
        api.service
            .compare(&p, id, &field, q.from, q.to)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

// ----- PIR ----------------------------------------------------------------------------------

/// A Post-Implementation Review (FR-CHG-011).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChangePirDto {
    /// The Change achieved its goal.
    pub successful: bool,
    /// When work actually started.
    pub actual_start: Option<DateTime<Utc>>,
    /// When work actually ended.
    pub actual_end: Option<DateTime<Utc>>,
    /// Lessons Learned.
    #[serde(default)]
    pub lessons_learned: String,
    /// Follow-up actions.
    #[serde(default)]
    pub follow_up: String,
    /// When it was recorded (output).
    #[serde(default = "Utc::now")]
    pub recorded_at: DateTime<Utc>,
}

impl From<Pir> for ChangePirDto {
    fn from(p: Pir) -> Self {
        Self {
            successful: p.successful,
            actual_start: p.actual_start,
            actual_end: p.actual_end,
            lessons_learned: p.lessons_learned,
            follow_up: p.follow_up,
            recorded_at: p.recorded_at,
        }
    }
}

/// Records the Post-Implementation Review of a finished Change (needs `changes.update`).
#[utoipa::path(put, path = "/changes/{id}/pir", tag = "Changes", params(("id" = Uuid, Path)), request_body = ChangePirDto, responses((status = 200, body = ChangeDto)))]
async fn record_change_pir(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ChangePirDto>,
) -> ApiResult<Json<ChangeDto>> {
    let pir = Pir {
        successful: b.successful,
        actual_start: b.actual_start,
        actual_end: b.actual_end,
        lessons_learned: b.lessons_learned,
        follow_up: b.follow_up,
        recorded_at: b.recorded_at,
    };
    let change = api.service.record_pir(&p, id, pir).await?;
    Ok(Json(ChangeDto::new(change, &bands(&api, &p).await?)))
}

// ----- Attachments --------------------------------------------------------------------------

/// A file attached to a Change.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChangeAttachmentDto {
    /// ID.
    pub id: Uuid,
    /// File name.
    pub file_name: String,
    /// Detected media type.
    pub media_type: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// When.
    pub created_at: DateTime<Utc>,
}

impl From<ChangeAttachment> for ChangeAttachmentDto {
    fn from(a: ChangeAttachment) -> Self {
        Self {
            id: a.id,
            file_name: a.file_name,
            media_type: a.media_type,
            size_bytes: a.size_bytes,
            created_at: a.created_at,
        }
    }
}

/// Files attached to a Change.
#[utoipa::path(get, path = "/changes/{id}/attachments", tag = "Changes", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ChangeAttachmentDto>)))]
async fn list_change_attachments(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ChangeAttachmentDto>>> {
    Ok(Json(
        api.service
            .attachments(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Uploads a file (multipart field `file`); the media type is detected from the content
/// (needs `changes.update`).
#[utoipa::path(post, path = "/changes/{id}/attachments", tag = "Changes", params(("id" = Uuid, Path)),
    request_body(content_type = "multipart/form-data", description = "A `file` field"),
    responses((status = 201, body = ChangeAttachmentDto), (status = 413, description = "Too large")))]
async fn upload_change_attachment(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    mut form: axum::extract::Multipart,
) -> ApiResult<Response> {
    use cooper_platform::attachments::{BlobError, clean_file_name};
    // Check access before storing anything.
    api.service.get(&p, id).await?;
    let bad = |m: &str| ApiError::from(ChangeError::invalid("file", m.to_owned()));
    while let Some(mut field) = form
        .next_field()
        .await
        .map_err(|_| bad("is not valid multipart data"))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let file_name = clean_file_name(field.file_name().unwrap_or("file"));
        let fail = |e: BlobError| match e {
            BlobError::TooLarge { limit } => ProblemJson::new(StatusCode::PAYLOAD_TOO_LARGE)
                .with_detail(format!(
                    "Files may be at most {} MB.",
                    limit / (1024 * 1024)
                ))
                .into_response(),
            other => {
                tracing::error!(error = %other, "attachment storage failed");
                ApiError(AppError::Internal).into_response()
            }
        };
        let mut writer = match api
            .blobs
            .writer(api.upload_limit.max_upload_bytes().await)
            .await
        {
            Ok(w) => w,
            Err(e) => return Ok(fail(e)),
        };
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| bad("upload was interrupted or is too large"))?
        {
            if let Err(e) = writer.write(&chunk).await {
                return Ok(fail(e));
            }
        }
        let stored = match writer.finish().await {
            Ok(s) => s,
            Err(e) => return Ok(fail(e)),
        };
        let a = api
            .service
            .attach(
                &p,
                id,
                file_name,
                stored.media_type.to_owned(),
                stored.size,
                stored.sha256,
            )
            .await?;
        return Ok(created(ChangeAttachmentDto::from(a)));
    }
    Err(bad("is missing"))
}

/// Downloads an attached file (always as a download, never rendered inline).
#[utoipa::path(get, path = "/changes/{id}/attachments/{attachment_id}/content", tag = "Changes", params(("id" = Uuid, Path), ("attachment_id" = Uuid, Path)), responses((status = 200, description = "The file")))]
async fn download_change_attachment(
    State(api): State<ChangesApi>,
    Authenticated(p): Authenticated,
    Path((id, attachment)): Path<(Uuid, Uuid)>,
) -> ApiResult<Response> {
    use axum::http::{HeaderValue, header};
    let info = api
        .service
        .attachments(&p, id)
        .await?
        .into_iter()
        .find(|a| a.id == attachment)
        .ok_or(ApiError(AppError::NotFound("Attachment")))?;
    let bytes = api.blobs.read(&info.sha256).await.map_err(|e| {
        tracing::error!(error = %e, attachment = %info.id, "attachment content missing");
        ApiError(AppError::Internal)
    })?;
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&info.media_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    let ascii: String = info
        .file_name
        .chars()
        .map(|c| {
            if (c.is_ascii_graphic() && c != '"' && c != '\\') || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(response)
}
