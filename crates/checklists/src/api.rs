//! HTTP API of Checklists (FR-TSK-008, FR-TKT-032, ADR-0043).

use axum::Json;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use cooper_kernel::TenantId;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::domain::{Checklist, ChecklistError, ClosureMode, ItemInput, TargetKind, Template};
use crate::service::{AppError, ChecklistService};

/// State the handlers need.
#[derive(Clone)]
pub struct ChecklistApi {
    /// The service.
    pub service: ChecklistService,
}

/// `/api/v1` path prefixes owned by checklists (FR-GEN-005).
pub const ROUTE_PREFIXES: &[&str] = &["/checklist-templates", "/checklists"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    ChecklistApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_checklist_templates, create_checklist_template))
        .routes(routes!(get_checklist_template))
        .routes(routes!(publish_checklist_template))
        .routes(routes!(list_ticket_checklists, attach_ticket_checklist))
        .routes(routes!(list_task_checklists, attach_task_checklist))
        .routes(routes!(get_checklist))
        .routes(routes!(complete_item))
        .routes(routes!(reopen_item))
}

/// An error as problem+json.
pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        Self(e)
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
            AppError::Invalid(ChecklistError::Invalid { field, .. }) => {
                typed(StatusCode::UNPROCESSABLE_ENTITY, "validation", "Invalid input").with_extension("field", field)
            }
            AppError::Invalid(ChecklistError::InvalidState(_)) => typed(StatusCode::CONFLICT, "rule-violated", "Not allowed in the current state"),
            AppError::Forbidden(f) => forbidden(&f),
            AppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            AppError::Conflict(_) => typed(StatusCode::CONFLICT, "conflict", "Conflict"),
            AppError::Internal => ProblemJson::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// One item of a template version or a request.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ItemDto {
    /// Text.
    pub text: String,
    /// Mandatory: blocks closure when the Checklist is blocking.
    #[serde(default)]
    pub mandatory: bool,
    /// Needs an answer when done.
    #[serde(default)]
    pub requires_input: bool,
}

impl From<&ItemInput> for ItemDto {
    fn from(i: &ItemInput) -> Self {
        Self { text: i.text.clone(), mandatory: i.mandatory, requires_input: i.requires_input }
    }
}

fn items(v: Vec<ItemDto>) -> Vec<ItemInput> {
    v.into_iter().map(|i| ItemInput { text: i.text, mandatory: i.mandatory, requires_input: i.requires_input }).collect()
}

/// A template with its current version.
#[derive(Debug, Serialize, ToSchema)]
pub struct TemplateDto {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub company_id: Uuid,
    /// Name.
    pub name: String,
    /// Category.
    pub category: String,
    /// Role.
    pub role: String,
    /// Current version number.
    pub current_version: u32,
    /// Closure mode of the current version.
    pub closure_mode: String,
    /// Items of the current version.
    pub items: Vec<ItemDto>,
    /// Number of published versions.
    pub versions: usize,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

impl From<&Template> for TemplateDto {
    fn from(t: &Template) -> Self {
        Self {
            id: t.id,
            company_id: t.tenant_id.as_uuid(),
            name: t.name.clone(),
            category: t.category.clone(),
            role: t.role.clone(),
            current_version: t.current.number,
            closure_mode: t.current.closure_mode.as_str().to_owned(),
            items: t.current.items.iter().map(ItemDto::from).collect(),
            versions: t.versions().count(),
            created_at: t.created_at,
        }
    }
}

/// A new template.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TemplateRequest {
    /// Company.
    pub company_id: Uuid,
    /// Name.
    pub name: String,
    /// Category.
    pub category: String,
    /// Role.
    pub role: String,
    /// `advisory` or `blocking`.
    pub closure_mode: String,
    /// Items.
    pub items: Vec<ItemDto>,
}

/// A new version of a template.
#[derive(Debug, Deserialize, ToSchema)]
pub struct VersionRequest {
    /// `advisory` or `blocking`.
    pub closure_mode: String,
    /// The complete item list.
    pub items: Vec<ItemDto>,
}

/// A Checklist on a ticket or task.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChecklistDto {
    /// ID.
    pub id: Uuid,
    /// `ticket` or `task`.
    pub target: String,
    /// The ticket or task.
    pub target_id: Uuid,
    /// The template it came from.
    pub template_id: Uuid,
    /// The version it copies.
    pub template_version: u32,
    /// Name.
    pub name: String,
    /// `advisory` or `blocking`.
    pub closure_mode: String,
    /// Items.
    pub items: Vec<ChecklistItemDto>,
    /// Whether it blocks closure now.
    pub blocks_closure: bool,
    /// When it was attached.
    pub created_at: DateTime<Utc>,
}

/// One item of a Checklist.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChecklistItemDto {
    /// ID.
    pub id: Uuid,
    /// Position.
    pub position: u32,
    /// Text.
    pub text: String,
    /// Mandatory.
    pub mandatory: bool,
    /// Needs an answer when done.
    pub requires_input: bool,
    /// Done.
    pub done: bool,
    /// Answer.
    pub answer: Option<String>,
    /// When it was last completed.
    pub done_at: Option<DateTime<Utc>>,
}

impl From<&Checklist> for ChecklistDto {
    fn from(c: &Checklist) -> Self {
        Self {
            id: c.id,
            target: c.target.as_str().to_owned(),
            target_id: c.target_id,
            template_id: c.template_id,
            template_version: c.template_version,
            name: c.name.clone(),
            closure_mode: c.closure_mode.as_str().to_owned(),
            items: c
                .items
                .iter()
                .map(|i| ChecklistItemDto {
                    id: i.id,
                    position: i.position,
                    text: i.text.clone(),
                    mandatory: i.mandatory,
                    requires_input: i.requires_input,
                    done: i.done,
                    answer: i.answer.clone(),
                    done_at: i.done_at,
                })
                .collect(),
            blocks_closure: c.blocks_closure(),
            created_at: c.created_at,
        }
    }
}

/// Attaches a template.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AttachRequest {
    /// The template (of the target's Company).
    pub template_id: Uuid,
}

/// The answer to an item that needs one.
#[derive(Debug, Deserialize, ToSchema, Default)]
pub struct CompleteRequest {
    /// The answer, required when the item needs one.
    pub answer: Option<String>,
}

fn mode(s: &str) -> Result<ClosureMode, ApiError> {
    s.parse::<ClosureMode>().map_err(|e| ApiError(AppError::Invalid(e)))
}

/// Checklist Templates in the caller's Companies (needs `checklists.read`).
#[utoipa::path(get, path = "/checklist-templates", tag = "Checklists", responses((status = 200, body = [TemplateDto])))]
async fn list_checklist_templates(State(api): State<ChecklistApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<TemplateDto>>> {
    Ok(Json(api.service.templates(&p).await?.iter().map(TemplateDto::from).collect()))
}

/// Creates a template (needs `checklists.configure`).
#[utoipa::path(post, path = "/checklist-templates", tag = "Checklists", request_body = TemplateRequest, responses((status = 201, body = TemplateDto)))]
async fn create_checklist_template(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Json(b): Json<TemplateRequest>) -> ApiResult<Response> {
    let t = api
        .service
        .create_template(&p, TenantId::from_uuid(b.company_id), &b.name, &b.category, &b.role, mode(&b.closure_mode)?, items(b.items))
        .await?;
    Ok((StatusCode::CREATED, Json(TemplateDto::from(&t))).into_response())
}

/// One template (needs `checklists.read`).
#[utoipa::path(get, path = "/checklist-templates/{id}", tag = "Checklists", params(("id" = Uuid, Path)), responses((status = 200, body = TemplateDto)))]
async fn get_checklist_template(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<TemplateDto>> {
    Ok(Json(TemplateDto::from(&api.service.template(&p, id).await?)))
}

/// Publishes a new version of a template (needs `checklists.configure`).
#[utoipa::path(post, path = "/checklist-templates/{id}/versions", tag = "Checklists", params(("id" = Uuid, Path)), request_body = VersionRequest, responses((status = 201, body = TemplateDto)))]
async fn publish_checklist_template(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<VersionRequest>) -> ApiResult<Response> {
    let t = api.service.publish_template(&p, id, mode(&b.closure_mode)?, items(b.items)).await?;
    Ok((StatusCode::CREATED, Json(TemplateDto::from(&t))).into_response())
}

/// Checklists on a ticket (needs `checklists.read`).
#[utoipa::path(get, path = "/tickets/{id}/checklists", tag = "Checklists", params(("id" = Uuid, Path)), responses((status = 200, body = [ChecklistDto])))]
async fn list_ticket_checklists(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<ChecklistDto>>> {
    Ok(Json(api.service.on(&p, TargetKind::Ticket, id).await?.iter().map(ChecklistDto::from).collect()))
}

/// Attaches a template to a ticket (needs `checklists.write`).
#[utoipa::path(post, path = "/tickets/{id}/checklists", tag = "Checklists", params(("id" = Uuid, Path)), request_body = AttachRequest, responses((status = 201, body = ChecklistDto)))]
async fn attach_ticket_checklist(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<AttachRequest>) -> ApiResult<Response> {
    let c = api.service.attach(&p, TargetKind::Ticket, id, b.template_id).await?;
    Ok((StatusCode::CREATED, Json(ChecklistDto::from(&c))).into_response())
}

/// Checklists on a task (needs `checklists.read`).
#[utoipa::path(get, path = "/tasks/{id}/checklists", tag = "Checklists", params(("id" = Uuid, Path)), responses((status = 200, body = [ChecklistDto])))]
async fn list_task_checklists(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<ChecklistDto>>> {
    Ok(Json(api.service.on(&p, TargetKind::Task, id).await?.iter().map(ChecklistDto::from).collect()))
}

/// Attaches a template to a task (needs `checklists.write`).
#[utoipa::path(post, path = "/tasks/{id}/checklists", tag = "Checklists", params(("id" = Uuid, Path)), request_body = AttachRequest, responses((status = 201, body = ChecklistDto)))]
async fn attach_task_checklist(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<AttachRequest>) -> ApiResult<Response> {
    let c = api.service.attach(&p, TargetKind::Task, id, b.template_id).await?;
    Ok((StatusCode::CREATED, Json(ChecklistDto::from(&c))).into_response())
}

/// One Checklist (needs `checklists.read`).
#[utoipa::path(get, path = "/checklists/{id}", tag = "Checklists", params(("id" = Uuid, Path)), responses((status = 200, body = ChecklistDto)))]
async fn get_checklist(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<ChecklistDto>> {
    let found = api.service.on_checklist(&p, id).await?;
    Ok(Json(ChecklistDto::from(&found)))
}

/// Completes an item (needs `checklists.write`).
#[utoipa::path(post, path = "/checklists/{id}/items/{item_id}/complete", tag = "Checklists", params(("id" = Uuid, Path), ("item_id" = Uuid, Path)), request_body = CompleteRequest, responses((status = 200, body = ChecklistDto)))]
async fn complete_item(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path((id, item)): Path<(Uuid, Uuid)>, Json(b): Json<CompleteRequest>) -> ApiResult<Json<ChecklistDto>> {
    let c = api.service.complete_item(&p, id, item, b.answer.as_deref()).await?;
    Ok(Json(ChecklistDto::from(&c)))
}

/// Marks an item not done again (needs `checklists.write`).
#[utoipa::path(post, path = "/checklists/{id}/items/{item_id}/reopen", tag = "Checklists", params(("id" = Uuid, Path), ("item_id" = Uuid, Path)), responses((status = 200, body = ChecklistDto)))]
async fn reopen_item(State(api): State<ChecklistApi>, Authenticated(p): Authenticated, Path((id, item)): Path<(Uuid, Uuid)>) -> ApiResult<Json<ChecklistDto>> {
    let c = api.service.reopen_item(&p, id, item).await?;
    Ok(Json(ChecklistDto::from(&c)))
}
