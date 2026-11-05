//! Identity API end to end: setup, sessions, CSRF, Capabilities and tenant isolation
//! (ADR-0011), driven through the full router with `oneshot`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use axum::Router;
use axum::body::Body;
use cooper_http::{HttpConfig, PROBLEM_JSON};
use cooper_server::{AppState, SessionSettings, router};
use http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt as _;

const ADMIN_PASSWORD: &str = "correct horse battery";

#[derive(Clone)]
struct Client {
    app: Router,
    cookie: Option<String>,
    csrf: Option<String>,
}

struct Reply {
    status: StatusCode,
    headers: http::HeaderMap,
    body: Value,
}

impl Client {
    fn anonymous(pool: &PgPool) -> Self {
        let state = AppState::with_settings(
            pool.clone(),
            std::sync::Arc::new(cooper_kernel::SystemClock),
            SessionSettings {
                secure_cookies: false,
                ..SessionSettings::default()
            },
        )
        .unwrap();
        Self {
            app: router(state, &HttpConfig::default()),
            cookie: None,
            csrf: None,
        }
    }

    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> Reply {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(cookie) = &self.cookie {
            req = req.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = &self.csrf {
            req = req.header("x-csrf-token", csrf);
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self.app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        Reply {
            status,
            headers,
            body,
        }
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Method::GET, uri, None).await
    }
    async fn post(&self, uri: &str, body: Value) -> Reply {
        self.send(Method::POST, uri, Some(body)).await
    }
    async fn put(&self, uri: &str, body: Value) -> Reply {
        self.send(Method::PUT, uri, Some(body)).await
    }

    async fn sign_in(&self, login: &str, password: &str) -> Self {
        let reply = self
            .post(
                "/api/v1/session",
                json!({ "login": login, "password": password }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
        let set_cookie = reply.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Lax"));
        let cookie = set_cookie.split(';').next().unwrap().to_owned();
        Self {
            app: self.app.clone(),
            cookie: Some(cookie),
            csrf: Some(reply.body["csrf_token"].as_str().unwrap().to_owned()),
        }
    }
}

async fn setup(pool: &PgPool) -> (Client, Client) {
    let anon = Client::anonymous(pool);
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
    (anon, admin)
}

async fn default_company(admin: &Client) -> String {
    let reply = admin.get("/api/v1/companies").await;
    reply.body["items"][0]["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrations = "../../migrations")]
async fn first_run_setup_happens_once(pool: PgPool) {
    let anon = Client::anonymous(&pool);
    assert_eq!(
        anon.get("/api/v1/setup").await.body,
        json!({ "required": true })
    );
    let (anon, _admin) = setup(&pool).await;
    assert_eq!(
        anon.get("/api/v1/setup").await.body,
        json!({ "required": false })
    );
    let again = anon
        .post(
            "/api/v1/setup",
            json!({
                "company_name": "X", "ticket_code": "X", "username": "x", "email": "x@x.example",
                "full_name": "X", "password": "another long password"
            }),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.headers[header::CONTENT_TYPE], PROBLEM_JSON);
}

#[sqlx::test(migrations = "../../migrations")]
async fn sign_in_session_and_sign_out(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    for (login, password) in [("admin", "wrong password!!"), ("ghost", ADMIN_PASSWORD)] {
        let reply = anon
            .post(
                "/api/v1/session",
                json!({ "login": login, "password": password }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            reply.body["type"],
            "https://cooper.dev/problems/invalid-credentials"
        );
    }
    assert_eq!(
        anon.get("/api/v1/session").await.status,
        StatusCode::UNAUTHORIZED
    );

    let session = admin.get("/api/v1/session").await;
    assert_eq!(session.status, StatusCode::OK);
    assert_eq!(session.body["analyst"]["username"], "admin");
    assert_eq!(session.body["is_admin"], true);
    assert_eq!(session.body["all_companies"], true);
    assert!(session.body["analyst"].get("password_hash").is_none());

    let out = admin.send(Method::DELETE, "/api/v1/session", None).await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(
        out.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        admin.get("/api/v1/session").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn unsafe_requests_need_the_csrf_token(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let no_csrf = Client {
        csrf: None,
        ..admin.clone()
    };
    let reply = no_csrf
        .post("/api/v1/teams", json!({ "name": "Service Desk" }))
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(
        reply.body["type"],
        "https://cooper.dev/problems/csrf-token-invalid"
    );
    let wrong = Client {
        csrf: Some("nope".into()),
        ..admin.clone()
    };
    assert_eq!(
        wrong
            .post("/api/v1/teams", json!({ "name": "Service Desk" }))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        admin
            .post("/api/v1/teams", json!({ "name": "Service Desk" }))
            .await
            .status,
        StatusCode::CREATED
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn endpoints_check_capabilities(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let created = admin
        .post(
            "/api/v1/analysts",
            json!({
                "username": "desk", "email": "desk@default.example", "full_name": "Dee Desk",
                "password": "desk password 123"
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let desk = anon.sign_in("desk", "desk password 123").await;

    let denied = desk.post("/api/v1/teams", json!({ "name": "Nope" })).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    assert_eq!(denied.body["capability"], "teams.manage");
    assert_eq!(desk.get("/api/v1/analysts").await.status, StatusCode::OK);
    assert_eq!(
        desk.get("/api/v1/requesters").await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        anon.get("/api/v1/analysts").await.status,
        StatusCode::UNAUTHORIZED
    );

    // Grant teams.manage through a Role on a Team the Analyst belongs to.
    let role = admin
        .post(
            "/api/v1/roles",
            json!({ "name": "Team admins", "capabilities": ["teams.manage"] }),
        )
        .await;
    let team = admin
        .post("/api/v1/teams", json!({ "name": "Service Desk" }))
        .await;
    let team_id = team.body["id"].as_str().unwrap();
    let desk_id = created.body["id"].as_str().unwrap();
    assert_eq!(
        admin
            .put(
                &format!("/api/v1/teams/{team_id}/roles"),
                json!({ "role_ids": [role.body["id"]] })
            )
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        admin
            .put(
                &format!("/api/v1/teams/{team_id}/members"),
                json!({ "analyst_ids": [desk_id] })
            )
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        desk.post("/api/v1/teams", json!({ "name": "Second line" }))
            .await
            .status,
        StatusCode::CREATED
    );

    let unknown = admin
        .post(
            "/api/v1/roles",
            json!({ "name": "Bad", "capabilities": ["tickets.teleport"] }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../../migrations")]
async fn requesters_are_isolated_by_company_access(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let default_id = default_company(&admin).await;
    let other = admin.post("/api/v1/companies", json!({ "name": "Other Ltd", "ticket_code": "OTH", "sender_domains": ["other.example"] })).await;
    assert_eq!(other.status, StatusCode::CREATED, "{:?}", other.body);
    let other_id = other.body["id"].as_str().unwrap();
    let mine = admin.post("/api/v1/requesters", json!({ "company_id": default_id, "full_name": "Mia Mine", "email": "mia@default.example" })).await;
    let theirs = admin.post("/api/v1/requesters", json!({ "company_id": other_id, "full_name": "Theo Theirs", "email": "theo@other.example" })).await;
    assert_eq!(theirs.status, StatusCode::CREATED);

    let analyst = admin.post("/api/v1/analysts", json!({
        "username": "scoped", "email": "scoped@default.example", "full_name": "Sam Scoped", "password": "scoped password 1"
    })).await;
    let analyst_id = analyst.body["id"].as_str().unwrap();
    let role = admin
        .post(
            "/api/v1/roles",
            json!({ "name": "Readers", "capabilities": ["requesters.read"] }),
        )
        .await;
    admin
        .put(
            &format!("/api/v1/analysts/{analyst_id}/roles"),
            json!({ "role_ids": [role.body["id"]] }),
        )
        .await;
    let access = admin
        .put(
            &format!("/api/v1/analysts/{analyst_id}/company-access"),
            json!({ "all": false, "company_ids": [default_id] }),
        )
        .await;
    assert_eq!(access.status, StatusCode::OK);
    let scoped = anon.sign_in("scoped", "scoped password 1").await;

    let list = scoped.get("/api/v1/requesters").await;
    let ids: Vec<&str> = list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![mine.body["id"].as_str().unwrap()]);
    let hidden = scoped
        .get(&format!(
            "/api/v1/requesters/{}",
            theirs.body["id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    let companies = scoped.get("/api/v1/companies").await;
    assert_eq!(companies.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        scoped
            .get(&format!("/api/v1/companies/{other_id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn sender_domains_are_unique_and_never_freemail(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let taken = admin.post("/api/v1/companies", json!({ "name": "Copycat", "ticket_code": "CC", "sender_domains": ["default.example"] })).await;
    assert_eq!(taken.status, StatusCode::CONFLICT, "{:?}", taken.body);
    let freemail = admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Gmailers", "ticket_code": "GM", "sender_domains": ["gmail.com"] }),
        )
        .await;
    assert_eq!(freemail.status, StatusCode::UNPROCESSABLE_ENTITY);
    let code = admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Dup code", "ticket_code": "def" }),
        )
        .await;
    assert_eq!(code.status, StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn deactivating_an_analyst_ends_their_sessions_and_is_audited(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let created = admin.post("/api/v1/analysts", json!({
        "username": "leaver", "email": "leaver@default.example", "full_name": "Lee Leaver", "password": "leaver password 1"
    })).await;
    let id = created.body["id"].as_str().unwrap();
    let leaver = anon.sign_in("leaver", "leaver password 1").await;
    assert_eq!(leaver.get("/api/v1/session").await.status, StatusCode::OK);

    let off = admin
        .post(&format!("/api/v1/analysts/{id}/deactivate"), json!({}))
        .await;
    assert_eq!(off.status, StatusCode::OK);
    assert_eq!(off.body["is_active"], false);
    assert_eq!(
        leaver.get("/api/v1/session").await.status,
        StatusCode::UNAUTHORIZED
    );
    let again = anon
        .post(
            "/api/v1/session",
            json!({ "login": "leaver", "password": "leaver password 1" }),
        )
        .await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);

    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_entries WHERE aggregate_type = 'analyst' AND aggregate_id = $1::uuid")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(audits >= 2, "creation and deactivation are audited");

    // The last Administrator cannot deactivate themselves.
    let session = admin.get("/api/v1/session").await;
    let admin_id = session.body["analyst"]["id"].as_str().unwrap();
    let self_off = admin
        .post(
            &format!("/api/v1/analysts/{admin_id}/deactivate"),
            json!({}),
        )
        .await;
    assert_eq!(self_off.status, StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn lists_are_paginated_with_cursors(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    for name in ["Alpha", "Bravo", "Charlie", "Delta", "Echo"] {
        assert_eq!(
            admin
                .post("/api/v1/teams", json!({ "name": name }))
                .await
                .status,
            StatusCode::CREATED
        );
    }
    let first = admin.get("/api/v1/teams?limit=2").await;
    assert_eq!(first.body["items"].as_array().unwrap().len(), 2);
    let cursor = first.body["next_cursor"].as_str().unwrap();
    let second = admin
        .get(&format!("/api/v1/teams?limit=2&cursor={cursor}"))
        .await;
    assert_eq!(second.body["items"][0]["name"], "Charlie");
    let filtered = admin.get("/api/v1/teams?q=ech").await;
    assert_eq!(filtered.body["items"][0]["name"], "Echo");
    assert_eq!(
        admin.get("/api/v1/teams?limit=500").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        admin.get("/api/v1/teams?cursor=garbage").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn openapi_document_describes_the_api(pool: PgPool) {
    let anon = Client::anonymous(&pool);
    let doc = anon.get("/api/v1/openapi.json").await;
    assert_eq!(doc.status, StatusCode::OK);
    let paths = doc.body["paths"].as_object().unwrap();
    for path in [
        "/api/v1/session",
        "/api/v1/analysts/{id}",
        "/api/v1/requesters",
        "/api/v1/setup",
    ] {
        assert!(paths.contains_key(path), "missing {path}");
    }
    assert!(
        doc.body["components"]["schemas"]
            .get("AnalystDto")
            .is_some()
    );
}
