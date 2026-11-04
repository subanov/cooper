//! The full router with its middleware stack, driven with `oneshot`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: failures should abort the test

use std::time::Duration;

use axum::body::Body;
use axum::routing::{get, post};
use cooper_http::{HttpConfig, PROBLEM_JSON, REQUEST_ID_HEADER};
use cooper_server::{AppState, router};
use http::{Request, Response, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt as _;

async fn json(response: Response<Body>) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn get_req(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn live_is_ok(pool: PgPool) {
    let response = router(AppState::new(pool), &HttpConfig::default())
        .oneshot(get_req("/health/live"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await, serde_json::json!({ "status": "ok" }));
}

#[sqlx::test(migrations = "../../migrations")]
async fn ready_when_database_is_migrated(pool: PgPool) {
    let response = router(AppState::new(pool), &HttpConfig::default())
        .oneshot(get_req("/health/ready"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["checks"]["migrations"], "ok");
}

#[sqlx::test(migrations = false)]
async fn not_ready_while_migrations_are_pending(pool: PgPool) {
    let response = router(AppState::new(pool), &HttpConfig::default())
        .oneshot(get_req("/health/ready"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::CONTENT_TYPE], PROBLEM_JSON);
    assert_eq!(json(response).await["checks"]["migrations"], "pending");
}

#[sqlx::test(migrations = false)]
async fn unknown_route_is_a_404_problem(pool: PgPool) {
    let response = router(AppState::new(pool), &HttpConfig::default())
        .oneshot(get_req("/api/v1/nope"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CONTENT_TYPE], PROBLEM_JSON);
    assert_eq!(json(response).await["title"], "Not Found");
}

#[sqlx::test(migrations = false)]
async fn request_id_is_generated_or_echoed(pool: PgPool) {
    let app = router(AppState::new(pool), &HttpConfig::default());

    let generated = app.clone().oneshot(get_req("/health/live")).await.unwrap();
    assert!(!generated.headers()[REQUEST_ID_HEADER].is_empty());

    let request = Request::get("/health/live")
        .header(REQUEST_ID_HEADER, "abc-123")
        .body(Body::empty())
        .unwrap();
    let echoed = app.oneshot(request).await.unwrap();
    assert_eq!(echoed.headers()[REQUEST_ID_HEADER], "abc-123");
}

async fn panicking_handler() -> &'static str {
    panic!("boom")
}

#[sqlx::test(migrations = false)]
async fn middleware_errors_use_problem_json(pool: PgPool) {
    let http = HttpConfig {
        body_limit_bytes: 8,
        request_timeout: Duration::from_millis(50),
        ..HttpConfig::default()
    };
    let routes = axum::Router::new()
        .route("/echo", post(|body: String| async move { body }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "late"
            }),
        )
        .route("/panic", get(panicking_handler));
    let app = cooper_http::middleware::apply(routes, &http);
    let _ = pool;

    let too_big = Request::post("/echo")
        .body(Body::from("this body is too large"))
        .unwrap();
    let response = app.clone().oneshot(too_big).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers()[header::CONTENT_TYPE], PROBLEM_JSON);

    let response = app.clone().oneshot(get_req("/slow")).await.unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(json(response).await["status"], 408);

    let response = app.oneshot(get_req("/panic")).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()[header::CONTENT_TYPE], PROBLEM_JSON);
}

#[sqlx::test(migrations = false)]
async fn cors_allows_configured_origins_only(pool: PgPool) {
    let http = HttpConfig {
        cors_allowed_origins: vec![http::HeaderValue::from_static("https://desk.example")],
        ..HttpConfig::default()
    };
    let app = router(AppState::new(pool), &http);
    let request = |origin: &'static str| {
        Request::get("/health/live")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap()
    };

    let allowed = app
        .clone()
        .oneshot(request("https://desk.example"))
        .await
        .unwrap();
    assert_eq!(
        allowed.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://desk.example"
    );
    let denied = app.oneshot(request("https://evil.example")).await.unwrap();
    assert!(
        denied
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}
