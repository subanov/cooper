//! Live Updates (ADR-0022, FR-GEN-010): `GET /api/v1/live` tells a signed-in Analyst
//! which tickets changed, only for tickets they may see, and never sends ticket data.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::time::Duration;

use axum::body::Body;
use common::{ADMIN_PASSWORD, Client};
use cooper_http::HttpConfig;
use cooper_server::{AppState, SecuritySettings, router};
use http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt as _;

const DESK_CAPS: &[&str] = &[
    "tickets.read",
    "tickets.create",
    "tickets.update",
    "requesters.read",
];

struct World {
    state: AppState,
    anon: Client,
    admin: Client,
    company: String,
}

impl World {
    async fn new(pool: &PgPool) -> Self {
        let state = AppState::with_settings(
            pool.clone(),
            std::sync::Arc::new(cooper_kernel::SystemClock),
            SecuritySettings {
                secure_cookies: false,
                ..common::test_settings()
            },
        )
        .unwrap();
        let anon = Client {
            app: router(state.clone(), &HttpConfig::default()),
            cookie: None,
            csrf: None,
            peer: None,
            headers: Vec::new(),
        };
        let reply = anon
            .post(
                "/api/v1/setup",
                json!({
                    "company_name": "Default Company", "ticket_code": "DEF", "sender_domains": ["default.example"],
                    "username": "admin", "email": "admin@default.example", "full_name": "Ada Admin",
                    "password": ADMIN_PASSWORD
                }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
        let admin = anon.sign_in("admin", ADMIN_PASSWORD).await;
        let company = admin.get("/api/v1/companies").await.body["items"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            state,
            anon,
            admin,
            company,
        }
    }

    async fn company(&self, name: &str, code: &str, domain: &str) -> String {
        let r = self
            .admin
            .post(
                "/api/v1/companies",
                json!({ "name": name, "ticket_code": code, "sender_domains": [domain] }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }

    /// An Analyst with `caps` and access to `companies` only.
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

    async fn ticket(&self, client: &Client, company: &str, body: Value) -> String {
        let mut body = body;
        body["company_id"] = json!(company);
        let r = client.post("/api/v1/tickets", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_owned()
    }
}

/// An open `text/event-stream`, read event by event.
struct Stream {
    body: Body,
    buf: String,
}

impl Stream {
    /// Opens the stream as `client` and waits for `ready`.
    async fn open(client: &Client) -> Self {
        let response = live_request(client).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let mut stream = Self {
            body: response.into_body(),
            buf: String::new(),
        };
        let (event, _) = stream.next().await.expect("ready");
        assert_eq!(event, "ready");
        stream
    }

    /// The next event as `(name, data)`; `None` when the server ended the stream.
    async fn next(&mut self) -> Option<(String, Value)> {
        loop {
            while let Some(end) = self.buf.find("\n\n") {
                let block: String = self.buf.drain(..end + 2).collect();
                let mut name = None;
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        name = Some(v.trim().to_owned());
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim());
                    }
                }
                if let Some(name) = name {
                    return Some((name, serde_json::from_str(&data).unwrap()));
                }
            }
            let frame = tokio::time::timeout(Duration::from_secs(10), self.body.frame())
                .await
                .expect("an event within 10 s");
            match frame {
                None => return None,
                Some(frame) => {
                    if let Ok(bytes) = frame.unwrap().into_data() {
                        self.buf.push_str(std::str::from_utf8(&bytes).unwrap());
                    }
                }
            }
        }
    }

    /// The next `invalidate` event's ticket ID.
    async fn invalidated(&mut self) -> String {
        let (name, data) = self.next().await.expect("an open stream");
        assert_eq!(name, "invalidate", "{data}");
        assert_eq!(
            data.as_object().unwrap().len(),
            2,
            "only topic and id, never ticket data: {data}"
        );
        assert_eq!(data["topic"], "ticket");
        data["id"].as_str().unwrap().to_owned()
    }
}

async fn live_request(client: &Client) -> http::Response<Body> {
    let mut req = Request::get("/api/v1/live");
    if let Some(cookie) = &client.cookie {
        req = req.header(header::COOKIE, cookie);
    }
    for (name, value) in &client.headers {
        req = req.header(name.as_str(), value.as_str());
    }
    client
        .app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn analysts_are_told_which_ticket_changed_including_changes_without_events(pool: PgPool) {
    let w = World::new(&pool).await;
    let (_, desk) = w.analyst("desk", DESK_CAPS, &[&w.company]).await;
    let mut live = Stream::open(&desk).await;

    let id = w
        .ticket(&w.admin, &w.company, json!({ "subject": "Printer jam" }))
        .await;
    assert_eq!(live.invalidated().await, id, "ticket.created");

    let note = w
        .admin
        .post(
            &format!("/api/v1/tickets/{id}/notes"),
            json!({ "body": "Looking" }),
        )
        .await;
    assert_eq!(note.status, StatusCode::CREATED);
    assert_eq!(live.invalidated().await, id, "ticket.note_added");

    let time = w
        .admin
        .post(
            &format!("/api/v1/tickets/{id}/time-entries"),
            json!({ "minutes": 15 }),
        )
        .await;
    assert_eq!(time.status, StatusCode::CREATED, "{:?}", time.body);
    assert_eq!(
        live.invalidated().await,
        id,
        "Time Entries raise no Domain Event but still refresh the screen"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn live_updates_are_isolated_by_company_access(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w.company("Other Ltd", "OTH", "other.example").await;
    let (_, mine) = w.analyst("mine", DESK_CAPS, &[&w.company]).await;
    let (_, theirs) = w.analyst("theirs", DESK_CAPS, &[&other]).await;
    let mut mine_live = Stream::open(&mine).await;
    let mut theirs_live = Stream::open(&theirs).await;

    let default_ticket = w
        .ticket(&w.admin, &w.company, json!({ "subject": "Default" }))
        .await;
    let other_ticket = w
        .ticket(&w.admin, &other, json!({ "subject": "Other" }))
        .await;
    let default_again = w
        .ticket(&w.admin, &w.company, json!({ "subject": "Default again" }))
        .await;

    // Changes arrive in commit order, so the next event proves the skipped one was
    // never sent.
    assert_eq!(mine_live.invalidated().await, default_ticket);
    assert_eq!(
        mine_live.invalidated().await,
        default_again,
        "the other Company's ticket is not announced"
    );
    assert_eq!(
        theirs_live.invalidated().await,
        other_ticket,
        "the Default Company's ticket is not announced"
    );

    let mut admin_live = Stream::open(&w.admin).await;
    let later = w
        .ticket(&w.admin, &other, json!({ "subject": "Other again" }))
        .await;
    assert_eq!(
        admin_live.invalidated().await,
        later,
        "Administrators see every Company"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn confidential_tickets_are_announced_only_to_analysts_who_may_see_them(pool: PgPool) {
    let w = World::new(&pool).await;
    let (hr_member, hr) = w.analyst("hrdesk", DESK_CAPS, &[&w.company]).await;
    let (_, it) = w.analyst("itdesk", DESK_CAPS, &[&w.company]).await;
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
    let mut hr_live = Stream::open(&hr).await;
    let mut it_live = Stream::open(&it).await;

    let secret = w
        .ticket(
            &hr,
            &w.company,
            json!({ "subject": "Grievance", "department_id": dept["id"] }),
        )
        .await;
    let printers = w
        .ticket(&it, &w.company, json!({ "subject": "Printers" }))
        .await;

    assert_eq!(hr_live.invalidated().await, secret);
    assert_eq!(hr_live.invalidated().await, printers);
    assert_eq!(
        it_live.invalidated().await,
        printers,
        "the Confidential ticket is not announced to an Analyst outside its Department"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn live_updates_need_an_analyst_session(pool: PgPool) {
    let w = World::new(&pool).await;
    assert_eq!(
        live_request(&w.anon).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let created = w
        .admin
        .post(
            "/api/v1/api-keys",
            json!({
                "name": "Monitoring", "capabilities": ["tickets.read"],
                "company_access": { "all": true }, "rate_limit_per_minute": 30
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let bearer = Client {
        headers: vec![(
            "authorization".into(),
            format!("Bearer {}", created.body["key"].as_str().unwrap()),
        )],
        ..w.anon.clone()
    };
    assert_eq!(
        live_request(&bearer).await.status(),
        StatusCode::UNAUTHORIZED,
        "integrations use webhooks, not the browser stream"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn streams_end_when_the_server_shuts_down(pool: PgPool) {
    let w = World::new(&pool).await;
    let mut live = Stream::open(&w.admin).await;
    w.state.live.shutdown();
    assert!(live.next().await.is_none(), "the stream ends");
}

#[sqlx::test(migrations = "../../migrations")]
async fn presence_changes_are_announced_to_analysts_who_can_see_the_ticket(pool: PgPool) {
    let w = World::new(&pool).await;
    let other = w.company("Other Ltd", "OTH", "other.example").await;
    let (_, ann) = w.analyst("ann", DESK_CAPS, &[&w.company]).await;
    let (_, bob) = w.analyst("bob", DESK_CAPS, &[&w.company]).await;
    let (_, theirs) = w.analyst("theirs", DESK_CAPS, &[&other]).await;
    let ticket = w
        .ticket(&w.admin, &w.company, json!({ "subject": "Busy ticket" }))
        .await;
    let mut ann_live = Stream::open(&ann).await;
    let mut theirs_live = Stream::open(&theirs).await;

    let path = format!("/api/v1/tickets/{ticket}/presence");
    assert_eq!(
        bob.put(&path, json!({ "composing": true })).await.status,
        StatusCode::OK
    );
    let (name, data) = ann_live.next().await.unwrap();
    assert_eq!(name, "invalidate");
    assert_eq!(data, json!({ "topic": "ticket_presence", "id": ticket }));

    // A repeated heartbeat with nothing new is not announced: Ann's next event is the
    // marker ticket created after it. `theirs` (another Company) never hears of it.
    assert_eq!(
        bob.put(&path, json!({ "composing": true })).await.status,
        StatusCode::OK
    );
    let marker = w
        .ticket(&w.admin, &w.company, json!({ "subject": "Marker" }))
        .await;
    assert_eq!(ann_live.invalidated().await, marker);
    let their_marker = w
        .ticket(&w.admin, &other, json!({ "subject": "Their marker" }))
        .await;
    assert_eq!(theirs_live.invalidated().await, their_marker);
}
