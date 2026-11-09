//! Portal Notifications and Notification Preferences (FR-PRT-007, ADR-0033): Requesters
//! see only their own, per Company, and their preferences decide what they get in-app
//! and which Email Templates they are sent.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use common::{Client, setup};
use cooper_kernel::SystemClock;
use cooper_platform::outbox::Dispatcher;
use cooper_server::{AppState, SecuritySettings};
use http::{StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

const PORTAL_PASSWORD: &str = "my portal password";

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
    // Handlers may raise further events (Email Templates queue an email).
    for _ in 0..3 {
        let stats = d.dispatch_once().await.unwrap();
        assert_eq!(stats.retried + stats.dead, 0, "{stats:?}");
    }
}

async fn link_token(pool: &PgPool, to: &str) -> String {
    let body: String = sqlx::query_scalar(
        "SELECT text_body FROM outbound_emails WHERE kind = 'portal_invitation' AND $1 = ANY(to_addresses)
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(to)
    .fetch_one(pool)
    .await
    .unwrap();
    let start = body.find("/portal/set-password?token=").unwrap() + 27;
    body[start..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect()
}

async fn requester(admin: &Client, company: &Value, email: &str) -> Value {
    let reply = admin
        .post(
            "/api/v1/requesters",
            json!({ "company_id": company, "full_name": "Sam Smith", "email": email }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body
}

async fn portal_for(pool: &PgPool, anon: &Client, admin: &Client, requester: &Value) -> Client {
    let email = requester["email"].as_str().unwrap().to_owned();
    let path = format!(
        "/api/v1/requesters/{}/portal-invitation",
        requester["id"].as_str().unwrap()
    );
    assert_eq!(
        admin.post(&path, json!({})).await.status,
        StatusCode::ACCEPTED
    );
    let token = link_token(pool, &email).await;
    let reply = anon
        .post(
            "/api/v1/portal/password",
            json!({ "token": token, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let reply = anon
        .post(
            "/api/v1/portal/session",
            json!({ "email": email, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let cookie = reply.headers[header::SET_COOKIE].to_str().unwrap();
    Client {
        cookie: Some(cookie.split(';').next().unwrap().to_owned()),
        csrf: Some(reply.body["csrf_token"].as_str().unwrap().to_owned()),
        ..anon.clone()
    }
}

async fn key_id(admin: &Client, path: &str, key: &str) -> Value {
    admin
        .get(&format!("/api/v1/{path}"))
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["key"] == key)
        .unwrap_or_else(|| panic!("{key} in {path}"))["id"]
        .clone()
}

async fn ticket(admin: &Client, company: &Value, requester: &Value, subject: &str) -> String {
    let reply = admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": company, "subject": subject, "requester_id": requester["id"] }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body["id"].as_str().unwrap().to_owned()
}

async fn close(admin: &Client, id: &str) {
    let closed = key_id(admin, "ticket-statuses", "closed").await;
    let fixed = key_id(admin, "resolution-codes", "fixed_remotely").await;
    let reply = admin
        .post(
            &format!("/api/v1/tickets/{id}/status"),
            json!({ "status_id": closed, "resolution_code_id": fixed }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
}

fn kinds(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["kind"].as_str().unwrap().to_owned())
        .collect()
}

async fn template_emails(pool: &PgPool, to: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM outbound_emails WHERE kind = 'email_template' AND $1 = ANY(to_addresses)",
    )
    .bind(to)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn requesters_see_only_their_own_portal_notifications_per_company(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let default = admin.get("/api/v1/companies").await.body["items"][0]["id"].clone();
    let globex = admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Globex", "ticket_code": "GLX", "sender_domains": [] }),
        )
        .await
        .body["id"]
        .clone();
    let sam = requester(&admin, &default, "sam@contractor.example").await;
    let sam_at_globex = requester(&admin, &globex, "sam@contractor.example").await;
    let lee = requester(&admin, &default, "lee@contractor.example").await;
    let sam_portal = portal_for(&pool, &anon, &admin, &sam).await;
    let lee_portal = portal_for(&pool, &anon, &admin, &lee).await;

    let t = ticket(&admin, &default, &sam, "Printer jam").await;
    ticket(&admin, &globex, &sam_at_globex, "Globex laptop").await;
    let reply = admin
        .post(
            &format!("/api/v1/tickets/{t}/replies"),
            json!({ "body": "Please power-cycle it." }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let internal = admin
        .post(
            &format!("/api/v1/tickets/{t}/notes"),
            json!({ "body": "Internal only" }),
        )
        .await;
    assert_eq!(internal.status, StatusCode::CREATED, "{:?}", internal.body);
    close(&admin, &t).await;
    dispatch(&pool).await;
    dispatch(&pool).await; // redelivery changes nothing

    let mine = sam_portal.get("/api/v1/portal/notifications").await;
    assert_eq!(mine.status, StatusCode::OK, "{:?}", mine.body);
    assert_eq!(
        kinds(&mine.body),
        ["ticket_closed", "ticket_replied", "ticket_created"],
        "newest first, no internal Note, nothing from Globex"
    );
    assert_eq!(mine.body["items"][0]["ticket_id"], t.as_str());
    assert!(
        mine.body["items"][0]["title"]
            .as_str()
            .unwrap()
            .contains("closed")
    );
    assert_eq!(
        sam_portal
            .get("/api/v1/portal/notifications/unread-count")
            .await
            .body["unread"],
        3
    );
    assert!(kinds(&lee_portal.get("/api/v1/portal/notifications").await.body).is_empty());

    // Lee can neither read nor mark Sam's.
    let sams = mine.body["items"][0]["id"].as_str().unwrap();
    assert_eq!(
        lee_portal
            .post(
                &format!("/api/v1/portal/notifications/{sams}/read"),
                json!({})
            )
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        sam_portal
            .post(
                &format!("/api/v1/portal/notifications/{sams}/read"),
                json!({})
            )
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        kinds(
            &sam_portal
                .get("/api/v1/portal/notifications?unread=true")
                .await
                .body
        ),
        ["ticket_replied", "ticket_created"]
    );

    // The same Portal Account acting for Sam at Globex sees Globex's only.
    let switched = sam_portal
        .put(
            "/api/v1/portal/session/requester",
            json!({ "requester_id": sam_at_globex["id"] }),
        )
        .await;
    assert_eq!(switched.status, StatusCode::OK, "{:?}", switched.body);
    assert_eq!(
        kinds(&sam_portal.get("/api/v1/portal/notifications").await.body),
        ["ticket_created"]
    );
    assert_eq!(
        sam_portal
            .post("/api/v1/portal/notifications/read-all", json!({}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        sam_portal
            .get("/api/v1/portal/notifications/unread-count")
            .await
            .body["unread"],
        0
    );

    // Portal endpoints are for Portal sessions only.
    assert_eq!(
        admin.get("/api/v1/portal/notifications").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        anon.get("/api/v1/portal/notification-preferences")
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn notification_preferences_decide_in_app_and_email(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let default = admin.get("/api/v1/companies").await.body["items"][0]["id"].clone();
    for (event, subject) in [("ticket_created", "Logged"), ("ticket_closed", "Closed")] {
        let r = admin
            .post(
                "/api/v1/email-templates",
                json!({ "name": subject, "event": event, "subject": subject, "body": "Hi {requester_name}" }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }
    let sam = requester(&admin, &default, "sam@default.example").await;
    let portal = portal_for(&pool, &anon, &admin, &sam).await;

    let defaults = portal.get("/api/v1/portal/notification-preferences").await;
    assert_eq!(defaults.status, StatusCode::OK);
    assert_eq!(
        defaults.body["preferences"],
        json!([
            { "kind": "ticket_created", "in_app": true, "email": true },
            { "kind": "ticket_replied", "in_app": true, "email": true },
            { "kind": "ticket_closed", "in_app": true, "email": true }
        ])
    );
    let bad = portal
        .put(
            "/api/v1/portal/notification-preferences",
            json!({ "preferences": [{ "kind": "everything", "in_app": false, "email": false }] }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let saved = portal
        .put(
            "/api/v1/portal/notification-preferences",
            json!({ "preferences": [
                { "kind": "ticket_closed", "in_app": false, "email": true },
                { "kind": "ticket_created", "in_app": true, "email": false }
            ] }),
        )
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{:?}", saved.body);
    assert_eq!(saved.body["preferences"][1]["kind"], "ticket_replied");
    assert_eq!(saved.body["preferences"][2]["in_app"], false);
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_entries WHERE aggregate_type = 'requester' AND aggregate_id = $1::uuid
           AND actor_kind = 'requester'",
    )
    .bind(sam["id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);

    let t = ticket(&admin, &default, &sam, "Monitor flickers").await;
    close(&admin, &t).await;
    dispatch(&pool).await;
    assert_eq!(
        kinds(&portal.get("/api/v1/portal/notifications").await.body),
        ["ticket_created"],
        "closure is off in-app"
    );
    let subjects: Vec<String> = sqlx::query_scalar(
        "SELECT subject FROM outbound_emails WHERE kind = 'email_template' AND 'sam@default.example' = ANY(to_addresses)",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(subjects.len(), 1, "{subjects:?}");
    assert!(subjects[0].contains("Closed"), "creation email is off");
    assert_eq!(template_emails(&pool, "sam@default.example").await, 1);
}
