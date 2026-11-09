//! Change Management end to end (ADR-0027): the stage order, risk scoring, CAB approval
//! rounds with notifications, material edits that restart approval, Standard and
//! Emergency Changes, templates with versions, the calendar, checklist, comments, links,
//! text revisions, the PIR, attachments, Company isolation and the Changes Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use common::{Client, setup, test_settings};
use cooper_kernel::SystemClock;
use cooper_platform::outbox::Dispatcher;
use cooper_server::AppState;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

const STANDARD: &str = "019a0000-0000-7000-8000-000000000a01";
const NORMAL: &str = "019a0000-0000-7000-8000-000000000a02";
const EMERGENCY: &str = "019a0000-0000-7000-8000-000000000a03";
const DRAFT: &str = "019a0000-0000-7000-8000-000000000a11";
const PENDING_APPROVAL: &str = "019a0000-0000-7000-8000-000000000a13";
const APPROVED: &str = "019a0000-0000-7000-8000-000000000a14";
const SCHEDULED: &str = "019a0000-0000-7000-8000-000000000a16";
const COMPLETED: &str = "019a0000-0000-7000-8000-000000000a18";
const MEDIUM: &str = "019a0000-0000-7000-8000-000000000a22";

const CAPS: &[&str] = &[
    "changes.read",
    "changes.create",
    "changes.update",
    "changes.approve",
    "tickets.read",
];

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
            .post(
                "/api/v1/analysts",
                json!({ "username": name, "email": format!("{name}@default.example"),
                        "full_name": name, "password": "analyst password 1" }),
            )
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

    async fn change(&self, client: &Client, body: Value) -> Value {
        let mut body = body;
        body["company_id"] = json!(self.company);
        let r = client.post("/api/v1/changes", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    async fn dispatch(&self) {
        let state =
            AppState::with_settings(self.pool.clone(), Arc::new(SystemClock), test_settings())
                .unwrap();
        let dispatcher = state.event_handlers().into_iter().fold(
            Dispatcher::new(self.pool.clone(), Arc::new(SystemClock)),
            Dispatcher::with_handler,
        );
        let stats = dispatcher.dispatch_once().await.unwrap();
        assert_eq!(stats.retried + stats.dead, 0, "{stats:?}");
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

async fn move_to(client: &Client, change: &str, status: &str) -> common::Reply {
    client
        .post(
            &format!("/api/v1/changes/{change}/status"),
            json!({ "status_id": status }),
        )
        .await
}

async fn kinds(client: &Client) -> Vec<String> {
    client.get("/api/v1/notifications").await.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["kind"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test(migrations = "../../migrations")]
async fn a_cab_change_is_approved_by_its_members_round_by_round(pool: PgPool) {
    let w = World::new(&pool).await;
    let company = w.company.clone();
    let owner = w.analyst("olga", CAPS, &[&company]).await;
    let alice = w.analyst("alice", CAPS, &[&company]).await;
    let bob = w.analyst("bob", CAPS, &[&company]).await;
    let eve = w.analyst("eve", CAPS, &[&company]).await;

    let c = w
        .change(
            &owner.client,
            json!({ "title": "Upgrade core switch", "risk_likelihood": 3, "risk_impact_score": 4,
                    "requested_by_id": owner.id, "assigned_to_id": owner.id, "cab_required": true,
                    "description": "Replace firmware" }),
        )
        .await;
    assert_eq!(c["number"], "CHG-00001");
    assert_eq!(c["status_id"], DRAFT);
    assert_eq!(c["stage"], "draft");
    assert_eq!(c["type_id"], NORMAL, "default Change Type");
    assert_eq!(c["priority_id"], MEDIUM);
    assert_eq!(c["risk_score"], 12);
    assert_eq!(c["risk_level"], "High");
    assert_eq!(c["cab_approval_type"], "all");
    assert_eq!(c["approval_round"], 0);
    let cid = id(&c).to_owned();
    let path = format!("/api/v1/changes/{cid}");

    let r = move_to(&owner.client, &cid, PENDING_APPROVAL).await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "a CAB Change needs members first"
    );
    let r = owner
        .client
        .put(
            &format!("{path}/cab"),
            json!([{ "analyst_id": alice.id }, { "analyst_id": bob.id }]),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = move_to(&owner.client, &cid, APPROVED).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "only votes approve");

    let r = move_to(&owner.client, &cid, PENDING_APPROVAL).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["stage"], "awaiting");
    assert_eq!(r.body["approval_round"], 1);
    let r = owner.client.put(&format!("{path}/cab"), json!([])).await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "the CAB is fixed once submitted"
    );
    w.dispatch().await;
    assert_eq!(kinds(&alice.client).await, ["change_submitted"]);
    assert_eq!(kinds(&bob.client).await, ["change_submitted"]);
    assert!(kinds(&eve.client).await.is_empty(), "not a CAB member");

    let awaiting = alice
        .client
        .get("/api/v1/changes?awaiting_my_vote=true")
        .await
        .body;
    assert_eq!(awaiting["items"].as_array().unwrap().len(), 1);
    let r = eve
        .client
        .post(&format!("{path}/votes"), json!({ "decision": "approve" }))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "only CAB members vote");
    let r = alice
        .client
        .post(&format!("{path}/votes"), json!({ "decision": "approve" }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["stage"], "awaiting", "Bob has not voted yet");
    let r = alice
        .client
        .post(
            &format!("{path}/votes"),
            json!({ "decision": "reject", "comment": "x" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "one vote per round");
    assert!(
        alice
            .client
            .get("/api/v1/changes?awaiting_my_vote=true")
            .await
            .body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let r = bob
        .client
        .post(&format!("{path}/votes"), json!({ "decision": "reject" }))
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a rejection needs a reason"
    );
    let r = bob
        .client
        .post(
            &format!("{path}/votes"),
            json!({ "decision": "reject", "comment": "No rollback plan" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["stage"], "draft",
        "rejected Changes go back to Draft"
    );
    w.dispatch().await;
    assert_eq!(kinds(&owner.client).await, ["change_rejected"]);

    // Round 2: fix the plan and resubmit; both approve.
    let r = owner
        .client
        .put(&path, json!({ "title": "Upgrade core switch", "cab_required": true, "rollback_plan": "Reload old image",
                            "requested_by_id": owner.id, "assigned_to_id": owner.id }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = move_to(&owner.client, &cid, PENDING_APPROVAL).await;
    assert_eq!(r.body["approval_round"], 2);
    alice
        .client
        .post(&format!("{path}/votes"), json!({ "decision": "approve" }))
        .await;
    let r = bob
        .client
        .post(&format!("{path}/votes"), json!({ "decision": "approve" }))
        .await;
    assert_eq!(r.body["stage"], "approved", "{:?}", r.body);
    assert_eq!(r.body["status_id"], APPROVED);
    assert!(r.body["approved_at"].is_string());
    let round1 = owner
        .client
        .get(&format!("{path}/votes?round=1"))
        .await
        .body;
    assert_eq!(
        round1.as_array().unwrap().len(),
        2,
        "earlier rounds are kept"
    );

    // A material edit after approval restarts approval with a new round.
    let r = owner
        .client
        .put(&path, json!({ "title": "Upgrade core switch", "cab_required": true, "rollback_plan": "Reload old image",
                            "test_plan": "Ping all VLANs", "requested_by_id": owner.id, "assigned_to_id": owner.id }))
        .await;
    assert_eq!(r.body["stage"], "awaiting", "{:?}", r.body);
    assert_eq!(r.body["approval_round"], 3);
    assert!(r.body["approved_at"].is_null());
    alice
        .client
        .post(&format!("{path}/votes"), json!({ "decision": "approve" }))
        .await;
    bob.client
        .post(&format!("{path}/votes"), json!({ "decision": "approve" }))
        .await;

    let r = move_to(&owner.client, &cid, SCHEDULED).await;
    assert_eq!(r.body["stage"], "implementing", "{:?}", r.body);
    let r = owner
        .client
        .put(&path, json!({ "title": "Too late" }))
        .await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "no edits while implementing"
    );
    let r = owner
        .client
        .put(&format!("{path}/pir"), json!({ "successful": true }))
        .await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "the PIR comes after completion"
    );
    let r = move_to(&owner.client, &cid, COMPLETED).await;
    assert_eq!(r.body["stage"], "done");
    assert!(r.body["closed_at"].is_string());
    let r = owner
        .client
        .put(
            &format!("{path}/pir"),
            json!({ "successful": true, "lessons_learned": "Pre-stage firmware" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["pir"]["lessons_learned"], "Pre-stage firmware");

    // Text revisions: every saved version of a long text field, comparable.
    let revs = owner
        .client
        .get(&format!("{path}/revisions/rollback_plan"))
        .await
        .body;
    let revs = revs.as_array().unwrap();
    assert_eq!(revs.len(), 1, "{revs:?}");
    let desc = owner
        .client
        .get(&format!("{path}/revisions/test_plan"))
        .await
        .body;
    assert_eq!(desc.as_array().unwrap().len(), 1);

    // History is audited.
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_entries WHERE aggregate_type = 'change' AND aggregate_id = $1",
    )
    .bind(uuid::Uuid::parse_str(&cid).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(audit >= 10, "{audit}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn standard_changes_skip_approval_and_emergency_changes_are_approved_afterwards(
    pool: PgPool,
) {
    let w = World::new(&pool).await;
    let normal = w.change(&w.admin, json!({ "title": "Normal" })).await;
    let r = move_to(&w.admin, id(&normal), SCHEDULED).await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "a Normal Change needs approval first"
    );

    let standard = w
        .change(
            &w.admin,
            json!({ "title": "Patch Tuesday", "type_id": STANDARD }),
        )
        .await;
    let r = move_to(&w.admin, id(&standard), SCHEDULED).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["stage"], "implementing");

    let approver = w.analyst("ann", CAPS, &[&w.company]).await;
    let emergency = w
        .change(
            &w.admin,
            json!({ "title": "Mitigate CVE", "type_id": EMERGENCY, "approver_id": approver.id }),
        )
        .await;
    let eid = id(&emergency);
    move_to(&w.admin, eid, PENDING_APPROVAL).await;
    let r = move_to(&w.admin, eid, SCHEDULED).await;
    assert_eq!(
        r.body["stage"], "implementing",
        "Emergency Changes start before approval: {:?}",
        r.body
    );
    let r = w
        .admin
        .post(
            &format!("/api/v1/changes/{eid}/votes"),
            json!({ "decision": "approve" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::FORBIDDEN,
        "only the named Approver decides"
    );
    let r = approver
        .client
        .post(
            &format!("/api/v1/changes/{eid}/votes"),
            json!({ "decision": "approve" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["stage"], "implementing",
        "approval afterwards keeps it under way"
    );
    assert!(r.body["approved_at"].is_string());
}

#[sqlx::test(migrations = "../../migrations")]
async fn templates_prefill_changes_and_keep_their_versions(pool: PgPool) {
    let w = World::new(&pool).await;
    let t = w
        .admin
        .post(
            "/api/v1/change-templates",
            json!({ "name": "Firewall rule", "content": { "title": "Open port", "type_id": STANDARD,
                    "test_plan": "Telnet from client", "risk_likelihood": 1, "risk_impact_score": 2 } }),
        )
        .await;
    assert_eq!(t.status, StatusCode::CREATED, "{:?}", t.body);
    assert_eq!(t.body["version"], 1);
    let tid = id(&t.body).to_owned();
    let c = w
        .change(
            &w.admin,
            json!({ "template_id": tid, "description": "Port 8443 for app X" }),
        )
        .await;
    assert_eq!(c["title"], "Open port");
    assert_eq!(c["type_id"], STANDARD);
    assert_eq!(c["test_plan"], "Telnet from client");
    assert_eq!(
        c["description"], "Port 8443 for app X",
        "what the Analyst typed wins"
    );
    assert_eq!(c["risk_level"], "Low");
    assert_eq!(c["template_version"], 1);

    let r = w
        .admin
        .put(
            &format!("/api/v1/change-templates/{tid}"),
            json!({ "name": "Firewall rule", "content": { "title": "Open a port" } }),
        )
        .await;
    assert_eq!(r.body["version"], 2, "{:?}", r.body);
    let r = w
        .admin
        .put(
            &format!("/api/v1/change-templates/{tid}"),
            json!({ "name": "Firewall rules", "content": { "title": "Open a port" } }),
        )
        .await;
    assert_eq!(r.body["version"], 2, "renaming keeps the version");
    let again = w
        .admin
        .get(&format!("/api/v1/changes/{}", id(&c)))
        .await
        .body;
    assert_eq!(
        again["template_version"], 1,
        "Changes keep the version they came from"
    );

    // Team-restricted templates are invisible to other Analysts.
    let team = w
        .admin
        .post("/api/v1/teams", json!({ "name": "Network" }))
        .await
        .body;
    w.admin
        .put(&format!("/api/v1/change-templates/{tid}"), json!({ "name": "Firewall rules", "team_ids": [team["id"]], "content": { "title": "Open a port" } }))
        .await;
    let outsider = w.analyst("otto", CAPS, &[&w.company]).await;
    assert!(
        outsider
            .client
            .get("/api/v1/change-templates")
            .await
            .body
            .as_array()
            .unwrap()
            .is_empty()
    );
    let r = outsider
        .client
        .post(
            "/api/v1/changes",
            json!({ "company_id": w.company, "template_id": tid }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../../migrations")]
async fn the_calendar_shows_overlapping_windows(pool: PgPool) {
    let w = World::new(&pool).await;
    let a = w
        .change(&w.admin, json!({ "title": "A", "work_start": "2026-12-01T20:00:00Z", "work_end": "2026-12-01T23:00:00Z" }))
        .await;
    let b = w
        .change(&w.admin, json!({ "title": "B", "outage_start": "2026-12-01T22:00:00Z", "outage_end": "2026-12-02T01:00:00Z" }))
        .await;
    w.change(&w.admin, json!({ "title": "C", "work_start": "2026-12-03T20:00:00Z", "work_end": "2026-12-03T21:00:00Z" }))
        .await;
    let r = w
        .admin
        .post("/api/v1/changes", json!({ "company_id": w.company, "title": "Bad", "work_start": "2026-12-03T20:00:00Z", "work_end": "2026-12-03T19:00:00Z" }))
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a window ends after it starts"
    );

    let cal = w
        .admin
        .get("/api/v1/change-calendar?from=2026-12-01T00:00:00Z&to=2026-12-02T00:00:00Z")
        .await
        .body;
    assert_eq!(cal["changes"].as_array().unwrap().len(), 2, "{cal:?}");
    let overlaps = cal["overlaps"].as_array().unwrap();
    assert_eq!(overlaps.len(), 1);
    let pair = [
        overlaps[0]["change_id"].clone(),
        overlaps[0]["other_change_id"].clone(),
    ];
    assert!(pair.contains(&a["id"]) && pair.contains(&b["id"]));
    let r = w
        .admin
        .get("/api/v1/change-calendar?from=2026-01-01T00:00:00Z&to=2027-06-01T00:00:00Z")
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "at most 366 days"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn changes_carry_checklists_comments_links_and_attachments(pool: PgPool) {
    let w = World::new(&pool).await;
    let c = w
        .change(
            &w.admin,
            json!({ "title": "Move file server", "description": "one\ntwo" }),
        )
        .await;
    let cid = id(&c).to_owned();
    let path = format!("/api/v1/changes/{cid}");

    let item = w
        .admin
        .post(
            &format!("{path}/checklist"),
            json!({ "text": "Snapshot VM" }),
        )
        .await;
    assert_eq!(item.status, StatusCode::CREATED, "{:?}", item.body);
    let r = w
        .admin
        .put(
            &format!("{path}/checklist/{}", id(&item.body)),
            json!({ "text": "Snapshot VM", "is_done": true }),
        )
        .await;
    assert_eq!(r.body["is_done"], true);
    assert!(r.body["done_by_id"].is_string());
    w.admin
        .post(&format!("{path}/checklist"), json!({ "text": "Remove" }))
        .await;
    let list = w.admin.get(&format!("{path}/checklist")).await.body;
    let second = list.as_array().unwrap()[1]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = w
        .admin
        .send(Method::DELETE, &format!("{path}/checklist/{second}"), None)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        w.admin
            .get(&format!("{path}/checklist"))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let r = w
        .admin
        .post(
            &format!("{path}/comments"),
            json!({ "body": "Scheduled with facilities" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(
        w.admin.get(&format!("{path}/comments")).await.body[0]["body"],
        "Scheduled with facilities"
    );

    let ticket = w
        .admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": w.company, "subject": "File server slow" }),
        )
        .await
        .body;
    let problem = w
        .admin
        .post(
            "/api/v1/problems",
            json!({ "company_id": w.company, "title": "Disk latency" }),
        )
        .await
        .body;
    let other = w.change(&w.admin, json!({ "title": "Related" })).await;
    for (kind, target) in [
        ("ticket", &ticket),
        ("problem", &problem),
        ("change", &other),
    ] {
        let r = w
            .admin
            .post(
                &format!("{path}/links"),
                json!({ "kind": kind, "target_id": target["id"] }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{kind}: {:?}", r.body);
    }
    let r = w
        .admin
        .post(
            &format!("{path}/links"),
            json!({ "kind": "change", "target_id": cid }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "not to itself");
    let r = w
        .admin
        .post(
            &format!("{path}/links"),
            json!({ "kind": "ticket", "target_id": uuid::Uuid::now_v7() }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let links = w.admin.get(&format!("{path}/links")).await.body;
    let numbers: Vec<&str> = links
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["number"].as_str().unwrap())
        .collect();
    assert_eq!(
        numbers,
        [ticket["number"].as_str().unwrap(), "PRB-00001", "CHG-00002"]
    );
    let by_ticket = w
        .admin
        .get(&format!("/api/v1/changes?ticket_id={}", id(&ticket)))
        .await
        .body;
    assert_eq!(
        by_ticket["items"][0]["id"],
        cid.as_str(),
        "the ticket's side of the link"
    );
    let r = w
        .admin
        .send(
            Method::DELETE,
            &format!("{path}/links/problem/{}", id(&problem)),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        w.admin
            .get(&format!("{path}/links"))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Revisions and comparison.
    w.admin
        .put(
            &path,
            json!({ "title": "Move file server", "description": "one\nthree" }),
        )
        .await;
    let revs = w
        .admin
        .get(&format!("{path}/revisions/description"))
        .await
        .body;
    let revs = revs.as_array().unwrap();
    assert_eq!(revs.len(), 2);
    let diff = w
        .admin
        .get(&format!(
            "{path}/revisions/description/compare?from={}&to={}",
            id(&revs[0]),
            id(&revs[1])
        ))
        .await
        .body;
    let lines: Vec<(String, String)> = diff
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["kind"].as_str().unwrap().to_owned(),
                l["text"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            ("same".into(), "one".into()),
            ("removed".into(), "two".into()),
            ("added".into(), "three".into())
        ]
    );

    // Attachments.
    let boundary = "cooperboundary42";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"../plan.pdf\"\r\nContent-Type: text/html\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(b"%PDF-1.7 plan");
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let (status, _, bytes) = w
        .admin
        .send_raw(
            Method::POST,
            &format!("{path}/attachments"),
            &format!("multipart/form-data; boundary={boundary}"),
            body,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let a: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(a["file_name"], "plan.pdf");
    assert_eq!(a["media_type"], "application/pdf");
    let (status, headers, content) = w
        .admin
        .send_raw(
            Method::GET,
            &format!("{path}/attachments/{}/content", id(&a)),
            "text/plain",
            Vec::new(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content, b"%PDF-1.7 plan");
    assert!(
        headers["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn changes_are_isolated_by_company_and_switched_off_with_the_module(pool: PgPool) {
    let w = World::new(&pool).await;
    let c = w
        .change(&w.admin, json!({ "title": "Default Company change" }))
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
    let r = outsider
        .client
        .get(&format!("/api/v1/changes/{}", id(&c)))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert!(
        outsider.client.get("/api/v1/changes").await.body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let r = outsider
        .client
        .post(
            "/api/v1/changes",
            json!({ "company_id": w.company, "title": "x" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::NOT_FOUND,
        "cannot create in an invisible Company"
    );
    let r = outsider
        .client
        .post(
            &format!("/api/v1/changes/{}/comments", id(&c)),
            json!({ "body": "hi" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    let reader = w.analyst("rita", &["changes.read"], &[&w.company]).await;
    let r = reader
        .client
        .post(
            "/api/v1/changes",
            json!({ "company_id": w.company, "title": "x" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = reader
        .client
        .post("/api/v1/change-types", json!({ "name": "Major" }))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = w
        .admin
        .post(
            "/api/v1/change-types",
            json!({ "name": "Major", "approval_route": "review" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);

    let r = w
        .admin
        .put(
            "/api/v1/change-risk-levels",
            json!([{ "name": "All", "min_score": 1, "max_score": 20 }]),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "bands cover 1-25"
    );
    let r = w
        .admin
        .put("/api/v1/change-risk-levels", json!([{ "name": "Low", "min_score": 1, "max_score": 9 }, { "name": "High", "min_score": 10, "max_score": 25 }]))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let r = w
        .admin
        .put("/api/v1/modules/changes", json!({ "is_enabled": false }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        w.admin.get("/api/v1/changes").await.status,
        StatusCode::NOT_FOUND,
        "the Module is off"
    );
    assert_eq!(
        w.admin
            .get("/api/v1/change-calendar?from=2026-01-01T00:00:00Z&to=2026-02-01T00:00:00Z")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}
