//! Notifications end to end (ADR-0016): domain events → outbox → handlers → the bell.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use common::{Client, setup};
use cooper_kernel::{FixedClock, SystemClock};
use cooper_platform::jobs::{Job, JobContext};
use cooper_platform::outbox::Dispatcher;
use cooper_server::{AppState, SecuritySettings};
use cooper_service_desk::infrastructure::SlaMonitor;
use http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

const CAPS: &[&str] = &[
    "tickets.read",
    "tickets.create",
    "tickets.update",
    "tickets.assign",
];

async fn dispatch(pool: &PgPool) {
    let state = AppState::with_settings(
        pool.clone(),
        Arc::new(SystemClock),
        SecuritySettings::ephemeral().unwrap(),
    )
    .unwrap();
    let dispatcher = state.event_handlers().into_iter().fold(
        Dispatcher::new(pool.clone(), Arc::new(SystemClock)),
        Dispatcher::with_handler,
    );
    let stats = dispatcher.dispatch_once().await.unwrap();
    assert_eq!(stats.retried + stats.dead, 0, "{stats:?}");
}

async fn analyst(admin: &Client, anon: &Client, name: &str) -> (String, Client) {
    let a = admin.post("/api/v1/analysts", json!({ "username": name, "email": format!("{name}@default.example"), "full_name": name, "password": "analyst password 1" })).await;
    let id = a.body["id"].as_str().unwrap().to_owned();
    let role = admin
        .post(
            "/api/v1/roles",
            json!({ "name": format!("{name} role"), "capabilities": CAPS }),
        )
        .await;
    admin
        .put(
            &format!("/api/v1/analysts/{id}/roles"),
            json!({ "role_ids": [role.body["id"]] }),
        )
        .await;
    admin
        .put(
            &format!("/api/v1/analysts/{id}/company-access"),
            json!({ "all": true }),
        )
        .await;
    (id, anon.sign_in(name, "analyst password 1").await)
}

async fn ticket(client: &Client, pool: &PgPool, extra: Value) -> Value {
    let company: String = sqlx::query_scalar("SELECT id::text FROM tenants LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    let mut body = json!({ "company_id": company, "subject": "Printer on fire" });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let r = client.post("/api/v1/tickets", body).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.body
}

fn kinds(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["kind"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test(migrations = "../../migrations")]
async fn assignments_and_mentions_reach_the_bell_once(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let (dee_id, dee) = analyst(&admin, &anon, "dee").await;
    let (_, sam) = analyst(&admin, &anon, "sam").await;

    let t = ticket(&admin, &pool, json!({ "analyst_id": dee_id })).await;
    let id = t["id"].as_str().unwrap();
    dee.post(
        &format!("/api/v1/tickets/{id}/notes"),
        json!({ "body": "@sam can you have a look? @nobody" }),
    )
    .await;
    dispatch(&pool).await;
    dispatch(&pool).await;

    let bell = dee.get("/api/v1/notifications").await.body;
    assert_eq!(kinds(&bell), ["ticket_assigned"]);
    assert_eq!(
        bell["items"][0]["title"],
        format!(
            "Ticket {} was assigned to you",
            t["number"].as_str().unwrap()
        )
    );
    assert_eq!(bell["items"][0]["ticket_id"], t["id"]);
    assert_eq!(
        kinds(&sam.get("/api/v1/notifications").await.body),
        ["mentioned"]
    );
    assert!(
        kinds(&admin.get("/api/v1/notifications").await.body).is_empty(),
        "nobody is told about their own action"
    );

    // Re-delivering the same events (at-least-once) creates no duplicates.
    sqlx::query("UPDATE outbox_events SET published_at = NULL, attempts = 0, next_attempt_at = now() - interval '1 second'").execute(&pool).await.unwrap();
    dispatch(&pool).await;
    assert_eq!(kinds(&dee.get("/api/v1/notifications").await.body).len(), 1);

    assert_eq!(
        dee.get("/api/v1/notifications/unread-count").await.body["unread"],
        1
    );
    let nid = bell["items"][0]["id"].as_str().unwrap();
    assert_eq!(
        sam.post(&format!("/api/v1/notifications/{nid}/read"), json!({}))
            .await
            .status,
        StatusCode::NOT_FOUND,
        "not your Notification"
    );
    assert_eq!(
        dee.post(&format!("/api/v1/notifications/{nid}/read"), json!({}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        dee.get("/api/v1/notifications/unread-count").await.body["unread"],
        0
    );
    assert!(kinds(&dee.get("/api/v1/notifications?unread=true").await.body).is_empty());
    assert_eq!(
        sam.post("/api/v1/notifications/read-all", json!({}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn sla_breaches_follow_the_notification_rules(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let (dee_id, dee) = analyst(&admin, &anon, "dee").await;
    let (boss_id, boss) = analyst(&admin, &anon, "boss").await;
    admin.post("/api/v1/sla-notification-rules", json!({ "name": "Breaches", "trigger": "breach", "target": "both", "notify_assignee": true, "analyst_ids": [boss_id] })).await;
    let p = admin
        .get("/api/v1/ticket-priorities")
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "high")
        .unwrap()
        .clone();
    admin
        .put(
            &format!("/api/v1/ticket-priorities/{}", p["id"].as_str().unwrap()),
            json!({ "name": "High", "sort_order": 30, "response_minutes": 5 }),
        )
        .await;

    let t = ticket(
        &admin,
        &pool,
        json!({ "analyst_id": dee_id, "priority_id": p["id"] }),
    )
    .await;
    let created: DateTime<Utc> = t["created_at"].as_str().unwrap().parse().unwrap();
    let ctx = JobContext {
        pool: pool.clone(),
        clock: Arc::new(FixedClock::new(created + Duration::minutes(10))),
    };
    SlaMonitor.run(&ctx).await.unwrap();
    // The monitor ran on a clock ten minutes ahead; make its events due on the real clock.
    sqlx::query("UPDATE outbox_events SET next_attempt_at = now() WHERE published_at IS NULL")
        .execute(&pool)
        .await
        .unwrap();
    dispatch(&pool).await;

    let dee_kinds = kinds(&dee.get("/api/v1/notifications").await.body);
    assert!(
        dee_kinds.contains(&"sla_breached".to_owned()),
        "{dee_kinds:?}"
    );
    assert!(
        !dee_kinds.contains(&"sla_warning".to_owned()),
        "no warning rule"
    );
    assert_eq!(
        kinds(&boss.get("/api/v1/notifications").await.body),
        ["sla_breached"]
    );
}
