//! Morning Checks end to end (ADR-0031): Check Statuses, Check Groups and Checks, the
//! day's round and recording results, Result Links to tickets and Tasks (with the
//! ticket's related records), the history grid and its CSV export, reminders, the time
//! zone of Check Dates, Company isolation and the Morning Checks Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use common::{Client, setup, setup_with_clock, test_settings};
use cooper_kernel::{Clock, FixedClock};
use cooper_platform::jobs::{Job, JobContext};
use cooper_platform::outbox::Dispatcher;
use cooper_server::AppState;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

const GREEN: &str = "019a0000-0000-7000-8000-000000000c01";
const AMBER: &str = "019a0000-0000-7000-8000-000000000c02";
const RED: &str = "019a0000-0000-7000-8000-000000000c03";
const RECORDER: &[&str] = &["morning-checks.read", "morning-checks.record"];

struct World {
    pool: PgPool,
    anon: Client,
    admin: Client,
    company: String,
}

struct Person {
    id: String,
    client: Client,
}

impl World {
    async fn new(pool: &PgPool) -> Self {
        let (anon, admin) = setup(pool).await;
        Self::with(pool, anon, admin).await
    }

    async fn with(pool: &PgPool, anon: Client, admin: Client) -> Self {
        let company = admin.get("/api/v1/companies").await.body["items"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            pool: pool.clone(),
            anon,
            admin,
            company,
        }
    }

    async fn analyst(&self, name: &str, caps: &[&str], companies: &[&str]) -> Person {
        let a = self
            .admin
            .post("/api/v1/analysts", json!({ "username": name, "email": format!("{name}@default.example"), "full_name": name, "password": "analyst password 1" }))
            .await
            .body;
        let id = a["id"].as_str().unwrap().to_owned();
        let role = self
            .admin
            .post(
                "/api/v1/roles",
                json!({ "name": format!("{name} role"), "capabilities": caps }),
            )
            .await
            .body;
        self.admin
            .put(
                &format!("/api/v1/analysts/{id}/roles"),
                json!({ "role_ids": [role["id"]] }),
            )
            .await;
        self.admin
            .put(
                &format!("/api/v1/analysts/{id}/company-access"),
                json!({ "all": false, "company_ids": companies }),
            )
            .await;
        Person {
            id,
            client: self.anon.sign_in(name, "analyst password 1").await,
        }
    }

    async fn group(&self, body: Value) -> Value {
        let mut body = body;
        body["company_id"] = json!(self.company);
        let r = self.admin.post("/api/v1/morning-checks/groups", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    async fn check(&self, body: Value) -> Value {
        let mut body = body;
        if body.get("company_id").is_none() {
            body["company_id"] = json!(self.company);
        }
        let r = self.admin.post("/api/v1/morning-checks/checks", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    async fn setting(&self, key: &str, value: Value) {
        let r = self
            .admin
            .put(
                &format!("/api/v1/settings/{key}"),
                json!({ "value": value }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }

    async fn state(&self, clock: Arc<dyn Clock>) -> AppState {
        AppState::with_settings(self.pool.clone(), clock, test_settings()).unwrap()
    }

    async fn run_reminders(&self, at: DateTime<Utc>) -> String {
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(at));
        let state = self.state(clock.clone()).await;
        let job = cooper_morning_checks::infrastructure::MorningCheckReminderJob {
            service: state.morning_checks.service.clone(),
        };
        job.run(&JobContext {
            pool: self.pool.clone(),
            clock,
        })
        .await
        .unwrap()
        .unwrap()
    }

    /// Delivers pending events as of `at` (events are due from when they occurred).
    async fn dispatch(&self, at: DateTime<Utc>) {
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::new(at));
        let state = self.state(clock.clone()).await;
        let dispatcher = state.event_handlers().into_iter().fold(
            Dispatcher::new(self.pool.clone(), clock),
            Dispatcher::with_handler,
        );
        let stats = dispatcher.dispatch_once().await.unwrap();
        assert_eq!(stats.retried + stats.dead, 0, "{stats:?}");
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

fn names(day: &Value) -> Vec<String> {
    day["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["check"]["name"].as_str().unwrap().to_owned())
        .collect()
}

async fn record(client: &Client, check: &Value, body: Value) -> (StatusCode, Value) {
    let mut body = body;
    body["check_id"] = check["id"].clone();
    let r = client.post("/api/v1/morning-checks/results", body).await;
    (r.status, r.body)
}

#[sqlx::test(migrations = "../../migrations")]
async fn check_statuses_are_seeded_and_configurable(pool: PgPool) {
    let w = World::new(&pool).await;
    let statuses = w.admin.get("/api/v1/morning-checks/statuses").await.body;
    let seeded: Vec<(String, String, bool)> = statuses
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["name"].as_str().unwrap().to_owned(),
                s["colour"].as_str().unwrap().to_owned(),
                s["requires_notes"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        seeded,
        [
            ("Green".into(), "#28a745".into(), false),
            ("Amber".into(), "#ffc107".into(), true),
            ("Red".into(), "#dc3545".into(), true),
        ]
    );
    let r = w
        .admin
        .post(
            "/api/v1/morning-checks/statuses",
            json!({ "name": "Blue", "colour": "blue" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = w
        .admin
        .post(
            "/api/v1/morning-checks/statuses",
            json!({ "name": "green", "colour": "#00ff00" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "names are unique");
    let r = w
        .admin
        .put(
            &format!("/api/v1/morning-checks/statuses/{AMBER}"),
            json!({ "name": "Amber", "colour": "#ffc107", "requires_notes": false, "sort_order": 20 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["requires_notes"], false);
    let reader = w
        .analyst("rita", &["morning-checks.read"], &[&w.company])
        .await;
    assert_eq!(
        reader
            .client
            .post(
                "/api/v1/morning-checks/statuses",
                json!({ "name": "Blue", "colour": "#0000ff" })
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn analysts_record_one_result_per_check_per_day_with_notes_where_required(pool: PgPool) {
    let w = World::new(&pool).await;
    let ann = w.analyst("ann", RECORDER, &[&w.company]).await;
    let bob = w.analyst("bob", RECORDER, &[&w.company]).await;
    let r = w
        .admin
        .post(
            "/api/v1/morning-checks/groups",
            json!({ "company_id": w.company, "name": "x", "assigned_team_id": ann.id, "assigned_analyst_id": ann.id }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a Team or an Analyst, not both"
    );
    let infra = w
        .group(json!({ "name": "Infrastructure", "assigned_analyst_id": ann.id }))
        .await;
    let backups = w
        .check(
            json!({ "name": "Backups completed", "check_group_id": infra["id"], "sort_order": 1 }),
        )
        .await;
    assert_eq!(backups["routing"]["kind"], "analyst");
    assert_eq!(backups["routing"]["analyst_id"], ann.id.as_str());
    assert_eq!(backups["routing"]["source"], "group");
    let disk = w
        .check(json!({ "name": "Disk space", "assigned_analyst_id": bob.id }))
        .await;

    let day = ann.client.get("/api/v1/morning-checks/results").await.body;
    assert_eq!(names(&day), ["Backups completed", "Disk space"]);
    assert_eq!(day["check_date"], day["today"], "default today");
    assert!(day["items"][0]["result"].is_null());
    let mine = ann
        .client
        .get("/api/v1/morning-checks/results?mine=true")
        .await
        .body;
    assert_eq!(names(&mine), ["Backups completed"]);

    let (status, body) = record(&ann.client, &backups, json!({ "status_id": AMBER })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["field"], "notes", "Amber requires notes");
    let (status, first) = record(
        &ann.client,
        &backups,
        json!({ "status_id": AMBER, "notes": "SQL01 ran late" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{first:?}");
    assert_eq!(first["recorded_by"]["id"], ann.id.as_str());
    let (status, same) = record(
        &ann.client,
        &backups,
        json!({ "status_id": AMBER, "notes": "SQL01 ran late" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(same["id"], first["id"], "one result per Check per date");
    let (status, changed) = record(&bob.client, &backups, json!({ "status_id": GREEN })).await;
    assert_eq!(status, StatusCode::OK, "{changed:?}");
    assert_eq!(changed["id"], first["id"]);
    assert_eq!(
        changed["recorded_by"]["id"],
        ann.id.as_str(),
        "first recorder kept"
    );
    assert_eq!(
        changed["modified_by"]["id"],
        bob.id.as_str(),
        "edits record who"
    );
    assert_eq!(changed["notes"], "");

    let history = ann
        .client
        .get(&format!(
            "/api/v1/morning-checks/results/{}/history",
            id(&first)
        ))
        .await
        .body;
    let actions: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(
        actions.contains(&"created") && actions.contains(&"updated"),
        "{history:?}"
    );

    let tomorrow = (Utc::now() + chrono::Duration::days(2)).date_naive();
    let (status, body) = record(
        &ann.client,
        &disk,
        json!({ "status_id": GREEN, "check_date": tomorrow }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["field"], "check_date");
    let (status, _) = record(
        &ann.client,
        &disk,
        json!({ "status_id": RED, "notes": "C: is full", "check_date": "2026-01-05" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "earlier dates can be filled in"
    );

    let r = w
        .admin
        .put(
            &format!("/api/v1/morning-checks/checks/{}", id(&disk)),
            json!({ "name": "Disk space", "assigned_analyst_id": bob.id, "is_active": false }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(!r.body["in_round"].as_bool().unwrap());
    let day = ann.client.get("/api/v1/morning-checks/results").await.body;
    assert_eq!(
        names(&day),
        ["Backups completed"],
        "inactive Checks leave the round"
    );
    let (status, _) = record(&ann.client, &disk, json!({ "status_id": GREEN })).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let r = ann
        .client
        .send(
            Method::DELETE,
            &format!("/api/v1/morning-checks/results/{}", id(&first)),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let day = ann.client.get("/api/v1/morning-checks/results").await.body;
    assert!(
        day["items"][0]["result"].is_null(),
        "cleared: unrecorded again"
    );

    let reader = w
        .analyst("rita", &["morning-checks.read"], &[&w.company])
        .await;
    let (status, _) = record(&reader.client, &backups, json!({ "status_id": GREEN })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let r = reader
        .client
        .post(
            "/api/v1/morning-checks/checks",
            json!({ "company_id": w.company, "name": "x" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../../migrations")]
async fn result_links_reach_follow_up_tickets_and_tasks_of_the_same_company(pool: PgPool) {
    let w = World::new(&pool).await;
    let mut caps = RECORDER.to_vec();
    caps.extend(["tasks.read", "tasks.create", "tickets.read"]);
    let ann = w.analyst("ann", &caps, &[&w.company]).await;
    let team = w
        .admin
        .post("/api/v1/teams", json!({ "name": "Infrastructure" }))
        .await
        .body;
    let group = w
        .group(json!({ "name": "Servers", "assigned_team_id": team["id"] }))
        .await;
    let check = w
        .check(json!({ "name": "Disk space", "check_group_id": group["id"] }))
        .await;
    let (_, result) = record(
        &ann.client,
        &check,
        json!({ "status_id": RED, "notes": "=C: is 99% full" }),
    )
    .await;
    let links = format!("/api/v1/morning-checks/results/{}/links", id(&result));

    let ticket = w
        .admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": w.company, "subject": "Disk full on FS01" }),
        )
        .await
        .body;
    let r = ann
        .client
        .post(
            &links,
            json!({ "kind": "ticket", "target_id": ticket["id"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["reference"], ticket["number"]);
    let r = ann
        .client
        .post(
            &links,
            json!({ "kind": "ticket", "target_id": ticket["id"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "already linked");

    let r = ann
        .client
        .post(
            &format!(
                "/api/v1/morning-checks/results/{}/follow-up-task",
                id(&result)
            ),
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["kind"], "task");
    let task_id = r.body["target_id"].as_str().unwrap().to_owned();
    let task = ann
        .client
        .get(&format!("/api/v1/tasks/{task_id}"))
        .await
        .body;
    assert_eq!(
        task["title"],
        "Disk space: Red on ".to_owned() + result["check_date"].as_str().unwrap()
    );
    assert_eq!(
        task["assigned_team_id"], team["id"],
        "routed like the Check"
    );
    assert!(
        task["description"]
            .as_str()
            .unwrap()
            .contains("=C: is 99% full")
    );

    let listed = ann.client.get(&links).await.body;
    let kinds: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["ticket", "task"]);

    let related = w
        .admin
        .get(&format!("/api/v1/tickets/{}/related", id(&ticket)))
        .await
        .body;
    assert_eq!(related["check_results"][0]["id"], result["id"]);
    assert!(
        related["check_results"][0]["title"]
            .as_str()
            .unwrap()
            .starts_with("Disk space (")
    );

    let no_tickets = w.analyst("nick", RECORDER, &[&w.company]).await;
    let listed = no_tickets.client.get(&links).await.body;
    assert!(
        listed.as_array().unwrap().is_empty(),
        "links to records the caller cannot see are hidden: {listed:?}"
    );
    let r = no_tickets
        .client
        .post(
            &format!(
                "/api/v1/morning-checks/results/{}/follow-up-task",
                id(&result)
            ),
            json!({ "title": "x" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "needs tasks.create");

    let other = w
        .admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
        )
        .await
        .body;
    let foreign = w
        .admin
        .post(
            "/api/v1/tasks",
            json!({ "company_id": other["id"], "title": "Elsewhere" }),
        )
        .await
        .body;
    let r = w
        .admin
        .post(
            &links,
            json!({ "kind": "task", "target_id": foreign["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "same Company only"
    );
    let r = ann
        .client
        .post(
            &links,
            json!({ "kind": "change", "target_id": foreign["id"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = ann
        .client
        .send(
            Method::DELETE,
            &format!("{links}/ticket/{}", id(&ticket)),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        ann.client.get(&links).await.body.as_array().unwrap().len(),
        1
    );
    let r = ann
        .client
        .send(
            Method::DELETE,
            &format!("{links}/ticket/{}", id(&ticket)),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(
        ann.client
            .get(&format!("/api/v1/tickets/{}", id(&ticket)))
            .await
            .status,
        StatusCode::OK,
        "unlinking leaves the ticket"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn history_grid_is_colour_coded_and_export_is_safe_csv(pool: PgPool) {
    let w = World::new(&pool).await;
    let ann = w.analyst("ann", RECORDER, &[&w.company]).await;
    let a = w
        .check(json!({ "name": "Backups, nightly", "sort_order": 1 }))
        .await;
    let b = w
        .check(json!({ "name": "Disk space", "sort_order": 2 }))
        .await;
    for (check, date, status, notes) in [
        (&a, "2026-10-01", GREEN, ""),
        (&a, "2026-10-02", RED, "=HYPERLINK(\"http://evil\")"),
        (&b, "2026-10-02", AMBER, "said \"slow\", then\nrecovered"),
    ] {
        let (s, body) = record(
            &ann.client,
            check,
            json!({ "status_id": status, "notes": notes, "check_date": date }),
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "{body:?}");
    }
    let grid = ann
        .client
        .get("/api/v1/morning-checks/history?from=2026-10-01&to=2026-10-03")
        .await;
    assert_eq!(grid.status, StatusCode::OK, "{:?}", grid.body);
    let g = grid.body;
    assert_eq!(
        g["dates"],
        json!(["2026-10-01", "2026-10-02", "2026-10-03"])
    );
    assert_eq!(g["statuses"][2]["colour"], "#dc3545");
    assert_eq!(g["rows"][0]["name"], "Backups, nightly");
    let cells: Vec<Value> = g["rows"][0]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["status_id"].clone())
        .collect();
    assert_eq!(cells, [json!(GREEN), json!(RED), Value::Null]);
    assert_eq!(g["rows"][1]["cells"][1]["has_notes"], true);
    assert_eq!(
        ann.client
            .get("/api/v1/morning-checks/history?from=2026-01-01&to=2026-10-03")
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "at most 92 days"
    );

    let (status, headers, bytes) = ann
        .client
        .send_raw(
            Method::GET,
            "/api/v1/morning-checks/history/export?from=2026-10-01&to=2026-10-31",
            "text/plain",
            Vec::new(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/csv")
    );
    assert!(
        headers["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment; filename=\"morning-checks-2026-10-01-2026-10-02.csv\"")
    );
    let csv = String::from_utf8(bytes).unwrap();
    let lines: Vec<&str> = csv.split("\r\n").collect();
    assert_eq!(
        lines[0],
        "Check Date,Company ID,Check Group,Check,Check Status,Notes,Recorded By,Recorded At,Modified By,Modified At"
    );
    assert!(lines[1].starts_with(&format!(
        "2026-10-01,{},,\"Backups, nightly\",Green,,ann,",
        w.company
    )));
    assert!(
        lines[2].contains(",Red,\"'=HYPERLINK(\"\"http://evil\"\")\",ann,"),
        "formulas are defused: {}",
        lines[2]
    );
    assert!(
        csv.contains(",Amber,\"said \"\"slow\"\", then\nrecovered\",ann,"),
        "{csv}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn unrecorded_checks_remind_assigned_analysts_once_per_day(pool: PgPool) {
    let w = World::new(&pool).await;
    let ann = w.analyst("ann", RECORDER, &[&w.company]).await;
    let bob = w.analyst("bob", RECORDER, &[&w.company]).await;
    let cat = w.analyst("cat", RECORDER, &[&w.company]).await;
    let team = w
        .admin
        .post("/api/v1/teams", json!({ "name": "Infrastructure" }))
        .await
        .body;
    w.admin
        .put(
            &format!("/api/v1/teams/{}/members", id(&team)),
            json!({ "analyst_ids": [ann.id, bob.id] }),
        )
        .await;
    let servers = w
        .group(json!({ "name": "Servers", "assigned_team_id": team["id"] }))
        .await;
    let disk = w
        .check(json!({ "name": "Disk space", "check_group_id": servers["id"] }))
        .await;
    let backups = w
        .check(json!({ "name": "Backups", "check_group_id": servers["id"] }))
        .await;
    let ups = w
        .check(json!({ "name": "UPS", "assigned_analyst_id": cat.id }))
        .await;
    let monday_9 = "2026-11-09T08:00:00Z".parse().unwrap(); // 09:00 in Berlin
    let monday_11: DateTime<Utc> = "2026-11-09T10:00:00Z".parse().unwrap();

    assert_eq!(
        w.run_reminders(monday_11).await,
        "sent 0 reminders",
        "off by default"
    );
    w.setting("morning_checks.reminders", json!(true)).await;
    w.setting("morning_checks.reminder_time", json!("10:30"))
        .await;
    w.setting("general.time_zone", json!("Europe/Berlin")).await;
    assert_eq!(
        w.run_reminders(monday_9).await,
        "sent 0 reminders",
        "too early"
    );

    let (s, _) = record(
        &cat.client,
        &ups,
        json!({ "status_id": GREEN, "check_date": "2026-11-09" }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = record(
        &ann.client,
        &disk,
        json!({ "status_id": GREEN, "check_date": "2026-11-09" }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(
        w.run_reminders(monday_11).await,
        "sent 2 reminders",
        "Ann and Bob for Backups; Cat recorded hers"
    );
    assert_eq!(
        w.run_reminders(monday_11).await,
        "sent 0 reminders",
        "once per day, also across worker restarts"
    );
    let (s, _) = record(
        &bob.client,
        &backups,
        json!({ "status_id": GREEN, "check_date": "2026-11-09" }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    w.dispatch(monday_11 + chrono::Duration::minutes(1)).await;
    for person in [&ann, &bob, &cat] {
        let bell = person.client.get("/api/v1/notifications").await.body;
        assert!(
            bell["items"].as_array().unwrap().is_empty(),
            "recorded before the reminder was delivered: {bell:?}"
        );
    }

    let tuesday: DateTime<Utc> = "2026-11-10T10:00:00Z".parse().unwrap();
    assert_eq!(w.run_reminders(tuesday).await, "sent 3 reminders");
    w.dispatch(tuesday + chrono::Duration::minutes(1)).await;
    for person in [&ann, &bob] {
        let bell = person.client.get("/api/v1/notifications").await.body;
        assert_eq!(bell["items"].as_array().unwrap().len(), 1, "{bell:?}");
        assert_eq!(bell["items"][0]["kind"], "checks_unrecorded");
        assert_eq!(
            bell["items"][0]["title"],
            "Morning Checks not recorded: Servers"
        );
        assert_eq!(
            bell["items"][0]["body"],
            "2 Checks for 2026-11-10 still have no result."
        );
    }
    let bell = cat.client.get("/api/v1/notifications").await.body;
    assert_eq!(
        bell["items"][0]["body"],
        "1 Check for 2026-11-10 still has no result."
    );

    let saturday: DateTime<Utc> = "2026-11-14T12:00:00Z".parse().unwrap();
    assert_eq!(
        w.run_reminders(saturday).await,
        "sent 0 reminders",
        "weekends are off"
    );
    w.setting("morning_checks.reminder_weekends", json!(true))
        .await;
    assert_eq!(w.run_reminders(saturday).await, "sent 3 reminders");
}

#[sqlx::test(migrations = "../../migrations")]
async fn check_dates_are_calendar_days_in_the_instance_time_zone(pool: PgPool) {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::new("2026-11-09T12:00:00Z".parse().unwrap()));
    let (anon, admin) = setup_with_clock(&pool, clock).await;
    let w = World::with(&pool, anon, admin).await;
    let check = w.check(json!({ "name": "Backups" })).await;
    let day = w.admin.get("/api/v1/morning-checks/results").await.body;
    assert_eq!(day["today"], "2026-11-09", "UTC by default");
    w.setting("general.time_zone", json!("Pacific/Kiritimati"))
        .await;
    let day = w.admin.get("/api/v1/morning-checks/results").await.body;
    assert_eq!(day["today"], "2026-11-10", "UTC+14");
    let (s, r) = record(&w.admin, &check, json!({ "status_id": GREEN })).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(r["check_date"], "2026-11-10");
    w.setting("general.time_zone", json!("Mars/Olympus")).await;
    let day = w.admin.get("/api/v1/morning-checks/results").await.body;
    assert_eq!(
        day["today"], "2026-11-09",
        "an unknown zone falls back to UTC"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn other_companies_checks_stay_hidden_and_the_module_can_be_switched_off(pool: PgPool) {
    let w = World::new(&pool).await;
    let check = w.check(json!({ "name": "Backups" })).await;
    let (_, result) = record(&w.admin, &check, json!({ "status_id": GREEN })).await;
    let other = w
        .admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
        )
        .await
        .body;
    let mut caps = RECORDER.to_vec();
    caps.push("morning-checks.configure");
    let ola = w.analyst("ola", &caps, &[id(&other)]).await;
    assert_eq!(
        ola.client
            .get(&format!("/api/v1/morning-checks/checks/{}", id(&check)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ola.client
            .get(&format!("/api/v1/morning-checks/results/{}", id(&result)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert!(names(&ola.client.get("/api/v1/morning-checks/results").await.body).is_empty());
    let (s, _) = record(&ola.client, &check, json!({ "status_id": GREEN })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let r = ola
        .client
        .post(
            "/api/v1/morning-checks/checks",
            json!({ "company_id": w.company, "name": "x" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let theirs = ola
        .client
        .post(
            "/api/v1/morning-checks/groups",
            json!({ "company_id": other["id"], "name": "Theirs" }),
        )
        .await
        .body;
    let r = w
        .admin
        .post(
            "/api/v1/morning-checks/checks",
            json!({ "company_id": w.company, "name": "x", "check_group_id": theirs["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a group of another Company"
    );
    assert_eq!(
        ola.client
            .get(&format!(
                "/api/v1/morning-checks/results?company_id={}",
                w.company
            ))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let r = w
        .admin
        .put(
            "/api/v1/modules/morning-checks",
            json!({ "is_enabled": false }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        w.admin.get("/api/v1/morning-checks/results").await.status,
        StatusCode::NOT_FOUND
    );
}
