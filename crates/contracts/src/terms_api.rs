//! HTTP API of Contract Terms (FR-CON-005, ADR-0042).

use axum::Json;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cooper_http::auth::forbidden;
use cooper_http::{Authenticated, Problem as ProblemJson};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::domain::ContractId;
use crate::terms::TermTab;
use crate::terms_service::{TermsAppError, TermsService};

/// State the handlers need.
#[derive(Clone)]
pub struct TermsApi {
    /// The service.
    pub service: TermsService,
}

/// `/api/v1` paths the Terms add to the Contracts Module.
pub const ROUTE_PREFIXES: &[&str] = &["/contract-term-tabs"];

/// The routes, to be nested under `/api/v1`.
pub fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    TermsApi: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(list_tabs, create_tab))
        .routes(routes!(update_tab))
        .routes(routes!(get_terms, set_terms))
}

/// An error as problem+json.
pub struct ApiError(pub TermsAppError);

impl From<TermsAppError> for ApiError {
    fn from(e: TermsAppError) -> Self {
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
            TermsAppError::Invalid(e) => {
                let field = match &e {
                    crate::terms::TermsError::Invalid { field, .. } => *field,
                };
                typed(StatusCode::UNPROCESSABLE_ENTITY, "validation", "Invalid input").with_extension("field", field)
            }
            TermsAppError::Contract(_) | TermsAppError::NotFound(_) => typed(StatusCode::NOT_FOUND, "not-found", "Not found"),
            TermsAppError::Forbidden(f) => forbidden(&f),
            TermsAppError::Conflict => typed(StatusCode::CONFLICT, "conflict", "Conflict"),
            TermsAppError::Internal => ProblemJson::internal(),
        }
        .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// A Terms tab.
#[derive(Debug, Serialize, ToSchema)]
pub struct TermTabDto {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Display order.
    pub sort_order: i32,
    /// Active.
    pub active: bool,
}

impl From<&TermTab> for TermTabDto {
    fn from(t: &TermTab) -> Self {
        Self { id: t.id, name: t.name.clone(), sort_order: t.sort_order, active: t.active }
    }
}

/// A tab to create or change.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TermTabRequest {
    /// Name.
    pub name: String,
    /// Display order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active (default true).
    #[serde(default = "yes")]
    pub active: bool,
}

fn yes() -> bool {
    true
}

/// One value of a Contract's Terms.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TermValueDto {
    /// The tab.
    pub tab_id: Uuid,
    /// The value.
    pub value: String,
}

/// The Terms of a Contract.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TermsDto {
    /// Values by tab (blank values are not kept).
    pub values: Vec<TermValueDto>,
}

/// All Contract Terms tabs (needs `contracts.read`).
#[utoipa::path(get, path = "/contract-term-tabs", tag = "Contracts", responses((status = 200, body = [TermTabDto])))]
async fn list_tabs(State(api): State<TermsApi>, Authenticated(p): Authenticated) -> ApiResult<Json<Vec<TermTabDto>>> {
    let tabs = api.service.tabs(&p).await?;
    Ok(Json(tabs.iter().map(TermTabDto::from).collect()))
}

/// Creates a Contract Terms tab (needs `contracts.configure`).
#[utoipa::path(post, path = "/contract-term-tabs", tag = "Contracts", request_body = TermTabRequest, responses((status = 201, body = TermTabDto)))]
async fn create_tab(State(api): State<TermsApi>, Authenticated(p): Authenticated, Json(b): Json<TermTabRequest>) -> ApiResult<Response> {
    let tab = api.service.create_tab(&p, &b.name, b.sort_order).await?;
    Ok((StatusCode::CREATED, Json(TermTabDto::from(&tab))).into_response())
}

/// Changes a Contract Terms tab (needs `contracts.configure`).
#[utoipa::path(put, path = "/contract-term-tabs/{id}", tag = "Contracts", params(("id" = Uuid, Path)), request_body = TermTabRequest, responses((status = 200, body = TermTabDto)))]
async fn update_tab(State(api): State<TermsApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<TermTabRequest>) -> ApiResult<Json<TermTabDto>> {
    let tab = api.service.update_tab(&p, id, &b.name, b.sort_order, b.active).await?;
    Ok(Json(TermTabDto::from(&tab)))
}

/// The Terms of a Contract (needs `contracts.read`).
#[utoipa::path(get, path = "/contracts/{id}/terms", tag = "Contracts", params(("id" = Uuid, Path)), responses((status = 200, body = TermsDto)))]
async fn get_terms(State(api): State<TermsApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>) -> ApiResult<Json<TermsDto>> {
    let values = api.service.values(&p, ContractId::from_uuid(id)).await?;
    Ok(Json(TermsDto { values: values.into_iter().map(|(tab_id, value)| TermValueDto { tab_id, value }).collect() }))
}

/// Replaces the Terms of a Contract (needs `contracts.write`).
#[utoipa::path(put, path = "/contracts/{id}/terms", tag = "Contracts", params(("id" = Uuid, Path)), request_body = TermsDto, responses((status = 200, body = TermsDto)))]
async fn set_terms(State(api): State<TermsApi>, Authenticated(p): Authenticated, Path(id): Path<Uuid>, Json(b): Json<TermsDto>) -> ApiResult<Json<TermsDto>> {
    let given: Vec<(Uuid, String)> = b.values.into_iter().map(|v| (v.tab_id, v.value)).collect();
    let values = api.service.set_values(&p, ContractId::from_uuid(id), &given).await?;
    Ok(Json(TermsDto { values: values.into_iter().map(|(tab_id, value)| TermValueDto { tab_id, value }).collect() }))
}
