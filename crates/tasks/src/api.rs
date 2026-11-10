//! HTTP API of Tasks (FR-TSK).

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::{AnalystId, Cursor, Limit, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::application::{AppError, Column, NewTask, TaskService};
use crate::domain::{Lookup, LookupInput, LookupKind, Task, TaskDetails, TaskError};
use crate::ports::{LinkKind, Recurrence, TaskComment, TaskLink, TaskQuery, TimeEntry};
use crate::recurrence::RecurrenceRule;

/// State the handlers need.
#[derive(Clone)]
pub struct TasksApi {
    /// The service.
    pub service: TaskService,
}

/// `/api/v1` path prefixes owned by the Tasks Module (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &["/tasks", "/task-statuses", "/task-priorities"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    TasksApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_task_statuses, create_task_status))
        .routes(routes!(update_task_status))
        .routes(routes!(list_task_priorities, create_task_priority))
        .routes(routes!(update_task_priority))
        .routes(routes!(list_tasks, create_task))
        .routes(routes!(task_board))
        .routes(routes!(get_task, edit_task))
        .routes(routes!(move_task))
        .routes(routes!(get_task_collaborators, set_task_collaborators))
        .routes(routes!(list_task_comments, add_task_comment))
        .routes(routes!(list_task_time, add_task_time))
        .routes(routes!(delete_task_time))
        .routes(routes!(list_task_links, link_task))
        .routes(routes!(unlink_task))
        .routes(routes!(get_task_recurrence, set_task_recurrence))
        .routes(routes!(task_history))
}

/// An application error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
    }
}

impl From<TaskError> for ApiError {
    fn from(e: TaskError) -> Self {
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
            AppError::Rule(TaskError::Invalid { field, .. }) => typed(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "Invalid input",
            )
            .with_extension("field", field),
            AppError::Rule(TaskError::InvalidState(_)) => typed(
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

// ----- Reference data ---------------------------------------------------------------------------

/// A Task Status or Priority.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskLookupDto {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// Statuses: finished.
    pub is_closed: bool,
    /// Statuses: finished by completing (repeats on completion).
    pub is_done: bool,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// Default.
    pub is_default: bool,
    /// Order (Board columns follow it).
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl From<Lookup> for TaskLookupDto {
    fn from(l: Lookup) -> Self {
        Self {
            id: l.id,
            key: l.key,
            name: l.name,
            is_closed: l.is_closed,
            is_done: l.is_done,
            colour: l.colour,
            is_default: l.is_default,
            sort_order: l.sort_order,
            is_active: l.is_active,
        }
    }
}

/// Creates or changes a Task Status or Priority.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TaskLookupRequest {
    /// Name.
    pub name: String,
    /// Statuses: closed.
    #[serde(default)]
    pub is_closed: bool,
    /// Statuses: done (closed by completing).
    #[serde(default)]
    pub is_done: bool,
    /// `#rrggbb`.
    pub colour: Option<String>,
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

const fn yes() -> bool {
    true
}

impl From<TaskLookupRequest> for LookupInput {
    fn from(r: TaskLookupRequest) -> Self {
        Self {
            name: r.name,
            is_closed: r.is_closed,
            is_done: r.is_done,
            colour: r.colour,
            is_default: r.is_default,
            sort_order: r.sort_order,
            is_active: r.is_active,
        }
    }
}

macro_rules! lookup_handlers {
    ($kind:expr, $path:literal, $item:literal, $list:ident, $create:ident, $update:ident) => {
        #[utoipa::path(get, path = $path, tag = "Tasks", responses((status = 200, body = Vec<TaskLookupDto>)))]
        async fn $list(State(api): State<TasksApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<TaskLookupDto>>> {
            Ok(Json(api.service.lookups(&p, $kind).await?.into_iter().map(Into::into).collect()))
        }

        #[utoipa::path(post, path = $path, tag = "Tasks", request_body = TaskLookupRequest, responses((status = 201, body = TaskLookupDto)))]
        async fn $create(State(api): State<TasksApi>, Authenticated(p): Authenticated, Json(b): Json<TaskLookupRequest>) -> ApiResult<Response> {
            Ok(created(TaskLookupDto::from(api.service.create_lookup(&p, $kind, b.into()).await?)))
        }

        #[utoipa::path(put, path = $item, tag = "Tasks", params(("id" = Uuid, Path)), request_body = TaskLookupRequest, responses((status = 200, body = TaskLookupDto)))]
        async fn $update(
            State(api): State<TasksApi>,
            Authenticated(p): Authenticated,
            Path(id): Path<Uuid>,
            Json(b): Json<TaskLookupRequest>,
        ) -> ApiResult<Json<TaskLookupDto>> {
            Ok(Json(api.service.update_lookup(&p, $kind, id, b.into()).await?.into()))
        }
    };
}

lookup_handlers!(
    LookupKind::Status,
    "/task-statuses",
    "/task-statuses/{id}",
    list_task_statuses,
    create_task_status,
    update_task_status
);
lookup_handlers!(
    LookupKind::Priority,
    "/task-priorities",
    "/task-priorities/{id}",
    list_task_priorities,
    create_task_priority,
    update_task_priority
);

// ----- Tasks ------------------------------------------------------------------------------------

/// A Task (FR-TSK-001).
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Status.
    pub status_id: Uuid,
    /// Whether the status is closed.
    pub is_closed: bool,
    /// Priority.
    pub priority_id: Uuid,
    /// Planned start.
    pub start_date: Option<NaiveDate>,
    /// Deadline.
    pub due_date: Option<NaiveDate>,
    /// Estimate in minutes.
    pub estimate_minutes: Option<i32>,
    /// Assigned Analyst.
    pub assigned_analyst_id: Option<Uuid>,
    /// Assigned Team.
    pub assigned_team_id: Option<Uuid>,
    /// Parent (for a Subtask).
    pub parent_task_id: Option<Uuid>,
    /// Tags.
    pub tags: Vec<String>,
    /// Place in its Board column.
    pub board_position: i64,
    /// Repeat series.
    pub recurrence_id: Option<Uuid>,
    /// The occurrence it followed.
    pub previous_occurrence_id: Option<Uuid>,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
    /// Completed or cancelled.
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<Task> for TaskDto {
    fn from(t: Task) -> Self {
        let d = t.details().clone();
        Self {
            id: t.id(),
            company_id: t.tenant_id().as_uuid(),
            title: d.title,
            description: d.description,
            status_id: t.status_id(),
            is_closed: t.is_closed(),
            priority_id: d.priority_id,
            start_date: d.start_date,
            due_date: d.due_date,
            estimate_minutes: d.estimate_minutes,
            assigned_analyst_id: d.assigned_analyst.map(|a| a.as_uuid()),
            assigned_team_id: d.assigned_team.map(|x| x.as_uuid()),
            parent_task_id: t.parent_task_id(),
            tags: d.tags,
            board_position: t.board_position(),
            recurrence_id: t.recurrence_id(),
            previous_occurrence_id: t.previous_occurrence_id(),
            created_at: t.created_at(),
            updated_at: t.updated_at(),
            completed_at: t.completed_at(),
        }
    }
}

/// Creates or edits a Task.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TaskRequest {
    /// Company (create only; a Subtask takes its parent's).
    pub company_id: Option<Uuid>,
    /// Parent, to create a Subtask (create only).
    pub parent_task_id: Option<Uuid>,
    /// Status (create only; default when absent).
    pub status_id: Option<Uuid>,
    /// Title.
    pub title: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Priority (default when absent).
    pub priority_id: Option<Uuid>,
    /// Planned start.
    pub start_date: Option<NaiveDate>,
    /// Deadline.
    pub due_date: Option<NaiveDate>,
    /// Estimate in minutes.
    pub estimate_minutes: Option<i32>,
    /// Assigned Analyst.
    pub assigned_analyst_id: Option<Uuid>,
    /// Assigned Team.
    pub assigned_team_id: Option<Uuid>,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Collaborators (create only; use the collaborators endpoint afterwards).
    #[serde(default)]
    pub collaborator_ids: Vec<Uuid>,
}

impl TaskRequest {
    fn details(&self) -> TaskDetails {
        TaskDetails {
            title: self.title.clone(),
            description: self.description.clone(),
            priority_id: self.priority_id.unwrap_or_else(Uuid::nil),
            start_date: self.start_date,
            due_date: self.due_date,
            estimate_minutes: self.estimate_minutes,
            assigned_analyst: self.assigned_analyst_id.map(AnalystId::from_uuid),
            assigned_team: self.assigned_team_id.map(TeamId::from_uuid),
            tags: self.tags.clone(),
        }
    }
}

/// A page of Tasks.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskPage {
    /// Tasks, newest activity first.
    pub items: Vec<TaskDto>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

/// Filters (also for the Board).
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TaskListParams {
    /// Open (`true`) or closed (`false`).
    pub open: Option<bool>,
    /// Status.
    pub status_id: Option<Uuid>,
    /// Assigned Analyst (`me` is accepted).
    pub assignee: Option<String>,
    /// Assigned Team.
    pub team_id: Option<Uuid>,
    /// Collaborator (`me` is accepted).
    pub collaborator: Option<String>,
    /// Tag.
    pub tag: Option<String>,
    /// Subtasks of this Task.
    pub parent_task_id: Option<Uuid>,
    /// Top-level Tasks only.
    pub top_level: Option<bool>,
    /// Due on or before.
    pub due_by: Option<NaiveDate>,
    /// Linked to this ticket.
    pub ticket_id: Option<Uuid>,
    /// Linked to this Change.
    pub change_id: Option<Uuid>,
    /// Title text.
    pub q: Option<String>,
    /// Page size (1 to 200; per column on the Board).
    pub limit: Option<u32>,
    /// Cursor.
    pub cursor: Option<String>,
}

fn analyst_param(
    p: &cooper_kernel::Principal,
    field: &'static str,
    v: Option<&str>,
) -> Result<Option<AnalystId>, ApiError> {
    match v {
        None => Ok(None),
        Some("me") => match p.actor() {
            cooper_kernel::Actor::Analyst(a) => Ok(Some(a)),
            _ => Err(TaskError::invalid(field, "`me` needs an Analyst").into()),
        },
        Some(id) => {
            Ok(Some(AnalystId::from_uuid(id.parse().map_err(|_| {
                TaskError::invalid(field, "must be an ID or `me`")
            })?)))
        }
    }
}

fn query(p: &cooper_kernel::Principal, q: TaskListParams) -> Result<TaskQuery, ApiError> {
    let limit = q
        .limit
        .map_or(Ok(Limit::DEFAULT), Limit::new)
        .map_err(|_| TaskError::invalid("limit", "must be between 1 and 200"))?;
    let after = q
        .cursor
        .map(|c| Cursor::from_client(c).decode::<(DateTime<Utc>, Uuid)>())
        .transpose()
        .map_err(|_| TaskError::invalid("cursor", "is not valid"))?;
    let linked = match (q.ticket_id, q.change_id) {
        (Some(t), None) => Some((LinkKind::Ticket, t)),
        (None, Some(c)) => Some((LinkKind::Change, c)),
        (None, None) => None,
        (Some(_), Some(_)) => {
            return Err(
                TaskError::invalid("change_id", "cannot be combined with ticket_id").into(),
            );
        }
    };
    Ok(TaskQuery {
        open: q.open,
        status_id: q.status_id,
        assigned_analyst: analyst_param(p, "assignee", q.assignee.as_deref())?,
        assigned_team: q.team_id.map(TeamId::from_uuid),
        collaborator: analyst_param(p, "collaborator", q.collaborator.as_deref())?,
        tag: q
            .tag
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty()),
        parent_id: q.parent_task_id,
        top_level: q.top_level.unwrap_or(false),
        due_by: q.due_by,
        linked,
        search: q.q,
        board_order: false,
        limit,
        after,
    })
}

/// Tasks in your Companies, newest activity first (needs `tasks.read`).
#[utoipa::path(get, path = "/tasks", tag = "Tasks", params(TaskListParams), responses((status = 200, body = TaskPage)))]
async fn list_tasks(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<TaskListParams>,
) -> ApiResult<Json<TaskPage>> {
    let query = query(&p, q)?;
    let rows = api.service.list(&p, &query).await?;
    let page = cooper_kernel::Page::from_rows(rows, query.limit, |t| {
        Cursor::encode(&(t.updated_at(), t.id()))
    })
    .map_err(|_| AppError::Internal)?;
    Ok(Json(TaskPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor.map(|c| c.as_str().to_owned()),
    }))
}

/// Creates a Task or Subtask (needs `tasks.create`).
#[utoipa::path(post, path = "/tasks", tag = "Tasks", request_body = TaskRequest, responses((status = 201, body = TaskDto)))]
async fn create_task(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Json(b): Json<TaskRequest>,
) -> ApiResult<Response> {
    let new = NewTask {
        tenant_id: b.company_id.map(TenantId::from_uuid),
        details: b.details(),
        status_id: b.status_id,
        parent_id: b.parent_task_id,
        collaborators: b
            .collaborator_ids
            .iter()
            .copied()
            .map(AnalystId::from_uuid)
            .collect(),
    };
    Ok(created(TaskDto::from(api.service.create(&p, new).await?)))
}

/// A Board column.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskBoardColumnDto {
    /// The status.
    pub status: TaskLookupDto,
    /// Its top-level Tasks in Board order.
    pub tasks: Vec<TaskDto>,
    /// More Tasks than `limit` are in this column.
    pub has_more: bool,
}

/// The Board (FR-TSK-005): a column per active status, top-level Tasks in Board order.
#[utoipa::path(get, path = "/tasks/board", tag = "Tasks", params(TaskListParams), responses((status = 200, body = Vec<TaskBoardColumnDto>)))]
async fn task_board(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Query(q): Query<TaskListParams>,
) -> ApiResult<Json<Vec<TaskBoardColumnDto>>> {
    let query = query(&p, q)?;
    let limit = usize::try_from(query.limit.get()).unwrap_or(usize::MAX);
    let columns: Vec<Column> = api.service.board(&p, query).await?;
    Ok(Json(
        columns
            .into_iter()
            .map(|c| {
                let has_more = c.tasks.len() > limit;
                TaskBoardColumnDto {
                    status: c.status.into(),
                    tasks: c.tasks.into_iter().take(limit).map(Into::into).collect(),
                    has_more,
                }
            })
            .collect(),
    ))
}

/// One Task.
#[utoipa::path(get, path = "/tasks/{id}", tag = "Tasks", params(("id" = Uuid, Path)), responses((status = 200, body = TaskDto)))]
async fn get_task(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TaskDto>> {
    Ok(Json(api.service.get(&p, id).await?.into()))
}

/// Edits a Task (needs `tasks.update`); assigning an Analyst notifies them.
#[utoipa::path(put, path = "/tasks/{id}", tag = "Tasks", params(("id" = Uuid, Path)), request_body = TaskRequest, responses((status = 200, body = TaskDto)))]
async fn edit_task(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<TaskRequest>,
) -> ApiResult<Json<TaskDto>> {
    Ok(Json(api.service.edit(&p, id, b.details()).await?.into()))
}

/// A move on the Board.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TaskMoveRequest {
    /// The status (column).
    pub status_id: Uuid,
    /// Place in the column, 0-based (absent: keep the position).
    pub index: Option<usize>,
}

/// Moves a Task to a status and place on the Board (needs `tasks.update`). Completing
/// a Task that repeats on completion creates its next occurrence.
#[utoipa::path(post, path = "/tasks/{id}/status", tag = "Tasks", params(("id" = Uuid, Path)), request_body = TaskMoveRequest, responses((status = 200, body = TaskDto)))]
async fn move_task(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<TaskMoveRequest>,
) -> ApiResult<Json<TaskDto>> {
    Ok(Json(
        api.service
            .move_to(&p, id, b.status_id, b.index)
            .await?
            .into(),
    ))
}

/// Collaborators.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TaskCollaboratorsDto {
    /// The Analysts.
    pub analyst_ids: Vec<Uuid>,
}

fn collaborators(set: BTreeSet<AnalystId>) -> TaskCollaboratorsDto {
    TaskCollaboratorsDto {
        analyst_ids: set.into_iter().map(|a| a.as_uuid()).collect(),
    }
}

/// Collaborators of a Task.
#[utoipa::path(get, path = "/tasks/{id}/collaborators", tag = "Tasks", params(("id" = Uuid, Path)), responses((status = 200, body = TaskCollaboratorsDto)))]
async fn get_task_collaborators(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TaskCollaboratorsDto>> {
    Ok(Json(collaborators(
        api.service.collaborators(&p, id).await?,
    )))
}

/// Replaces the collaborators (needs `tasks.update`).
#[utoipa::path(put, path = "/tasks/{id}/collaborators", tag = "Tasks", params(("id" = Uuid, Path)), request_body = TaskCollaboratorsDto, responses((status = 200, body = TaskCollaboratorsDto)))]
async fn set_task_collaborators(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<TaskCollaboratorsDto>,
) -> ApiResult<Json<TaskCollaboratorsDto>> {
    let who = b
        .analyst_ids
        .into_iter()
        .map(AnalystId::from_uuid)
        .collect();
    Ok(Json(collaborators(
        api.service.set_collaborators(&p, id, who).await?,
    )))
}

/// A comment.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskCommentDto {
    /// ID.
    pub id: Uuid,
    /// Author.
    pub author_id: Option<Uuid>,
    /// Text.
    pub body: String,
    /// When.
    pub created_at: DateTime<Utc>,
}

impl From<TaskComment> for TaskCommentDto {
    fn from(c: TaskComment) -> Self {
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
pub struct TaskCommentRequest {
    /// Text.
    pub body: String,
}

/// Comments, oldest first.
#[utoipa::path(get, path = "/tasks/{id}/comments", tag = "Tasks", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<TaskCommentDto>)))]
async fn list_task_comments(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TaskCommentDto>>> {
    Ok(Json(
        api.service
            .comments(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Adds a comment (needs `tasks.update`).
#[utoipa::path(post, path = "/tasks/{id}/comments", tag = "Tasks", params(("id" = Uuid, Path)), request_body = TaskCommentRequest, responses((status = 201, body = TaskCommentDto)))]
async fn add_task_comment(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<TaskCommentRequest>,
) -> ApiResult<Response> {
    Ok(created(TaskCommentDto::from(
        api.service.add_comment(&p, id, &b.body).await?,
    )))
}

/// Time spent.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskTimeEntryDto {
    /// ID.
    pub id: Uuid,
    /// Who.
    pub analyst_id: Uuid,
    /// Which day.
    pub work_date: NaiveDate,
    /// How long.
    pub minutes: i32,
    /// What.
    pub note: String,
}

impl From<TimeEntry> for TaskTimeEntryDto {
    fn from(e: TimeEntry) -> Self {
        Self {
            id: e.id,
            analyst_id: e.analyst_id.as_uuid(),
            work_date: e.work_date,
            minutes: e.minutes,
            note: e.note,
        }
    }
}

/// Records time.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TaskTimeRequest {
    /// Which day (default today).
    pub work_date: Option<NaiveDate>,
    /// Minutes, 1–1440.
    pub minutes: i32,
    /// What was done.
    #[serde(default)]
    pub note: String,
}

/// Time entries, by day.
#[utoipa::path(get, path = "/tasks/{id}/time-entries", tag = "Tasks", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<TaskTimeEntryDto>)))]
async fn list_task_time(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TaskTimeEntryDto>>> {
    Ok(Json(
        api.service
            .time_entries(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Records your time on a Task (needs `tasks.update`).
#[utoipa::path(post, path = "/tasks/{id}/time-entries", tag = "Tasks", params(("id" = Uuid, Path)), request_body = TaskTimeRequest, responses((status = 201, body = TaskTimeEntryDto)))]
async fn add_task_time(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<TaskTimeRequest>,
) -> ApiResult<Response> {
    Ok(created(TaskTimeEntryDto::from(
        api.service
            .add_time(&p, id, b.work_date, b.minutes, &b.note)
            .await?,
    )))
}

/// Removes a time entry: your own, or anyone's with `tasks.configure`.
#[utoipa::path(delete, path = "/tasks/{id}/time-entries/{entry_id}", tag = "Tasks", params(("id" = Uuid, Path), ("entry_id" = Uuid, Path)), responses((status = 204)))]
async fn delete_task_time(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path((id, entry)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service.delete_time(&p, id, entry).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A linked ticket or Change.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskLinkDto {
    /// `ticket` or `change`.
    pub kind: String,
    /// The record.
    pub target_id: Uuid,
    /// Its number.
    pub number: String,
    /// When it was linked.
    pub linked_at: DateTime<Utc>,
}

impl From<(TaskLink, String)> for TaskLinkDto {
    fn from((l, number): (TaskLink, String)) -> Self {
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
pub struct TaskLinkRequest {
    /// `ticket` or `change`.
    pub kind: String,
    /// The record (same Company).
    pub target_id: Uuid,
}

/// Linked tickets and Changes you may see (FR-TSK-004).
#[utoipa::path(get, path = "/tasks/{id}/links", tag = "Tasks", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<TaskLinkDto>)))]
async fn list_task_links(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TaskLinkDto>>> {
    Ok(Json(
        api.service
            .links(&p, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

/// Links a ticket or Change of the same Company (needs `tasks.update`).
#[utoipa::path(post, path = "/tasks/{id}/links", tag = "Tasks", params(("id" = Uuid, Path)), request_body = TaskLinkRequest, responses((status = 201, body = TaskLinkDto)))]
async fn link_task(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<TaskLinkRequest>,
) -> ApiResult<Response> {
    let kind = b.kind.parse::<LinkKind>()?;
    Ok(created(TaskLinkDto::from(
        api.service.link(&p, id, kind, b.target_id).await?,
    )))
}

/// Removes a link (needs `tasks.update`).
#[utoipa::path(delete, path = "/tasks/{id}/links/{kind}/{target_id}", tag = "Tasks", params(("id" = Uuid, Path), ("kind" = String, Path), ("target_id" = Uuid, Path)), responses((status = 204)))]
async fn unlink_task(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path((id, kind, target)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<StatusCode> {
    api.service
        .unlink(&p, id, kind.parse::<LinkKind>()?, target)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A Task's repeat series.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskRecurrenceDto {
    /// Series ID.
    pub id: Uuid,
    /// The rule: `frequency` (`daily`, `weekly` with `weekdays`, `monthly` with `on` =
    /// `day_of_month` (`day`) or `nth_weekday` (`nth`, `weekday`), `yearly` with `month`
    /// and `day`), `interval`, `mode` (`on_completion` or `fixed`), and `ends` (`never`,
    /// `on_date` with `date`, `after_count` with `count`).
    #[schema(value_type = Object)]
    pub rule: serde_json::Value,
    /// Occurrences made so far.
    pub occurrences_created: u32,
    /// Fixed schedule: the next date.
    pub next_date: Option<NaiveDate>,
    /// Active.
    pub is_active: bool,
}

impl From<Recurrence> for TaskRecurrenceDto {
    fn from(r: Recurrence) -> Self {
        Self {
            id: r.id,
            rule: serde_json::to_value(&r.rule).unwrap_or_default(),
            occurrences_created: r.occurrences_created,
            next_date: r.next_date,
            is_active: r.is_active,
        }
    }
}

/// Sets or stops a Recurrence.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TaskRecurrenceRequest {
    /// The rule (see the response), or null to stop repeating.
    #[schema(value_type = Object)]
    pub rule: Option<serde_json::Value>,
}

/// The Task's Recurrence, if any.
#[utoipa::path(get, path = "/tasks/{id}/recurrence", tag = "Tasks", params(("id" = Uuid, Path)), responses((status = 200, body = Option<TaskRecurrenceDto>)))]
async fn get_task_recurrence(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Option<TaskRecurrenceDto>>> {
    Ok(Json(api.service.recurrence(&p, id).await?.map(Into::into)))
}

/// Starts, replaces or stops a Task's Recurrence (needs `tasks.update`; FR-TSK-006).
#[utoipa::path(put, path = "/tasks/{id}/recurrence", tag = "Tasks", params(("id" = Uuid, Path)), request_body = TaskRecurrenceRequest, responses((status = 200, body = Option<TaskRecurrenceDto>)))]
async fn set_task_recurrence(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
    Json(b): Json<TaskRecurrenceRequest>,
) -> ApiResult<Json<Option<TaskRecurrenceDto>>> {
    let rule = b
        .rule
        .map(serde_json::from_value::<RecurrenceRule>)
        .transpose()
        .map_err(|e| TaskError::invalid("rule", format!("is not a valid rule: {e}")))?;
    let (_, series) = api.service.set_recurrence(&p, id, rule).await?;
    Ok(Json(series.map(Into::into)))
}

/// One change in a Task's history.
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskHistoryDto {
    /// When.
    pub at: DateTime<Utc>,
    /// Who (`{kind, id}`).
    #[schema(value_type = Object)]
    pub actor: serde_json::Value,
    /// `created` or `updated`.
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

/// The audit trail of a Task (FR-TSK-003).
#[utoipa::path(get, path = "/tasks/{id}/history", tag = "Tasks", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<TaskHistoryDto>)))]
async fn task_history(
    State(api): State<TasksApi>,
    Authenticated(p): Authenticated,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TaskHistoryDto>>> {
    Ok(Json(
        api.service
            .history(&p, id)
            .await?
            .into_iter()
            .map(|e| TaskHistoryDto {
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
