//! Portal sign-in (FR-PRT-001, ADR-0021): Portal Invitations, set-password links, Portal
//! sessions separate from Analyst sessions, switching Requesters and Self-Registration.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use common::{Client, setup};
use http::{Method, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

const PORTAL_PASSWORD: &str = "my portal password";

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
    body[start..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect()
}

async fn create_requester(admin: &Client, company: &Value, email: &str) -> Value {
    let reply = admin
        .post(
            "/api/v1/requesters",
            json!({ "company_id": company, "full_name": "Sam Smith", "email": email }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body
}

async fn portal_sign_in(anon: &Client, email: &str) -> Client {
    let reply = anon
        .post(
            "/api/v1/portal/session",
            json!({ "email": email, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    let set_cookie = reply.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(set_cookie.starts_with("cooper_portal_session="));
    assert!(set_cookie.contains("Path=/api/v1/portal") && set_cookie.contains("HttpOnly"));
    Client {
        cookie: Some(set_cookie.split(';').next().unwrap().to_owned()),
        csrf: Some(reply.body["csrf_token"].as_str().unwrap().to_owned()),
        ..anon.clone()
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn invited_requester_signs_in_to_the_portal_only(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let companies = admin.get("/api/v1/companies").await.body;
    let default = companies["items"][0]["id"].clone();
    let sam = create_requester(&admin, &default, "sam@default.example").await;

    let reply = anon
        .post(
            &format!(
                "/api/v1/requesters/{}/portal-invitation",
                sam["id"].as_str().unwrap()
            ),
            json!({}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    let reply = admin
        .post(
            &format!(
                "/api/v1/requesters/{}/portal-invitation",
                sam["id"].as_str().unwrap()
            ),
            json!({}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{:?}", reply.body);
    let token = link_token(&pool, "portal_invitation", "sam@default.example").await;

    let reply = anon
        .post(
            "/api/v1/portal/password",
            json!({ "token": token, "password": "short" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let reply = anon
        .post(
            "/api/v1/portal/password",
            json!({ "token": token, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
    let reply = anon
        .post(
            "/api/v1/portal/password",
            json!({ "token": token, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "links work once");

    let wrong = anon
        .post(
            "/api/v1/portal/session",
            json!({ "email": "sam@default.example", "password": "not my password" }),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);

    let portal = portal_sign_in(&anon, "SAM@default.example").await;
    let session = portal.get("/api/v1/portal/session").await;
    assert_eq!(session.status, StatusCode::OK);
    assert_eq!(session.body["requester"]["id"], sam["id"]);
    assert_eq!(session.body["requester"]["company_name"], "Default Company");

    // The Portal cookie never authenticates Analyst endpoints, and an Analyst session
    // never authenticates the Portal.
    assert_eq!(
        portal.get("/api/v1/requesters").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        portal.get("/api/v1/session").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        admin.get("/api/v1/portal/session").await.status,
        StatusCode::UNAUTHORIZED
    );

    // Unsafe Portal requests need the CSRF token.
    let no_csrf = Client {
        csrf: None,
        ..portal.clone()
    };
    assert_eq!(
        no_csrf
            .send(Method::DELETE, "/api/v1/portal/session", None)
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Deactivating the Requester ends Portal access.
    let reply = admin
        .post(
            &format!(
                "/api/v1/requesters/{}/deactivate",
                sam["id"].as_str().unwrap()
            ),
            json!({}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        portal.get("/api/v1/portal/session").await.status,
        StatusCode::UNAUTHORIZED
    );
    let reply = admin
        .post(
            &format!(
                "/api/v1/requesters/{}/reactivate",
                sam["id"].as_str().unwrap()
            ),
            json!({}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        portal.get("/api/v1/portal/session").await.status,
        StatusCode::OK
    );

    let out = portal
        .send(Method::DELETE, "/api/v1/portal/session", None)
        .await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert_eq!(
        portal.get("/api/v1/portal/session").await.status,
        StatusCode::UNAUTHORIZED
    );
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT change_set_id) FROM audit_entries WHERE aggregate_type = 'portal_account'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn one_portal_account_switches_between_companies_and_reset_ends_sessions(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let default = admin.get("/api/v1/companies").await.body["items"][0]["id"].clone();
    let globex = admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Globex", "ticket_code": "GLX", "sender_domains": [] }),
        )
        .await;
    assert_eq!(globex.status, StatusCode::CREATED, "{:?}", globex.body);
    let first = create_requester(&admin, &default, "pat@contractor.example").await;
    let second = create_requester(&admin, &globex.body["id"], "pat@contractor.example").await;

    let reply = anon
        .post(
            "/api/v1/portal/password-reset",
            json!({ "email": "nobody@contractor.example" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    let reply = anon
        .post(
            "/api/v1/portal/password-reset",
            json!({ "email": "pat@contractor.example" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    let token = link_token(&pool, "portal_password_reset", "pat@contractor.example").await;
    let reply = anon
        .post(
            "/api/v1/portal/password",
            json!({ "token": token, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);

    let portal = portal_sign_in(&anon, "pat@contractor.example").await;
    let session = portal.get("/api/v1/portal/session").await.body;
    assert_eq!(session["requester"]["id"], first["id"]);
    assert_eq!(session["requesters"].as_array().unwrap().len(), 2);
    let switched = portal
        .put(
            "/api/v1/portal/session/requester",
            json!({ "requester_id": second["id"] }),
        )
        .await;
    assert_eq!(switched.status, StatusCode::OK, "{:?}", switched.body);
    assert_eq!(switched.body["requester"]["company_name"], "Globex");
    assert_eq!(
        portal.get("/api/v1/portal/session").await.body["requester"]["id"],
        second["id"]
    );
    let stranger = create_requester(&admin, &default, "lee@contractor.example").await;
    let reply = portal
        .put(
            "/api/v1/portal/session/requester",
            json!({ "requester_id": stranger["id"] }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    // Setting a new password through a link ends the other Portal sessions.
    anon.post(
        "/api/v1/portal/password-reset",
        json!({ "email": "pat@contractor.example" }),
    )
    .await;
    let token = link_token(&pool, "portal_password_reset", "pat@contractor.example").await;
    let reply = anon
        .post(
            "/api/v1/portal/password",
            json!({ "token": token, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(
        portal.get("/api/v1/portal/session").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn self_registration_confirms_the_address_before_creating_the_requester(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let register = |email: &str| json!({ "full_name": "Nia New", "email": email });
    let reply = anon
        .post(
            "/api/v1/portal/registration",
            register("nia@default.example"),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "off by default");

    let reply = admin
        .put(
            "/api/v1/settings/portal.self_registration",
            json!({ "value": true }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    // A fresh server, so the cached security policy is reloaded.
    let anon = Client::anonymous(&pool);
    let reply = anon
        .post("/api/v1/portal/registration", register("nia@gmail.com"))
        .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        reply.body
    );
    let reply = anon
        .post(
            "/api/v1/portal/registration",
            register("nia@default.example"),
        )
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{:?}", reply.body);
    let requesters = admin.get("/api/v1/requesters").await.body;
    assert_eq!(
        requesters["items"].as_array().unwrap().len(),
        0,
        "nothing before confirmation"
    );

    let token = link_token(&pool, "portal_registration", "nia@default.example").await;
    let reply = anon
        .post(
            "/api/v1/portal/password",
            json!({ "token": token, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{:?}", reply.body);
    let requesters = admin.get("/api/v1/requesters").await.body;
    assert_eq!(requesters["items"][0]["full_name"], "Nia New");
    let portal = portal_sign_in(&anon, "nia@default.example").await;
    assert_eq!(
        portal.get("/api/v1/portal/session").await.body["requester"]["company_name"],
        "Default Company"
    );

    // The Portal Module switch turns the Portal API off.
    let reply = admin
        .put("/api/v1/modules/portal", json!({ "is_enabled": false }))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(
        Client::anonymous(&pool)
            .post(
                "/api/v1/portal/session",
                json!({ "email": "nia@default.example", "password": PORTAL_PASSWORD }),
            )
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}
