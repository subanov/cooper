//! Service desk end to end (ADR-0014): reference data, numbering, lifecycle,
//! assignment, Confidential tickets, Notes, time, timeline, views, search, soft delete.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use common::{Client, setup};
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

const DESK_CAPS: &[&str] = &[
    "tickets.read",
    "tickets.create",
    "tickets.update",
    "tickets.assign",
    "tickets.close",
    "requesters.read",
];

struct World {
    anon: Client,
    admin: Client,
    company: String,
}

impl World {
    async fn new(pool: &PgPool) -> Self {
        let (anon, admin) = setup(pool).await;
        let company = admin.get("/api/v1/companies").await.body["items"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            anon,
            admin,
            company,
        }
    }

    async fn lookup(&self, path: &str, key: &str) -> String {
        let rows = self.admin.get(&format!("/api/v1/{path}")).await.body;
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["key"] == key)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn analyst(&self, username: &str, caps: &[&str]) -> (String, Client) {
        let created = self.admin.post("/api/v1/analysts", json!({
            "username": username, "email": format!("{username}@default.example"), "full_name": username,
            "password": "analyst password 1"
        })).await;
        let id = created.body["id"].as_str().unwrap().to_owned();
        if !caps.is_empty() {
            let role = self
                .admin
                .post(
                    "/api/v1/roles",
                    json!({ "name": format!("{username} role"), "capabilities": caps }),
                )
                .await;
            self.admin
                .put(
                    &format!("/api/v1/analysts/{id}/roles"),
                    json!({ "role_ids": [role.body["id"]] }),
                )
                .await;
        }
        self.admin
            .put(
                &format!("/api/v1/analysts/{id}/company-access"),
                json!({ "all": false, "company_ids": [self.company] }),
            )
            .await;
        (id, self.anon.sign_in(username, "analyst password 1").await)
    }

    async fn ticket(&self, client: &Client, body: Value) -> Value {
        let mut body = body;
        body["company_id"] = json!(self.company);
        let r = client.post("/api/v1/tickets", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }
}

async fn outbox_types(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT event_type FROM outbox_events ORDER BY occurred_at, id")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn reference_data_is_seeded_and_configurable(pool: PgPool) {
    let w = World::new(&pool).await;
    let statuses = w.admin.get("/api/v1/ticket-statuses").await.body;
    let names: Vec<&str> = statuses
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Open",
            "In Progress",
            "On Hold",
            "Awaiting Response",
            "Closed"
        ]
    );
    assert_eq!(statuses[2]["pauses_sla"], true);
    assert_eq!(
        w.admin
            .get("/api/v1/resolution-codes")
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        10
    );

    let new_default = w
        .admin
        .post(
            "/api/v1/ticket-statuses",
            json!({ "name": "New", "is_default": true, "sort_order": 1 }),
        )
        .await;
    assert_eq!(new_default.status, StatusCode::CREATED);
    let defaults: Vec<Value> = w
        .admin
        .get("/api/v1/ticket-statuses")
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["is_default"] == true)
        .cloned()
        .collect();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0]["name"], "New");
    assert_eq!(
        w.admin
            .post("/api/v1/ticket-statuses", json!({ "name": "open" }))
            .await
            .status,
        StatusCode::CONFLICT
    );

    let a = w
        .admin
        .post("/api/v1/ticket-categories", json!({ "name": "Hardware" }))
        .await
        .body;
    let b = w
        .admin
        .post(
            "/api/v1/ticket-categories",
            json!({ "name": "Laptop", "parent_id": a["id"] }),
        )
        .await
        .body;
    let c = w
        .admin
        .post(
            "/api/v1/ticket-categories",
            json!({ "name": "Battery", "parent_id": b["id"] }),
        )
        .await
        .body;
    assert_eq!(c["depth"], 3);
    assert_eq!(
        w.admin
            .post(
                "/api/v1/ticket-categories",
                json!({ "name": "Cell", "parent_id": c["id"] })
            )
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let (_, desk) = w.analyst("desk", DESK_CAPS).await;
    assert_eq!(
        desk.get("/api/v1/ticket-priorities").await.status,
        StatusCode::OK
    );
    assert_eq!(
        desk.post("/api/v1/ticket-types", json!({ "name": "Change" }))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn tickets_are_numbered_per_company_with_the_configured_format(pool: PgPool) {
    let w = World::new(&pool).await;
    let one = w
        .ticket(&w.admin, json!({ "subject": "Printer jammed" }))
        .await;
    let two = w
        .ticket(&w.admin, json!({ "subject": "Mouse broken" }))
        .await;
    assert_eq!(one["number"], "DEF-000001");
    assert_eq!(two["number"], "DEF-000002");
    let normal = w.lookup("ticket-priorities", "normal").await;
    assert_eq!(one["priority_id"], normal.as_str(), "default Priority");
    assert_eq!(
        one["status_id"],
        w.lookup("ticket-statuses", "open").await.as_str(),
        "default status"
    );

    w.admin
        .put(
            "/api/v1/settings/tickets.number_format",
            json!({ "value": "{TYPE}{SEQ:4}" }),
        )
        .await;
    let incident = w.lookup("ticket-types", "incident").await;
    w.admin
        .put(
            &format!("/api/v1/ticket-types/{incident}"),
            json!({ "name": "Incident", "number_prefix": "INC", "sort_order": 10 }),
        )
        .await;
    let three = w
        .ticket(
            &w.admin,
            json!({ "subject": "Server down", "type_id": incident }),
        )
        .await;
    assert_eq!(three["number"], "INC0003");

    let found = w.admin.get("/api/v1/tickets/by-number/DEF-000002").await;
    assert_eq!(found.body["id"], two["id"]);
    let no_company = w
        .admin
        .post("/api/v1/tickets", json!({ "subject": "x" }))
        .await;
    assert_eq!(no_company.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../../migrations")]
async fn closing_needs_a_resolution_code_and_reopening_clears_it(pool: PgPool) {
    let w = World::new(&pool).await;
    let t = w.ticket(&w.admin, json!({ "subject": "VPN fails" })).await;
    let id = t["id"].as_str().unwrap();
    let closed = w.lookup("ticket-statuses", "closed").await;
    let open = w.lookup("ticket-statuses", "open").await;
    let fixed = w.lookup("resolution-codes", "fixed_remotely").await;

    let missing = w
        .admin
        .post(
            &format!("/api/v1/tickets/{id}/status"),
            json!({ "status_id": closed }),
        )
        .await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);
    let ok = w
        .admin
        .post(
            &format!("/api/v1/tickets/{id}/status"),
            json!({ "status_id": closed, "resolution_code_id": fixed }),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert!(ok.body["closed_at"].is_string());
    let reopened = w
        .admin
        .post(
            &format!("/api/v1/tickets/{id}/status"),
            json!({ "status_id": open }),
        )
        .await
        .body;
    assert!(reopened["closed_at"].is_null());
    assert!(reopened["resolution_code_id"].is_null());

    w.admin
        .put(
            "/api/v1/settings/tickets.require_closure_category",
            json!({ "value": true }),
        )
        .await;
    let needs_category = w
        .admin
        .post(
            &format!("/api/v1/tickets/{id}/status"),
            json!({ "status_id": closed, "resolution_code_id": fixed }),
        )
        .await;
    assert_eq!(needs_category.status, StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(
        outbox_types(&pool).await,
        ["ticket.created", "ticket.closed", "ticket.reopened"]
    );
    let (_, viewer) = w
        .analyst("viewer", &["tickets.read", "tickets.update"])
        .await;
    let forbidden = viewer
        .post(
            &format!("/api/v1/tickets/{id}/status"),
            json!({ "status_id": closed, "resolution_code_id": fixed }),
        )
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../../migrations")]
async fn assignee_must_belong_to_the_queue(pool: PgPool) {
    let w = World::new(&pool).await;
    let (member, _) = w.analyst("member", DESK_CAPS).await;
    let (outsider, _) = w.analyst("outsider", DESK_CAPS).await;
    let team = w
        .admin
        .post("/api/v1/teams", json!({ "name": "Service Desk" }))
        .await
        .body;
    let team_id = team["id"].as_str().unwrap();
    w.admin
        .put(
            &format!("/api/v1/teams/{team_id}/members"),
            json!({ "analyst_ids": [member] }),
        )
        .await;
    let t = w
        .ticket(&w.admin, json!({ "subject": "Laptop slow" }))
        .await;
    let id = t["id"].as_str().unwrap();

    let bad = w
        .admin
        .put(
            &format!("/api/v1/tickets/{id}/assignment"),
            json!({ "team_id": team_id, "analyst_id": outsider }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let ok = w
        .admin
        .put(
            &format!("/api/v1/tickets/{id}/assignment"),
            json!({ "team_id": team_id, "analyst_id": member }),
        )
        .await;
    assert_eq!(ok.body["analyst_id"], member.as_str());
    assert!(
        outbox_types(&pool)
            .await
            .contains(&"ticket.assigned".to_owned())
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn confidential_tickets_are_visible_only_to_their_departments_teams(pool: PgPool) {
    let w = World::new(&pool).await;
    let (hr_member, hr) = w.analyst("hrdesk", DESK_CAPS).await;
    let (_, it) = w.analyst("itdesk", DESK_CAPS).await;
    let (_, auditor) = w
        .analyst("auditor", &["tickets.read", "tickets.view_confidential"])
        .await;
    let team = w
        .admin
        .post("/api/v1/teams", json!({ "name": "HR Team" }))
        .await
        .body;
    w.admin
        .put(
            &format!("/api/v1/teams/{}/members", team["id"].as_str().unwrap()),
            json!({ "analyst_ids": [hr_member] }),
        )
        .await;
    let dept = w.admin.post("/api/v1/departments", json!({ "name": "HR", "default_sensitivity": "confidential", "team_ids": [team["id"]] })).await.body;

    let secret = w
        .ticket(
            &hr,
            json!({ "subject": "Grievance", "department_id": dept["id"] }),
        )
        .await;
    assert_eq!(secret["sensitivity"], "confidential", "Department default");
    let id = secret["id"].as_str().unwrap();
    w.ticket(&it, json!({ "subject": "Grievance about printers" }))
        .await;

    assert_eq!(
        hr.get(&format!("/api/v1/tickets/{id}")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        it.get(&format!("/api/v1/tickets/{id}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        auditor.get(&format!("/api/v1/tickets/{id}")).await.status,
        StatusCode::OK
    );
    let search = it.get("/api/v1/tickets?q=grievance").await.body;
    assert_eq!(
        search["items"].as_array().unwrap().len(),
        1,
        "Confidential content is excluded from search"
    );
    assert_eq!(
        hr.get("/api/v1/tickets?q=grievance").await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn notes_time_and_timeline(pool: PgPool) {
    let w = World::new(&pool).await;
    let (_, desk) = w.analyst("desk", DESK_CAPS).await;
    let t = w
        .ticket(&desk, json!({ "subject": "Outlook crashes" }))
        .await;
    let id = t["id"].as_str().unwrap();
    let note = desk
        .post(
            &format!("/api/v1/tickets/{id}/notes"),
            json!({ "body": "Rebuilt the mail profile" }),
        )
        .await;
    assert_eq!(note.status, StatusCode::CREATED);
    assert_eq!(note.body["is_public"], false, "internal by default");
    desk.post(
        &format!("/api/v1/tickets/{id}/time-entries"),
        json!({ "minutes": 25, "note": "remote session" }),
    )
    .await;
    desk.post(
        &format!("/api/v1/tickets/{id}/time-entries"),
        json!({ "minutes": 20 }),
    )
    .await;
    assert_eq!(
        desk.post(
            &format!("/api/v1/tickets/{id}/time-entries"),
            json!({ "minutes": 0 })
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        desk.get(&format!("/api/v1/tickets/{id}")).await.body["total_minutes"],
        45
    );

    let open = w.lookup("ticket-statuses", "in_progress").await;
    desk.post(
        &format!("/api/v1/tickets/{id}/status"),
        json!({ "status_id": open }),
    )
    .await;
    let timeline = desk
        .get(&format!("/api/v1/tickets/{id}/timeline"))
        .await
        .body;
    let kinds: Vec<&str> = timeline
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["change", "note", "time_entry", "time_entry", "change"]
    );
    let last = &timeline[4]["data"]["changes"][0];
    assert_eq!(last["field"], "status_id");

    let by_note = desk.get("/api/v1/tickets?q=profile").await.body;
    assert_eq!(by_note["items"][0]["id"], t["id"], "search finds Note text");
    let by_number = desk
        .get(&format!(
            "/api/v1/tickets?q={}",
            &t["number"].as_str().unwrap()[..5]
        ))
        .await
        .body;
    assert_eq!(by_number["items"].as_array().unwrap().len(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn built_in_views_and_paging(pool: PgPool) {
    let w = World::new(&pool).await;
    let (me, desk) = w.analyst("desk", DESK_CAPS).await;
    let mine = w
        .ticket(&w.admin, json!({ "subject": "Mine", "analyst_id": me }))
        .await;
    let unassigned = w.ticket(&w.admin, json!({ "subject": "Nobody's" })).await;
    let snoozed = w.ticket(&w.admin, json!({ "subject": "Later" })).await;
    w.admin
        .put(
            &format!("/api/v1/tickets/{}/snooze", snoozed["id"].as_str().unwrap()),
            json!({ "until": chrono::Utc::now() + chrono::Duration::days(30), "reason": "parts" }),
        )
        .await;

    let ids = |v: &Value| {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&desk.get("/api/v1/tickets?view=mine").await.body),
        [mine["id"].as_str().unwrap()]
    );
    let unassigned_view = ids(&desk.get("/api/v1/tickets?view=unassigned").await.body);
    assert_eq!(
        unassigned_view,
        [unassigned["id"].as_str().unwrap()],
        "snoozed tickets are hidden"
    );
    assert_eq!(
        ids(&desk.get("/api/v1/tickets?view=snoozed").await.body),
        [snoozed["id"].as_str().unwrap()]
    );
    assert_eq!(
        ids(&desk.get("/api/v1/tickets?view=open").await.body).len(),
        2
    );
    assert!(ids(&desk.get("/api/v1/tickets?view=recently_closed").await.body).is_empty());

    let first = desk
        .get("/api/v1/tickets?limit=2&sort=number&order=asc")
        .await
        .body;
    assert_eq!(first["items"][0]["number"], "DEF-000001");
    let next = desk
        .get(&format!(
            "/api/v1/tickets?limit=2&sort=number&order=asc&cursor={}",
            first["next_cursor"].as_str().unwrap()
        ))
        .await
        .body;
    assert_eq!(next["items"][0]["number"], "DEF-000003");
    assert_eq!(
        desk.get("/api/v1/tickets?view=whatever").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn soft_deleted_tickets_are_hidden_and_restored_by_administrators(pool: PgPool) {
    let w = World::new(&pool).await;
    let (_, desk) = w
        .analyst(
            "desk",
            &["tickets.read", "tickets.create", "tickets.delete"],
        )
        .await;
    let t = w.ticket(&desk, json!({ "subject": "Spam" })).await;
    let id = t["id"].as_str().unwrap();
    assert_eq!(
        desk.send(Method::DELETE, &format!("/api/v1/tickets/{id}"), None)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        desk.get(&format!("/api/v1/tickets/{id}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(
        desk.get("/api/v1/tickets").await.body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        desk.get("/api/v1/tickets?view=deleted").await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        desk.post(&format!("/api/v1/tickets/{id}/restore"), json!({}))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    assert_eq!(
        w.admin.get("/api/v1/tickets?view=deleted").await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let restored = w
        .admin
        .post(&format!("/api/v1/tickets/{id}/restore"), json!({}))
        .await;
    assert_eq!(restored.status, StatusCode::OK);
    assert_eq!(
        desk.get(&format!("/api/v1/tickets/{id}")).await.status,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn tickets_are_isolated_by_company(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w
        .admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH" }),
        )
        .await
        .body;
    let theirs = w
        .admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": other["id"], "subject": "Theirs" }),
        )
        .await
        .body;
    assert_eq!(
        theirs["number"], "OTH-000001",
        "each Company has its own sequence"
    );
    let (_, desk) = w.analyst("desk", DESK_CAPS).await;
    assert_eq!(
        desk.get(&format!(
            "/api/v1/tickets/{}",
            theirs["id"].as_str().unwrap()
        ))
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let create = desk
        .post(
            "/api/v1/tickets",
            json!({ "company_id": other["id"], "subject": "Sneaky" }),
        )
        .await;
    assert_eq!(create.status, StatusCode::NOT_FOUND);
    let requester = w.admin.post("/api/v1/requesters", json!({ "company_id": other["id"], "full_name": "Olga", "email": "olga@other.example" })).await.body;
    let cross = w
        .admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": w.company, "subject": "x", "requester_id": requester["id"] }),
        )
        .await;
    assert_eq!(cross.status, StatusCode::UNPROCESSABLE_ENTITY);
}
