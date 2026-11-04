//! Cooper HTTP server.
//!
//! Wires the bounded contexts' routers under `/api/v1`, adds health endpoints and wraps
//! everything in the standard middleware stack. The binary (`main.rs`) only reads
//! configuration and starts serving; [`router`] is public so tests can drive the full
//! stack with `tower::ServiceExt::oneshot`.

pub mod config;
mod health;
mod state;

use axum::Router;
use cooper_http::HttpConfig;

pub use state::AppState;

/// The complete application router.
pub fn router(state: AppState, http: &HttpConfig) -> Router {
    let api = Router::new();
    let app = Router::new()
        .merge(health::routes())
        .nest("/api/v1", api)
        .fallback(cooper_http::middleware::not_found)
        .with_state(state);
    cooper_http::middleware::apply(app, http)
}
