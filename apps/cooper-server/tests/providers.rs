//! Microsoft Graph and Gmail Mailboxes (FR-EML-001, ADR-0020) against local fakes of the
//! provider APIs: sign-in, fetching MIME, dispositions and refresh token rotation.

#![cfg(all(feature = "graph", feature = "google"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
use axum::{Form, Json, Router};
use base64::Engine;
use common::{Client, setup};
use cooper_kernel::SystemClock;
use cooper_platform::jobs::{Job, JobContext};
use cooper_server::AppState;
use cooper_service_desk::domain::Provider;
use cooper_service_desk::infrastructure::{GmailClient, GraphClient, ImapClient, MailboxPoller};
use http::{HeaderMap, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

/// Requests the fake provider received: `(method path, detail)`.
#[derive(Clone, Default)]
struct Calls(Arc<Mutex<Vec<(String, String)>>>);

impl Calls {
    fn push(&self, what: &str, detail: impl Into<String>) {
        self.0
            .lock()
            .unwrap()
            .push((what.to_owned(), detail.into()));
    }
    fn all(&self) -> Vec<(String, String)> {
        self.0.lock().unwrap().clone()
    }
}

fn mime(subject: &str) -> String {
    format!(
        "From: Pat <pat@acme.example>\r\nTo: desk@acme.example\r\nSubject: {subject}\r\nMessage-ID: <{}@acme.example>\r\nContent-Type: text/plain\r\n\r\nBlack screen.\r\n",
        uuid::Uuid::now_v7()
    )
}

fn bearer(headers: &HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

fn graph_fake(calls: Calls) -> Router {
    Router::new()
        .route(
            "/{tenant}/oauth2/v2.0/token",
            post(
                |State(c): State<Calls>,
                 Path(tenant): Path<String>,
                 Form(f): Form<HashMap<String, String>>| async move {
                    c.push("token", format!("{tenant} {} {}", f["grant_type"], f.get("refresh_token").cloned().unwrap_or_default()));
                    if f.get("refresh_token").map(String::as_str) == Some("expired") {
                        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant", "error_description": "AADSTS70008: The refresh token has expired.\r\nTrace ID: x" })));
                    }
                    (StatusCode::OK, Json(json!({ "access_token": "graph-at", "refresh_token": "rotated-rt", "token_type": "Bearer" })))
                },
            ),
        )
        .route(
            "/v1.0/me/mailFolders",
            get(|State(c): State<Calls>, Query(q): Query<HashMap<String, String>>| async move {
                c.push("folders", q["$filter"].clone());
                Json(json!({ "value": [{ "id": "folder-done" }] }))
            }),
        )
        .route(
            "/v1.0/me/mailFolders/{folder}/messages",
            get(|State(c): State<Calls>, Path(folder): Path<String>, headers: HeaderMap, Query(q): Query<HashMap<String, String>>| async move {
                c.push("list", format!("{folder} {} {}", bearer(&headers), q["$filter"]));
                Json(json!({ "value": [{ "id": "AAMk=1" }] }))
            }),
        )
        .route(
            "/v1.0/me/messages/{id}/$value",
            get(|State(c): State<Calls>, Path(id): Path<String>| async move {
                c.push("raw", id);
                mime("Graph laptop")
            }),
        )
        .route(
            "/v1.0/me/messages/{id}",
            patch(|State(c): State<Calls>, Path(id): Path<String>, Json(b): Json<Value>| async move {
                c.push("patch", format!("{id} {b}"));
                Json(json!({}))
            }),
        )
        .route(
            "/v1.0/me/messages/{id}/move",
            post(|State(c): State<Calls>, Path(id): Path<String>, Json(b): Json<Value>| async move {
                c.push("move", format!("{id} {}", b["destinationId"]));
                Json(json!({}))
            }),
        )
        .with_state(calls)
}

fn gmail_fake(calls: Calls) -> Router {
    Router::new()
        .route(
            "/token",
            post(|State(c): State<Calls>, Form(f): Form<HashMap<String, String>>| async move {
                c.push("token", format!("{} {}", f["grant_type"], f["refresh_token"]));
                Json(json!({ "access_token": "gmail-at" }))
            }),
        )
        .route(
            "/gmail/v1/users/me/labels",
            get(|| async { Json(json!({ "labels": [{ "id": "INBOX", "name": "INBOX" }, { "id": "Label_9", "name": "Cooper/Done" }] })) }),
        )
        .route(
            "/gmail/v1/users/me/messages",
            get(|State(c): State<Calls>, headers: HeaderMap, Query(q): Query<HashMap<String, String>>| async move {
                c.push("list", format!("{} {} {}", bearer(&headers), q["q"], q["labelIds"]));
                Json(json!({ "messages": [{ "id": "g2" }, { "id": "g1" }] }))
            }),
        )
        .route(
            "/gmail/v1/users/me/messages/{id}",
            get(|State(c): State<Calls>, Path(id): Path<String>| async move {
                c.push("raw", id.clone());
                let raw = base64::engine::general_purpose::URL_SAFE.encode(mime(&format!("Gmail {id}")));
                Json(json!({ "id": id, "raw": raw }))
            }),
        )
        .route(
            "/gmail/v1/users/me/messages/{id}/modify",
            post(|State(c): State<Calls>, Path(id): Path<String>, Json(b): Json<Value>| async move {
                c.push("modify", format!("{id} {b}"));
                Json(json!({}))
            }),
        )
        .with_state(calls)
}

async fn mailbox(admin: &Client, pool: &PgPool, body: Value) -> String {
    let company: String = sqlx::query_scalar("SELECT id::text FROM tenants WHERE is_default")
        .fetch_one(pool)
        .await
        .unwrap();
    let mut b = json!({ "name": "Desk", "address": "desk@acme.example", "company_id": company });
    b.as_object_mut()
        .unwrap()
        .extend(body.as_object().unwrap().clone());
    let r = admin.post("/api/v1/mailboxes", b).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(r.body.get("credentials").is_none());
    r.body["id"].as_str().unwrap().to_owned()
}

fn state(pool: &PgPool) -> AppState {
    AppState::with_settings(pool.clone(), Arc::new(SystemClock), common::test_settings()).unwrap()
}

async fn poll(pool: &PgPool, poller: MailboxPoller) -> String {
    let ctx = JobContext {
        pool: pool.clone(),
        clock: Arc::new(SystemClock),
    };
    poller.run(&ctx).await.unwrap().unwrap()
}

async fn subjects(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT subject FROM tickets ORDER BY created_at")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn credentials_are_checked_and_never_returned(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let company: String = sqlx::query_scalar("SELECT id::text FROM tenants WHERE is_default")
        .fetch_one(&pool)
        .await
        .unwrap();
    let bad = admin
        .post(
            "/api/v1/mailboxes",
            json!({ "name": "Desk", "address": "desk@acme.example", "company_id": company, "provider": "microsoft",
                    "auth_mode": "delegated", "credentials": { "tenant": "t", "client_id": "c", "client_secret": "s" } }),
        )
        .await;
    assert_eq!(
        bad.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        bad.body
    );
    assert!(
        bad.body["detail"]
            .as_str()
            .unwrap()
            .contains("refresh_token")
    );
    mailbox(
        &admin,
        &pool,
        json!({ "provider": "google", "credentials": { "client_id": "c", "client_secret": "top-secret", "refresh_token": "rt" } }),
    )
    .await;
    let stored: String = sqlx::query_scalar("SELECT secret_encrypted FROM mailboxes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!stored.contains("top-secret"));
    let listed = admin.get("/api/v1/mailboxes").await.body;
    assert_eq!(listed[0]["has_secret"], true);
    assert!(listed[0].get("credentials").is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn graph_delegated_mailbox_imports_moves_and_stores_the_rotated_token(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let calls = Calls::default();
    let base = serve(graph_fake(calls.clone())).await;
    let id = mailbox(
        &admin,
        &pool,
        json!({ "provider": "microsoft", "auth_mode": "delegated", "post_import_action": "move", "move_to_folder": "Cooper's Done",
                "credentials": { "tenant": "contoso.example", "client_id": "app", "client_secret": "s", "refresh_token": "first-rt" } }),
    )
    .await;
    let s = state(&pool);
    let poller = MailboxPoller::new(
        s.service_desk.service.clone(),
        s.service_desk.blobs.clone(),
        Arc::new(ImapClient),
    )
    .with_client(
        Provider::Microsoft,
        Arc::new(GraphClient::with_endpoints(&base, &format!("{base}/v1.0")).unwrap()),
    );
    let summary = poll(&pool, poller).await;
    assert!(summary.ends_with(": 1"), "{summary}");
    assert_eq!(subjects(&pool).await, ["Graph laptop"]);

    let calls = calls.all();
    assert_eq!(
        calls[0],
        (
            "token".into(),
            "contoso.example refresh_token first-rt".into()
        )
    );
    let list = calls.iter().find(|c| c.0 == "list").unwrap();
    assert!(list.1.starts_with("inbox Bearer graph-at "), "{list:?}");
    assert!(list.1.contains("isRead eq false"));
    assert!(calls.contains(&("raw".into(), "AAMk=1".into())));
    assert!(calls.contains(&("folders".into(), "displayName eq 'Cooper''s Done'".into())));
    assert!(calls.contains(&("patch".into(), r#"AAMk=1 {"isRead":true}"#.into())));
    assert!(calls.contains(&("move".into(), r#"AAMk=1 "folder-done""#.into())));

    // Microsoft rotated the refresh token: the new one is stored (encrypted).
    let secret = s
        .service_desk
        .service
        .mailbox_secret(id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    let creds: Value = serde_json::from_str(&secret).unwrap();
    assert_eq!(creds["refresh_token"], "rotated-rt");
    assert_eq!(creds["client_secret"], "s");
}

#[sqlx::test(migrations = "../../migrations")]
async fn graph_sign_in_errors_reach_mailbox_health(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let base = serve(graph_fake(Calls::default())).await;
    mailbox(
        &admin,
        &pool,
        json!({ "provider": "microsoft", "auth_mode": "delegated",
                "credentials": { "tenant": "t", "client_id": "app", "client_secret": "s", "refresh_token": "expired" } }),
    )
    .await;
    let s = state(&pool);
    let poller = MailboxPoller::new(
        s.service_desk.service.clone(),
        s.service_desk.blobs.clone(),
        Arc::new(ImapClient),
    )
    .with_client(
        Provider::Microsoft,
        Arc::new(GraphClient::with_endpoints(&base, &format!("{base}/v1.0")).unwrap()),
    );
    poll(&pool, poller).await;
    let health = admin.get("/api/v1/mailboxes").await.body[0]["health"].clone();
    assert_eq!(health["consecutive_failures"], 1);
    assert_eq!(
        health["last_error"],
        "sign-in failed: invalid_grant (AADSTS70008: The refresh token has expired.)"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn gmail_mailbox_imports_oldest_first_and_relabels(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let calls = Calls::default();
    let base = serve(gmail_fake(calls.clone())).await;
    mailbox(
        &admin,
        &pool,
        json!({ "provider": "google", "post_import_action": "move", "move_to_folder": "cooper/done",
                "credentials": { "client_id": "c", "client_secret": "s", "refresh_token": "google-rt" } }),
    )
    .await;
    let s = state(&pool);
    let poller = MailboxPoller::new(
        s.service_desk.service.clone(),
        s.service_desk.blobs.clone(),
        Arc::new(ImapClient),
    )
    .with_client(
        Provider::Google,
        Arc::new(
            GmailClient::with_endpoints(&format!("{base}/token"), &format!("{base}/gmail/v1"))
                .unwrap(),
        ),
    );
    let summary = poll(&pool, poller).await;
    assert!(summary.ends_with(": 2"), "{summary}");
    assert_eq!(subjects(&pool).await, ["Gmail g1", "Gmail g2"]);
    let calls = calls.all();
    assert_eq!(calls[0], ("token".into(), "refresh_token google-rt".into()));
    assert!(calls.contains(&("list".into(), "Bearer gmail-at is:unread INBOX".into())));
    assert!(calls.contains(&(
        "modify".into(),
        r#"g1 {"addLabelIds":["Label_9"],"removeLabelIds":["UNREAD","INBOX"]}"#.into()
    )));
    // No rotation for Google: the secret is unchanged.
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_entries WHERE aggregate_type = 'mailbox' AND action = 'updated'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn graph_app_only_mailbox_reads_the_users_mailbox(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let calls = Calls::default();
    let fake = Router::new()
        .route(
            "/{tenant}/oauth2/v2.0/token",
            post(
                |State(c): State<Calls>, Form(f): Form<HashMap<String, String>>| async move {
                    c.push("token", format!("{} {}", f["grant_type"], f["scope"]));
                    Json(json!({ "access_token": "app-at" }))
                },
            ),
        )
        .route(
            "/v1.0/users/{user}/mailFolders/inbox/messages",
            get(
                |State(c): State<Calls>, Path(user): Path<String>| async move {
                    c.push("list", user);
                    Json(json!({ "value": [] }))
                },
            ),
        )
        .with_state(calls.clone());
    let base = serve(fake).await;
    mailbox(
        &admin,
        &pool,
        json!({ "provider": "microsoft", "auth_mode": "app_only",
                "credentials": { "tenant": "t", "client_id": "app", "client_secret": "s" } }),
    )
    .await;
    let s = state(&pool);
    let poller = MailboxPoller::new(
        s.service_desk.service.clone(),
        s.service_desk.blobs.clone(),
        Arc::new(ImapClient),
    )
    .with_client(
        Provider::Microsoft,
        Arc::new(GraphClient::with_endpoints(&base, &format!("{base}/v1.0")).unwrap()),
    );
    let summary = poll(&pool, poller).await;
    assert!(summary.ends_with(": 0"), "{summary}");
    assert_eq!(
        calls.all(),
        [
            (
                "token".into(),
                "client_credentials https://graph.microsoft.com/.default".into()
            ),
            ("list".into(), "desk@acme.example".into()),
        ]
    );
}
