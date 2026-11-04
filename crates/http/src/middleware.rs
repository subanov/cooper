//! The standard middleware stack (AGENTS.md §6).

use std::any::Any;
use std::time::Duration;

use axum::BoxError;
use axum::Router;
use axum::body::{Body, Bytes, HttpBody};
use axum::response::{IntoResponse, Response};
use http::{HeaderName, HeaderValue, Request, StatusCode, header};
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::problem::Problem;

/// Header carrying the request ID. Set by the client or generated (UUID), echoed in the
/// response and recorded on the request's tracing span.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Settings for the middleware stack.
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Origins allowed by CORS. Empty disables CORS headers.
    pub cors_allowed_origins: Vec<HeaderValue>,
    /// Requests running longer are answered with 408.
    pub request_timeout: Duration,
    /// Larger request bodies are rejected with 413.
    pub body_limit_bytes: usize,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            cors_allowed_origins: Vec::new(),
            request_timeout: Duration::from_secs(30),
            body_limit_bytes: 25 * 1024 * 1024,
        }
    }
}

/// Wraps `router` in the standard stack, outermost first: request ID → tracing →
/// request ID propagation → panic → problem+json for bare errors → CORS → compression →
/// timeout → body limit.
pub fn apply<S>(router: Router<S>, config: &HttpConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let request_id = HeaderName::from_static(REQUEST_ID_HEADER);
    let cors = if config.cors_allowed_origins.is_empty() {
        CorsLayer::new()
    } else {
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(config.cors_allowed_origins.clone()))
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
            .expose_headers([request_id.clone()])
    };

    router.layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::new(request_id.clone(), MakeRequestUuid))
            .layer(
                TraceLayer::new_for_http().make_span_with(|req: &Request<Body>| {
                    let request_id = req
                        .headers()
                        .get(REQUEST_ID_HEADER)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    tracing::info_span!(
                        "http_request",
                        method = %req.method(),
                        path = %req.uri().path(),
                        request_id,
                    )
                }),
            )
            .layer(PropagateRequestIdLayer::new(request_id))
            .layer(CatchPanicLayer::custom(panic_response))
            .layer(axum::middleware::map_response(problem_for_bare_errors))
            .layer(cors)
            .layer(CompressionLayer::new())
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                config.request_timeout,
            ))
            .layer(RequestBodyLimitLayer::new(config.body_limit_bytes)),
    )
}

/// Router fallback: unknown routes answer with a 404 problem.
pub async fn not_found() -> Problem {
    Problem::not_found()
}

/// Gives error responses produced by middleware or axum extractors (timeout, body limit,
/// method not allowed, …) a problem+json body, so every error has the same shape.
async fn problem_for_bare_errors<B>(response: Response<B>) -> Response
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    let status = response.status();
    let is_error = status.is_client_error() || status.is_server_error();
    let is_problem = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| {
            v.as_bytes()
                .starts_with(crate::problem::PROBLEM_JSON.as_bytes())
        });
    if !is_error || is_problem {
        return response.map(Body::new);
    }
    let (mut parts, _body) = response.into_parts();
    let problem = Problem::new(status).into_response();
    parts.headers.remove(header::CONTENT_LENGTH);
    let (problem_parts, body) = problem.into_parts();
    parts.headers.extend(problem_parts.headers);
    Response::from_parts(parts, body)
}

fn panic_response(panic: Box<dyn Any + Send + 'static>) -> Response {
    let message = panic
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    tracing::error!(panic = %message, "request handler panicked");
    Problem::internal().into_response()
}
