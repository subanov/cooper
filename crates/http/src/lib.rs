//! Cooper HTTP building blocks.
//!
//! - [`Problem`]: RFC 9457 `application/problem+json` errors (FR-API-004). Each context
//!   maps its application errors to a `Problem` in one `IntoResponse` implementation.
//! - [`middleware`]: the standard `tower-http` stack (tracing, request ID, timeout,
//!   compression, CORS, body size limit, panic handling) applied to the whole router.

pub mod middleware;
mod problem;

pub use middleware::{HttpConfig, REQUEST_ID_HEADER};
pub use problem::{PROBLEM_JSON, Problem};
