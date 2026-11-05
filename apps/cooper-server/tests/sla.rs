//! SLA end to end (ADR-0015): due times from Priorities and Calendars, pause and
//! resume, response, SLA state in responses, and the once-only monitor job.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use common::{Client, setup};
use cooper_kernel::FixedClock;
use cooper_platform::jobs::{Job, JobContext};
use cooper_service_desk::infrastructure::SlaMonitor;
use http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn lookup(admin: &Client, path: &str, key: &str) -> Value {
    admin
        .get(&format!("/api/v1/{path}"))
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == key)
        .unwrap()
        .clone()
}

async fn set_targets(
    admin: &Client,
    key: &str,
    response: u32,
    resolution: u32,
    calendar: Option<&str>,
) -> String {
    let p = lookup(admin, "ticket-priorities", key).await;
    let id = p["id"].as_str().unwrap().to_owned();
    let r = admin.put(&format!("/api/v1/ticket-priorities/{id}"), json!({
        "name": p["name"], "is_default": p["is_default"], "sort_order": p["sort_order"],
        "response_minutes": response, "resolution_minutes": resolution, "sla_calendar_id": calendar
    })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    id
}

async fn ticket(admin: &Client, pool: &PgPool, priority: &str) -> Value {
    let company: String = sqlx::query_scalar("SELECT id::text FROM tenants LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    let r = admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": company, "subject": "SLA test", "priority_id": priority }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.body
}

fn at(v: &Value) -> DateTime<Utc> {
    v.as_str().unwrap().parse().unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn due_times_follow_the_priority_without_a_calendar(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let high = set_targets(&admin, "high", 60, 240, None).await;
    let t = ticket(&admin, &pool, &high).await;
    let created = at(&t["created_at"]);
    assert_eq!(
        at(&t["sla"]["response"]["due_at"]),
        created + Duration::minutes(60)
    );
    assert_eq!(
        at(&t["sla"]["resolution"]["due_at"]),
        created + Duration::minutes(240)
    );
    assert_eq!(t["sla"]["response"]["state"], "on_track");
    let remaining = t["sla"]["response"]["remaining_seconds"].as_i64().unwrap();
    assert!((3500..=3600).contains(&remaining), "{remaining}");

    // Priority change recomputes from creation.
    let urgent = set_targets(&admin, "urgent", 15, 60, None).await;
    let id = t["id"].as_str().unwrap();
    let edited = admin
        .put(
            &format!("/api/v1/tickets/{id}"),
            json!({ "subject": "SLA test", "priority_id": urgent }),
        )
        .await
        .body;
    assert_eq!(
        at(&edited["sla"]["resolution"]["due_at"]),
        created + Duration::minutes(60)
    );

    // A Note visible to the Requester is the response.
    admin
        .post(
            &format!("/api/v1/tickets/{id}/notes"),
            json!({ "body": "On it", "is_public": true }),
        )
        .await;
    let after = admin.get(&format!("/api/v1/tickets/{id}")).await.body;
    assert_eq!(after["sla"]["response"]["state"], "met");
    assert!(after["sla"]["responded_at"].is_string());
}

#[sqlx::test(migrations = "../../migrations")]
async fn pausing_statuses_stop_the_clock(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let high = set_targets(&admin, "high", 60, 240, None).await;
    let t = ticket(&admin, &pool, &high).await;
    let id = t["id"].as_str().unwrap();
    let original = at(&t["sla"]["resolution"]["due_at"]);
    let on_hold = lookup(&admin, "ticket-statuses", "on_hold").await;
    let paused = admin
        .post(
            &format!("/api/v1/tickets/{id}/status"),
            json!({ "status_id": on_hold["id"] }),
        )
        .await
        .body;
    assert_eq!(paused["sla"]["resolution"]["state"], "paused");
    // Pretend the pause started two hours ago.
    sqlx::query(
        "UPDATE tickets SET sla_paused_at = sla_paused_at - interval '2 hours' WHERE id = $1::uuid",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let open = lookup(&admin, "ticket-statuses", "open").await;
    let resumed = admin
        .post(
            &format!("/api/v1/tickets/{id}/status"),
            json!({ "status_id": open["id"] }),
        )
        .await
        .body;
    let shifted = at(&resumed["sla"]["resolution"]["due_at"]);
    assert!(
        shifted >= original + Duration::minutes(119)
            && shifted <= original + Duration::minutes(121),
        "{shifted} vs {original}"
    );
    assert!(resumed["sla"]["paused_at"].is_null());
}

#[sqlx::test(migrations = "../../migrations")]
async fn calendars_count_only_working_time(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let nine_five = json!({ "start": "09:00:00", "end": "17:00:00" });
    let cal = admin
        .post(
            "/api/v1/sla-calendars",
            json!({
                "name": "London office", "time_zone": "Europe/London", "is_default": true,
                "hours": [nine_five, nine_five, nine_five, nine_five, nine_five, null, null],
                "holidays": ["2026-12-25"]
            }),
        )
        .await;
    assert_eq!(cal.status, StatusCode::CREATED, "{:?}", cal.body);
    let bad = admin.post("/api/v1/sla-calendars", json!({ "name": "Bad", "time_zone": "Mars/Base", "hours": [null, null, null, null, null, null, null] })).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Priorities without their own calendar use the default calendar.
    let high = set_targets(&admin, "high", 120, 960, None).await;
    let t = ticket(&admin, &pool, &high).await;
    let created = at(&t["created_at"]);
    let calendar = cooper_service_desk::domain::SlaCalendar::restore(
        uuid::Uuid::nil(),
        "x".into(),
        chrono_tz::Europe::London,
        {
            let h = Some(cooper_service_desk::domain::DayHours {
                start: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                end: chrono::NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            });
            [h, h, h, h, h, None, None]
        },
        [chrono::NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()].into(),
        true,
        Utc::now(),
        Utc::now(),
    );
    let expected = calendar.add_working_minutes(created, 120).unwrap();
    let actual = at(&t["sla"]["response"]["due_at"]);
    assert!(
        (actual - expected).num_seconds().abs() <= 1,
        "{actual} vs {expected}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn monitor_raises_each_warning_and_breach_once(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let high = set_targets(&admin, "high", 10, 30, None).await;
    let t = ticket(&admin, &pool, &high).await;
    let clock = Arc::new(FixedClock::new(
        at(&t["created_at"]) + Duration::minutes(20),
    ));
    let ctx = JobContext {
        pool: pool.clone(),
        clock: clock.clone(),
    };

    SlaMonitor.run(&ctx).await.unwrap();
    let events: Vec<String> = sqlx::query_scalar("SELECT event_type || ':' || (payload->>'target') FROM outbox_events WHERE event_type LIKE 'ticket.sla%' ORDER BY 1")
        .fetch_all(&pool).await.unwrap();
    // At 20 minutes: the response target (10) is breached and was in warning (8); the
    // resolution warning (80% of 30 = 24) is not due yet.
    assert_eq!(
        events,
        [
            "ticket.sla_breached:response",
            "ticket.sla_warning:response"
        ]
    );

    SlaMonitor.run(&ctx).await.unwrap();
    let again: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type LIKE 'ticket.sla%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(again, 2, "nothing is raised twice");

    clock.set(at(&t["created_at"]) + Duration::minutes(40));
    SlaMonitor.run(&ctx).await.unwrap();
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_events WHERE event_type = 'ticket.sla_breached'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(total, 2, "the resolution breach follows later");
    let raised: i64 = sqlx::query_scalar("SELECT count(*) FROM sla_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(raised, 4, "two targets, two triggers each");
}

#[sqlx::test(migrations = "../../migrations")]
async fn notification_rules_are_validated(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let ok = admin.post("/api/v1/sla-notification-rules", json!({ "name": "Breaches to the Queue", "trigger": "breach", "target": "both", "notify_team": true })).await;
    assert_eq!(ok.status, StatusCode::CREATED, "{:?}", ok.body);
    let nobody = admin
        .post(
            "/api/v1/sla-notification-rules",
            json!({ "name": "Nobody", "trigger": "warning", "target": "response" }),
        )
        .await;
    assert_eq!(nobody.status, StatusCode::UNPROCESSABLE_ENTITY);
    let bad = admin.post("/api/v1/sla-notification-rules", json!({ "name": "Bad", "trigger": "soon", "target": "response", "notify_assignee": true })).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        admin
            .get("/api/v1/sla-notification-rules")
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
