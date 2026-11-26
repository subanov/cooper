//! HTTP API of procedures on tasks (FR-TSK-007, ADR-0043).

use axum::Json;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::procedures::{Procedure, ProcedureError};
use crate::procedures_service::{ProcedureAppError, ProcedureService};

/// State the handlers need.
#[derive(Clone)]
pub struct ProcedureApi {
    /// The service.
    pub service: ProcedureService,
}

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ProcedureApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_procedures, attach_procedure))
        .routes(routes!(detach_procedure))
}

/// An error as problem+json.
pub struct ApiError(pub ProcedureAppError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ProcedureAppError::Task(e) = self.0.clone() {
            return crate::api::ApiError(e).into_response();
        }
        let detail = self.0.to_string();
        let typed = |status: StatusCode, slug: &str, title: &str| {
            ProblemJson::new(status)
                .with_type(format!("https://cooper.dev/problems/{slug}"), title)
                .with_detail(detail.clone())
        };
        match self.0 {
            ProcedureAppError::Rule(ProcedureError::Invalid { field, .. }) => {
                typed(StatusCode::UNPROCESSABLE_ENTITY, "validation", "Invalid input").with_extension("field", field)
            }
            ProcedureAppError::Task(_) => ProblemJson::internal(),
            ProcedureAppError::Forbidden(f) => forbidden(&f),
            ProcedureAppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            ProcedureAppError::Internal => ProblemJson::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// A procedure: an article's text as it was when attached.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProcedureDto {
    /// ID.
    pub id: Uuid,
    /// The task.
    pub task_id: Uuid,
    /// The article it came from.
    pub article_id: Uuid,
    /// The article version it copies.
    pub article_version: i32,
    /// Title, as attached.
    pub title: String,
    /// Body, as attached.
    pub body: String,
    /// When it was attached.
    pub attached_at: DateTime<Utc>,
}

impl From<&Procedure> for ProcedureDto {
    fn from(p: &Procedure) -> Self {
        Self {
            id: p.id,
            task_id: p.task_id,
            article_id: p.article_id,
            article_version: p.article_version,
            title: p.title.clone(),
            body: p.body.clone(),
            attached_at: p.attached_at,
        }
    }
}

/// The article to attach.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AttachProcedureRequest {
    /// A published Knowledge Article the caller may read.
    pub article_id: Uuid,
}

/// Procedures of a task (needs `tasks.read`).
#[utoipa::path(get, path = "/tasks/{id}/procedures", tag = "Tasks", params(("id" = Uuid, Path)), responses((status = 200, body = [ProcedureDto])))]
async fn list_procedures(State(api): State<ProcedureApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<ProcedureDto>>> {
    let rows = api.service.list(&p, id).await.map_err(ApiError)?;
    Ok(Json(rows.iter().map(ProcedureDto::from).collect()))
}

/// Attaches an article to a task as a procedure (needs `tasks.update`).
#[utoipa::path(post, path = "/tasks/{id}/procedures", tag = "Tasks", params(("id" = Uuid, Path)), request_body = AttachProcedureRequest, responses((status = 201, body = ProcedureDto)))]
async fn attach_procedure(State(api): State<ProcedureApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<AttachProcedureRequest>) -> ApiResult<Response> {
    let proc = api.service.attach(&p, id, b.article_id).await.map_err(ApiError)?;
    Ok((StatusCode::CREATED, Json(ProcedureDto::from(&proc))).into_response())
}

/// Removes a procedure from a task (needs `tasks.update`).
#[utoipa::path(delete, path = "/tasks/{id}/procedures/{procedure_id}", tag = "Tasks", params(("id" = Uuid, Path), ("procedure_id" = Uuid, Path)), responses((status = 204)))]
async fn detach_procedure(State(api): State<ProcedureApi>, Authenticated(p): Authenticated, Path((id, procedure)): Path<(Uuid, Uuid)>) -> ApiResult<StatusCode> {
    api.service.detach(&p, id, procedure).await.map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
