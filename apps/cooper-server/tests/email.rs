//! Replies, the outbound email queue, Reply Templates, attachments and password reset,
//! end to end.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use common::{ADMIN_PASSWORD, Client, setup};
use cooper_kernel::SystemClock;
use cooper_notifications::email::{EmailSender, MemoryTransport};
use cooper_platform::jobs::{Job, JobContext};
use cooper_platform::outbox::Dispatcher;
use cooper_server::{AppState, SecuritySettings};
use http::{Method, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

async fn dispatch(pool: &PgPool) {
    let state = AppState::with_settings(
        pool.clone(),
        Arc::new(SystemClock),
        SecuritySettings::ephemeral().unwrap(),
    )
    .unwrap();
    let d = state.event_handlers().into_iter().fold(
        Dispatcher::new(pool.clone(), Arc::new(SystemClock)),
        Dispatcher::with_handler,
    );
    let stats = d.dispatch_once().await.unwrap();
    assert_eq!(stats.retried + stats.dead, 0, "{stats:?}");
}

async fn send_all(pool: &PgPool, transport: Arc<MemoryTransport>) -> String {
    let ctx = JobContext {
        pool: pool.clone(),
        clock: Arc::new(SystemClock),
    };
    EmailSender::new(Some(transport))
        .run(&ctx)
        .await
        .unwrap()
        .unwrap()
}

async fn ticket_with_requester(admin: &Client, pool: &PgPool) -> Value {
    let company: String = sqlx::query_scalar("SELECT id::text FROM tenants LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    let r = admin.post("/api/v1/requesters", json!({ "company_id": company, "full_name": "Sam Smith", "email": "sam@default.example" })).await.body;
    admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": company, "subject": "VPN drops", "requester_id": r["id"] }),
        )
        .await
        .body
}

#[sqlx::test(migrations = "../../migrations")]
async fn replies_are_threaded_signed_queued_and_sent(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    admin
        .put(
            "/api/v1/settings/email.from_address",
            json!({ "value": "Service Desk <desk@acme.example>" }),
        )
        .await;
    assert_eq!(
        admin
            .put(
                "/api/v1/session/signature",
                json!({ "signature": "Ada\nIT Service Desk" })
            )
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let t = ticket_with_requester(&admin, &pool).await;
    let id = t["id"].as_str().unwrap();
    let number = t["number"].as_str().unwrap();

    let reply = admin
        .post(
            &format!("/api/v1/tickets/{id}/replies"),
            json!({ "body": "Please restart the client.", "cc": ["boss@default.example"] }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(reply.body["subject"], format!("[SDREF:{number}] VPN drops"));
    assert_eq!(reply.body["to"], json!(["sam@default.example"]));
    assert!(
        reply.body["text_body"]
            .as_str()
            .unwrap()
            .ends_with("-- \nAda\nIT Service Desk")
    );
    assert!(
        reply.body["message_id"]
            .as_str()
            .unwrap()
            .ends_with("@acme.example>")
    );

    let detail = admin.get(&format!("/api/v1/tickets/{id}")).await.body;
    assert!(
        detail["sla"]["responded_at"].is_string(),
        "a Reply is the SLA response"
    );
    let kinds: Vec<String> = admin
        .get(&format!("/api/v1/tickets/{id}/timeline"))
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_owned())
        .collect();
    assert!(kinds.contains(&"email".to_owned()));
    assert_eq!(
        admin.get("/api/v1/tickets?q=restart").await.body["items"][0]["id"],
        t["id"],
        "email text is searchable"
    );

    // Second reply threads onto the first.
    let second = admin
        .post(
            &format!("/api/v1/tickets/{id}/replies"),
            json!({ "body": "Any luck?" }),
        )
        .await
        .body;
    dispatch(&pool).await;
    dispatch(&pool).await;
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbound_emails WHERE status = 'queued'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        queued, 2,
        "one queued email per reply, even when delivered twice"
    );

    let transport = Arc::new(MemoryTransport::default());
    assert_eq!(send_all(&pool, transport.clone()).await, "sent 2, failed 0");
    let sent = transport.sent();
    let follow_up = sent
        .iter()
        .find(|e| e.message_id == second["message_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        follow_up.in_reply_to.as_deref(),
        reply.body["message_id"].as_str()
    );
    assert_eq!(follow_up.from, "Service Desk <desk@acme.example>");
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbound_email_attempts WHERE succeeded")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attempts, 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn failed_sends_are_logged_retried_and_given_up(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let t = ticket_with_requester(&admin, &pool).await;
    admin
        .post(
            &format!("/api/v1/tickets/{}/replies", t["id"].as_str().unwrap()),
            json!({ "body": "Hello" }),
        )
        .await;
    dispatch(&pool).await;
    let transport = Arc::new(MemoryTransport::default());
    *transport.fail_with.lock().unwrap() = Some("550 mailbox unavailable".into());
    assert_eq!(send_all(&pool, transport.clone()).await, "sent 0, failed 1");
    let (status, attempts, error): (String, i32, Option<String>) =
        sqlx::query_as("SELECT status, attempts, last_error FROM outbound_emails")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (status.as_str(), attempts, error.as_deref()),
        ("queued", 1, Some("550 mailbox unavailable"))
    );

    sqlx::query("UPDATE outbound_emails SET attempts = 7, next_attempt_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    send_all(&pool, transport).await;
    let status: String = sqlx::query_scalar("SELECT status FROM outbound_emails")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "dead", "given up after the last attempt");
    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbound_email_attempts WHERE NOT succeeded")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(logged, 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn reply_needs_a_requester_and_templates_fill_placeholders(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let company: String = sqlx::query_scalar("SELECT id::text FROM tenants LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let lonely = admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": company, "subject": "No requester" }),
        )
        .await
        .body;
    let r = admin
        .post(
            &format!("/api/v1/tickets/{}/replies", lonely["id"].as_str().unwrap()),
            json!({ "body": "Hi" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let bad = admin
        .post(
            "/api/v1/reply-templates",
            json!({ "name": "Typo", "body": "Hi {requester}" }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let tpl = admin.post("/api/v1/reply-templates", json!({ "name": "Closing", "body": "Hi {requester_name}, {ticket_number} ({ticket_subject}) is fixed. {analyst_name}" })).await.body;
    let t = ticket_with_requester(&admin, &pool).await;
    let rendered = admin
        .get(&format!(
            "/api/v1/tickets/{}/reply-templates/{}",
            t["id"].as_str().unwrap(),
            tpl["id"].as_str().unwrap()
        ))
        .await
        .body;
    assert_eq!(
        rendered["text"],
        format!(
            "Hi Sam Smith, {} (VPN drops) is fixed. Ada Admin",
            t["number"].as_str().unwrap()
        )
    );
}

fn multipart(name: &str, content: &[u8]) -> (String, Vec<u8>) {
    let boundary = "cooperboundary42";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: text/html\r\n\r\n").into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[sqlx::test(migrations = "../../migrations")]
async fn attachments_are_stored_by_content_and_size_limited(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let t = ticket_with_requester(&admin, &pool).await;
    let id = t["id"].as_str().unwrap();
    let (ct, body) = multipart("../../report.pdf", b"%PDF-1.7 pretend");
    let (status, _, bytes) = admin
        .send_raw(
            Method::POST,
            &format!("/api/v1/tickets/{id}/attachments"),
            &ct,
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
    assert_eq!(a["file_name"], "report.pdf", "path parts removed");
    assert_eq!(
        a["media_type"], "application/pdf",
        "detected from content, not the client's text/html"
    );

    let list = admin
        .get(&format!("/api/v1/tickets/{id}/attachments"))
        .await
        .body;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (status, headers, content) = admin
        .send_raw(
            Method::GET,
            &format!(
                "/api/v1/tickets/{id}/attachments/{}/content",
                a["id"].as_str().unwrap()
            ),
            "text/plain",
            Vec::new(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content, b"%PDF-1.7 pretend");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"report.pdf\""
    );
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");

    admin
        .put(
            "/api/v1/settings/attachments.max_size_mb",
            json!({ "value": 1 }),
        )
        .await;
    let (ct, big) = multipart("big.bin", &vec![b'x'; 1024 * 1024 + 1]);
    let (status, _, _) = admin
        .send_raw(
            Method::POST,
            &format!("/api/v1/tickets/{id}/attachments"),
            &ct,
            big,
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[sqlx::test(migrations = "../../migrations")]
async fn password_reset_by_emailed_one_time_token(pool: PgPool) {
    let (anon, _) = setup(&pool).await;
    assert_eq!(
        anon.post("/api/v1/password-reset", json!({ "login": "ghost" }))
            .await
            .status,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        anon.post("/api/v1/password-reset", json!({ "login": "admin" }))
            .await
            .status,
        StatusCode::ACCEPTED
    );
    let body: String =
        sqlx::query_scalar("SELECT text_body FROM outbound_emails WHERE kind = 'password_reset'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let token = body
        .split("token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();

    let weak = anon
        .post(
            "/api/v1/password-reset/confirm",
            json!({ "token": token, "password": "short" }),
        )
        .await;
    assert_eq!(weak.status, StatusCode::UNPROCESSABLE_ENTITY);
    let ok = anon
        .post(
            "/api/v1/password-reset/confirm",
            json!({ "token": token, "password": "a brand new password" }),
        )
        .await;
    assert_eq!(ok.status, StatusCode::NO_CONTENT);
    let again = anon
        .post(
            "/api/v1/password-reset/confirm",
            json!({ "token": token, "password": "another new password" }),
        )
        .await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        anon.post(
            "/api/v1/session",
            json!({ "login": "admin", "password": ADMIN_PASSWORD })
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    anon.sign_in("admin", "a brand new password").await;
}
