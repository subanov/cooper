//! Cooper HTTP building blocks.
//!
//! - [`Problem`]: RFC 9457 `application/problem+json` errors (FR-API-004). Each context
//!   maps its application errors to a `Problem` in one `IntoResponse` implementation.
//! - [`middleware`]: the standard `tower-http` stack (tracing, request ID, timeout,
//!   compression, CORS, body size limit, panic handling) applied to the whole router.
//! - [`metrics`]: per-route request metrics and the `/metrics` listener (ADR-0023).
//! - [`csv`]: CSV exports, escaped and protected against spreadsheet formula injection.
//! - [`Authenticated`]: the signed-in [`cooper_kernel::Principal`] of a request.

pub mod auth;
pub mod client_ip;
pub mod csv;
pub mod metrics;
pub mod middleware;
mod problem;

pub use auth::Authenticated;
pub use middleware::{HttpConfig, REQUEST_ID_HEADER};
pub use problem::{PROBLEM_JSON, Problem};
