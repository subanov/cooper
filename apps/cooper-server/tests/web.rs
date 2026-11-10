//! The web UI contract (ADR-0010): the committed OpenAPI document the TypeScript client
//! is generated from, and how non-API paths are answered.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use axum::body::Body;
use cooper_http::HttpConfig;
use http::{Request, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt as _;

#[test]
fn committed_openapi_document_is_current() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/openapi.json");
    let committed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let current = serde_json::to_value(cooper_server::openapi()).unwrap();
    assert!(
        committed == current,
        "web/openapi.json is out of date: run `just web-api` and commit the result"
    );
}

/// Regression: query structs without `#[into_params(parameter_in = Query)]` were documented
/// as path parameters, so the generated TypeScript client could not send their filters.
#[test]
fn every_path_parameter_appears_in_its_url_template() {
    let doc = serde_json::to_value(cooper_server::openapi()).unwrap();
    let mut wrong = Vec::new();
    for (url, item) in doc["paths"].as_object().unwrap() {
        for (method, operation) in item.as_object().unwrap() {
            let Some(parameters) = operation["parameters"].as_array() else {
                continue;
            };
            for parameter in parameters.iter().filter(|p| p["in"] == "path") {
                let name = parameter["name"].as_str().unwrap();
                if !url.contains(&format!("{{{name}}}")) {
                    wrong.push(format!("{method} {url}: `{name}` is not in the URL"));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "add #[into_params(parameter_in = Query)] to these query structs:\n{}",
        wrong.join("\n")
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn api_paths_never_fall_back_to_the_spa(pool: PgPool) {
    let state = cooper_server::AppState::new(pool).unwrap();
    let app = cooper_server::router(state, &HttpConfig::default());
    for path in ["/api/v1/no-such-thing", "/api/nope", "/health/nope"] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json",
            "{path}"
        );
    }
    // With a built SPA, client routes get the shell with a strict CSP; without one
    // (Rust-only builds), they are a plain 404. Debug builds read web/dist from disk.
    let built = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../web/dist/index.html"
    ))
    .exists();
    let response = app
        .oneshot(
            Request::get("/portal/tickets/42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    if built {
        assert_eq!(response.status(), StatusCode::OK);
        let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(csp.contains("script-src 'self'") && !csp.contains("unsafe"));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
    } else {
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
