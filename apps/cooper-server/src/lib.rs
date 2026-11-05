//! Cooper HTTP server.
//!
//! Wires the bounded contexts' routers under `/api/v1`, adds health endpoints and the
//! OpenAPI document, and wraps everything in the standard middleware stack. The binary
//! (`main.rs`) only reads configuration and starts serving; [`router`] is public so tests
//! can drive the full stack with `tower::ServiceExt::oneshot`.

mod adapters;
pub mod config;
mod health;
mod state;

use axum::routing::get;
use axum::{Json, Router};
use cooper_http::HttpConfig;
use utoipa::OpenApi as _;
use utoipa::openapi::OpenApi;
use utoipa_axum::router::OpenApiRouter;

pub use state::{
    AppState, SecuritySettings, StateError, capability_catalogue, module_routes, setting_catalogue,
};

#[derive(utoipa::OpenApi)]
#[openapi(
    info(title = "Cooper API", description = "Cooper IT Service Management REST API (FR-API-001)."),
    tags(
        (name = "Setup and session", description = "First-run setup, sign-in and sign-out"),
        (name = "Companies", description = "Companies (Tenants) and Sender Domains"),
        (name = "Analysts", description = "IT staff, their Roles and Company access"),
        (name = "Teams", description = "Teams (Queues), members, Roles and Company access"),
        (name = "Departments", description = "Service areas and their Teams"),
        (name = "Roles", description = "RBAC Roles and Capabilities"),
        (name = "Requesters", description = "People who ask IT for help"),
        (name = "Cost Centres", description = "Financial units per Company"),
        (name = "API Keys", description = "Credentials for integrations"),
        (name = "Tickets", description = "Tickets, Notes, Time Entries, views and search"),
        (name = "Notifications", description = "The signed-in Analyst's bell"),
        (name = "Mailboxes", description = "Email intake: Mailboxes, Whitelist and Rejected Emails"),
        (name = "SLA", description = "SLA Calendars and Notification Rules"),
        (name = "Ticket reference data", description = "Statuses, Priorities, Types, Origins, Resolution Codes and Categories"),
        (name = "Administration", description = "Modules, System Settings, Background Jobs and the system log"),
    )
)]
struct ApiDoc;

/// The `/api/v1` routes and their OpenAPI description.
fn api() -> (Router<AppState>, OpenApi) {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(cooper_identity::api::router())
        .merge(cooper_administration::api::router())
        .merge(cooper_service_desk::api::router())
        .merge(cooper_notifications::api::router())
        .split_for_parts()
}

/// The OpenAPI document of `/api/v1` (FR-API-003), with paths prefixed by `/api/v1`.
#[must_use]
pub fn openapi() -> OpenApi {
    let (_, mut doc) = api();
    let paths = std::mem::take(&mut doc.paths.paths);
    doc.paths.paths = paths
        .into_iter()
        .map(|(path, item)| (format!("/api/v1{path}"), item))
        .collect();
    doc
}

/// The complete application router.
pub fn router(state: AppState, http: &HttpConfig) -> Router {
    let (routes, _) = api();
    let doc = openapi();
    let api = routes
        .route("/openapi.json", get(move || async move { Json(doc) }))
        .layer(axum::middleware::from_fn_with_state(
            state.identity.clone(),
            cooper_identity::api::authenticate,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.administration.gate.clone(),
            cooper_administration::api::module_gate,
        ));
    let app = Router::new()
        .merge(health::routes())
        .nest("/api/v1", api)
        .fallback(cooper_http::middleware::not_found)
        .with_state(state);
    cooper_http::middleware::apply(app, http)
}
