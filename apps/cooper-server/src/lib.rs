//! Cooper HTTP server.
//!
//! Wires the bounded contexts' routers under `/api/v1`, adds health endpoints, the
//! OpenAPI document and the web UI (ADR-0010), and wraps everything in the standard
//! middleware stack. The binary
//! (`main.rs`) only reads configuration and starts serving; [`router`] is public so tests
//! can drive the full stack with `tower::ServiceExt::oneshot`.

mod adapters;
pub mod config;
mod health;
pub mod live;
mod state;
mod web;

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
        (name = "Problems", description = "Problems, Known Errors and their incident tickets"),
        (name = "Administration", description = "Modules, System Settings, Background Jobs and the system log"),
        (name = "Live updates", description = "Server-Sent Events telling the web UI which data changed (ADR-0022)"),
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
        .merge(cooper_problem_management::api::router())
        .merge(live::router())
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
        .fallback(web::fallback)
        .with_state(state);
    cooper_http::middleware::apply(app, http)
}

#[cfg(test)]
mod openapi_tests {
    use std::collections::BTreeMap;

    /// Contexts are merged into one document: two handlers with the same function name
    /// in different crates would share an `operationId`, and the generated web client
    /// would type one endpoint with the other's response.
    #[test]
    fn operation_ids_are_unique_across_contexts() {
        let doc = super::openapi();
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for (path, item) in &doc.paths.paths {
            let ops = [
                &item.get,
                &item.put,
                &item.post,
                &item.delete,
                &item.patch,
                &item.head,
                &item.options,
            ];
            for op in ops.into_iter().flatten() {
                if let Some(id) = &op.operation_id
                    && let Some(other) = seen.insert(id.clone(), path.clone())
                {
                    panic!("operationId {id} is used by {other} and {path}");
                }
            }
        }
    }
}
