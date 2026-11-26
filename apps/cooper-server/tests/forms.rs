//! Forms end to end (ADR-0039, ADR-0040): Forms with typed fields and kept versions;
//! Portal settings and audiences; Requesters filling in and submitting Forms with Drafts;
//! approval rounds and returns; Submission Actions; Capabilities and Company scoping.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use common::{Client, setup};
use http::{StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

const READER: &[&str] = &["forms.read"];
const WRITER: &[&str] = &["forms.read", "forms.write"];
const APPROVER: &[&str] = &["forms.read", "forms.approve"];
const PORTAL_PASSWORD: &str = "my portal password";

struct World {
    pool: PgPool,
    anon: Client,
    admin: Client,
    company: String,
}

/// The token of the newest queued email of `kind` to `to`.
async fn link_token(pool: &PgPool, kind: &str, to: &str) -> String {
    let body: String = sqlx::query_scalar(
        "SELECT text_body FROM outbound_emails WHERE kind = $1 AND $2 = ANY(to_addresses)
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(kind)
    .bind(to)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| panic!("no {kind} email to {to}: {e}"));
    let start = body.find("/portal/set-password?token=").expect("link") + 27;
    body[start..].chars().take_while(char::is_ascii_hexdigit).collect()
}

impl World {
    async fn new(pool: &PgPool) -> Self {
        let (anon, admin) = setup(pool).await;
        let company = admin.get("/api/v1/companies").await.body["items"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self { pool: pool.clone(), anon, admin, company }
    }

    /// An Analyst with `caps` and access to `companies`; returns its client and ID.
    async fn analyst(&self, name: &str, caps: &[&str], companies: &[&str]) -> (Client, String) {
        let a = self
            .admin
            .post("/api/v1/analysts", json!({ "username": name, "email": format!("{name}@default.example"), "full_name": name, "password": "analyst password 1" }))
            .await
            .body;
        let id = a["id"].as_str().unwrap().to_owned();
        let role = self
            .admin
            .post("/api/v1/roles", json!({ "name": format!("{name} role"), "capabilities": caps }))
            .await
            .body;
        self.admin
            .put(&format!("/api/v1/analysts/{id}/roles"), json!({ "role_ids": [role["id"]] }))
            .await;
        self.admin
            .put(
                &format!("/api/v1/analysts/{id}/company-access"),
                json!({ "all": false, "company_ids": companies }),
            )
            .await;
        (self.anon.sign_in(name, "analyst password 1").await, id)
    }

    async fn analyst_client(&self, name: &str, caps: &[&str], companies: &[&str]) -> Client {
        self.analyst(name, caps, companies).await.0
    }

    /// A Requester in the default Company, signed in to the Portal.
    async fn requester(&self, name: &str) -> (Client, String) {
        let email = format!("{name}@default.example");
        let r = self
            .admin
            .post("/api/v1/requesters", json!({ "company_id": self.company, "full_name": name, "email": email }))
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        let id = r.body["id"].as_str().unwrap().to_owned();
        let invite = self
            .admin
            .post(&format!("/api/v1/requesters/{id}/portal-invitation"), json!({}))
            .await;
        assert_eq!(invite.status, StatusCode::ACCEPTED, "{:?}", invite.body);
        let token = link_token(&self.pool, "portal_invitation", &email).await;
        let set = self
            .anon
            .post("/api/v1/portal/password", json!({ "token": token, "password": PORTAL_PASSWORD }))
            .await;
        assert_eq!(set.status, StatusCode::NO_CONTENT, "{:?}", set.body);
        let reply = self
            .anon
            .post("/api/v1/portal/session", json!({ "email": email, "password": PORTAL_PASSWORD }))
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
        let set_cookie = reply.headers[header::SET_COOKIE].to_str().unwrap();
        let client = Client {
            cookie: Some(set_cookie.split(';').next().unwrap().to_owned()),
            csrf: Some(reply.body["csrf_token"].as_str().unwrap().to_owned()),
            ..self.anon.clone()
        };
        (client, id)
    }
}

fn field(key: &str) -> Value {
    json!({ "key": key, "label": format!("Label {key}"), "kind": "text" })
}

fn form_body(company: &str, name: &str, fields: Vec<Value>) -> Value {
    json!({ "company_id": company, "name": name, "description": "", "fields": fields })
}

async fn create_form(w: &World, writer: &Client, name: &str, fields: Vec<Value>) -> String {
    let created = writer.post("/api/v1/forms", form_body(&w.company, name, fields)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    created.body["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrations = "../../migrations")]
async fn forms_keep_every_published_version(pool: PgPool) {
    let w = World::new(&pool).await;
    let writer = w.analyst_client("wendy", WRITER, &[&w.company]).await;

    let created = writer
        .post(
            "/api/v1/forms",
            form_body(
                &w.company,
                "Leave request",
                vec![
                    field("start"),
                    json!({ "key": "reason", "label": "Reason", "kind": "choice", "required": true, "options": ["Holiday", "Sick"], "width": "half" }),
                ],
            ),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let id = created.body["id"].as_str().unwrap().to_owned();
    assert_eq!(created.body["current_version"], 1);
    assert_eq!(created.body["fields"][1]["options"], json!(["Holiday", "Sick"]));
    assert_eq!(created.body["portal"]["visible"], false, "a new Form is not on the Portal");

    let published = writer
        .post(&format!("/api/v1/forms/{id}/versions"), json!({ "fields": [field("start"), field("end")] }))
        .await;
    assert_eq!(published.status, StatusCode::CREATED, "{:?}", published.body);
    assert_eq!(published.body["current_version"], 2);
    assert_eq!(published.body["versions"].as_array().unwrap().len(), 2);
    assert_eq!(published.body["versions"][0]["field_count"], 2);

    let renamed = writer
        .put(
            &format!("/api/v1/forms/{id}"),
            json!({ "name": "Holiday request", "description": "Annual leave", "is_active": false }),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{:?}", renamed.body);
    assert_eq!(renamed.body["current_version"], 2, "editing details publishes no version");
    assert_eq!(renamed.body["is_active"], false);

    let listed = writer
        .get(&format!("/api/v1/forms?company_id={}&active=false", w.company))
        .await
        .body;
    assert_eq!(listed["items"][0]["name"], "Holiday request");
}

#[sqlx::test(migrations = "../../migrations")]
async fn invalid_forms_are_rejected_with_a_reason(pool: PgPool) {
    let w = World::new(&pool).await;
    let writer = w.analyst_client("wendy", WRITER, &[&w.company]).await;

    let bad_key = writer.post("/api/v1/forms", form_body(&w.company, "Bad key", vec![field("1st")])).await;
    assert_eq!(bad_key.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", bad_key.body);
    assert_eq!(bad_key.body["field"], "key");

    let no_options = writer
        .post(
            "/api/v1/forms",
            form_body(&w.company, "No options", vec![json!({ "key": "pick", "label": "Pick", "kind": "choice" })]),
        )
        .await;
    assert_eq!(no_options.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", no_options.body);

    let unknown_kind = writer
        .post("/api/v1/forms", form_body(&w.company, "Unknown kind", vec![json!({ "key": "a", "label": "A", "kind": "file" })]))
        .await;
    assert_eq!(unknown_kind.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", unknown_kind.body);

    let first = writer.post("/api/v1/forms", form_body(&w.company, "Leave", vec![field("a")])).await;
    assert_eq!(first.status, StatusCode::CREATED);
    let same_name = writer.post("/api/v1/forms", form_body(&w.company, "leave", vec![field("a")])).await;
    assert_eq!(same_name.status, StatusCode::CONFLICT, "{:?}", same_name.body);
}

#[sqlx::test(migrations = "../../migrations")]
async fn only_staff_with_the_capability_can_build_and_see_forms(pool: PgPool) {
    let w = World::new(&pool).await;
    let reader = w.analyst_client("rita", READER, &[&w.company]).await;
    let writer = w.analyst_client("wendy", WRITER, &[&w.company]).await;

    let denied = reader.post("/api/v1/forms", form_body(&w.company, "Nope", vec![field("a")])).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "{:?}", denied.body);

    let id = create_form(&w, &writer, "Visible", vec![field("a")]).await;
    assert_eq!(reader.get(&format!("/api/v1/forms/{id}")).await.status, StatusCode::OK);
    assert_eq!(w.anon.get(&format!("/api/v1/forms/{id}")).await.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn forms_stay_inside_the_companies_the_analyst_can_see(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w
        .admin
        .post("/api/v1/companies", json!({ "name": "Other Ltd", "ticket_code": "OTH", "sender_domains": ["other.example"] }))
        .await
        .body;
    let other_id = other["id"].as_str().unwrap().to_owned();
    let writer = w.analyst_client("wendy", WRITER, &[&w.company]).await;

    let elsewhere = writer.post("/api/v1/forms", form_body(&other_id, "Elsewhere", vec![field("a")])).await;
    assert_eq!(elsewhere.status, StatusCode::NOT_FOUND, "{:?}", elsewhere.body);

    let admin_form = w.admin.post("/api/v1/forms", form_body(&other_id, "Other's form", vec![field("a")])).await;
    assert_eq!(admin_form.status, StatusCode::CREATED);
    let id = admin_form.body["id"].as_str().unwrap();
    assert_eq!(writer.get(&format!("/api/v1/forms/{id}")).await.status, StatusCode::NOT_FOUND);
    let list = writer.get("/api/v1/forms").await.body;
    assert!(list["items"].as_array().unwrap().iter().all(|f| f["company_id"] != other_id));
}

#[sqlx::test(migrations = "../../migrations")]
async fn requesters_fill_in_forms_on_the_portal_and_a_task_is_created(pool: PgPool) {
    let w = World::new(&pool).await;
    let writer = w.analyst_client("wendy", WRITER, &[&w.company]).await;
    let (sam, _) = w.requester("sam").await;

    let id = create_form(
        &w,
        &writer,
        "Onboarding",
        vec![
            json!({ "key": "name", "label": "Name", "kind": "text", "required": true }),
            json!({ "key": "start", "label": "Start", "kind": "date" }),
            json!({ "key": "team", "label": "Team", "kind": "choice", "options": ["Sales", "Ops"] }),
        ],
    )
    .await;
    let settings = writer
        .put(
            &format!("/api/v1/forms/{id}/settings"),
            json!({
                "portal": { "visible": true, "audiences": [] },
                "approval": null,
                "actions": { "task": { "title": "Onboard {name}", "description": "Starts {start}" } }
            }),
        )
        .await;
    assert_eq!(settings.status, StatusCode::OK, "{:?}", settings.body);
    assert_eq!(settings.body["portal"]["visible"], true);
    assert_eq!(writer.get("/api/v1/portal/forms").await.status, StatusCode::UNAUTHORIZED, "a staff session is not a Portal session");

    let listed = sam.get("/api/v1/portal/forms").await;
    assert_eq!(listed.status, StatusCode::OK, "{:?}", listed.body);
    assert_eq!(listed.body[0]["id"], id.as_str());
    assert!(listed.body[0].get("portal").is_none(), "Requesters do not see settings");

    let missing = sam
        .post(&format!("/api/v1/portal/forms/{id}/submissions"), json!({ "answers": { "start": "2026-12-01" } }))
        .await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", missing.body);

    let draft = sam
        .put(&format!("/api/v1/portal/forms/{id}/draft"), json!({ "answers": { "name": "Ann" } }))
        .await;
    assert_eq!(draft.status, StatusCode::OK, "{:?}", draft.body);
    let saved = sam.get(&format!("/api/v1/portal/forms/{id}/draft")).await.body;
    assert_eq!(saved["answers"]["name"], "Ann");

    let sent = sam
        .post(
            &format!("/api/v1/portal/forms/{id}/submissions"),
            json!({ "answers": { "name": "Ann", "start": "2026-12-01", "team": "Sales" } }),
        )
        .await;
    assert_eq!(sent.status, StatusCode::CREATED, "{:?}", sent.body);
    assert_eq!(sent.body["status"], "completed", "no approval: actions run at once");
    assert_eq!(sam.get(&format!("/api/v1/portal/forms/{id}/draft")).await.body, Value::Null, "the draft is gone");

    let mine = sam.get("/api/v1/portal/submissions").await.body;
    assert_eq!(mine["items"][0]["answers"]["team"], "Sales");
    let for_staff = writer.get(&format!("/api/v1/forms/{id}/submissions")).await.body;
    assert_eq!(for_staff["items"][0]["status"], "completed");
    assert_eq!(for_staff["items"][0]["form_version"], 1);
    assert!(for_staff["items"][0]["task_id"].is_string(), "the task was created");
}

#[sqlx::test(migrations = "../../migrations")]
async fn approval_rounds_return_and_approve_a_submission(pool: PgPool) {
    let w = World::new(&pool).await;
    let writer = w.analyst_client("wendy", WRITER, &[&w.company]).await;
    let (ann, ann_id) = w.analyst("ann", APPROVER, &[&w.company]).await;
    let (sam, _) = w.requester("sam").await;

    let id = create_form(&w, &writer, "Equipment", vec![json!({ "key": "item", "label": "Item", "kind": "text", "required": true })]).await;
    let settings = writer
        .put(
            &format!("/api/v1/forms/{id}/settings"),
            json!({
                "portal": { "visible": true, "audiences": [] },
                "approval": {
                    "approvers": { "kind": "analysts", "analyst_ids": [ann_id] },
                    "required_approvals": 1, "required_rejections": 1, "exclude_requester": false
                },
                "actions": {}
            }),
        )
        .await;
    assert_eq!(settings.status, StatusCode::OK, "{:?}", settings.body);

    let sent = sam
        .post(&format!("/api/v1/portal/forms/{id}/submissions"), json!({ "answers": { "item": "Laptop" } }))
        .await;
    assert_eq!(sent.status, StatusCode::CREATED, "{:?}", sent.body);
    assert_eq!(sent.body["status"], "pending_approval");
    let sub = sent.body["id"].as_str().unwrap().to_owned();

    let silent = ann.post(&format!("/api/v1/form-submissions/{sub}/votes"), json!({ "decision": "reject", "comment": " " })).await;
    assert_eq!(silent.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", silent.body);

    let rejected = ann
        .post(&format!("/api/v1/form-submissions/{sub}/votes"), json!({ "decision": "reject", "comment": "Which model?" }))
        .await;
    assert_eq!(rejected.status, StatusCode::OK, "{:?}", rejected.body);
    assert_eq!(rejected.body["status"], "returned");
    let back = sam.get(&format!("/api/v1/portal/submissions/{sub}")).await.body;
    assert_eq!(back["rejections"][0], "Which model?");
    assert!(back.get("votes").is_none(), "Requesters do not see votes");

    let again = sam
        .post(&format!("/api/v1/portal/submissions/{sub}/resubmit"), json!({ "answers": { "item": "Laptop 14in" } }))
        .await;
    assert_eq!(again.status, StatusCode::OK, "{:?}", again.body);
    assert_eq!(again.body["round"], 2);
    assert_eq!(again.body["status"], "pending_approval");

    let approved = ann.post(&format!("/api/v1/form-submissions/{sub}/votes"), json!({ "decision": "approve" })).await;
    assert_eq!(approved.status, StatusCode::OK, "{:?}", approved.body);
    assert_eq!(approved.body["status"], "completed");
    assert_eq!(approved.body["votes"].as_array().unwrap().len(), 2, "votes of both rounds are kept");

    let after = ann.post(&format!("/api/v1/form-submissions/{sub}/votes"), json!({ "decision": "approve" })).await;
    assert_eq!(after.status, StatusCode::CONFLICT, "{:?}", after.body);
}

#[sqlx::test(migrations = "../../migrations")]
async fn only_the_named_analysts_may_vote(pool: PgPool) {
    let w = World::new(&pool).await;
    let writer = w.analyst_client("wendy", WRITER, &[&w.company]).await;
    let (ann, ann_id) = w.analyst("ann", APPROVER, &[&w.company]).await;
    let (ned, _) = w.analyst("ned", APPROVER, &[&w.company]).await;
    let (sam, _) = w.requester("sam").await;

    let id = create_form(&w, &writer, "Access", vec![field("why")]).await;
    writer
        .put(
            &format!("/api/v1/forms/{id}/settings"),
            json!({
                "portal": { "visible": true, "audiences": [] },
                "approval": { "approvers": { "kind": "analysts", "analyst_ids": [ann_id] }, "required_approvals": 1, "required_rejections": 1 },
                "actions": {}
            }),
        )
        .await;
    let sent = sam.post(&format!("/api/v1/portal/forms/{id}/submissions"), json!({ "answers": { "why": "Project" } })).await;
    let sub = sent.body["id"].as_str().unwrap().to_owned();
    let not_named = ned.post(&format!("/api/v1/form-submissions/{sub}/votes"), json!({ "decision": "approve" })).await;
    assert_eq!(not_named.status, StatusCode::FORBIDDEN, "{:?}", not_named.body);
    let reader = w.analyst_client("rita", READER, &[&w.company]).await;
    let no_capability = reader.post(&format!("/api/v1/form-submissions/{sub}/votes"), json!({ "decision": "approve" })).await;
    assert_eq!(no_capability.status, StatusCode::FORBIDDEN, "{:?}", no_capability.body);
    let named = ann.post(&format!("/api/v1/form-submissions/{sub}/votes"), json!({ "decision": "approve" })).await;
    assert_eq!(named.status, StatusCode::OK, "{:?}", named.body);
}

#[sqlx::test(migrations = "../../migrations")]
async fn a_form_is_offered_only_to_its_audience(pool: PgPool) {
    let w = World::new(&pool).await;
    let writer = w.analyst_client("wendy", WRITER, &[&w.company]).await;
    let (sam, _) = w.requester("sam").await;
    let (_, ned_id) = w.requester("ned").await;

    let id = create_form(&w, &writer, "Only Ned", vec![field("a")]).await;
    let settings = writer
        .put(
            &format!("/api/v1/forms/{id}/settings"),
            json!({ "portal": { "visible": true, "audiences": [{ "kind": "requester", "id": ned_id }] }, "approval": null, "actions": {} }),
        )
        .await;
    assert_eq!(settings.status, StatusCode::OK, "{:?}", settings.body);
    assert_eq!(sam.get("/api/v1/portal/forms").await.body, json!([]), "Sam is not in the audience");
    assert_eq!(sam.get(&format!("/api/v1/portal/forms/{id}")).await.status, StatusCode::NOT_FOUND);
    let refused = sam.post(&format!("/api/v1/portal/forms/{id}/submissions"), json!({ "answers": { "a": "x" } })).await;
    assert_eq!(refused.status, StatusCode::NOT_FOUND, "{:?}", refused.body);

    let bad = writer
        .put(
            &format!("/api/v1/forms/{id}/settings"),
            json!({ "portal": { "visible": true, "audiences": [{ "kind": "team", "id": ned_id }] } }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", bad.body);
}
