//! Scheduled work on tickets (FR-TKT-031) on the Calendar, the rota (FR-SLA-008), and Asset
//! owners reminded of warranty and lease expiry (FR-AST-012; ADR-0042).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use common::{Client, setup, setup_with_clock, test_settings};
use cooper_kernel::{Clock, FixedClock};
use cooper_platform::jobs::{Job, JobContext};
use cooper_platform::outbox::Dispatcher;
use cooper_server::AppState;
use http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn dispatch(pool: &PgPool, at: &str) {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(at.parse().unwrap()));
    let state = AppState::with_settings(pool.clone(), clock.clone(), test_settings()).unwrap();
    let dispatcher = state
        .event_handlers()
        .into_iter()
        .fold(Dispatcher::new(pool.clone(), clock), Dispatcher::with_handler);
    let stats = dispatcher.dispatch_once().await.unwrap();
    let errors: Vec<String> = sqlx::query_scalar("SELECT COALESCE(last_error, '') FROM outbox_events WHERE last_error IS NOT NULL")
        .fetch_all(pool)
        .await
        .unwrap();
    assert_eq!(stats.retried + stats.dead, 0, "{stats:?} {errors:?}");
}

async fn analyst(admin: &Client, anon: &Client, name: &str) -> String {
    let a = admin
        .post("/api/v1/analysts", json!({ "username": name, "email": format!("{name}@default.example"), "full_name": name, "password": "analyst password 1" }))
        .await;
    assert_eq!(a.status, StatusCode::CREATED, "{:?}", a.body);
    let _ = anon;
    a.body["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrations = "../../migrations")]
async fn scheduled_work_is_on_the_owners_schedule_and_the_calendar(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let company = admin.get("/api/v1/companies").await.body["items"][0]["id"].as_str().unwrap().to_owned();
    let ann = analyst(&admin, &anon, "ann").await;
    let ticket = admin.post("/api/v1/tickets", json!({ "company_id": company, "subject": "Replace disk" })).await;
    assert_eq!(ticket.status, StatusCode::CREATED, "{:?}", ticket.body);
    let tid = ticket.body["id"].as_str().unwrap().to_owned();
    let number = ticket.body["number"].as_str().unwrap().to_owned();

    let days = admin
        .post(&format!("/api/v1/tickets/{tid}/scheduled-work"), json!({ "owner_id": ann, "all_day": true, "starts_on": "2026-12-02", "ends_on": "2026-12-04", "notes": "Swap the drive" }))
        .await;
    assert_eq!(days.status, StatusCode::CREATED, "{:?}", days.body);
    let day_id = days.body["id"].as_str().unwrap().to_owned();
    let timed = admin
        .post(&format!("/api/v1/tickets/{tid}/scheduled-work"), json!({ "owner_id": ann, "all_day": false, "starts_at": "2026-12-08T09:00:00Z", "ends_at": "2026-12-08T11:00:00Z" }))
        .await;
    assert_eq!(timed.status, StatusCode::CREATED, "{:?}", timed.body);

    let backwards = admin
        .post(&format!("/api/v1/tickets/{tid}/scheduled-work"), json!({ "owner_id": ann, "all_day": true, "starts_on": "2026-12-05", "ends_on": "2026-12-01" }))
        .await;
    assert_eq!(backwards.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", backwards.body);
    let stranger = admin
        .post(&format!("/api/v1/tickets/{tid}/scheduled-work"), json!({ "owner_id": uuid::Uuid::now_v7(), "all_day": true, "starts_on": "2026-12-05" }))
        .await;
    assert_eq!(stranger.status, StatusCode::UNPROCESSABLE_ENTITY, "an Owner must be an active Analyst");

    assert_eq!(admin.get(&format!("/api/v1/tickets/{tid}/scheduled-work")).await.body.as_array().unwrap().len(), 2);
    let mine = admin.get(&format!("/api/v1/scheduled-work?owner_id={ann}&from=2026-12-01&to=2026-12-31")).await.body;
    assert_eq!(mine.as_array().unwrap().len(), 2);

    let moved = admin
        .put(&format!("/api/v1/scheduled-work/{day_id}"), json!({ "owner_id": ann, "all_day": true, "starts_on": "2026-12-03", "ends_on": "2026-12-03", "notes": "Moved" }))
        .await;
    assert_eq!(moved.status, StatusCode::OK, "{:?}", moved.body);
    assert_eq!(moved.body["starts_on"], "2026-12-03");

    dispatch(&pool, "2026-12-01T10:00:00Z").await;
    let calendar = admin.get("/api/v1/calendar/events?from=2026-12-01&to=2026-12-31").await.body;
    let titles: Vec<String> = calendar["items"].as_array().unwrap().iter().map(|e| e["title"].as_str().unwrap_or_default().to_owned()).collect();
    assert!(titles.iter().any(|t| t.contains("Work starts") && t.contains(&number)), "{titles:?}");
    assert!(titles.iter().any(|t| t.contains("Work ends") && t.contains(&number)), "{titles:?}");

    let cancelled = admin.post(&format!("/api/v1/scheduled-work/{day_id}/cancel"), json!({})).await;
    assert_eq!(cancelled.status, StatusCode::OK, "{:?}", cancelled.body);
    assert_eq!(cancelled.body["active"], false);
    let after = admin.get(&format!("/api/v1/scheduled-work?owner_id={ann}&from=2026-12-01&to=2026-12-31")).await.body;
    assert_eq!(after.as_array().unwrap().len(), 1, "cancelled work leaves the schedule");
}

#[sqlx::test(migrations = "../../migrations")]
async fn the_rota_shows_who_is_working_and_on_call(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let company = admin.get("/api/v1/companies").await.body["items"][0]["id"].as_str().unwrap().to_owned();
    let ann = analyst(&admin, &anon, "ann").await;

    let shift = admin.post("/api/v1/rota/shifts", json!({ "company_id": company, "name": "Early", "starts_at": "07:00:00", "ends_at": "15:00:00" })).await;
    assert_eq!(shift.status, StatusCode::CREATED, "{:?}", shift.body);
    let office = admin.post("/api/v1/rota/locations", json!({ "company_id": company, "name": "Office" })).await;
    assert_eq!(office.status, StatusCode::CREATED, "{:?}", office.body);
    let wfh = admin.post("/api/v1/rota/locations", json!({ "company_id": company, "name": "WFH" })).await;
    assert_eq!(wfh.status, StatusCode::CREATED);
    let (shift_id, office_id) = (shift.body["id"].as_str().unwrap().to_owned(), office.body["id"].as_str().unwrap().to_owned());
    assert_eq!(admin.get(&format!("/api/v1/rota/shifts?company_id={company}")).await.body.as_array().unwrap().len(), 1);

    let entry = json!({ "company_id": company, "analyst_id": ann, "work_date": "2026-12-02", "shift_id": shift_id, "location_id": office_id, "on_call": true });
    let placed = admin.post("/api/v1/rota", entry.clone()).await;
    assert_eq!(placed.status, StatusCode::CREATED, "{:?}", placed.body);
    assert_eq!(admin.post("/api/v1/rota", entry).await.status, StatusCode::CONFLICT, "one shift per Analyst per day");

    let today = admin.get(&format!("/api/v1/rota?company_id={company}&from=2026-12-02")).await.body;
    assert_eq!(today.as_array().unwrap().len(), 1);
    assert_eq!(today[0]["on_call"], true);
    let other_day = admin.get(&format!("/api/v1/rota?company_id={company}&from=2026-12-03")).await.body;
    assert_eq!(other_day, json!([]));

    let no_location = admin
        .post("/api/v1/rota", json!({ "company_id": company, "analyst_id": ann, "work_date": "2026-12-03", "shift_id": shift_id, "location_id": uuid::Uuid::now_v7() }))
        .await;
    assert_eq!(no_location.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", no_location.body);
}

#[sqlx::test(migrations = "../../migrations")]
async fn asset_owners_are_reminded_before_warranty_and_lease_expire(pool: PgPool) {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new("2026-12-01T09:00:00Z".parse().unwrap()));
    let (anon, admin) = setup_with_clock(&pool, clock.clone()).await;
    let company = admin.get("/api/v1/companies").await.body["items"][0]["id"].as_str().unwrap().to_owned();
    let ann = analyst(&admin, &anon, "ann").await;
    let server = admin.post("/api/v1/asset-types", json!({ "name": "Server" })).await.body["id"].as_str().unwrap().to_owned();

    let due = admin.post("/api/v1/assets", json!({ "company_id": company, "asset_tag": "AST-1", "asset_type_id": server, "warranty_expires_on": "2026-12-15" })).await;
    assert_eq!(due.status, StatusCode::CREATED, "{:?}", due.body);
    let due_id = due.body["id"].as_str().unwrap().to_owned();
    let far = admin.post("/api/v1/assets", json!({ "company_id": company, "asset_tag": "AST-2", "asset_type_id": server, "warranty_expires_on": "2027-06-01" })).await;
    let far_id = far.body["id"].as_str().unwrap().to_owned();

    assert_eq!(admin.put(&format!("/api/v1/assets/{due_id}/owner"), json!({ "owner_id": ann })).await.status, StatusCode::OK);
    assert_eq!(admin.put(&format!("/api/v1/assets/{far_id}/owner"), json!({ "owner_id": ann })).await.status, StatusCode::OK);
    assert_eq!(
        admin.put(&format!("/api/v1/assets/{due_id}/owner"), json!({ "owner_id": uuid::Uuid::now_v7() })).await.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an owner must be an active Analyst"
    );

    let state = AppState::with_settings(pool.clone(), clock.clone(), test_settings()).unwrap();
    let job = cooper_assets::extras_store::AssetReminderJob { service: state.asset_extras.service.clone() };
    let ctx = JobContext { pool: pool.clone(), clock: clock.clone() };
    assert_eq!(job.run(&ctx).await.unwrap().unwrap(), "sent 1 Asset reminders", "only the warranty within 30 days is due");
    assert_eq!(job.run(&ctx).await.unwrap().unwrap(), "sent 0 Asset reminders", "each reminder is sent once");
    dispatch(&pool, "2026-12-01T10:00:00Z").await;

    let ann_client = anon.sign_in("ann", "analyst password 1").await;
    let bell: Value = ann_client.get("/api/v1/notifications").await.body;
    assert_eq!(bell["items"][0]["kind"], "asset_warranty_due", "{bell:?}");
    assert!(bell["items"][0]["title"].as_str().unwrap().contains("AST-1"));
}
