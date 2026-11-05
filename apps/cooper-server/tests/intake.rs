//! Email intake end to end (ADR-0018): Mailboxes, threading, Requester and Company
//! matching, loop protection, Whitelist, health alerts, and a real IMAP poll against
//! GreenMail (docker compose / CI service).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use common::{Client, setup};
use cooper_kernel::{Principal, SystemClock};
use cooper_platform::jobs::{Job, JobContext};
use cooper_platform::outbox::Dispatcher;
use cooper_server::AppState;
use cooper_service_desk::application::IntakeOutcome;
use cooper_service_desk::domain::{InboundEmail, Mailbox};
use http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn state(pool: &PgPool) -> AppState {
    AppState::with_settings(pool.clone(), Arc::new(SystemClock), common::test_settings()).unwrap()
}

async fn mailbox(admin: &Client, pool: &PgPool, extra: Value) -> Value {
    let company: String = sqlx::query_scalar("SELECT id::text FROM tenants WHERE is_default")
        .fetch_one(pool)
        .await
        .unwrap();
    let mut body = json!({
        "name": "Service Desk", "address": "desk@localhost", "provider": "imap",
        "imap_host": "localhost", "imap_port": 3143, "imap_security": "none", "imap_username": "desk@localhost",
        "password": "desk@localhost", "company_id": company
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let r = admin.post("/api/v1/mailboxes", body).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.body
}

async fn load(state: &AppState, id: &str) -> Mailbox {
    let all = state
        .service_desk
        .service
        .mailboxes(&Principal::automation())
        .await
        .unwrap();
    all.into_iter().find(|m| m.id().to_string() == id).unwrap()
}

fn email(from: &str, subject: &str, body: &str) -> InboundEmail {
    InboundEmail {
        message_id: format!("<{}@mail.example>", Uuid::now_v7()),
        from_address: from.into(),
        from_name: Some("Sam Smith".into()),
        to: vec!["desk@localhost".into()],
        subject: subject.into(),
        text_body: body.into(),
        ..InboundEmail::default()
    }
}

async fn dispatch(pool: &PgPool) {
    let s = state(pool);
    let d = s.event_handlers().into_iter().fold(
        Dispatcher::new(pool.clone(), Arc::new(SystemClock)),
        Dispatcher::with_handler,
    );
    d.dispatch_once().await.unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn mailbox_secrets_are_write_only_and_encrypted(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let m = mailbox(&admin, &pool, json!({})).await;
    assert!(m.get("password").is_none());
    let stored: String = sqlx::query_scalar("SELECT secret_encrypted FROM mailboxes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(stored.starts_with("v1.") && !stored.contains("desk@localhost"));
    let bad = admin.post("/api/v1/mailboxes", json!({ "name": "x", "address": "x@localhost", "provider": "imap", "company_id": Uuid::nil() })).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let listed = admin.get("/api/v1/mailboxes").await.body;
    assert_eq!(listed[0]["health"]["consecutive_failures"], 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn intake_creates_threads_and_reopens(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let acme = admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Acme", "ticket_code": "ACME", "sender_domains": ["acme.example"] }),
        )
        .await
        .body;
    let m = mailbox(&admin, &pool, json!({})).await;
    let s = state(&pool);
    let mb = load(&s, m["id"].as_str().unwrap()).await;
    let desk = &s.service_desk.service;

    // Unknown sender from a Sender Domain: Requester and ticket in that Company.
    let first = email("sam@acme.example", "Printer jammed", "It makes a noise.");
    let IntakeOutcome::Created(ticket) = desk.import_email(&mb, first.clone()).await.unwrap()
    else {
        panic!()
    };
    let t = admin.get(&format!("/api/v1/tickets/{ticket}")).await.body;
    assert_eq!(t["company_id"], acme["id"]);
    assert_eq!(t["number"], "ACME-000001");
    assert_eq!(t["description"], "It makes a noise.");
    let requesters = admin
        .get(&format!(
            "/api/v1/requesters?company_id={}",
            acme["id"].as_str().unwrap()
        ))
        .await
        .body;
    assert_eq!(requesters["items"][0]["email"], "sam@acme.example");
    assert_eq!(requesters["items"][0]["full_name"], "Sam Smith");
    assert_eq!(
        desk.import_email(&mb, first).await.unwrap(),
        IntakeOutcome::Duplicate
    );

    // A reply with the SDREF tag goes onto the ticket, and reopens it when closed.
    let closed = admin
        .get("/api/v1/ticket-statuses")
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["key"] == "closed")
        .unwrap()["id"]
        .clone();
    let code = admin.get("/api/v1/resolution-codes").await.body[0]["id"].clone();
    admin
        .post(
            &format!("/api/v1/tickets/{ticket}/status"),
            json!({ "status_id": closed, "resolution_code_id": code }),
        )
        .await;
    let reply = email(
        "sam@acme.example",
        "RE: [SDREF:ACME-000001] Printer jammed",
        "Still broken",
    );
    assert_eq!(
        desk.import_email(&mb, reply).await.unwrap(),
        IntakeOutcome::Appended(ticket)
    );
    let t = admin.get(&format!("/api/v1/tickets/{ticket}")).await.body;
    assert!(t["closed_at"].is_null(), "reopened");

    // Threading by In-Reply-To onto our own Reply.
    let ours = admin
        .post(
            &format!("/api/v1/tickets/{ticket}/replies"),
            json!({ "body": "Try turning it off and on." }),
        )
        .await
        .body;
    let mut threaded = email("sam@acme.example", "no tag here", "Did that");
    threaded.in_reply_to = Some(ours["message_id"].as_str().unwrap().to_owned());
    assert_eq!(
        desk.import_email(&mb, threaded).await.unwrap(),
        IntakeOutcome::Appended(ticket)
    );
    let emails = admin
        .get(&format!("/api/v1/tickets/{ticket}/emails"))
        .await
        .body;
    assert_eq!(
        emails.as_array().unwrap().len(),
        4,
        "first, reply, our reply, threaded"
    );

    // Freemail senders land in the Mailbox's default Company.
    let IntakeOutcome::Created(other) = desk
        .import_email(&mb, email("someone@gmail.com", "Hello", "x"))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        admin.get(&format!("/api/v1/tickets/{other}")).await.body["number"],
        "DEF-000001"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn loop_protection_whitelist_and_unknown_senders(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let m = mailbox(
        &admin,
        &pool,
        json!({
            "unknown_sender_action": "reject",
            "whitelist": [{ "pattern": "spam.example", "allow": false }]
        }),
    )
    .await;
    let s = state(&pool);
    let mb = load(&s, m["id"].as_str().unwrap()).await;
    let desk = &s.service_desk.service;

    let mut auto = email("sam@acme.example", "Out of office", "away");
    auto.headers = vec![("auto-submitted".into(), "auto-replied".into())];
    assert!(matches!(
        desk.import_email(&mb, auto).await.unwrap(),
        IntakeOutcome::Ignored(_)
    ));
    assert!(
        matches!(
            desk.import_email(&mb, email("desk@localhost", "loop", "x"))
                .await
                .unwrap(),
            IntakeOutcome::Ignored(_)
        ),
        "own mail"
    );
    assert!(matches!(
        desk.import_email(&mb, email("bot@spam.example", "Buy", "x"))
            .await
            .unwrap(),
        IntakeOutcome::Rejected(_)
    ));
    assert!(
        matches!(
            desk.import_email(&mb, email("new@nowhere.example", "Hi", "x"))
                .await
                .unwrap(),
            IntakeOutcome::Rejected(_)
        ),
        "unknown sender"
    );

    let rejected = admin
        .get(&format!(
            "/api/v1/mailboxes/{}/rejected-emails",
            m["id"].as_str().unwrap()
        ))
        .await
        .body;
    assert_eq!(rejected.as_array().unwrap().len(), 4);
    let tickets: i64 = sqlx::query_scalar("SELECT count(*) FROM tickets")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tickets, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn repeated_poll_failures_alert_administrators(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let m = mailbox(&admin, &pool, json!({ "imap_port": 1 })).await;
    let s = state(&pool);
    let ctx = JobContext {
        pool: pool.clone(),
        clock: Arc::new(SystemClock),
    };
    for _ in 0..3 {
        s.mailbox_poller().run(&ctx).await.unwrap();
    }
    let health = admin.get("/api/v1/mailboxes").await.body[0]["health"].clone();
    assert_eq!(health["consecutive_failures"], 3);
    assert!(health["last_error"].as_str().unwrap().contains("connect"));
    dispatch(&pool).await;
    let bell = admin.get("/api/v1/notifications").await.body;
    assert_eq!(bell["items"][0]["kind"], "mailbox_failing", "{bell:?}");
    let _ = m;
}

#[sqlx::test(migrations = "../../migrations")]
async fn imap_poll_against_greenmail(pool: PgPool) {
    use lettre::AsyncTransport as _;
    let host = std::env::var("COOPER_TEST_GREENMAIL_HOST").unwrap_or_else(|_| "localhost".into());
    if tokio::net::TcpStream::connect((host.as_str(), 3143))
        .await
        .is_err()
    {
        assert!(
            std::env::var("COOPER_REQUIRE_GREENMAIL").is_err(),
            "GreenMail is required but not reachable at {host}:3143"
        );
        eprintln!("GreenMail not reachable at {host}:3143; skipping");
        return;
    }
    let (_, admin) = setup(&pool).await;
    let user = format!("desk-{}@localhost", Uuid::now_v7());
    let m = mailbox(
        &admin,
        &pool,
        json!({ "address": user, "imap_host": host, "imap_username": user, "password": user }),
    )
    .await;

    let subject = format!("Laptop will not boot {}", Uuid::now_v7());
    let message = lettre::Message::builder()
        .from("Pat Poller <pat@acme.example>".parse().unwrap())
        .to(user.parse().unwrap())
        .subject(subject.clone())
        .header(lettre::message::header::ContentType::TEXT_PLAIN)
        .body("Black screen since this morning.".to_owned())
        .unwrap();
    lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(host.as_str())
        .port(3025)
        .build()
        .send(message)
        .await
        .unwrap();

    let s = state(&pool);
    let ctx = JobContext {
        pool: pool.clone(),
        clock: Arc::new(SystemClock),
    };
    let summary = s.mailbox_poller().run(&ctx).await.unwrap().unwrap();
    assert!(summary.ends_with(": 1"), "{summary}");
    let again = s.mailbox_poller().run(&ctx).await.unwrap().unwrap();
    assert!(
        again.ends_with(": 0"),
        "the message was marked read: {again}"
    );
    let found = admin
        .get(&format!("/api/v1/tickets?q={}", urlencode(&subject)))
        .await
        .body;
    assert_eq!(found["items"].as_array().unwrap().len(), 1, "{found:?}");
    let health = admin.get("/api/v1/mailboxes").await.body;
    assert!(
        health
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == m["id"] && x["health"]["last_success_at"].is_string())
    );
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
