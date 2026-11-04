//! Liveness and readiness endpoints (FR-SYS-006).
//!
//! - `GET /health/live`: the process is running. No dependencies are checked, so a
//!   database outage does not make the orchestrator restart healthy instances.
//! - `GET /health/ready`: the instance can serve traffic: the database answers and every
//!   migration this binary expects has been applied.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use cooper_http::Problem;
use cooper_platform::db::{self, MigrationStatus};
use http::StatusCode;
use serde_json::json;

use crate::state::AppState;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
}

async fn live() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn ready(State(state): State<AppState>) -> Response {
    match db::migration_status(&state.pool).await {
        Ok(MigrationStatus::UpToDate) => {
            Json(json!({ "status": "ready", "checks": { "database": "ok", "migrations": "ok" } }))
                .into_response()
        }
        Ok(MigrationStatus::Pending { versions }) => {
            tracing::warn!(?versions, "not ready: migrations pending");
            Problem::new(StatusCode::SERVICE_UNAVAILABLE)
                .with_detail("Database migrations are pending.")
                .with_extension(
                    "checks",
                    json!({ "database": "ok", "migrations": "pending" }),
                )
                .into_response()
        }
        Err(error) => {
            tracing::warn!(%error, "not ready: database unavailable");
            Problem::new(StatusCode::SERVICE_UNAVAILABLE)
                .with_detail("The database is unavailable.")
                .with_extension("checks", json!({ "database": "unavailable" }))
                .into_response()
        }
    }
}
