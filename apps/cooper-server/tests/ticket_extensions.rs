//! Ticket extensions end to end (ADR-0024): Ticket Number History, Ticket Type Rules,
//! snooze, Ticket Presence, Merge and Split with undo, Ticket Links, Ticket Fields and
//! bulk actions, including tenant isolation and Confidential checks.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use common::{Client, setup};
use cooper_kernel::SystemClock;
use cooper_server::AppState;
use cooper_service_desk::application::IntakeOutcome;
use cooper_service_desk::domain::InboundEmail;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const INCIDENT: &str = "019a0000-0000-7000-8000-000000000301";
const SERVICE_REQUEST: &str = "019a0000-0000-7000-8000-000000000302";
const DUPLICATE: &str = "019a0000-0000-7000-8000-000000000508";
const FIXED: &str = "019a0000-0000-7000-8000-000000000501";

const DESK_CAPS: &[&str] = &[
    "tickets.read",
    "tickets.create",
    "tickets.update",
    "tickets.assign",
    "tickets.close",
    "tickets.merge",
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

    async fn company(&self, name: &str, code: &str) -> String {
        let r = self
            .admin
            .post(
                "/api/v1/companies",
                json!({ "name": name, "ticket_code": code, "sender_domains": [format!("{}.example", code.to_lowercase())] }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }

    async fn analyst(&self, username: &str, caps: &[&str], companies: &[&str]) -> (String, Client) {
        let created = self.admin.post("/api/v1/analysts", json!({
            "username": username, "email": format!("{username}@default.example"), "full_name": username,
            "password": "analyst password 1"
        })).await;
        let id = created.body["id"].as_str().unwrap().to_owned();
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
        self.admin
            .put(
                &format!("/api/v1/analysts/{id}/company-access"),
                json!({ "all": false, "company_ids": companies }),
            )
            .await;
        (id, self.anon.sign_in(username, "analyst password 1").await)
    }

    async fn ticket_in(&self, client: &Client, company: &str, body: Value) -> Value {
        let mut body = body;
        body["company_id"] = json!(company);
        let r = client.post("/api/v1/tickets", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    async fn ticket(&self, subject: &str) -> Value {
        self.ticket_in(&self.admin, &self.company, json!({ "subject": subject }))
            .await
    }

    async fn note(&self, ticket: &Value, body: &str) -> String {
        let r = self
            .admin
            .post(
                &format!("/api/v1/tickets/{}/notes", id(ticket)),
                json!({ "body": body }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }

    async fn notes_of(&self, ticket: &str) -> Vec<String> {
        let timeline = self
            .admin
            .get(&format!("/api/v1/tickets/{ticket}/timeline"))
            .await
            .body;
        timeline
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == "note")
            .map(|i| i["data"]["body"].as_str().unwrap_or_default().to_owned())
            .collect()
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

async fn closed_status(admin: &Client) -> String {
    admin
        .get("/api/v1/ticket-statuses")
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["is_closed"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[sqlx::test(migrations = "../../migrations")]
async fn renumbered_tickets_keep_resolving_by_their_old_number(pool: PgPool) {
    let w = World::new(&pool).await;
    let t = w.ticket("Old number").await;
    let old = t["number"].as_str().unwrap().to_owned();
    let (_, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    assert_eq!(
        desk.post(&format!("/api/v1/tickets/{}/renumber", id(&t)), json!({}))
            .await
            .status,
        StatusCode::FORBIDDEN,
        "renumbering needs tickets.configure"
    );
    let renumbered = w
        .admin
        .post(&format!("/api/v1/tickets/{}/renumber", id(&t)), json!({}))
        .await;
    assert_eq!(renumbered.status, StatusCode::OK, "{:?}", renumbered.body);
    assert_ne!(renumbered.body["number"], old.as_str());

    let by_old = desk.get(&format!("/api/v1/tickets/by-number/{old}")).await;
    assert_eq!(by_old.status, StatusCode::OK);
    assert_eq!(by_old.body["id"], t["id"]);
    let search = desk.get(&format!("/api/v1/tickets?q={old}")).await.body;
    assert_eq!(search["items"][0]["id"], t["id"], "found by its old number");
    let history = desk
        .get(&format!("/api/v1/tickets/{}/number-history", id(&t)))
        .await
        .body;
    assert_eq!(history[0]["number"], old.as_str());
    assert_eq!(history[0]["reason"], "renumbered");
}

#[sqlx::test(migrations = "../../migrations")]
async fn type_rules_choose_the_type_unless_one_was_chosen(pool: PgPool) {
    let w = World::new(&pool).await;
    let rule = w
        .admin
        .post(
            "/api/v1/ticket-type-rules",
            json!({ "name": "Outages are incidents", "ticket_type_id": INCIDENT,
                    "conditions": { "channels": ["analyst"], "subject_keywords": ["outage", "is down"] } }),
        )
        .await;
    assert_eq!(rule.status, StatusCode::CREATED, "{:?}", rule.body);
    assert_eq!(
        w.admin
            .post(
                "/api/v1/ticket-type-rules",
                json!({ "name": "Everything", "ticket_type_id": INCIDENT, "conditions": {} }),
            )
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a rule needs a condition"
    );

    assert_eq!(w.ticket("VPN IS DOWN").await["type_id"], INCIDENT);
    assert_eq!(w.ticket("New laptop").await["type_id"], Value::Null);
    let chosen = w
        .ticket_in(
            &w.admin,
            &w.company,
            json!({ "subject": "Outage report request", "type_id": SERVICE_REQUEST }),
        )
        .await;
    assert_eq!(chosen["type_id"], SERVICE_REQUEST, "a chosen type wins");

    // Emailed tickets: the rule is limited to the analyst channel.
    let state =
        AppState::with_settings(pool.clone(), Arc::new(SystemClock), common::test_settings())
            .unwrap();
    let company = w.company.clone();
    let mailbox = w.admin.post("/api/v1/mailboxes", json!({
        "name": "Desk", "address": "desk@localhost", "provider": "imap", "imap_host": "localhost",
        "imap_port": 3143, "imap_security": "none", "imap_username": "desk@localhost",
        "password": "x", "company_id": company
    })).await;
    assert_eq!(mailbox.status, StatusCode::CREATED, "{:?}", mailbox.body);
    let mb = state
        .service_desk
        .service
        .mailboxes(&cooper_kernel::Principal::automation())
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id().to_string() == mailbox.body["id"].as_str().unwrap())
        .unwrap();
    let IntakeOutcome::Created(emailed) = state
        .service_desk
        .service
        .import_email(&mb, email("sam@default.example", "Printer outage"))
        .await
        .unwrap()
    else {
        panic!("expected a new ticket")
    };
    assert_eq!(
        w.admin
            .get(&format!("/api/v1/tickets/{emailed}"))
            .await
            .body["type_id"],
        Value::Null
    );
}

fn email(from: &str, subject: &str) -> InboundEmail {
    InboundEmail {
        message_id: format!("<{}@mail.example>", Uuid::now_v7()),
        from_address: from.into(),
        from_name: Some("Sam Smith".into()),
        to: vec!["desk@localhost".into()],
        subject: subject.into(),
        text_body: "Hello".into(),
        ..InboundEmail::default()
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn snoozing_records_who_and_is_bounded(pool: PgPool) {
    let w = World::new(&pool).await;
    let (desk_id, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    let t = w.ticket("Waiting for parts").await;
    let until = chrono::Utc::now() + chrono::Duration::days(3);
    let r = desk
        .put(
            &format!("/api/v1/tickets/{}/snooze", id(&t)),
            json!({ "until": until, "reason": "supplier ships Tuesday" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["snoozed_by_id"], desk_id.as_str());
    assert!(r.body["snoozed_at"].is_string());
    let r = desk
        .put(
            &format!("/api/v1/tickets/{}/snooze", id(&t)),
            json!({ "until": chrono::Utc::now() + chrono::Duration::days(800) }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = desk
        .send(
            Method::DELETE,
            &format!("/api/v1/tickets/{}/snooze", id(&t)),
            None,
        )
        .await;
    assert_eq!(r.body["snoozed_by_id"], Value::Null);
}

#[sqlx::test(migrations = "../../migrations")]
async fn presence_shows_other_analysts_and_respects_company_access(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w.company("Other", "OTH").await;
    let (ann_id, ann) = w.analyst("ann", DESK_CAPS, &[&w.company]).await;
    let (bob_id, bob) = w.analyst("bob", DESK_CAPS, &[&w.company]).await;
    let (_, outsider) = w.analyst("outsider", DESK_CAPS, &[&other]).await;
    let t = w.ticket("Shared").await;
    let path = format!("/api/v1/tickets/{}/presence", id(&t));

    let seen = ann.put(&path, json!({})).await;
    assert_eq!(seen.status, StatusCode::OK);
    assert_eq!(seen.body, json!([]), "nobody else yet");
    let seen = bob.put(&path, json!({ "composing": true })).await.body;
    assert_eq!(seen[0]["analyst_id"], ann_id.as_str());
    let seen = ann.get(&path).await.body;
    assert_eq!(seen.as_array().unwrap().len(), 1);
    assert_eq!(seen[0]["analyst_id"], bob_id.as_str());
    assert_eq!(seen[0]["is_composing"], true);

    assert_eq!(
        bob.send(Method::DELETE, &path, None).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(ann.get(&path).await.body, json!([]));

    assert_eq!(
        outsider.put(&path, json!({})).await.status,
        StatusCode::NOT_FOUND,
        "another Company's ticket"
    );
    let admin_sees = w.admin.get(&path).await.body;
    assert_eq!(
        admin_sees[0]["analyst_id"],
        ann_id.as_str(),
        "Administrators see presence too"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn merge_moves_the_conversation_and_undo_restores_it(pool: PgPool) {
    let w = World::new(&pool).await;
    let (_, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    let source = w.ticket("Printer broken (duplicate)").await;
    let target = w.ticket("Printer broken").await;
    w.note(&source, "Source note").await;
    w.admin
        .post(
            &format!("/api/v1/tickets/{}/time-entries", id(&source)),
            json!({ "minutes": 10 }),
        )
        .await;

    let (_, reader) = w
        .analyst("reader", &["tickets.read", "tickets.update"], &[&w.company])
        .await;
    assert_eq!(
        reader
            .post(
                &format!("/api/v1/tickets/{}/merge", id(&source)),
                json!({ "target_ticket_id": id(&target) })
            )
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let merge = desk
        .post(
            &format!("/api/v1/tickets/{}/merge", id(&source)),
            json!({ "target_ticket_id": id(&target) }),
        )
        .await;
    assert_eq!(merge.status, StatusCode::CREATED, "{:?}", merge.body);
    assert_eq!(merge.body["moved"]["note_ids"].as_array().unwrap().len(), 1);
    assert_eq!(
        merge.body["moved"]["time_entry_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let s = desk
        .get(&format!("/api/v1/tickets/{}", id(&source)))
        .await
        .body;
    assert_eq!(s["merged_into_id"], target["id"]);
    assert!(s["closed_at"].is_string());
    assert_eq!(s["resolution_code_id"], DUPLICATE);
    assert_eq!(w.notes_of(id(&target)).await, ["Source note"]);
    assert!(w.notes_of(id(&source)).await.is_empty());
    let links = desk
        .get(&format!("/api/v1/tickets/{}/links", id(&source)))
        .await
        .body;
    assert_eq!(links[0]["relation"], "duplicate_of");

    assert_eq!(
        desk.post(
            &format!("/api/v1/tickets/{}/merge", id(&source)),
            json!({ "target_ticket_id": id(&target) })
        )
        .await
        .status,
        StatusCode::CONFLICT,
        "already merged"
    );

    let merge_id = merge.body["id"].as_str().unwrap();
    let undone = desk
        .post(&format!("/api/v1/ticket-merges/{merge_id}/undo"), json!({}))
        .await;
    assert_eq!(undone.status, StatusCode::OK, "{:?}", undone.body);
    let s = desk
        .get(&format!("/api/v1/tickets/{}", id(&source)))
        .await
        .body;
    assert_eq!(s["merged_into_id"], Value::Null);
    assert_eq!(s["closed_at"], Value::Null, "reopened exactly as it was");
    assert_eq!(w.notes_of(id(&source)).await, ["Source note"]);
    assert!(w.notes_of(id(&target)).await.is_empty());
    assert_eq!(
        desk.get(&format!("/api/v1/tickets/{}/links", id(&source)))
            .await
            .body,
        json!([])
    );
    assert_eq!(
        desk.post(&format!("/api/v1/ticket-merges/{merge_id}/undo"), json!({}))
            .await
            .status,
        StatusCode::CONFLICT
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn merges_never_cross_companies(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w.company("Other", "OTH").await;
    let mine = w.ticket("Mine").await;
    let theirs = w
        .ticket_in(&w.admin, &other, json!({ "subject": "Theirs" }))
        .await;
    let r = w
        .admin
        .post(
            &format!("/api/v1/tickets/{}/merge", id(&mine)),
            json!({ "target_ticket_id": id(&theirs) }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);

    let (_, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    assert_eq!(
        desk.post(
            &format!("/api/v1/tickets/{}/merge", id(&mine)),
            json!({ "target_ticket_id": id(&theirs) })
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "a ticket outside the Analyst's Companies does not exist for them"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn replies_to_a_merged_ticket_reach_the_survivor(pool: PgPool) {
    let w = World::new(&pool).await;
    let source = w.ticket("Duplicate").await;
    let target = w.ticket("Master").await;
    let r = w
        .admin
        .post(
            &format!("/api/v1/tickets/{}/merge", id(&source)),
            json!({ "target_ticket_id": id(&target) }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);

    let state =
        AppState::with_settings(pool.clone(), Arc::new(SystemClock), common::test_settings())
            .unwrap();
    let mailbox = w.admin.post("/api/v1/mailboxes", json!({
        "name": "Desk", "address": "desk@localhost", "provider": "imap", "imap_host": "localhost",
        "imap_port": 3143, "imap_security": "none", "imap_username": "desk@localhost",
        "password": "x", "company_id": w.company
    })).await;
    let mb = state
        .service_desk
        .service
        .mailboxes(&cooper_kernel::Principal::automation())
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id().to_string() == mailbox.body["id"].as_str().unwrap())
        .unwrap();
    let reply = email(
        "sam@default.example",
        &format!(
            "RE: [SDREF:{}] Duplicate",
            source["number"].as_str().unwrap()
        ),
    );
    let outcome = state
        .service_desk
        .service
        .import_email(&mb, reply)
        .await
        .unwrap();
    assert_eq!(
        outcome,
        IntakeOutcome::Appended(cooper_kernel::TicketId::from_uuid(
            id(&target).parse().unwrap()
        ))
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn split_moves_notes_to_a_new_ticket_and_undo_brings_them_back(pool: PgPool) {
    let w = World::new(&pool).await;
    let (_, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    let source = w.ticket("Printer and monitor").await;
    w.note(&source, "Printer is offline").await;
    let monitor = w.note(&source, "Also my monitor flickers").await;

    assert_eq!(
        desk.post(&format!("/api/v1/tickets/{}/split", id(&source)), json!({}))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "nothing selected"
    );
    assert_eq!(
        desk.post(
            &format!("/api/v1/tickets/{}/split", id(&source)),
            json!({ "note_ids": [Uuid::now_v7()] })
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "rows must be on the ticket"
    );
    let split = desk
        .post(
            &format!("/api/v1/tickets/{}/split", id(&source)),
            json!({ "note_ids": [monitor], "subject": "Monitor flickers" }),
        )
        .await;
    assert_eq!(split.status, StatusCode::CREATED, "{:?}", split.body);
    let new_id = split.body["new_ticket_id"].as_str().unwrap().to_owned();
    assert_eq!(split.body["new_ticket"]["subject"], "Monitor flickers");
    assert_eq!(w.notes_of(&new_id).await, ["Also my monitor flickers"]);
    let left = w.notes_of(id(&source)).await;
    assert_eq!(left.len(), 2);
    assert!(
        left.iter()
            .any(|n| n.starts_with("Moved 0 email(s) and 1 Note(s)"))
    );
    let links = desk
        .get(&format!("/api/v1/tickets/{}/links", id(&source)))
        .await
        .body;
    assert_eq!(links[0]["relation"], "related");
    assert_eq!(links[0]["ticket"]["id"], new_id.as_str());

    let split_id = split.body["id"].as_str().unwrap();
    let undone = desk
        .post(&format!("/api/v1/ticket-splits/{split_id}/undo"), json!({}))
        .await;
    assert_eq!(undone.status, StatusCode::OK, "{:?}", undone.body);
    assert_eq!(
        w.notes_of(id(&source)).await,
        ["Printer is offline", "Also my monitor flickers"]
    );
    assert_eq!(
        desk.get(&format!("/api/v1/tickets/{new_id}")).await.status,
        StatusCode::NOT_FOUND,
        "the new ticket is deleted"
    );

    // Once the new ticket has activity of its own, undo is refused.
    let again = w.note(&source, "Keyboard too").await;
    let split = desk
        .post(
            &format!("/api/v1/tickets/{}/split", id(&source)),
            json!({ "note_ids": [again] }),
        )
        .await
        .body;
    w.note(
        &json!({ "id": split["new_ticket_id"] }),
        "Working on the keyboard",
    )
    .await;
    assert_eq!(
        desk.post(
            &format!(
                "/api/v1/ticket-splits/{}/undo",
                split["id"].as_str().unwrap()
            ),
            json!({})
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn links_are_typed_directional_and_hide_tickets_you_cannot_see(pool: PgPool) {
    let w = World::new(&pool).await;
    let (_, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    let parent = w.ticket("Office move").await;
    let child = w.ticket("Move printers").await;
    let grandchild = w.ticket("Move the big printer").await;
    let path = |t: &Value| format!("/api/v1/tickets/{}/links", id(t));

    let r = desk
        .post(
            &path(&child),
            json!({ "ticket_id": id(&parent), "relation": "child_of" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["relation"], "child_of");
    let from_parent = desk.get(&path(&parent)).await.body;
    assert_eq!(from_parent[0]["relation"], "parent_of");
    assert_eq!(from_parent[0]["ticket"]["id"], child["id"]);

    assert_eq!(
        desk.post(
            &path(&grandchild),
            json!({ "ticket_id": id(&child), "relation": "child_of" })
        )
        .await
        .status,
        StatusCode::CREATED
    );
    let cycle = desk
        .post(
            &path(&parent),
            json!({ "ticket_id": id(&grandchild), "relation": "child_of" }),
        )
        .await;
    assert_eq!(
        cycle.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        cycle.body
    );
    let second_parent = desk
        .post(
            &path(&grandchild),
            json!({ "ticket_id": id(&parent), "relation": "child_of" }),
        )
        .await;
    assert_eq!(
        second_parent.status,
        StatusCode::CONFLICT,
        "{:?}",
        second_parent.body
    );
    assert_eq!(
        desk.post(
            &path(&parent),
            json!({ "ticket_id": id(&parent), "relation": "related" })
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        desk.post(
            &path(&parent),
            json!({ "ticket_id": id(&child), "relation": "cousin" })
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let related = desk
        .post(
            &path(&parent),
            json!({ "ticket_id": id(&grandchild), "relation": "related" }),
        )
        .await;
    assert_eq!(related.status, StatusCode::CREATED);
    assert_eq!(
        desk.post(
            &path(&grandchild),
            json!({ "ticket_id": id(&parent), "relation": "related" })
        )
        .await
        .status,
        StatusCode::CONFLICT,
        "related is symmetric"
    );

    // A link to a Confidential ticket is not shown to Analysts who cannot see it.
    let team = w
        .admin
        .post("/api/v1/teams", json!({ "name": "HR Team" }))
        .await
        .body;
    let dept = w.admin.post("/api/v1/departments", json!({ "name": "HR", "default_sensitivity": "confidential", "team_ids": [team["id"]] })).await.body;
    let secret = w
        .ticket_in(
            &w.admin,
            &w.company,
            json!({ "subject": "Grievance", "department_id": dept["id"] }),
        )
        .await;
    assert_eq!(
        w.admin
            .post(
                &path(&parent),
                json!({ "ticket_id": id(&secret), "relation": "related" })
            )
            .await
            .status,
        StatusCode::CREATED
    );
    let seen: Vec<Value> = desk
        .get(&path(&parent))
        .await
        .body
        .as_array()
        .unwrap()
        .clone();
    assert!(seen.iter().all(|l| l["ticket"]["id"] != secret["id"]));
    assert_eq!(
        w.admin
            .get(&path(&parent))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let link_id = related.body["id"].as_str().unwrap();
    assert_eq!(
        desk.send(Method::DELETE, &format!("{}/{link_id}", path(&child)), None)
            .await
            .status,
        StatusCode::NOT_FOUND,
        "the link is not one of this ticket's"
    );
    assert_eq!(
        desk.send(
            Method::DELETE,
            &format!("{}/{link_id}", path(&parent)),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    let other = w.company("Other", "OTH").await;
    let theirs = w
        .ticket_in(&w.admin, &other, json!({ "subject": "Theirs" }))
        .await;
    assert_eq!(
        w.admin
            .post(
                &path(&parent),
                json!({ "ticket_id": id(&theirs), "relation": "related" })
            )
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "links never cross Companies"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn ticket_fields_are_typed_and_required_when_configured(pool: PgPool) {
    let w = World::new(&pool).await;
    let field = |body: Value| {
        let admin = w.admin.clone();
        async move {
            let r = admin.post("/api/v1/ticket-fields", body).await;
            assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
            r.body
        }
    };
    field(json!({ "key": "asset_tag", "label": "Asset tag", "field_type": "text", "required_on_create": true })).await;
    let root = field(
        json!({ "key": "root_cause", "label": "Root cause", "field_type": "long_text",
                             "required_on_close": true, "type_ids": [INCIDENT] }),
    )
    .await;
    field(json!({ "key": "device", "label": "Device", "field_type": "dropdown", "options": ["Laptop", "Desktop"],
                  "ask_requester": true })).await;
    assert_eq!(
        w.admin
            .post(
                "/api/v1/ticket-fields",
                json!({ "key": "asset_tag", "label": "Again", "field_type": "text" })
            )
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        w.admin
            .put(
                &format!("/api/v1/ticket-fields/{}", id(&root)),
                json!({ "key": "root_cause", "label": "Root cause", "field_type": "text" })
            )
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the type never changes"
    );

    let missing = w
        .admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": w.company, "subject": "No tag" }),
        )
        .await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(missing.body["fields"], json!(["asset_tag"]));
    let bad = w
        .admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": w.company, "subject": "x",
        "fields": { "asset_tag": "A1", "device": "Tablet" } }),
        )
        .await;
    assert_eq!(
        bad.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        bad.body
    );
    let t = w
        .ticket_in(
            &w.admin,
            &w.company,
            json!({ "subject": "Incident", "type_id": INCIDENT,
        "fields": { "asset_tag": "A1", "device": "Laptop" } }),
        )
        .await;

    let values = w
        .admin
        .get(&format!("/api/v1/tickets/{}/fields", id(&t)))
        .await
        .body;
    let value = |key: &str| {
        values
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["field"]["key"] == key)
            .unwrap()["value"]
            .clone()
    };
    assert_eq!(value("asset_tag"), "A1");
    assert_eq!(value("root_cause"), Value::Null);

    let closed = closed_status(&w.admin).await;
    let status_path = format!("/api/v1/tickets/{}/status", id(&t));
    let close_body = json!({ "status_id": closed, "resolution_code_id": FIXED });
    let close = || w.admin.post(&status_path, close_body.clone());
    let refused = close().await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        refused.body
    );
    assert_eq!(refused.body["fields"], json!(["root_cause"]));
    let set = w
        .admin
        .put(
            &format!("/api/v1/tickets/{}/fields", id(&t)),
            json!({ "fields": { "root_cause": "Loose cable" } }),
        )
        .await;
    assert_eq!(set.status, StatusCode::OK, "{:?}", set.body);
    assert_eq!(close().await.status, StatusCode::OK);

    let history = w
        .admin
        .get(&format!("/api/v1/tickets/{}/timeline", id(&t)))
        .await
        .body;
    assert!(
        history.to_string().contains("fields.root_cause"),
        "field changes are audited"
    );

    // The Portal asks only fields marked ask_requester.
    let (_, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    let listed = desk.get("/api/v1/ticket-fields").await.body;
    assert_eq!(listed.as_array().unwrap().len(), 3);
    assert_eq!(
        desk.post(
            "/api/v1/ticket-fields",
            json!({ "key": "x", "label": "x", "field_type": "text" })
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn bulk_actions_report_each_ticket(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w.company("Other", "OTH").await;
    let (desk_id, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    let a = w.ticket("A").await;
    let b = w.ticket("B").await;
    let theirs = w
        .ticket_in(&w.admin, &other, json!({ "subject": "Theirs" }))
        .await;

    let r = desk
        .post(
            "/api/v1/tickets/bulk",
            json!({ "ticket_ids": [id(&a), id(&b), id(&theirs)],
            "action": "assign", "analyst_id": desk_id }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let results = r.body.as_array().unwrap();
    assert_eq!(results[0]["ok"], true);
    assert_eq!(results[1]["ok"], true);
    assert_eq!(results[2]["ok"], false);
    assert_eq!(
        results[2]["status"], 404,
        "another Company's ticket is not found"
    );
    assert_eq!(
        desk.get(&format!("/api/v1/tickets/{}", id(&a))).await.body["analyst_id"],
        desk_id.as_str()
    );

    let priorities = w.admin.get("/api/v1/ticket-priorities").await.body;
    let high = priorities
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "High")
        .unwrap()["id"]
        .clone();
    let r = desk
        .post(
            "/api/v1/tickets/bulk",
            json!({ "ticket_ids": [id(&a)], "action": "priority", "priority_id": high }),
        )
        .await;
    assert_eq!(r.body[0]["ok"], true, "{:?}", r.body);
    assert_eq!(
        desk.get(&format!("/api/v1/tickets/{}", id(&a))).await.body["priority_id"],
        high
    );

    let r = desk
        .post(
            "/api/v1/tickets/bulk",
            json!({ "ticket_ids": [id(&a), id(&b)], "action": "merge",
        "target_ticket_id": id(&b) }),
        )
        .await;
    assert_eq!(r.body[0]["ok"], true, "{:?}", r.body);
    assert_eq!(r.body[1]["ok"], true, "the target itself is skipped");

    let r = desk
        .post(
            "/api/v1/tickets/bulk",
            json!({ "ticket_ids": [id(&b)], "action": "close", "resolution_code_id": FIXED }),
        )
        .await;
    assert_eq!(r.body[0]["ok"], true, "{:?}", r.body);
    assert!(desk.get(&format!("/api/v1/tickets/{}", id(&b))).await.body["closed_at"].is_string());

    assert_eq!(
        desk.post(
            "/api/v1/tickets/bulk",
            json!({ "ticket_ids": [], "action": "assign" })
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        desk.post(
            "/api/v1/tickets/bulk",
            json!({ "ticket_ids": [id(&a)], "action": "explode" })
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
