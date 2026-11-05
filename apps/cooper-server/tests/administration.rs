//! Administration API end to end: Modules, System Settings, Background Jobs and the
//! system log (ADR-0012).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use common::{Client, setup};
use http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

async fn desk_analyst(anon: &Client, admin: &Client, caps: &[&str]) -> Client {
    let analyst = admin.post("/api/v1/analysts", json!({
        "username": "desk", "email": "desk@default.example", "full_name": "Dee Desk", "password": "desk password 123"
    })).await;
    if !caps.is_empty() {
        let role = admin
            .post(
                "/api/v1/roles",
                json!({ "name": "Ops", "capabilities": caps }),
            )
            .await;
        assert_eq!(role.status, StatusCode::CREATED, "{:?}", role.body);
        let id = analyst.body["id"].as_str().unwrap();
        admin
            .put(
                &format!("/api/v1/analysts/{id}/roles"),
                json!({ "role_ids": [role.body["id"]] }),
            )
            .await;
    }
    anon.sign_in("desk", "desk password 123").await
}

#[sqlx::test(migrations = "../../migrations")]
async fn modules_are_listed_and_switched_by_administrators(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let desk = desk_analyst(&anon, &admin, &[]).await;

    let modules = desk.get("/api/v1/modules").await;
    assert_eq!(modules.status, StatusCode::OK);
    assert_eq!(modules.body.as_array().unwrap().len(), 15);
    assert!(
        modules
            .body
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["is_enabled"] == true)
    );

    assert_eq!(
        desk.put("/api/v1/modules/cmdb", json!({ "is_enabled": false }))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let off = admin
        .put("/api/v1/modules/cmdb", json!({ "is_enabled": false }))
        .await;
    assert_eq!(off.status, StatusCode::OK);
    assert_eq!(off.body["is_enabled"], false);
    let tickets = admin
        .put("/api/v1/modules/tickets", json!({ "is_enabled": false }))
        .await;
    assert_eq!(tickets.status, StatusCode::CONFLICT);
    assert_eq!(
        admin
            .put("/api/v1/modules/plugins", json!({ "is_enabled": false }))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_entries WHERE aggregate_type = 'module'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audits, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn system_settings_are_typed(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let settings = admin.get("/api/v1/settings").await;
    let name = settings
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "general.instance_name")
        .unwrap()
        .clone();
    assert_eq!(name["value"], "Cooper");
    assert_eq!(name["kind"], "text");

    let bad = admin
        .put(
            "/api/v1/settings/general.instance_name",
            json!({ "value": 42 }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let ok = admin
        .put(
            "/api/v1/settings/general.instance_name",
            json!({ "value": "Acme IT" }),
        )
        .await;
    assert_eq!(ok.body["value"], "Acme IT");
    assert_eq!(
        admin
            .put("/api/v1/settings/nope.nope", json!({ "value": 1 }))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let desk = desk_analyst(&anon, &admin, &[]).await;
    assert_eq!(
        desk.get("/api/v1/settings").await.status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn background_jobs_can_be_rescheduled_and_run(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    sqlx::query("INSERT INTO background_jobs (name, schedule_kind, interval_seconds, next_run_at, created_at, updated_at)
                 VALUES ('mailbox-poll', 'interval', 60, now() + interval '1 minute', now(), now())")
        .execute(&pool).await.unwrap();

    let jobs = admin.get("/api/v1/background-jobs").await;
    assert_eq!(
        jobs.body[0]["schedule"],
        json!({ "kind": "interval", "interval_seconds": 60 })
    );

    let daily = admin
        .put(
            "/api/v1/background-jobs/mailbox-poll/schedule",
            json!({ "kind": "daily", "daily_at": "02:30" }),
        )
        .await;
    assert_eq!(daily.status, StatusCode::OK, "{:?}", daily.body);
    assert_eq!(daily.body["schedule"]["daily_at"], "02:30");
    let invalid = admin
        .put(
            "/api/v1/background-jobs/mailbox-poll/schedule",
            json!({ "kind": "interval", "interval_seconds": 1 }),
        )
        .await;
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        admin
            .put(
                "/api/v1/background-jobs/ghost/schedule",
                json!({ "kind": "off" })
            )
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let run = admin
        .post("/api/v1/background-jobs/mailbox-poll/run", json!({}))
        .await;
    assert_eq!(run.status, StatusCode::ACCEPTED);
    let runs = admin.get("/api/v1/background-jobs/mailbox-poll/runs").await;
    assert_eq!(runs.body["items"], json!([]));

    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_entries WHERE aggregate_type = 'background_job'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1);

    let desk = desk_analyst(&anon, &admin, &[]).await;
    assert_eq!(
        desk.get("/api/v1/background-jobs").await.status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn system_log_is_filtered_and_needs_its_capability(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    sqlx::query("INSERT INTO system_log_entries (id, occurred_at, level, source, target, message)
                 VALUES (gen_random_uuid(), now(), 'error', 'worker', 'cooper_service_desk::mail', 'IMAP login failed'),
                        (gen_random_uuid(), now(), 'warn', 'server', 'cooper_http', 'slow request')")
        .execute(&pool).await.unwrap();

    let all = admin.get("/api/v1/system-log").await;
    assert_eq!(all.body["items"].as_array().unwrap().len(), 2);
    let errors = admin.get("/api/v1/system-log?level=error").await;
    assert_eq!(errors.body["items"][0]["message"], "IMAP login failed");
    let search = admin
        .get("/api/v1/system-log?q=slow&target=cooper_http")
        .await;
    assert_eq!(search.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        admin.get("/api/v1/system-log?level=debug").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let viewer = desk_analyst(&anon, &admin, &["system.view_log"]).await;
    assert_eq!(
        viewer.get("/api/v1/system-log").await.status,
        StatusCode::OK
    );
    assert_eq!(
        viewer.get("/api/v1/settings").await.status,
        StatusCode::FORBIDDEN
    );
}
