//! HTTP API of Problem Management.

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{AnalystId, Cursor, Limit, TenantId, TicketId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, ProblemService};
use crate::domain::{
    HistoryEntry, Lookup, LookupInput, LookupKind, Problem, ProblemDetails, ProblemError,
    ProblemNote, ProblemQuery, TicketAction, TicketSummary,
};

/// State the handlers need.
#[derive(Clone)]
pub struct ProblemsApi {
    /// The service.
    pub service: ProblemService,
}

/// `/api/v1` path prefixes owned by the Problems Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &["/problems", "/problem-statuses", "/problem-priorities"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ProblemsApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_problem_statuses, create_problem_status))
        .routes(routes!(update_problem_status))
        .routes(routes!(list_problem_priorities, create_problem_priority))
        .routes(routes!(update_problem_priority))
        .routes(routes!(list_problems, create_problem))
        .routes(routes!(get_problem, edit_problem))
        .routes(routes!(change_problem_status))
        .routes(routes!(assign_problem))
        .routes(routes!(list_problem_notes, add_problem_note))
        .routes(routes!(problem_history))
        .routes(routes!(list_problem_tickets, link_problem_ticket))
        .routes(routes!(unlink_problem_ticket))
        .routes(routes!(act_on_problem_tickets))
        .routes(routes!(publish_problem_known_error))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<ProblemError> for ApiError {
    fn from(e: ProblemError) -> Self {
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
            AppError::Rule(ProblemError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(ProblemError::InvalidState(_)) => typed(
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

/// A Problem Status or Priority.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemLookupDto {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// Closing status (Statuses).
    pub is_closed: bool,
    /// Default for new Problems.
    pub is_default: bool,
    /// Display order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl From<Lookup> for ProblemLookupDto {
    fn from(l: Lookup) -> Self {
        Self {
            id: l.id,
            key: l.key,
            name: l.name,
            is_closed: l.is_closed,
            is_default: l.is_default,
            sort_order: l.sort_order,
            is_active: l.is_active,
        }
    }
}

/// Creates or changes a Problem Status or Priority.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ProblemLookupRequest {
    /// Name.
    pub name: String,
    /// Closing status (ignored for Priorities).
    #[serde(default)]
    pub is_closed: bool,
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

impl From<ProblemLookupRequest> for LookupInput {
    fn from(r: ProblemLookupRequest) -> Self {
        Self {
            name: r.name,
            is_closed: r.is_closed,
            is_default: r.is_default,
            sort_order: r.sort_order,
            is_active: r.is_active,
        }
    }
}

/// A Problem (FR-PRB-001).
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemDto {
    /// ID.
    pub id: Uuid,
    /// Problem Number (`PRB-00001`).
    pub number: String,
    /// Company.
    pub company_id: Uuid,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Status.
    pub status_id: Uuid,
    /// Priority.
    pub priority_id: Uuid,
    /// Assigned Analyst.
    pub analyst_id: Option<Uuid>,
    /// Root Cause.
    pub root_cause: String,
    /// Workaround.
    pub workaround: String,
    /// Known Error.
    pub is_known_error: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
    /// When it was resolved.
    pub closed_at: Option<DateTime<Utc>>,
}

impl From<Problem> for ProblemDto {
    fn from(p: Problem) -> Self {
        let d = p.details().clone();
        Self {
            id: p.id(),
            number: p.number().to_owned(),
            company_id: p.tenant_id().as_uuid(),
            title: d.title,
            description: d.description,
            status_id: p.status_id(),
            priority_id: d.priority_id,
            analyst_id: p.assigned_analyst_id().map(|a| a.as_uuid()),
            root_cause: d.root_cause,
            workaround: d.workaround,
            is_known_error: d.is_known_error,
            created_at: p.created_at(),
            updated_at: p.updated_at(),
            closed_at: p.closed_at(),
        }
    }
}

/// Creates or edits a Problem.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ProblemRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Title.
    pub title: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Priority (default when absent).
    pub priority_id: Option<Uuid>,
    /// Assigned Analyst (create only; use the assignment endpoint afterwards).
    pub analyst_id: Option<Uuid>,
    /// Root Cause.
    #[serde(default)]
    pub root_cause: String,
    /// Workaround.
    #[serde(default)]
    pub workaround: String,
    /// Known Error (needs a Root Cause or Workaround).
    #[serde(default)]
    pub is_known_error: bool,
}

impl ProblemRequest {
    fn details(&self) -> ProblemDetails {
        ProblemDetails {
            title: self.title.clone(),
            description: self.description.clone(),
            priority_id: self.priority_id.unwrap_or_else(Uuid::nil),
            root_cause: self.root_cause.clone(),
            workaround: self.workaround.clone(),
            is_known_error: self.is_known_error,
        }
    }
}

/// A page of Problems.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemPage {
    /// Problems, newest activity first.
    pub items: Vec<ProblemDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Filters.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ProblemListParams {
    /// Open (`true`) or resolved (`false`).
    pub open: Option<bool>,
    /// Known Errors only.
    pub known_error: Option<bool>,
    /// Assigned to.
    pub analyst_id: Option<Uuid>,
    /// Problems linked to this ticket (the ticket's side of FR-PRB-003).
    pub ticket_id: Option<Uuid>,
    /// Number prefix or text.
    pub q: Option<String>,
    /// Page size (1 to 200).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

/// A status change.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ProblemStatusRequest {
    /// The status.
    pub status_id: Uuid,
}

/// An assignment.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ProblemAssignmentRequest {
    /// The Analyst, or null for nobody.
    pub analyst_id: Option<Uuid>,
}

/// A Note.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProblemNoteDto {
    /// ID (output).
    #[serde(default)]
    pub id: Uuid,
    /// Text.
    pub body: String,
    /// Author (output).
    #[serde(default)]
    pub author_id: Option<Uuid>,
    /// Created (output).
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
}

impl From<ProblemNote> for ProblemNoteDto {
    fn from(n: ProblemNote) -> Self {
        Self {
            id: n.id,
            body: n.body,
            author_id: n.author_id.map(|a| a.as_uuid()),
            created_at: n.created_at,
        }
    }
}

/// One change in a Problem's problem_history.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemHistoryDto {
    /// When.
    pub at: DateTime<Utc>,
    /// Who (`{kind, id}`).
    #[schema(value_type = Object)]
    pub actor: serde_json::Value,
    /// `created`, `updated`, `deleted` or `restored`.
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

impl From<HistoryEntry> for ProblemHistoryDto {
    fn from(e: HistoryEntry) -> Self {
        Self {
            at: e.occurred_at,
            actor: serde_json::json!({ "kind": e.actor.kind().as_str(), "id": e.actor.id() }),
            action: e.action.as_str().to_owned(),
            field: e.field,
            old_value: e.old_value,
            new_value: e.new_value,
        }
    }
}

/// A linked ticket.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemTicketDto {
    /// Ticket.
    pub id: Uuid,
    /// Ticket Number.
    pub number: String,
    /// Subject.
    pub subject: String,
    /// Closed.
    pub is_closed: bool,
    /// When it was linked.
    pub linked_at: DateTime<Utc>,
}

/// Links a ticket.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ProblemLinkTicketRequest {
    /// The ticket.
    pub ticket_id: Uuid,
}

/// An action for every linked ticket of a resolved Problem (FR-PRB-006).
#[derive(Debug, Deserialize, ToSchema)]
pub struct ProblemTicketActionRequest {
    /// `close` or `note`.
    pub action: String,
    /// Resolution Code (`close`).
    pub resolution_code_id: Option<Uuid>,
    /// Text (`note`).
    pub body: Option<String>,
    /// The Note is visible to the Requester (`note`).
    #[serde(default)]
    pub public: bool,
}

/// The outcome on one ticket.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemTicketOutcomeDto {
    /// Ticket.
    pub ticket_id: Uuid,
    /// Succeeded.
    pub ok: bool,
    /// Why it failed.
    pub error: Option<String>,
}

macro_rules! lookup_handlers {
    ($kind:expr, $path:literal, $item:literal, $list:ident, $create:ident, $update:ident) => {
        #[utoipa::path(get, path = $path, tag = "Problems", responses((status = 200, body = Vec<ProblemLookupDto>)))]
        async fn $list(State(api): State<ProblemsApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<ProblemLookupDto>>> {
            Ok(Json(api.service.lookups(&p, $kind).await?.into_iter().map(Into::into).collect()))
        }

        #[utoipa::path(post, path = $path, tag = "Problems", request_body = ProblemLookupRequest, responses((status = 201, body = ProblemLookupDto)))]
        async fn $create(State(api): State<ProblemsApi>, Authenticated(p): Authenticated, Json(b): Json<ProblemLookupRequest>) -> ApiResult<Response> {
            let l = api.service.create_lookup(&p, $kind, b.into()).await?;
            Ok((StatusCode::CREATED, Json(ProblemLookupDto::from(l))).into_response())
        }

        #[utoipa::path(put, path = $item, tag = "Problems", params(("id" = Uuid, Path)), request_body = ProblemLookupRequest, responses((status = 200, body = ProblemLookupDto)))]
        async fn $update(State(api): State<ProblemsApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<ProblemLookupRequest>) -> ApiResult<Json<ProblemLookupDto>> {
            Ok(Json(api.service.update_lookup(&p, $kind, id, b.into()).await?.into()))
        }
    };
}

lookup_handlers!(
    LookupKind::Status,
    "/problem-statuses",
    "/problem-statuses/{id}",
    list_problem_statuses,
    create_problem_status,
    update_problem_status
);
lookup_handlers!(
    LookupKind::Priority,
    "/problem-priorities",
    "/problem-priorities/{id}",
    list_problem_priorities,
    create_problem_priority,
    update_problem_priority
);

/// Problems in your Companies, newest activity first (needs `problems.read`).
#[utoipa::path(get, path = "/problems", tag = "Problems", params(ProblemListParams), responses((status = 200, body = ProblemPage)))]
async fn list_problems(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<ProblemListParams>,
) -> ApiResult<Json<ProblemPage>> {
    let limit = q
        .limit
        .map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| ProblemError::invalid("limit", "must be between 1 and 200"))?;
    let after = q
        .cursor
        .map(|c| Cursor::from_client(c).decode::<(DateTime<Utc>, Uuid)>())
        .transpose()
        .map_err(|_| ProblemError::invalid("cursor", "is not valid"))?;
    let query = ProblemQuery {
        open: q.open,
        known_error: q.known_error,
        analyst_id: q.analyst_id.map(AnalystId::from_uuid),
        ticket_id: q.ticket_id.map(TicketId::from_uuid),
        search: q.q,
        limit,
        after,
    };
    let rows = api.service.list(&p, &query).await?;
    let page =
        cooper_kernel::Page::from_rows(rows, limit, |p| Cursor::encode(&(p.updated_at(), p.id())))
            .map_err(|_| AppError::Internal)?;
    Ok(Json(ProblemPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Creates a Problem (needs `problems.create`).
#[utoipa::path(post, path = "/problems", tag = "Problems", request_body = ProblemRequest, responses((status = 201, body = ProblemDto)))]
async fn create_problem(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<ProblemRequest>,
) -> ApiResult<Response> {
    let tenant = b
        .company_id
        .map(TenantId::from_uuid)
        .ok_or_else(|| ProblemError::invalid("company_id", "is required"))?;
    let problem = api
        .service
        .create(
            &p,
            tenant,
            b.details(),
            b.analyst_id.map(AnalystId::from_uuid),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(ProblemDto::from(problem))).into_response())
}

/// One Problem.
#[utoipa::path(get, path = "/problems/{id}", tag = "Problems", params(("id" = Uuid, Path)), responses((status = 200, body = ProblemDto)))]
async fn get_problem(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ProblemDto>> {
    Ok(Json(api.service.get(&p, id).await?.into()))
}

/// Edits a Problem: details, Root Cause, Workaround, Known Error (needs `problems.update`).
#[utoipa::path(put, path = "/problems/{id}", tag = "Problems", params(("id" = Uuid, Path)), request_body = ProblemRequest, responses((status = 200, body = ProblemDto)))]
async fn edit_problem(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ProblemRequest>,
) -> ApiResult<Json<ProblemDto>> {
    Ok(Json(api.service.edit(&p, id, b.details()).await?.into()))
}

/// Moves a Problem to another status (needs `problems.update`).
#[utoipa::path(post, path = "/problems/{id}/status", tag = "Problems", params(("id" = Uuid, Path)), request_body = ProblemStatusRequest, responses((status = 200, body = ProblemDto)))]
async fn change_problem_status(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ProblemStatusRequest>,
) -> ApiResult<Json<ProblemDto>> {
    Ok(Json(
        api.service.change_status(&p, id, b.status_id).await?.into(),
    ))
}

/// Assigns a Problem (needs `problems.update`).
#[utoipa::path(put, path = "/problems/{id}/assignment", tag = "Problems", params(("id" = Uuid, Path)), request_body = ProblemAssignmentRequest, responses((status = 200, body = ProblemDto)))]
async fn assign_problem(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ProblemAssignmentRequest>,
) -> ApiResult<Json<ProblemDto>> {
    Ok(Json(
        api.service
            .assign(&p, id, b.analyst_id.map(AnalystId::from_uuid))
            .await?
            .into(),
    ))
}

/// Notes, oldest first.
#[utoipa::path(get, path = "/problems/{id}/notes", tag = "Problems", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ProblemNoteDto>)))]
async fn list_problem_notes(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ProblemNoteDto>>> {
    Ok(Json(
        api.service
            .notes(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Adds a Note (needs `problems.update`).
#[utoipa::path(post, path = "/problems/{id}/notes", tag = "Problems", params(("id" = Uuid, Path)), request_body = ProblemNoteDto, responses((status = 201, body = ProblemNoteDto)))]
async fn add_problem_note(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ProblemNoteDto>,
) -> ApiResult<Response> {
    let note = api.service.add_note(&p, id, &b.body).await?;
    Ok((StatusCode::CREATED, Json(ProblemNoteDto::from(note))).into_response())
}

/// The audit trail of a Problem, oldest first (FR-PRB-007).
#[utoipa::path(get, path = "/problems/{id}/history", tag = "Problems", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ProblemHistoryDto>)))]
async fn problem_history(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ProblemHistoryDto>>> {
    Ok(Json(
        api.service
            .history(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// The linked tickets you may see (FR-PRB-003). The ticket's side:
/// `GET /problems?ticket_id=…`.
#[utoipa::path(get, path = "/problems/{id}/tickets", tag = "Problems", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<ProblemTicketDto>)))]
async fn list_problem_tickets(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ProblemTicketDto>>> {
    let rows = api.service.linked_tickets(&p, id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|(l, t)| linked(&t, l.linked_at))
            .collect(),
    ))
}

fn linked(t: &TicketSummary, at: DateTime<Utc>) -> ProblemTicketDto {
    ProblemTicketDto {
        id: t.id.as_uuid(),
        number: t.number.clone(),
        subject: t.subject.clone(),
        is_closed: t.is_closed,
        linked_at: at,
    }
}

/// Links a ticket of the same Company (needs `problems.update`).
#[utoipa::path(post, path = "/problems/{id}/tickets", tag = "Problems", params(("id" = Uuid, Path)), request_body = ProblemLinkTicketRequest, responses((status = 201, body = ProblemTicketDto)))]
async fn link_problem_ticket(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ProblemLinkTicketRequest>,
) -> ApiResult<Response> {
    let t = api
        .service
        .link_ticket(&p, id, TicketId::from_uuid(b.ticket_id))
        .await?;
    Ok((StatusCode::CREATED, Json(linked(&t, api.service.now()))).into_response())
}

/// Unlinks a ticket (needs `problems.update`).
#[utoipa::path(delete, path = "/problems/{id}/tickets/{ticket_id}", tag = "Problems", params(("id" = Uuid, Path), ("ticket_id" = Uuid, Path)), responses((status = 204, description = "Unlinked")))]
async fn unlink_problem_ticket(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path((id, ticket)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service
        .unlink_ticket(&p, id, TicketId::from_uuid(ticket))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Closes or adds a Note to every linked ticket of a resolved Problem (FR-PRB-006).
/// Each ticket goes through the usual service desk checks.
#[utoipa::path(post, path = "/problems/{id}/tickets/actions", tag = "Problems", params(("id" = Uuid, Path)), request_body = ProblemTicketActionRequest, responses((status = 200, body = Vec<ProblemTicketOutcomeDto>)))]
async fn act_on_problem_tickets(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<ProblemTicketActionRequest>,
) -> ApiResult<Json<Vec<ProblemTicketOutcomeDto>>> {
    let action = match b.action.as_str() {
        "close" => TicketAction::Close {
            resolution_code_id: b.resolution_code_id.ok_or_else(|| {
                ProblemError::invalid("resolution_code_id", "is required to close")
            })?,
        },
        "note" => TicketAction::Note {
            body: b
                .body
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| ProblemError::invalid("body", "is required for a Note"))?,
            public: b.public,
        },
        _ => return Err(ProblemError::invalid("action", "must be close or note").into()),
    };
    let outcomes = api.service.act_on_tickets(&p, id, &action).await?;
    Ok(Json(
        outcomes
            .into_iter()
            .map(|o| ProblemTicketOutcomeDto {
                ticket_id: o.ticket_id.as_uuid(),
                ok: o.error.is_none(),
                error: o.error,
            })
            .collect(),
    ))
}

/// The Knowledge Article of a Known Error.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemKnowledgeArticleDto {
    /// The Article (unpublished when new, for review in Knowledge).
    pub article_id: Uuid,
}

/// Publishes a Known Error's Root Cause and Workaround as an internal Knowledge Article,
/// or saves a new version of the one published before (needs `problems.update` and
/// `knowledge.write`; FR-PRB-005).
#[utoipa::path(post, path = "/problems/{id}/knowledge-article", tag = "Problems", params(("id" = Uuid, Path)),
    responses((status = 200, body = ProblemKnowledgeArticleDto), (status = 409, description = "Not a Known Error with a Workaround")))]
async fn publish_problem_known_error(
    State(api): State<ProblemsApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ProblemKnowledgeArticleDto>> {
    Ok(Json(ProblemKnowledgeArticleDto {
        article_id: api.service.publish_known_error(&p, id).await?,
    }))
}
