//! Knowledge end to end (ADR-0028): Articles with sanitised HTML and versions, the
//! lifecycle, Audiences and Companies, Folders with access lists and the administrator
//! floor, permission-aware search, ticket links, the knowledge audit, Review Date
//! Notifications, Portal reading, Known Errors published from Problems, and the
//! Knowledge Module switch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use common::{Client, setup, test_settings};
use cooper_kernel::{Clock, FixedClock};
use cooper_platform::jobs::{Job, JobContext};
use cooper_platform::outbox::Dispatcher;
use cooper_server::AppState;
use http::{Method, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

const READER: &[&str] = &["knowledge.read", "tickets.read"];
const WRITER: &[&str] = &[
    "knowledge.read",
    "knowledge.write",
    "tickets.read",
    "tickets.update",
];
const PORTAL_PASSWORD: &str = "my portal password";

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

    async fn article(&self, client: &Client, body: Value) -> Value {
        let r = client.post("/api/v1/knowledge/articles", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body
    }

    async fn publish(&self, id: &str) {
        let r = self
            .admin
            .put(
                &format!("/api/v1/knowledge/articles/{id}/published"),
                json!({ "is_published": true }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }

    /// A Requester of `company` signed in to the Portal.
    async fn requester(&self, company: &str, email: &str) -> Person {
        let r = self
            .admin
            .post(
                "/api/v1/requesters",
                json!({ "company_id": company, "full_name": email, "email": email }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        let id = r.body["id"].as_str().unwrap().to_owned();
        self.admin
            .post(
                &format!("/api/v1/requesters/{id}/portal-invitation"),
                json!({}),
            )
            .await;
        let body: String = sqlx::query_scalar(
            "SELECT text_body FROM outbound_emails WHERE kind = 'portal_invitation' AND $1 = ANY(to_addresses) ORDER BY created_at DESC LIMIT 1",
        )
        .bind(email)
        .fetch_one(&self.pool)
        .await
        .unwrap();
        let start = body.find("/portal/set-password?token=").unwrap() + 27;
        let token: String = body[start..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect();
        let r = self
            .anon
            .post(
                "/api/v1/portal/password",
                json!({ "token": token, "password": PORTAL_PASSWORD }),
            )
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
        let r = self
            .anon
            .post(
                "/api/v1/portal/session",
                json!({ "email": email, "password": PORTAL_PASSWORD }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let cookie = r.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let client = Client {
            cookie: Some(cookie),
            csrf: Some(r.body["csrf_token"].as_str().unwrap().to_owned()),
            ..self.anon.clone()
        };
        Person { id, client }
    }
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

fn titles(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["title"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test(migrations = "../../migrations")]
async fn articles_are_sanitised_versioned_published_and_archived(pool: PgPool) {
    let w = World::new(&pool).await;
    let writer = w.analyst("wendy", WRITER, &[&w.company]).await;
    let reader = w.analyst("rob", READER, &[&w.company]).await;
    let a = w
        .article(
            &writer.client,
            json!({ "company_id": w.company, "title": "Reset a VPN token",
                    "body": "<p onclick=\"steal()\">Open the <b>portal</b></p><script>alert(1)</script>",
                    "tags": ["VPN", " vpn", "Remote"], "next_review_date": "2027-01-01" }),
        )
        .await;
    assert_eq!(a["body"], "<p>Open the <b>portal</b></p>", "sanitised");
    assert_eq!(a["body_text"], "Open the portal");
    assert_eq!(a["tags"], json!(["remote", "vpn"]));
    assert_eq!(a["audience"], "internal");
    assert_eq!(a["is_published"], false);
    assert_eq!(a["version"], 1);
    assert_eq!(a["owner_id"], writer.id.as_str(), "the author owns it");
    let path = format!("/api/v1/knowledge/articles/{}", id(&a));

    assert_eq!(
        reader.client.get(&path).await.status,
        StatusCode::NOT_FOUND,
        "unpublished Articles are for writers"
    );
    assert!(titles(&reader.client.get("/api/v1/knowledge/articles").await.body).is_empty());
    assert_eq!(
        reader
            .client
            .get("/api/v1/knowledge/articles?state=current")
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let r = writer
        .client
        .put(&path, json!({ "title": "Reset a VPN token", "body": "<p>Open the <b>portal</b></p><p>Click reset</p>", "tags": ["vpn"] }))
        .await;
    assert_eq!(r.body["version"], 2, "{:?}", r.body);
    let r = writer.client.put(&path, json!({ "title": "Reset a VPN token", "body": r.body["body"], "tags": ["vpn", "token"] })).await;
    assert_eq!(r.body["version"], 2, "tags alone make no version");
    let r = writer
        .client
        .post(&format!("{path}/versions/1/restore"), json!({}))
        .await;
    assert_eq!(r.body["version"], 3);
    assert_eq!(r.body["body"], "<p>Open the <b>portal</b></p>");
    let versions = writer.client.get(&format!("{path}/versions")).await.body;
    let numbers: Vec<i64> = versions
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["version_no"].as_i64().unwrap())
        .collect();
    assert_eq!(numbers, [3, 2, 1]);

    let r = reader
        .client
        .put(
            &format!("{path}/published"),
            json!({ "is_published": true }),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = writer
        .client
        .put(
            &format!("{path}/published"),
            json!({ "is_published": true }),
        )
        .await;
    assert_eq!(r.body["is_published"], true);
    let opened = reader.client.get(&path).await;
    assert_eq!(opened.status, StatusCode::OK);
    reader.client.get(&path).await;
    assert_eq!(
        writer.client.get(&path).await.body["view_count"],
        2,
        "views are counted (this one after it is read)"
    );

    let found = reader
        .client
        .get("/api/v1/knowledge/articles?q=portal%20reset")
        .await
        .body;
    assert_eq!(titles(&found), ["Reset a VPN token"]);
    assert!(
        titles(
            &reader
                .client
                .get("/api/v1/knowledge/articles?q=printer")
                .await
                .body
        )
        .is_empty()
    );
    assert_eq!(
        titles(
            &reader
                .client
                .get("/api/v1/knowledge/articles?tag=VPN")
                .await
                .body
        ),
        ["Reset a VPN token"]
    );

    let r = writer
        .client
        .post(&format!("{path}/archive"), json!({}))
        .await;
    assert_eq!(r.body["is_archived"], true);
    assert_eq!(r.body["is_published"], false, "archiving unpublishes");
    assert_eq!(reader.client.get(&path).await.status, StatusCode::NOT_FOUND);
    let r = writer.client.put(&path, json!({ "title": "x" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "restore before editing");
    let archived = writer
        .client
        .get("/api/v1/knowledge/articles?state=archived")
        .await
        .body;
    assert_eq!(titles(&archived), ["Reset a VPN token"]);
    let r = writer
        .client
        .post(&format!("{path}/restore"), json!({}))
        .await;
    assert_eq!(
        (
            r.body["is_archived"].clone(),
            r.body["is_published"].clone()
        ),
        (json!(false), json!(false))
    );

    let history = w.admin.get(&format!("{path}/history")).await.body;
    let actions: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["action"].as_str().unwrap())
        .collect();
    for expected in ["created", "updated", "viewed", "deleted", "restored"] {
        assert!(actions.contains(&expected), "{expected} in {actions:?}");
    }
    assert_eq!(
        reader.client.get(&format!("{path}/history")).await.status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn folders_and_access_lists_narrow_every_reader_and_the_floor_is_audited(pool: PgPool) {
    let w = World::new(&pool).await;
    let team = w
        .admin
        .post("/api/v1/teams", json!({ "name": "Network" }))
        .await
        .body;
    let insider = w.analyst("ingrid", READER, &[&w.company]).await;
    let outsider = w.analyst("otto", READER, &[&w.company]).await;
    w.admin
        .put(
            &format!("/api/v1/teams/{}/members", id(&team)),
            json!({ "analyst_ids": [insider.id] }),
        )
        .await;

    let root = w
        .admin
        .post(
            "/api/v1/knowledge/folders",
            json!({ "company_id": null, "name": "Infrastructure" }),
        )
        .await;
    assert_eq!(root.status, StatusCode::CREATED, "{:?}", root.body);
    let root_id = id(&root.body).to_owned();
    let child = w
        .admin
        .post(
            "/api/v1/knowledge/folders",
            json!({ "company_id": null, "parent_id": root_id, "name": "Routers" }),
        )
        .await
        .body;
    let dup = w
        .admin
        .post(
            "/api/v1/knowledge/folders",
            json!({ "company_id": null, "parent_id": root_id, "name": "routers" }),
        )
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    let r = w
        .admin
        .put(
            &format!("/api/v1/knowledge/folders/{root_id}"),
            json!({ "name": "Infrastructure", "parent_id": id(&child) }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "no cycles");

    let a = w.article(&w.admin, json!({ "company_id": null, "title": "Core router passwords", "folder_id": id(&child) })).await;
    w.publish(id(&a)).await;
    let open = w
        .article(
            &w.admin,
            json!({ "company_id": null, "title": "Printer drivers" }),
        )
        .await;
    w.publish(id(&open)).await;
    let path = format!("/api/v1/knowledge/articles/{}", id(&a));
    assert_eq!(
        outsider.client.get(&path).await.status,
        StatusCode::OK,
        "open until restricted"
    );

    let r = w
        .admin
        .put(
            &format!("/api/v1/knowledge/folders/{root_id}/access"),
            json!({ "is_restricted": true, "inherit": false, "principals": [{ "type": "team", "id": team["id"] }] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        outsider.client.get(&path).await.status,
        StatusCode::NOT_FOUND,
        "the child Folder inherits the restriction"
    );
    assert_eq!(
        insider.client.get(&path).await.status,
        StatusCode::OK,
        "Team members are let in"
    );
    assert_eq!(
        titles(&outsider.client.get("/api/v1/knowledge/articles").await.body),
        ["Printer drivers"]
    );
    assert_eq!(
        titles(
            &outsider
                .client
                .get("/api/v1/knowledge/articles?q=router")
                .await
                .body
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        outsider
            .client
            .get("/api/v1/knowledge/folders")
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        insider
            .client
            .get("/api/v1/knowledge/folders")
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // The Article's own rules narrow further: deny Ingrid on an Open Article.
    let r = w
        .admin
        .put(
            &format!("{path}/access"),
            json!({ "is_restricted": false, "inherit": false, "principals": [{ "type": "analyst", "id": insider.id }] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        insider.client.get(&path).await.status,
        StatusCode::NOT_FOUND
    );

    // The administrator floor always reads, and leaves an override in the audit.
    assert_eq!(w.admin.get(&path).await.status, StatusCode::OK);
    let history = w.admin.get(&format!("{path}/history")).await.body;
    assert!(
        history
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["action"] == "admin_override"),
        "{history:?}"
    );
    assert!(
        history
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["field"] == "access")
    );

    let r = w
        .admin
        .send(
            Method::DELETE,
            &format!("/api/v1/knowledge/folders/{root_id}"),
            None,
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "not while it holds something"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn companies_audiences_and_user_groups_decide_what_the_portal_shows(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w
        .admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
        )
        .await
        .body;
    let mine = w.article(&w.admin, json!({ "company_id": w.company, "title": "Default how-to", "audience": "customer", "body": "<p>wifi</p>" })).await;
    let shared = w.article(&w.admin, json!({ "company_id": null, "title": "Shared how-to", "audience": "public", "body": "<p>wifi</p>" })).await;
    let theirs = w.article(&w.admin, json!({ "company_id": id(&other), "title": "Other how-to", "audience": "customer", "body": "<p>wifi</p>" })).await;
    let internal = w
        .article(
            &w.admin,
            json!({ "company_id": w.company, "title": "Internal notes", "body": "<p>wifi</p>" }),
        )
        .await;
    let draft = w
        .article(
            &w.admin,
            json!({ "company_id": w.company, "title": "Draft how-to", "audience": "customer" }),
        )
        .await;
    let vip = w.article(&w.admin, json!({ "company_id": w.company, "title": "VIP how-to", "audience": "customer", "body": "<p>wifi</p>" })).await;
    for a in [&mine, &shared, &theirs, &internal, &vip] {
        w.publish(id(a)).await;
    }
    let _ = draft;

    let sam = w.requester(&w.company, "sam@default.example").await;
    let vips = w
        .admin
        .post("/api/v1/user-groups", json!({ "name": "VIPs" }))
        .await
        .body;
    w.admin
        .put(&format!("{}/access", format_args!("/api/v1/knowledge/articles/{}", id(&vip))),
             json!({ "is_restricted": true, "inherit": false, "principals": [{ "type": "user_group", "id": vips["id"] }] }))
        .await;

    let list = sam.client.get("/api/v1/portal/knowledge").await;
    assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
    let mut seen: Vec<String> = list
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["title"].as_str().unwrap().to_owned())
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        ["Default how-to", "Shared how-to"],
        "own Company or shared, customer or public, live, allowed"
    );
    assert_eq!(
        sam.client
            .get(&format!("/api/v1/portal/knowledge/{}", id(&internal)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        sam.client
            .get(&format!("/api/v1/portal/knowledge/{}", id(&theirs)))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let read = sam
        .client
        .get(&format!("/api/v1/portal/knowledge/{}", id(&mine)))
        .await;
    assert_eq!(read.status, StatusCode::OK);
    assert!(
        read.body.get("access").is_none(),
        "the Portal never sees access lists"
    );

    w.admin
        .put(
            &format!("/api/v1/user-groups/{}/members", id(&vips)),
            json!({ "members": [{ "kind": "requester", "id": sam.id }] }),
        )
        .await;
    let searched = sam.client.get("/api/v1/portal/knowledge?q=wifi").await.body;
    assert_eq!(
        searched.as_array().unwrap().len(),
        3,
        "now a VIP: {searched:?}"
    );
    assert_eq!(
        sam.client.get("/api/v1/knowledge/articles").await.status,
        StatusCode::UNAUTHORIZED,
        "the Analyst API is not for Portal sessions"
    );

    // An Analyst of the other Company cannot see this Company's Articles.
    let olga = w.analyst("olga", READER, &[id(&other)]).await;
    let mut seen = titles(&olga.client.get("/api/v1/knowledge/articles").await.body);
    seen.sort();
    assert_eq!(seen, ["Other how-to", "Shared how-to"]);
    let r = olga
        .client
        .get(&format!("/api/v1/knowledge/articles/{}", id(&mine)))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let writer = w.analyst("wade", WRITER, &[&w.company]).await;
    let r = writer
        .client
        .post(
            "/api/v1/knowledge/articles",
            json!({ "company_id": null, "title": "x" }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "sharing with every Company needs every Company"
    );

    let r = w
        .admin
        .put("/api/v1/modules/knowledge", json!({ "is_enabled": false }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        w.admin.get("/api/v1/knowledge/articles").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        sam.client.get("/api/v1/portal/knowledge").await.status,
        StatusCode::NOT_FOUND,
        "the Portal pages switch off too"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn articles_link_to_tickets_and_known_errors_become_articles(pool: PgPool) {
    let w = World::new(&pool).await;
    let a = w.article(&w.admin, json!({ "company_id": w.company, "title": "Outlook keeps asking for a password", "body": "<p>Clear the credential</p>" })).await;
    let ticket = w
        .admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": w.company, "subject": "Outlook prompts" }),
        )
        .await
        .body;
    let path = format!("/api/v1/knowledge/articles/{}", id(&a));
    let r = w
        .admin
        .post(
            &format!("{path}/tickets"),
            json!({ "ticket_id": ticket["id"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["number"], ticket["number"]);
    let r = w
        .admin
        .post(
            &format!("{path}/tickets"),
            json!({ "ticket_id": ticket["id"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let by_ticket = w
        .admin
        .get(&format!(
            "/api/v1/knowledge/articles?state=current&ticket_id={}",
            id(&ticket)
        ))
        .await
        .body;
    assert_eq!(
        titles(&by_ticket),
        ["Outlook keeps asking for a password"],
        "the ticket's side"
    );
    assert_eq!(
        by_ticket["items"][0]["body_text"], "Clear the credential",
        "ready to insert into a reply"
    );
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
            "/api/v1/tickets",
            json!({ "company_id": id(&other), "subject": "x" }),
        )
        .await
        .body;
    let r = w
        .admin
        .post(
            &format!("{path}/tickets"),
            json!({ "ticket_id": foreign["id"] }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a Company Article links to its Company's tickets"
    );
    let r = w
        .admin
        .send(
            Method::DELETE,
            &format!("{path}/tickets/{}", id(&ticket)),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);

    // FR-PRB-005.
    let p = w
        .admin
        .post(
            "/api/v1/problems",
            json!({ "company_id": w.company, "title": "Mail stalls" }),
        )
        .await
        .body;
    let ppath = format!("/api/v1/problems/{}", id(&p));
    let r = w
        .admin
        .post(&format!("{ppath}/knowledge-article"), json!({}))
        .await;
    assert_eq!(
        r.status,
        StatusCode::CONFLICT,
        "only Known Errors with a Workaround"
    );
    w.admin
        .put(&ppath, json!({ "title": "Mail stalls", "root_cause": "Full queue", "workaround": "Restart <transport>", "is_known_error": true }))
        .await;
    let r = w
        .admin
        .post(&format!("{ppath}/knowledge-article"), json!({}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let article = w
        .admin
        .get(&format!(
            "/api/v1/knowledge/articles/{}",
            r.body["article_id"].as_str().unwrap()
        ))
        .await
        .body;
    assert_eq!(article["title"], "Mail stalls");
    assert_eq!(article["company_id"], w.company.as_str());
    assert_eq!(
        article["is_published"], false,
        "reviewed in Knowledge before publishing"
    );
    assert_eq!(article["source_problem_id"], p["id"]);
    assert!(
        article["body"]
            .as_str()
            .unwrap()
            .contains("Restart &lt;transport&gt;"),
        "{article:?}"
    );
    w.admin
        .put(&ppath, json!({ "title": "Mail stalls", "root_cause": "Full queue", "workaround": "Restart the service", "is_known_error": true }))
        .await;
    let again = w
        .admin
        .post(&format!("{ppath}/knowledge-article"), json!({}))
        .await;
    assert_eq!(
        again.body["article_id"], r.body["article_id"],
        "publishing again updates the same Article"
    );
    let article = w
        .admin
        .get(&format!(
            "/api/v1/knowledge/articles/{}",
            r.body["article_id"].as_str().unwrap()
        ))
        .await
        .body;
    assert_eq!(article["version"], 2);
    let reader = w
        .analyst("pia", &["problems.read", "problems.update"], &[&w.company])
        .await;
    let r = reader
        .client
        .post(&format!("{ppath}/knowledge-article"), json!({}))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "needs knowledge.write too");
}

#[sqlx::test(migrations = "../../migrations")]
async fn owners_are_told_once_when_the_review_date_comes(pool: PgPool) {
    let w = World::new(&pool).await;
    let owner = w.analyst("ollie", WRITER, &[&w.company]).await;
    let a = w
        .article(&owner.client, json!({ "company_id": w.company, "title": "Backup runbook", "next_review_date": "2026-12-01" }))
        .await;
    let due = owner
        .client
        .get("/api/v1/knowledge/articles?state=current&owner=me&review_due=true")
        .await
        .body;
    assert!(
        titles(&due).is_empty(),
        "not due yet (today is before 2026-12-01)"
    );

    let later: Arc<dyn Clock> = Arc::new(FixedClock::new("2026-12-02T08:00:00Z".parse().unwrap()));
    let state = AppState::with_settings(pool.clone(), later.clone(), test_settings()).unwrap();
    let job = cooper_knowledge::infrastructure::KnowledgeReviewJob {
        service: state.knowledge.service.clone(),
    };
    let ctx = JobContext {
        pool: pool.clone(),
        clock: later.clone(),
    };
    assert_eq!(
        job.run(&ctx).await.unwrap().unwrap(),
        "announced 1 Review Dates"
    );
    assert_eq!(
        job.run(&ctx).await.unwrap().unwrap(),
        "announced 0 Review Dates",
        "once per date"
    );
    let dispatcher = state.event_handlers().into_iter().fold(
        Dispatcher::new(pool.clone(), later.clone()),
        Dispatcher::with_handler,
    );
    let stats = dispatcher.dispatch_once().await.unwrap();
    assert_eq!(
        (stats.total(), stats.retried + stats.dead),
        (1, 0),
        "{stats:?}"
    );
    let bell = owner.client.get("/api/v1/notifications").await.body;
    assert_eq!(bell["items"][0]["kind"], "article_review_due", "{bell:?}");
    assert_eq!(bell["items"][0]["article_id"], a["id"]);

    // A new Review Date is announced again when it comes.
    owner
        .client
        .put(
            &format!("/api/v1/knowledge/articles/{}", id(&a)),
            json!({ "title": "Backup runbook", "next_review_date": "2026-12-02" }),
        )
        .await;
    assert_eq!(
        job.run(&ctx).await.unwrap().unwrap(),
        "announced 1 Review Dates"
    );
}
