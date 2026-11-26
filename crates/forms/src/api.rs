//! HTTP API of Forms (FR-FRM-001 to FR-FRM-005, ADR-0039, ADR-0040): staff build Forms and
//! decide Submissions; Requesters use the Portal routes.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{AnalystId, Cursor, Limit, Page, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, FormsService};
use crate::domain::{
    ApprovalRule, Approvers, Audience, AudienceKind, Decision, Draft, FieldKind,
    FieldWidth, Form, FormDetails, FormField, FormFieldInput, FormId, FormsError, PortalSettings,
    Submission, SubmissionActions, SubmissionId, SubmissionStatus, TaskAction, TicketAction,
    Vote,
};
use crate::ports::{FormQuery, SubmissionQuery};

/// State the handlers need.
#[derive(Clone)]
pub struct FormsApi {
    /// The service.
    pub service: FormsService,
}

/// `/api/v1` path prefixes owned by the Forms Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &[
    "/forms",
    "/form-submissions",
    "/portal/forms",
    "/portal/submissions",
];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    FormsApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_forms, create_form))
        .routes(routes!(get_form, edit_form))
        .routes(routes!(publish_version))
        .routes(routes!(configure_form))
        .routes(routes!(list_submissions))
        .routes(routes!(get_submission))
        .routes(routes!(vote))
        .routes(routes!(retry_actions))
        .routes(routes!(portal_forms))
        .routes(routes!(portal_form))
        .routes(routes!(get_draft, save_draft))
        .routes(routes!(submit))
        .routes(routes!(my_submissions))
        .routes(routes!(my_submission))
        .routes(routes!(resubmit))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<FormsError> for ApiError {
    fn from(e: FormsError) -> Self {
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
            AppError::Rule(FormsError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(FormsError::InvalidState(_)) => typed(
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

fn limit(v: Option<u32>) -> Result<Limit, ApiError> {
    v.map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| FormsError::invalid("limit", "must be between 1 and 200").into())
}

fn decode<K: serde::de::DeserializeOwned>(cursor: Option<&str>) -> Result<Option<K>, ApiError> {
    cursor
        .map(|c| Cursor::from_client(c).decode::<K>())
        .transpose()
        .map_err(|_| FormsError::invalid("cursor", "is not valid").into())
}

fn page<T, D, K: Serialize>(
    rows: Vec<T>,
    limit: Limit,
    key: impl Fn(&T) -> K,
    dto: impl Fn(T) -> D,
) -> ApiResult<(Vec<D>, Option<String>)> {
    let page = Page::from_rows(rows, limit, |r| Cursor::encode(&key(r)))
        .map_err(|_| AppError::Internal)?;
    Ok((
        page.items.into_iter().map(dto).collect(),
        page.next_cursor.map(|c| c.as_str().to_owned()),
    ))
}

// ----- DTOs: Forms ----------------------------------------------------------------------

/// One Form Field.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct FormFieldDto {
    /// Stable identifier within the version.
    pub key: String,
    /// Label shown to the Requester.
    pub label: String,
    /// Field type: `text`, `long_text`, `number`, `date`, `boolean` or `choice`.
    pub kind: String,
    /// Whether an answer is required.
    #[serde(default)]
    pub required: bool,
    /// Options of a choice field.
    #[serde(default)]
    pub options: Vec<String>,
    /// Layout width: `full` or `half`.
    #[serde(default = "default_width")]
    pub width: String,
}

fn default_width() -> String {
    FieldWidth::Full.as_str().to_owned()
}

impl From<&FormField> for FormFieldDto {
    fn from(f: &FormField) -> Self {
        Self {
            key: f.key().to_owned(),
            label: f.label().to_owned(),
            kind: f.kind().as_str().to_owned(),
            required: f.required(),
            options: f.options().to_vec(),
            width: f.width().as_str().to_owned(),
        }
    }
}

fn field_input(f: FormFieldDto) -> Result<FormFieldInput, FormsError> {
    Ok(FormFieldInput {
        key: f.key,
        label: f.label,
        kind: f.kind.parse::<FieldKind>()?,
        required: f.required,
        options: f.options,
        width: f.width.parse::<FieldWidth>()?,
    })
}

/// A published version, without its fields.
#[derive(Debug, Serialize, ToSchema)]
pub struct FormVersionSummaryDto {
    /// Version number, from 1.
    pub number: u32,
    /// When it was published.
    pub created_at: DateTime<Utc>,
    /// Number of fields in it.
    pub field_count: usize,
}

/// One audience of a Portal-visible Form.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AudienceDto {
    /// `requester`, `group` or `department`.
    pub kind: String,
    /// The Requester, User Group or Department.
    pub id: Uuid,
}

/// Portal settings of a Form.
#[derive(Debug, Serialize, Deserialize, ToSchema, Default)]
pub struct PortalDto {
    /// Whether the Form is on the Portal.
    #[serde(default)]
    pub visible: bool,
    /// Audiences; none means every Requester of the Company.
    #[serde(default)]
    pub audiences: Vec<AudienceDto>,
}

/// Who approves: named Analysts, or the members of a Team.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApproversDto {
    /// `analysts` or `team`.
    pub kind: String,
    /// The Analysts (for `analysts`).
    #[serde(default)]
    pub analyst_ids: Vec<Uuid>,
    /// The Team (for `team`).
    pub team_id: Option<Uuid>,
}

/// The approval rule of a Form.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApprovalDto {
    /// Who may approve.
    pub approvers: ApproversDto,
    /// Approvals that approve a Submission in a round (1 to 50).
    pub required_approvals: u32,
    /// Rejections that return a Submission in a round (1 to 50).
    pub required_rejections: u32,
    /// Whether an Analyst with the Requester's e-mail address may not approve.
    #[serde(default)]
    pub exclude_requester: bool,
}

/// Opens a ticket on submission.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TicketActionDto {
    /// The Portal Catalogue Category of the ticket.
    pub category_id: Uuid,
    /// Subject template, with `{field_key}` placeholders.
    pub subject: String,
    /// Description template.
    #[serde(default)]
    pub description: String,
}

/// Creates a task on submission.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TaskActionDto {
    /// Title template.
    pub title: String,
    /// Description template.
    #[serde(default)]
    pub description: String,
}

/// Submission Actions of a Form.
#[derive(Debug, Serialize, Deserialize, ToSchema, Default)]
pub struct ActionsDto {
    /// The ticket to open, if any.
    pub ticket: Option<TicketActionDto>,
    /// The task to create, if any.
    pub task: Option<TaskActionDto>,
}

/// A Form with its current version and settings.
#[derive(Debug, Serialize, ToSchema)]
pub struct FormDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Active Forms are offered; inactive ones are kept.
    pub is_active: bool,
    /// The current version number.
    pub current_version: u32,
    /// Fields of the current version, in layout order.
    pub fields: Vec<FormFieldDto>,
    /// Every version, oldest first.
    pub versions: Vec<FormVersionSummaryDto>,
    /// Portal settings.
    pub portal: PortalDto,
    /// Approval rule, if any.
    pub approval: Option<ApprovalDto>,
    /// Submission Actions.
    pub actions: ActionsDto,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<&Form> for FormDto {
    fn from(f: &Form) -> Self {
        Self {
            id: f.id().as_uuid(),
            company_id: f.tenant_id().as_uuid(),
            name: f.details().name().to_owned(),
            description: f.details().description().to_owned(),
            is_active: f.is_active(),
            current_version: f.current().number(),
            fields: f.current().fields().iter().map(FormFieldDto::from).collect(),
            versions: f
                .versions()
                .map(|v| FormVersionSummaryDto {
                    number: v.number(),
                    created_at: v.created_at(),
                    field_count: v.fields().len(),
                })
                .collect(),
            portal: portal_dto(f.portal()),
            approval: f.approval().map(approval_dto),
            actions: actions_dto(f.actions()),
            created_at: f.created_at(),
            updated_at: f.updated_at(),
        }
    }
}

impl From<Form> for FormDto {
    fn from(f: Form) -> Self {
        Self::from(&f)
    }
}

fn portal_dto(p: &PortalSettings) -> PortalDto {
    PortalDto {
        visible: p.visible(),
        audiences: p
            .audiences()
            .iter()
            .map(|a| AudienceDto {
                kind: a.kind.as_str().to_owned(),
                id: a.id,
            })
            .collect(),
    }
}

fn approval_dto(r: &ApprovalRule) -> ApprovalDto {
    let approvers = match r.approvers() {
        Approvers::Analysts(list) => ApproversDto {
            kind: "analysts".into(),
            analyst_ids: list.iter().map(|a| a.as_uuid()).collect(),
            team_id: None,
        },
        Approvers::Team(team) => ApproversDto {
            kind: "team".into(),
            analyst_ids: Vec::new(),
            team_id: Some(team.as_uuid()),
        },
    };
    ApprovalDto {
        approvers,
        required_approvals: r.required_approvals(),
        required_rejections: r.required_rejections(),
        exclude_requester: r.exclude_requester(),
    }
}

fn actions_dto(a: &SubmissionActions) -> ActionsDto {
    ActionsDto {
        ticket: a.ticket().map(|t| TicketActionDto {
            category_id: t.category_id(),
            subject: t.subject().to_owned(),
            description: t.description().to_owned(),
        }),
        task: a.task().map(|t| TaskActionDto {
            title: t.title().to_owned(),
            description: t.description().to_owned(),
        }),
    }
}

/// A page of Forms.
#[derive(Debug, Serialize, ToSchema)]
pub struct FormPage {
    /// The Forms.
    pub items: Vec<FormDto>,
    /// Cursor of the next page, when there is one.
    pub next_cursor: Option<String>,
}

/// A new Form.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FormRequest {
    /// Company (required).
    pub company_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Fields of the first version.
    pub fields: Vec<FormFieldDto>,
}

/// Name, description and active flag of a Form.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FormUpdateRequest {
    /// Name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Active flag.
    pub is_active: bool,
}

/// Fields of a new version.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FormVersionRequest {
    /// The complete field list of the new version.
    pub fields: Vec<FormFieldDto>,
}

/// Portal settings, approval rule and Submission Actions of a Form.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FormSettingsRequest {
    /// Portal settings.
    #[serde(default)]
    pub portal: PortalDto,
    /// Approval rule, or none.
    pub approval: Option<ApprovalDto>,
    /// Submission Actions.
    #[serde(default)]
    pub actions: ActionsDto,
}

/// Query of the Form list.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FormListQuery {
    /// Only this Company.
    pub company_id: Option<Uuid>,
    /// Text in the name.
    pub q: Option<String>,
    /// Only active (`true`) or inactive (`false`) Forms.
    pub active: Option<bool>,
    /// Page size (1 to 200, default 50).
    pub limit: Option<u32>,
    /// Cursor from the previous page.
    pub cursor: Option<String>,
}

// ----- DTOs: Submissions, drafts ----------------------------------------------------------

/// One vote on a Submission.
#[derive(Debug, Serialize, ToSchema)]
pub struct VoteDto {
    /// Approval round.
    pub round: u32,
    /// The approver.
    pub approver_id: Uuid,
    /// `approve` or `reject`.
    pub decision: String,
    /// Comment.
    pub comment: String,
    /// When it was cast.
    pub cast_at: DateTime<Utc>,
}

impl From<&Vote> for VoteDto {
    fn from(v: &Vote) -> Self {
        Self {
            round: v.round,
            approver_id: v.approver.as_uuid(),
            decision: v.decision.as_str().to_owned(),
            comment: v.comment.clone(),
            cast_at: v.cast_at,
        }
    }
}

/// A Submission, for staff.
#[derive(Debug, Serialize, ToSchema)]
pub struct SubmissionDto {
    /// ID.
    pub id: Uuid,
    /// The Form.
    pub form_id: Uuid,
    /// The Form version the answers were given against.
    pub form_version: u32,
    /// The Requester.
    pub requester_id: Uuid,
    /// `pending_approval`, `returned`, `approved` or `completed`.
    pub status: String,
    /// Approval round, from 1.
    pub round: u32,
    /// Answers by field key.
    pub answers: BTreeMap<String, String>,
    /// Votes, in the order cast.
    pub votes: Vec<VoteDto>,
    /// The ticket the actions opened.
    pub ticket_id: Option<Uuid>,
    /// The task the actions created.
    pub task_id: Option<Uuid>,
    /// When it was submitted.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

fn status_name(s: SubmissionStatus) -> String {
    s.as_str().to_owned()
}

impl From<&Submission> for SubmissionDto {
    fn from(s: &Submission) -> Self {
        let r = s.record();
        Self {
            id: r.id.as_uuid(),
            form_id: r.form_id.as_uuid(),
            form_version: r.form_version,
            requester_id: r.requester_id.as_uuid(),
            status: status_name(r.status),
            round: r.round,
            answers: r.answers.clone(),
            votes: r.votes.iter().map(VoteDto::from).collect(),
            ticket_id: r.ticket_id,
            task_id: r.task_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// A page of Submissions, for staff.
#[derive(Debug, Serialize, ToSchema)]
pub struct SubmissionPage {
    /// The Submissions, newest first.
    pub items: Vec<SubmissionDto>,
    /// Cursor of the next page, when there is one.
    pub next_cursor: Option<String>,
}

/// A Submission as the Requester sees it: no approver identities.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalSubmissionDto {
    /// ID.
    pub id: Uuid,
    /// The Form.
    pub form_id: Uuid,
    /// The Form version the answers were given against.
    pub form_version: u32,
    /// Status.
    pub status: String,
    /// Approval round, from 1.
    pub round: u32,
    /// Answers by field key.
    pub answers: BTreeMap<String, String>,
    /// Decisions and comments of the current round's rejections, for the Requester to act on.
    pub rejections: Vec<String>,
    /// When it was submitted.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<&Submission> for PortalSubmissionDto {
    fn from(s: &Submission) -> Self {
        let r = s.record();
        Self {
            id: r.id.as_uuid(),
            form_id: r.form_id.as_uuid(),
            form_version: r.form_version,
            status: status_name(r.status),
            round: r.round,
            answers: r.answers.clone(),
            rejections: r
                .votes
                .iter()
                .filter(|v| v.round == r.round && v.decision == Decision::Reject)
                .map(|v| v.comment.clone())
                .collect(),
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// A page of the Requester's own Submissions.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalSubmissionPage {
    /// The Submissions, newest first.
    pub items: Vec<PortalSubmissionDto>,
    /// Cursor of the next page, when there is one.
    pub next_cursor: Option<String>,
}

/// A Form as the Requester fills it in.
#[derive(Debug, Serialize, ToSchema)]
pub struct PortalFormDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// The version the Requester fills in.
    pub current_version: u32,
    /// Fields, in layout order.
    pub fields: Vec<FormFieldDto>,
}

impl From<&Form> for PortalFormDto {
    fn from(f: &Form) -> Self {
        Self {
            id: f.id().as_uuid(),
            name: f.details().name().to_owned(),
            description: f.details().description().to_owned(),
            current_version: f.current().number(),
            fields: f.current().fields().iter().map(FormFieldDto::from).collect(),
        }
    }
}

/// A Requester's saved answers to a Form.
#[derive(Debug, Serialize, ToSchema)]
pub struct DraftDto {
    /// The Form.
    pub form_id: Uuid,
    /// Answers so far.
    pub answers: BTreeMap<String, String>,
    /// When they were saved.
    pub updated_at: DateTime<Utc>,
}

impl From<Draft> for DraftDto {
    fn from(d: Draft) -> Self {
        Self {
            form_id: d.form_id.as_uuid(),
            answers: d.answers,
            updated_at: d.updated_at,
        }
    }
}

/// Answers to a Form.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AnswersRequest {
    /// Answers by field key.
    #[serde(default)]
    pub answers: BTreeMap<String, String>,
}

/// Query of Submission lists.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SubmissionListQuery {
    /// Only this status.
    pub status: Option<String>,
    /// Page size (1 to 200, default 50).
    pub limit: Option<u32>,
    /// Cursor from the previous page.
    pub cursor: Option<String>,
}

/// A vote on a Submission.
#[derive(Debug, Deserialize, ToSchema)]
pub struct VoteRequest {
    /// `approve` or `reject`.
    pub decision: String,
    /// Comment (required to reject).
    #[serde(default)]
    pub comment: String,
}

// ----- Parsing helpers --------------------------------------------------------------------

fn parse_portal(p: PortalDto) -> Result<PortalSettings, FormsError> {
    let audiences = p
        .audiences
        .into_iter()
        .map(|a| {
            Ok(Audience {
                kind: a.kind.parse::<AudienceKind>()?,
                id: a.id,
            })
        })
        .collect::<Result<Vec<_>, FormsError>>()?;
    PortalSettings::new(p.visible, audiences)
}

fn parse_approval(a: ApprovalDto) -> Result<ApprovalRule, FormsError> {
    let approvers = match a.approvers.kind.as_str() {
        "analysts" => Approvers::Analysts(a.approvers.analyst_ids.into_iter().map(AnalystId::from_uuid).collect()),
        "team" => Approvers::Team(TeamId::from_uuid(
            a.approvers
                .team_id
                .ok_or_else(|| FormsError::invalid("approvers", "a team is required"))?,
        )),
        other => {
            return Err(FormsError::invalid(
                "approvers",
                format!("{other:?} is not analysts or team"),
            ));
        }
    };
    ApprovalRule::new(
        approvers,
        a.required_approvals,
        a.required_rejections,
        a.exclude_requester,
    )
}

fn parse_actions(a: ActionsDto) -> Result<SubmissionActions, FormsError> {
    let ticket = a
        .ticket
        .map(|t| TicketAction::new(t.category_id, &t.subject, &t.description))
        .transpose()?;
    let task = a
        .task
        .map(|t| TaskAction::new(&t.title, &t.description))
        .transpose()?;
    Ok(SubmissionActions::new(ticket, task))
}

fn parse_decision(s: &str) -> Result<Decision, FormsError> {
    s.parse::<Decision>()
}

// ----- Handlers: staff --------------------------------------------------------------------

/// Forms in the caller's Companies, by name (needs `forms.read`).
#[utoipa::path(get, path = "/forms", tag = "Forms", params(FormListQuery), responses((status = 200, body = FormPage)))]
async fn list_forms(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<FormListQuery>,
) -> ApiResult<Json<FormPage>> {
    let limit = limit(q.limit)?;
    let after = decode::<(String, Uuid)>(q.cursor.as_deref())?;
    let query = FormQuery {
        tenant_id: q.company_id.map(TenantId::from_uuid),
        search: q.q,
        active: q.active,
        limit,
        after,
    };
    let rows = api.service.list_forms(&p, query).await?;
    let (items, next_cursor) = page(
        rows,
        limit,
        |f| (f.details().name().to_lowercase(), f.id().as_uuid()),
        FormDto::from,
    )?;
    Ok(Json(FormPage { items, next_cursor }))
}

/// Creates a Form with its first version (needs `forms.write`).
#[utoipa::path(post, path = "/forms", tag = "Forms", request_body = FormRequest, responses((status = 201, body = FormDto)))]
async fn create_form(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<FormRequest>,
) -> ApiResult<Response> {
    let tenant = b
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| FormsError::invalid("company_id", "is required"))?;
    let details = FormDetails::new(b.name, b.description)?;
    let fields = b
        .fields
        .into_iter()
        .map(field_input)
        .collect::<Result<Vec<_>, _>>()?;
    let form = api.service.create_form(&p, tenant, details, fields).await?;
    Ok((StatusCode::CREATED, Json(FormDto::from(form))).into_response())
}

/// One Form with its versions and settings (needs `forms.read`).
#[utoipa::path(get, path = "/forms/{id}", tag = "Forms", params(("id" = Uuid, Path)), responses((status = 200, body = FormDto)))]
async fn get_form(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FormDto>> {
    Ok(Json(
        api.service
            .get_form(&p, FormId::from_uuid(id))
            .await?
            .into(),
    ))
}

/// Changes a Form's name, description and active flag, not its fields (needs
/// `forms.write`).
#[utoipa::path(put, path = "/forms/{id}", tag = "Forms", params(("id" = Uuid, Path)), request_body = FormUpdateRequest, responses((status = 200, body = FormDto)))]
async fn edit_form(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<FormUpdateRequest>,
) -> ApiResult<Json<FormDto>> {
    let details = FormDetails::new(b.name, b.description)?;
    Ok(Json(
        api.service
            .edit_form(&p, FormId::from_uuid(id), details, b.is_active)
            .await?
            .into(),
    ))
}

/// Publishes a new version of a Form's fields (needs `forms.write`). Earlier versions are
/// kept unchanged.
#[utoipa::path(post, path = "/forms/{id}/versions", tag = "Forms", params(("id" = Uuid, Path)), request_body = FormVersionRequest, responses((status = 201, body = FormDto)))]
async fn publish_version(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<FormVersionRequest>,
) -> ApiResult<Response> {
    let fields = b
        .fields
        .into_iter()
        .map(field_input)
        .collect::<Result<Vec<_>, _>>()?;
    let form = api
        .service
        .publish_version(&p, FormId::from_uuid(id), fields)
        .await?;
    Ok((StatusCode::CREATED, Json(FormDto::from(form))).into_response())
}

/// Replaces a Form's Portal settings, approval rule and Submission Actions (needs
/// `forms.write`). Publishes no version.
#[utoipa::path(put, path = "/forms/{id}/settings", tag = "Forms", params(("id" = Uuid, Path)), request_body = FormSettingsRequest, responses((status = 200, body = FormDto)))]
async fn configure_form(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<FormSettingsRequest>,
) -> ApiResult<Json<FormDto>> {
    let portal = parse_portal(b.portal)?;
    let approval = b.approval.map(parse_approval).transpose()?;
    let actions = parse_actions(b.actions)?;
    Ok(Json(
        api.service
            .configure_form(&p, FormId::from_uuid(id), portal, approval, actions)
            .await?
            .into(),
    ))
}

/// Submissions of a Form, newest first (needs `forms.read`).
#[utoipa::path(get, path = "/forms/{id}/submissions", tag = "Forms", params(("id" = Uuid, Path), SubmissionListQuery), responses((status = 200, body = SubmissionPage)))]
async fn list_submissions(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Query(q): Query<SubmissionListQuery>,
) -> ApiResult<Json<SubmissionPage>> {
    let limit = limit(q.limit)?;
    let status = q
        .status
        .as_deref()
        .map(|s| s.parse::<SubmissionStatus>())
        .transpose()?;
    let after = decode::<(DateTime<Utc>, Uuid)>(q.cursor.as_deref())?;
    let rows = api
        .service
        .list_submissions(
            &p,
            SubmissionQuery {
                tenant_id: None,
                form_id: Some(FormId::from_uuid(id)),
                requester_id: None,
                status,
                limit,
                after,
            },
        )
        .await?;
    let (items, next_cursor) = page(
        rows,
        limit,
        |s| (s.record().created_at, s.record().id.as_uuid()),
        |s| SubmissionDto::from(&s),
    )?;
    Ok(Json(SubmissionPage { items, next_cursor }))
}

/// One Submission with its votes (needs `forms.read`).
#[utoipa::path(get, path = "/form-submissions/{id}", tag = "Forms", params(("id" = Uuid, Path)), responses((status = 200, body = SubmissionDto)))]
async fn get_submission(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SubmissionDto>> {
    let s = api
        .service
        .get_submission(&p, SubmissionId::from_uuid(id))
        .await?;
    Ok(Json(SubmissionDto::from(&s)))
}

/// Votes on a Submission waiting for approval (needs `forms.approve`).
#[utoipa::path(post, path = "/form-submissions/{id}/votes", tag = "Forms", params(("id" = Uuid, Path)), request_body = VoteRequest, responses((status = 200, body = SubmissionDto)))]
async fn vote(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<VoteRequest>,
) -> ApiResult<Json<SubmissionDto>> {
    let decision = parse_decision(&b.decision)?;
    let s = api
        .service
        .vote(&p, SubmissionId::from_uuid(id), decision, &b.comment)
        .await?;
    Ok(Json(SubmissionDto::from(&s)))
}

/// Runs an approved Submission's actions again after a failure (needs `forms.approve`).
#[utoipa::path(post, path = "/form-submissions/{id}/actions", tag = "Forms", params(("id" = Uuid, Path)), responses((status = 200, body = SubmissionDto)))]
async fn retry_actions(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SubmissionDto>> {
    let s = api
        .service
        .retry_actions(&p, SubmissionId::from_uuid(id))
        .await?;
    Ok(Json(SubmissionDto::from(&s)))
}

// ----- Handlers: Portal -------------------------------------------------------------------

/// The Forms the signed-in Requester may fill in (FR-FRM-002).
#[utoipa::path(get, path = "/portal/forms", tag = "Forms", responses((status = 200, body = [PortalFormDto])))]
async fn portal_forms(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
) -> ApiResult<Json<Vec<PortalFormDto>>> {
    let forms = api.service.portal_forms(&p).await?;
    Ok(Json(forms.iter().map(PortalFormDto::from).collect()))
}

/// One Form the signed-in Requester may fill in.
#[utoipa::path(get, path = "/portal/forms/{id}", tag = "Forms", params(("id" = Uuid, Path)), responses((status = 200, body = PortalFormDto)))]
async fn portal_form(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PortalFormDto>> {
    let f = api
        .service
        .portal_form_for(&p, FormId::from_uuid(id))
        .await?;
    Ok(Json(PortalFormDto::from(&f)))
}

/// The Requester's Draft of a Form, or null (FR-FRM-005).
#[utoipa::path(get, path = "/portal/forms/{id}/draft", tag = "Forms", params(("id" = Uuid, Path)), responses((status = 200, body = Option<DraftDto>)))]
async fn get_draft(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Option<DraftDto>>> {
    let d = api.service.get_draft(&p, FormId::from_uuid(id)).await?;
    Ok(Json(d.map(DraftDto::from)))
}

/// Saves the Requester's Draft of a Form (FR-FRM-005).
#[utoipa::path(put, path = "/portal/forms/{id}/draft", tag = "Forms", params(("id" = Uuid, Path)), request_body = AnswersRequest, responses((status = 200, body = DraftDto)))]
async fn save_draft(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<AnswersRequest>,
) -> ApiResult<Json<DraftDto>> {
    let d = api
        .service
        .save_draft(&p, FormId::from_uuid(id), &b.answers)
        .await?;
    Ok(Json(DraftDto::from(d)))
}

/// Submits a Form (FR-FRM-003). Its Draft is removed.
#[utoipa::path(post, path = "/portal/forms/{id}/submissions", tag = "Forms", params(("id" = Uuid, Path)), request_body = AnswersRequest, responses((status = 201, body = PortalSubmissionDto)))]
async fn submit(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<AnswersRequest>,
) -> ApiResult<Response> {
    let s = api
        .service
        .submit(&p, FormId::from_uuid(id), &b.answers)
        .await?;
    Ok((StatusCode::CREATED, Json(PortalSubmissionDto::from(&s))).into_response())
}

/// The signed-in Requester's own Submissions, newest first.
#[utoipa::path(get, path = "/portal/submissions", tag = "Forms", params(SubmissionListQuery), responses((status = 200, body = PortalSubmissionPage)))]
async fn my_submissions(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<SubmissionListQuery>,
) -> ApiResult<Json<PortalSubmissionPage>> {
    let limit = limit(q.limit)?;
    let after = decode::<(DateTime<Utc>, Uuid)>(q.cursor.as_deref())?;
    let rows = api.service.my_submissions(&p, limit, after).await?;
    let (items, next_cursor) = page(
        rows,
        limit,
        |s| (s.record().created_at, s.record().id.as_uuid()),
        |s| PortalSubmissionDto::from(&s),
    )?;
    Ok(Json(PortalSubmissionPage { items, next_cursor }))
}

/// One of the Requester's own Submissions.
#[utoipa::path(get, path = "/portal/submissions/{id}", tag = "Forms", params(("id" = Uuid, Path)), responses((status = 200, body = PortalSubmissionDto)))]
async fn my_submission(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PortalSubmissionDto>> {
    let s = api
        .service
        .my_submission(&p, SubmissionId::from_uuid(id))
        .await?;
    Ok(Json(PortalSubmissionDto::from(&s)))
}

/// Amends a returned Submission and resubmits it for a new approval round (FR-FRM-004).
#[utoipa::path(post, path = "/portal/submissions/{id}/resubmit", tag = "Forms", params(("id" = Uuid, Path)), request_body = AnswersRequest, responses((status = 200, body = PortalSubmissionDto)))]
async fn resubmit(
    State(api): State<FormsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<AnswersRequest>,
) -> ApiResult<Json<PortalSubmissionDto>> {
    let s = api
        .service
        .resubmit(&p, SubmissionId::from_uuid(id), &b.answers)
        .await?;
    Ok(Json(PortalSubmissionDto::from(&s)))
}
