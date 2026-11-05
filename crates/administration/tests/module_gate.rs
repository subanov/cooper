//! FR-GEN-005: routes of a Module that is turned off answer 404.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::routing::get;
use cooper_administration::api::{ModuleGate, module_gate};
use cooper_administration::application::AdministrationService;
use cooper_administration::infrastructure::PgAdministrationStore;
use cooper_kernel::{Actor, Module, Principal, SystemClock, TenantScope};
use http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt as _;

#[sqlx::test(migrations = "../../migrations")]
async fn disabled_module_routes_answer_404(pool: PgPool) {
    let service = AdministrationService::new(
        Arc::new(PgAdministrationStore::new(pool)),
        Arc::new(SystemClock),
        &[],
    );
    let gate = ModuleGate::new(service.clone(), vec![("/assets", Module::Assets)]);
    let app = Router::new()
        .route("/api/v1/assets", get(|| async { "assets" }))
        .route("/api/v1/analysts", get(|| async { "analysts" }))
        .layer(axum::middleware::from_fn_with_state(
            gate.clone(),
            module_gate,
        ));
    let status = |uri: &'static str| {
        let app = app.clone();
        async move {
            app.oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status()
        }
    };
    assert_eq!(status("/api/v1/assets").await, StatusCode::OK);

    let admin = Principal::new(Actor::Automation, true, BTreeSet::new(), TenantScope::All);
    service
        .set_module_enabled(&admin, Module::Assets, false)
        .await
        .unwrap();
    gate.invalidate().await;
    assert_eq!(status("/api/v1/assets").await, StatusCode::NOT_FOUND);
    assert_eq!(
        status("/api/v1/analysts").await,
        StatusCode::OK,
        "routes without a Module are never gated"
    );

    service
        .set_module_enabled(&admin, Module::Assets, true)
        .await
        .unwrap();
    gate.invalidate().await;
    assert_eq!(status("/api/v1/assets").await, StatusCode::OK);
}
