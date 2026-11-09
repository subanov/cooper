//! HTTP metrics and the `/metrics` endpoint (ADR-0023).
//!
//! [`record`] runs inside the standard middleware stack, after routing, so requests are
//! labelled by their route template (`/api/v1/tickets/{id}`), never by the concrete path.
//! [`serve`] exposes the Prometheus text on its own address (`COOPER_METRICS_ADDR`), so
//! metrics are never reachable through the public listener.

use std::future::Future;
use std::net::SocketAddr;
use std::time::Instant;

use axum::Router;
use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use http::{Method, header};

/// Counter of HTTP requests; labels `method`, `route` (template), `status`.
pub const HTTP_REQUESTS: &str = "cooper_http_requests_total";
/// Histogram of time to the response head, in seconds; labels `method`, `route`.
/// Streams (Live Updates) count until their first byte.
pub const HTTP_DURATION: &str = "cooper_http_request_duration_seconds";

/// Middleware recording [`HTTP_REQUESTS`] and [`HTTP_DURATION`].
pub async fn record(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = method_label(request.method());
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |p| p.as_str().to_owned());
    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();
    metrics::counter!(HTTP_REQUESTS, "method" => method, "route" => route.clone(), "status" => status)
        .increment(1);
    metrics::histogram!(HTTP_DURATION, "method" => method, "route" => route)
        .record(started.elapsed().as_secs_f64());
    response
}

/// Standard methods by name; anything else is one `other` label (bounded cardinality).
fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::HEAD => "HEAD",
        Method::OPTIONS => "OPTIONS",
        _ => "other",
    }
}

/// The router answering `GET /metrics` with `render()` (Prometheus text format).
pub fn router<F>(render: F) -> Router
where
    F: Fn() -> String + Clone + Send + Sync + 'static,
{
    Router::new().route(
        "/metrics",
        get(move || {
            let render = render.clone();
            async move {
                (
                    [(
                        header::CONTENT_TYPE,
                        "text/plain; version=0.0.4; charset=utf-8",
                    )],
                    render(),
                )
            }
        }),
    )
}

/// Serves [`router`] on `addr` until `shutdown` resolves.
///
/// # Errors
///
/// Fails if the address cannot be bound.
pub async fn serve<F>(
    addr: SocketAddr,
    render: F,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()>
where
    F: Fn() -> String + Clone + Send + Sync + 'static,
{
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "metrics listening");
    axum::serve(listener, router(render))
        .with_graceful_shutdown(shutdown)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::StatusCode;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    #[tokio::test]
    async fn metrics_endpoint_serves_prometheus_text() {
        let app = router(|| "cooper_up 1\n".to_owned());
        let response = app
            .oneshot(http::Request::get("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/plain; version=0.0.4")
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"cooper_up 1\n");
    }

    #[test]
    fn unusual_methods_share_one_label() {
        assert_eq!(method_label(&Method::GET), "GET");
        assert_eq!(
            method_label(&Method::from_bytes(b"PROPFIND").unwrap()),
            "other"
        );
    }
}
