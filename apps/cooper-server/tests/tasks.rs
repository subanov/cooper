//! Tasks end to end (ADR-0029): Statuses and Priorities, Subtasks, Tags, Collaborators,
//! comments, time entries and history, links to tickets and Changes with the ticket's
//! related records, the Board, Recurrence on completion and on a fixed schedule,
//! assignment Notifications, Company isolation and the Tasks Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use chrono::Datelike;
use common::{Client, setup, test_settings};
use cooper_kernel::{Clock, FixedClock, SystemClock};
use cooper_platform::jobs::{Job, JobContext};
use cooper_platform::outbox::Dispatcher;
use cooper_server::AppState;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

const TO_DO: &str = "019a0000-0000-7000-8000-000000000b01";
const IN_PROGRESS: &str = "019a0000-0000-7000-8000-000000000b02";
const DONE: &str = "019a0000-0000-7000-8000-000000000b04";
const CANCELLED: &str = "019a0000-0000-7000-8000-000000000b05";
const MEDIUM: &str = "019a0000-0000-7000-8000-000000000b12";
const CAPS: &[&str] = &["tasks.read", "tasks.create", "tasks.update", "tickets.read"];

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

    async fn task(&self, client: &Client, body: Value) -> Value {
        let mut body = body;
        if body.get("parent_task_id").is_none() {
            body["company_id"] = json!(self.company);
        }
        let r = client.post("/api/v1/tasks", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    async fn dispatch(&self, clock: Arc<dyn Clock>) {
        let state =
            AppState::with_settings(self.pool.clone(), clock.clone(), test_settings()).unwrap();
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

fn titles(v: &Value) -> Vec<String> {
    let items = v.get("items").unwrap_or(v);
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test(migrations = "../../migrations")]
async fn tasks_carry_subtasks_collaborators_comments_time_and_history(pool: PgPool) {
    let w = World::new(&pool).await;
    let ann = w.analyst("ann", CAPS, &[&w.company]).await;
    let bob = w.analyst("bob", CAPS, &[&w.company]).await;
    let t = w
        .task(&ann.client, json!({ "title": "Renew certificates", "due_date": "2026-12-01", "start_date": "2026-11-20",
                                   "estimate_minutes": 90, "tags": ["Security", "security", "certs"],
                                   "assigned_analyst_id": bob.id, "collaborator_ids": [ann.id] }))
        .await;
    assert_eq!(t["status_id"], TO_DO, "default status");
    assert_eq!(t["priority_id"], MEDIUM, "default priority");
    assert_eq!(t["tags"], json!(["certs", "security"]));
    let path = format!("/api/v1/tasks/{}", id(&t));
    let r = ann.client.post("/api/v1/tasks", json!({ "company_id": w.company, "title": "x", "start_date": "2026-12-02", "due_date": "2026-12-01" })).await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "due before start"
    );

    w.dispatch(Arc::new(SystemClock)).await;
    let bell = bob.client.get("/api/v1/notifications").await.body;
    assert_eq!(bell["items"][0]["kind"], "task_assigned");
    assert_eq!(bell["items"][0]["task_id"], t["id"]);

    let sub = w
        .task(
            &ann.client,
            json!({ "title": "Web server", "parent_task_id": t["id"] }),
        )
        .await;
    assert_eq!(
        sub["company_id"],
        w.company.as_str(),
        "a Subtask takes its parent's Company"
    );
    let r = ann
        .client
        .post(
            "/api/v1/tasks",
            json!({ "title": "deeper", "parent_task_id": sub["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "one level of Subtasks"
    );
    let subs = ann
        .client
        .get(&format!("/api/v1/tasks?parent_task_id={}", id(&t)))
        .await
        .body;
    assert_eq!(titles(&subs), ["Web server"]);
    let top = ann.client.get("/api/v1/tasks?top_level=true").await.body;
    assert_eq!(titles(&top), ["Renew certificates"]);

    assert_eq!(
        ann.client.get(&format!("{path}/collaborators")).await.body["analyst_ids"],
        json!([ann.id])
    );
    let r = ann
        .client
        .put(
            &format!("{path}/collaborators"),
            json!({ "analyst_ids": [ann.id, bob.id] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let mine = bob.client.get("/api/v1/tasks?collaborator=me").await.body;
    assert_eq!(titles(&mine), ["Renew certificates"]);

    let r = bob
        .client
        .post(
            &format!("{path}/comments"),
            json!({ "body": "Waiting for the CSR" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let r = bob
        .client
        .post(
            &format!("{path}/time-entries"),
            json!({ "minutes": 45, "note": "Generated CSR" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let entry = id(&r.body).to_owned();
    let r = ann
        .client
        .send(
            Method::DELETE,
            &format!("{path}/time-entries/{entry}"),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "only your own time");
    assert_eq!(
        bob.client
            .post(&format!("{path}/time-entries"), json!({ "minutes": 0 }))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = bob
        .client
        .send(
            Method::DELETE,
            &format!("{path}/time-entries/{entry}"),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);

    let r = ann.client.put(&path, json!({ "title": "Renew TLS certificates", "tags": ["certs"], "assigned_analyst_id": ann.id })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let history = ann.client.get(&format!("{path}/history")).await.body;
    let fields: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["field"].as_str())
        .collect();
    for f in [
        "title",
        "assigned_analyst_id",
        "collaborators",
        "comment",
        "time_minutes",
    ] {
        assert!(fields.contains(&f), "{f} in {fields:?}");
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn the_board_orders_cards_and_moves_them_between_columns(pool: PgPool) {
    let w = World::new(&pool).await;
    let a = w.task(&w.admin, json!({ "title": "A" })).await;
    let b = w.task(&w.admin, json!({ "title": "B" })).await;
    let c = w.task(&w.admin, json!({ "title": "C" })).await;
    let column = |board: &Value, status: &str| -> Vec<String> {
        let col = board
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["status"]["id"] == status)
            .unwrap();
        titles(&col["tasks"])
    };
    let board = w.admin.get("/api/v1/tasks/board").await.body;
    assert_eq!(board.as_array().unwrap().len(), 5, "one column per status");
    assert_eq!(column(&board, TO_DO), ["A", "B", "C"], "in creation order");

    let r = w
        .admin
        .post(
            &format!("/api/v1/tasks/{}/status", id(&c)),
            json!({ "status_id": TO_DO, "index": 0 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    w.admin
        .post(
            &format!("/api/v1/tasks/{}/status", id(&a)),
            json!({ "status_id": IN_PROGRESS, "index": 0 }),
        )
        .await;
    let board = w.admin.get("/api/v1/tasks/board").await.body;
    assert_eq!(column(&board, TO_DO), ["C", "B"]);
    assert_eq!(column(&board, IN_PROGRESS), ["A"]);
    let r = w
        .admin
        .post(
            &format!("/api/v1/tasks/{}/status", id(&b)),
            json!({ "status_id": CANCELLED }),
        )
        .await;
    assert_eq!(r.body["is_closed"], true);
    assert!(r.body["completed_at"].is_string());
    let open = w.admin.get("/api/v1/tasks?open=true").await.body;
    assert_eq!(open["items"].as_array().unwrap().len(), 2);

    let r = w
        .admin
        .post(
            "/api/v1/task-statuses",
            json!({ "name": "Review", "sort_order": 25, "colour": "#aabbcc" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = w
        .admin
        .post(
            "/api/v1/task-statuses",
            json!({ "name": "Shipped", "is_done": true }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Done statuses are closed"
    );
    let board = w.admin.get("/api/v1/tasks/board").await.body;
    let names: Vec<&str> = board
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["status"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "To Do",
            "In Progress",
            "Review",
            "Blocked",
            "Done",
            "Cancelled"
        ]
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn completed_tasks_repeat_with_their_subtasks_and_series_end(pool: PgPool) {
    let w = World::new(&pool).await;
    let t = w.task(&w.admin, json!({ "title": "Test restore", "start_date": "2026-11-02", "due_date": "2026-11-05" })).await;
    w.task(
        &w.admin,
        json!({ "title": "Pick a backup", "parent_task_id": t["id"] }),
    )
    .await;
    let path = format!("/api/v1/tasks/{}", id(&t));
    let r = w.admin.put(&format!("{path}/recurrence"), json!({ "rule": { "frequency": "weekly", "weekdays": [8], "interval": 1, "mode": "on_completion", "ends": "never" } })).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = w
        .admin
        .put(&format!("{path}/recurrence"), json!({ "rule": { "frequency": "monthly", "on": "day_of_month", "day": 5, "interval": 1,
                                                               "mode": "on_completion", "ends": "after_count", "count": 2 } }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["occurrences_created"], 1);

    let r = w
        .admin
        .post(&format!("{path}/status"), json!({ "status_id": DONE }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let open = w
        .admin
        .get("/api/v1/tasks?open=true&top_level=true")
        .await
        .body;
    let next = &open["items"][0];
    assert_eq!(next["title"], "Test restore");
    assert_eq!(next["previous_occurrence_id"], t["id"]);
    let today = chrono::Utc::now().date_naive();
    let fifth = |d: chrono::NaiveDate| d.with_day(5).unwrap();
    let expected = if today.day() < 5 {
        fifth(today)
    } else {
        fifth(
            today
                .checked_add_months(chrono::Months::new(1))
                .unwrap()
                .with_day(1)
                .unwrap(),
        )
    };
    assert_eq!(
        next["due_date"],
        expected.to_string(),
        "the next 5th, counted from completion (today)"
    );
    assert_eq!(
        next["start_date"],
        (expected - chrono::Days::new(3)).to_string(),
        "same lead time"
    );
    let subs = w
        .admin
        .get(&format!("/api/v1/tasks?parent_task_id={}", id(next)))
        .await
        .body;
    assert_eq!(
        titles(&subs),
        ["Pick a backup"],
        "Subtasks repeat with their parent"
    );

    let r = w
        .admin
        .post(
            &format!("/api/v1/tasks/{}/status", id(next)),
            json!({ "status_id": DONE }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let open = w
        .admin
        .get("/api/v1/tasks?open=true&top_level=true")
        .await
        .body;
    assert!(
        titles(&open).is_empty(),
        "after 2 occurrences the series ends"
    );
    let series = w
        .admin
        .get(&format!("/api/v1/tasks/{}/recurrence", id(next)))
        .await
        .body;
    assert_eq!(series["is_active"], false);

    // Cancelling does not repeat.
    let c = w.task(&w.admin, json!({ "title": "Weekly check" })).await;
    w.admin
        .put(&format!("/api/v1/tasks/{}/recurrence", id(&c)), json!({ "rule": { "frequency": "daily", "interval": 1, "mode": "on_completion", "ends": "never" } }))
        .await;
    w.admin
        .post(
            &format!("/api/v1/tasks/{}/status", id(&c)),
            json!({ "status_id": CANCELLED }),
        )
        .await;
    assert!(
        titles(
            &w.admin
                .get("/api/v1/tasks?open=true&top_level=true")
                .await
                .body
        )
        .is_empty()
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn fixed_schedules_create_occurrences_even_when_the_last_is_open(pool: PgPool) {
    let w = World::new(&pool).await;
    let t = w
        .task(
            &w.admin,
            json!({ "title": "Compliance review", "due_date": "2026-11-01" }),
        )
        .await;
    let r = w
        .admin
        .put(&format!("/api/v1/tasks/{}/recurrence", id(&t)), json!({ "rule": { "frequency": "monthly", "on": "day_of_month", "day": 1,
                                                                                  "interval": 1, "mode": "fixed", "ends": "on_date", "date": "2027-01-15" } }))
        .await;
    assert_eq!(r.body["next_date"], "2026-12-01", "{:?}", r.body);

    let later: Arc<dyn Clock> = Arc::new(FixedClock::new("2027-03-01T06:00:00Z".parse().unwrap()));
    let state = AppState::with_settings(pool.clone(), later.clone(), test_settings()).unwrap();
    let job = cooper_tasks::infrastructure::TaskRecurrenceJob {
        service: state.tasks.service.clone(),
    };
    let ctx = JobContext {
        pool: pool.clone(),
        clock: later.clone(),
    };
    assert_eq!(
        job.run(&ctx).await.unwrap().unwrap(),
        "created 2 occurrences",
        "Dec 1 and Jan 1; Feb 1 is after the end"
    );
    assert_eq!(
        job.run(&ctx).await.unwrap().unwrap(),
        "created 0 occurrences"
    );
    let open = w.admin.get("/api/v1/tasks?open=true").await.body;
    let mut due: Vec<&str> = open["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["due_date"].as_str().unwrap())
        .collect();
    due.sort_unstable();
    assert_eq!(
        due,
        ["2026-11-01", "2026-12-01", "2027-01-01"],
        "the backlog is the point"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn tasks_link_to_tickets_and_changes_and_tickets_show_their_related_records(pool: PgPool) {
    let w = World::new(&pool).await;
    let t = w.task(&w.admin, json!({ "title": "Replace switch" })).await;
    let ticket = w
        .admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": w.company, "subject": "Network down" }),
        )
        .await
        .body;
    let change = w
        .admin
        .post(
            "/api/v1/changes",
            json!({ "company_id": w.company, "title": "Swap core switch" }),
        )
        .await
        .body;
    let problem = w
        .admin
        .post(
            "/api/v1/problems",
            json!({ "company_id": w.company, "title": "Switch flaps" }),
        )
        .await
        .body;
    let path = format!("/api/v1/tasks/{}", id(&t));
    let r = w
        .admin
        .post(
            &format!("{path}/links"),
            json!({ "kind": "ticket", "target_id": ticket["id"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["number"], ticket["number"]);
    let r = w
        .admin
        .post(
            &format!("{path}/links"),
            json!({ "kind": "change", "target_id": change["id"] }),
        )
        .await;
    assert_eq!(r.body["number"], "CHG-00001");
    assert_eq!(
        w.admin
            .post(
                &format!("{path}/links"),
                json!({ "kind": "ticket", "target_id": ticket["id"] })
            )
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        w.admin
            .post(
                &format!("{path}/links"),
                json!({ "kind": "contract", "target_id": ticket["id"] })
            )
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    w.admin
        .post(
            &format!("/api/v1/problems/{}/tickets", id(&problem)),
            json!({ "ticket_id": ticket["id"] }),
        )
        .await;
    w.admin
        .post(
            &format!("/api/v1/changes/{}/links", id(&change)),
            json!({ "kind": "ticket", "target_id": ticket["id"] }),
        )
        .await;

    let related = w
        .admin
        .get(&format!("/api/v1/tickets/{}/related", id(&ticket)))
        .await;
    assert_eq!(related.status, StatusCode::OK, "{:?}", related.body);
    assert_eq!(titles(&related.body["tasks"]), ["Replace switch"]);
    assert_eq!(related.body["changes"][0]["number"], "CHG-00001");
    assert_eq!(related.body["problems"][0]["number"], "PRB-00001");
    let desk = w.analyst("dee", &["tickets.read"], &[&w.company]).await;
    let related = desk
        .client
        .get(&format!("/api/v1/tickets/{}/related", id(&ticket)))
        .await
        .body;
    assert!(
        related["tasks"].as_array().unwrap().is_empty()
            && related["changes"].as_array().unwrap().is_empty(),
        "no read Capability, no records"
    );
    w.admin
        .put("/api/v1/modules/tasks", json!({ "is_enabled": false }))
        .await;
    let related = w
        .admin
        .get(&format!("/api/v1/tickets/{}/related", id(&ticket)))
        .await
        .body;
    assert!(
        related["tasks"].as_array().unwrap().is_empty(),
        "a switched-off Module is left out"
    );
    assert_eq!(related["changes"].as_array().unwrap().len(), 1);
    assert_eq!(
        w.admin.get("/api/v1/tasks").await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn tasks_are_isolated_by_company_and_need_capabilities(pool: PgPool) {
    let w = World::new(&pool).await;
    let t = w
        .task(&w.admin, json!({ "title": "Default Company task" }))
        .await;
    let other = w
        .admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
        )
        .await
        .body;
    let outsider = w.analyst("ola", CAPS, &[id(&other)]).await;
    assert_eq!(
        outsider
            .client
            .get(&format!("/api/v1/tasks/{}", id(&t)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert!(titles(&outsider.client.get("/api/v1/tasks").await.body).is_empty());
    assert_eq!(
        outsider
            .client
            .post(
                "/api/v1/tasks",
                json!({ "company_id": w.company, "title": "x" })
            )
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        outsider
            .client
            .post(
                "/api/v1/tasks",
                json!({ "title": "x", "parent_task_id": t["id"] })
            )
            .await
            .status,
        StatusCode::NOT_FOUND,
        "nor through a parent"
    );
    let reader = w.analyst("rita", &["tasks.read"], &[&w.company]).await;
    assert_eq!(
        reader
            .client
            .post(
                "/api/v1/tasks",
                json!({ "company_id": w.company, "title": "x" })
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        reader
            .client
            .post(
                &format!("/api/v1/tasks/{}/status", id(&t)),
                json!({ "status_id": DONE })
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        reader
            .client
            .post("/api/v1/task-priorities", json!({ "name": "P0" }))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}
