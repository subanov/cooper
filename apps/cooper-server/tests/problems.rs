//! Problem Management end to end (ADR-0026): lifecycle, Known Errors, Notes and history,
//! linked tickets from both sides, actions on linked tickets once resolved, Company
//! isolation and the Problems Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use common::{Client, setup};
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

const RESOLVED: &str = "019a0000-0000-7000-8000-000000000905";
const INVESTIGATING: &str = "019a0000-0000-7000-8000-000000000902";
const NEW: &str = "019a0000-0000-7000-8000-000000000901";
const MEDIUM: &str = "019a0000-0000-7000-8000-000000000912";
const FIXED: &str = "019a0000-0000-7000-8000-000000000501";

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

    async fn analyst(&self, name: &str, caps: &[&str], companies: &[&str]) -> Client {
        let a = self
            .admin
            .post(
                "/api/v1/analysts",
                json!({ "username": name, "email": format!("{name}@default.example"),
            "full_name": name, "password": "analyst password 1" }),
            )
            .await
            .body;
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
                &format!("/api/v1/analysts/{}/roles", a["id"].as_str().unwrap()),
                json!({ "role_ids": [role["id"]] }),
            )
            .await;
        self.admin
            .put(
                &format!(
                    "/api/v1/analysts/{}/company-access",
                    a["id"].as_str().unwrap()
                ),
                json!({ "all": false, "company_ids": companies }),
            )
            .await;
        self.anon.sign_in(name, "analyst password 1").await
    }

    async fn problem(&self, client: &Client, company: &str, title: &str) -> Value {
        let r = client
            .post(
                "/api/v1/problems",
                json!({ "company_id": company, "title": title }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    async fn ticket(&self, company: &str, subject: &str) -> String {
        let r = self
            .admin
            .post(
                "/api/v1/tickets",
                json!({ "company_id": company, "subject": subject }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn problems_move_from_new_to_resolved_with_a_known_error_and_a_history(pool: PgPool) {
    let w = World::new(&pool).await;
    let p = w
        .problem(&w.admin, &w.company, "Printers drop off the network")
        .await;
    assert_eq!(p["number"], "PRB-00001");
    assert_eq!(p["status_id"], NEW);
    assert_eq!(p["priority_id"], MEDIUM);
    assert_eq!(
        w.problem(&w.admin, &w.company, "Second").await["number"],
        "PRB-00002"
    );
    let path = format!("/api/v1/problems/{}", id(&p));

    let r = w
        .admin
        .put(
            &path,
            json!({ "title": "Printers drop off", "is_known_error": true }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a Known Error needs a Root Cause or Workaround"
    );
    let r = w
        .admin
        .put(&path, json!({ "title": "Printers drop off", "root_cause": "DHCP lease time", "workaround": "Reserve IPs", "is_known_error": true }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["is_known_error"], true);
    let known = w.admin.get("/api/v1/problems?known_error=true").await.body;
    assert_eq!(known["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        w.admin.get("/api/v1/problems?q=DHCP").await.body["items"][0]["id"],
        p["id"]
    );

    let note = w
        .admin
        .post(
            &format!("{path}/notes"),
            json!({ "body": "Seen on floor 3" }),
        )
        .await;
    assert_eq!(note.status, StatusCode::CREATED);
    assert_eq!(
        w.admin.get(&format!("{path}/notes")).await.body[0]["body"],
        "Seen on floor 3"
    );

    assert_eq!(
        w.admin
            .post(
                &format!("{path}/status"),
                json!({ "status_id": INVESTIGATING })
            )
            .await
            .status,
        StatusCode::OK
    );
    let resolved = w
        .admin
        .post(&format!("{path}/status"), json!({ "status_id": RESOLVED }))
        .await
        .body;
    assert!(resolved["closed_at"].is_string());
    let events: Vec<String> = sqlx::query_scalar("SELECT event_type FROM outbox_events WHERE aggregate_type = 'problem' ORDER BY occurred_at")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        events.contains(&"problem.resolved".to_owned()),
        "{events:?}"
    );
    let reopened = w
        .admin
        .post(
            &format!("{path}/status"),
            json!({ "status_id": INVESTIGATING }),
        )
        .await
        .body;
    assert_eq!(reopened["closed_at"], Value::Null);

    let history = w.admin.get(&format!("{path}/history")).await.body;
    let fields: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["field"].as_str())
        .collect();
    for f in [
        "root_cause",
        "workaround",
        "is_known_error",
        "status_id",
        "closed_at",
    ] {
        assert!(fields.contains(&f), "{f} is audited: {fields:?}");
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn linked_tickets_show_on_both_sides_and_can_be_closed_once_resolved(pool: PgPool) {
    let w = World::new(&pool).await;
    let p = w.problem(&w.admin, &w.company, "VPN outage").await;
    let path = format!("/api/v1/problems/{}", id(&p));
    let a = w.ticket(&w.company, "VPN down for Sam").await;
    let b = w.ticket(&w.company, "VPN down for Jo").await;
    for t in [&a, &b] {
        let r = w
            .admin
            .post(&format!("{path}/tickets"), json!({ "ticket_id": t }))
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }
    assert_eq!(
        w.admin
            .post(&format!("{path}/tickets"), json!({ "ticket_id": a }))
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        w.admin
            .get(&format!("{path}/tickets"))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        w.admin
            .get(&format!("/api/v1/problems?ticket_id={a}"))
            .await
            .body["items"][0]["id"],
        p["id"],
        "the ticket's side"
    );

    let close = json!({ "action": "close", "resolution_code_id": FIXED });
    assert_eq!(
        w.admin
            .post(&format!("{path}/tickets/actions"), close.clone())
            .await
            .status,
        StatusCode::CONFLICT,
        "resolve the Problem first"
    );
    w.admin
        .post(&format!("{path}/status"), json!({ "status_id": RESOLVED }))
        .await;
    let note = w
        .admin
        .post(
            &format!("{path}/tickets/actions"),
            json!({ "action": "note", "body": "Fixed by the VPN update", "public": true }),
        )
        .await;
    assert_eq!(note.status, StatusCode::OK, "{:?}", note.body);
    assert!(
        note.body
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["ok"] == true)
    );
    let r = w
        .admin
        .post(&format!("{path}/tickets/actions"), close)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    for t in [&a, &b] {
        assert!(w.admin.get(&format!("/api/v1/tickets/{t}")).await.body["closed_at"].is_string());
    }

    assert_eq!(
        w.admin
            .send(Method::DELETE, &format!("{path}/tickets/{b}"), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        w.admin
            .send(Method::DELETE, &format!("{path}/tickets/{b}"), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn problems_stay_in_their_company(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w
        .admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
        )
        .await
        .body["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let caps = [
        "problems.read",
        "problems.create",
        "problems.update",
        "tickets.read",
    ];
    let ours = w.analyst("ours", &caps, &[&w.company]).await;
    let theirs = w.analyst("theirs", &caps, &[&other]).await;
    let p = w.problem(&ours, &w.company, "Ours").await;
    let their_ticket = w.ticket(&other, "Theirs").await;

    assert_eq!(
        theirs
            .get(&format!("/api/v1/problems/{}", id(&p)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        theirs.get("/api/v1/problems").await.body["items"],
        json!([])
    );
    assert_eq!(
        theirs
            .post(
                "/api/v1/problems",
                json!({ "company_id": w.company, "title": "Sneaky" })
            )
            .await
            .status,
        StatusCode::NOT_FOUND,
        "cannot create in a Company you cannot see"
    );
    assert_eq!(
        ours.post(
            &format!("/api/v1/problems/{}/tickets", id(&p)),
            json!({ "ticket_id": their_ticket })
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "a ticket you cannot see"
    );
    assert_eq!(
        w.admin
            .post(
                &format!("/api/v1/problems/{}/tickets", id(&p)),
                json!({ "ticket_id": their_ticket })
            )
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "links never cross Companies"
    );

    let reader = w.analyst("reader", &["tickets.read"], &[&w.company]).await;
    assert_eq!(
        reader.get("/api/v1/problems").await.status,
        StatusCode::FORBIDDEN
    );

    assert_eq!(
        w.admin
            .put("/api/v1/modules/problems", json!({ "is_enabled": false }))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        ours.get("/api/v1/problems").await.status,
        StatusCode::NOT_FOUND,
        "the Module is off"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn statuses_and_priorities_are_configurable_with_one_default(pool: PgPool) {
    let w = World::new(&pool).await;
    let r = w
        .admin
        .post(
            "/api/v1/problem-priorities",
            json!({ "name": "Urgent", "is_default": true, "sort_order": 50 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let defaults: Vec<Value> = w
        .admin
        .get("/api/v1/problem-priorities")
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["is_default"] == true)
        .cloned()
        .collect();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0]["name"], "Urgent");
    assert_eq!(
        w.problem(&w.admin, &w.company, "x").await["priority_id"],
        r.body["id"]
    );
    assert_eq!(
        w.admin
            .post("/api/v1/problem-statuses", json!({ "name": "new" }))
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        w.admin
            .put(
                &format!("/api/v1/problem-priorities/{}", id(&r.body)),
                json!({ "name": "Urgent", "is_default": false })
            )
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "make another row the default instead"
    );
}
