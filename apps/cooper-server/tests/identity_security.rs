//! Two-factor sign-in, IP bans, API keys and encryption at rest, end to end (ADR-0013).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;

use chrono::{Duration, Utc};
use common::{ADMIN_PASSWORD, Client, setup, setup_with_clock};
use cooper_identity::domain::{STEP_SECONDS, TotpSecret};
use cooper_kernel::{Clock, FixedClock};
use http::{Method, StatusCode, header};
use serde_json::json;
use sqlx::PgPool;

/// A frozen server clock, so wall-clock TOTP step rollovers during a test cannot
/// change which codes the server accepts.
fn frozen_clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(Utc::now()))
}

/// The code for the server clock's current step plus `steps_ahead`.
fn code(clock: &FixedClock, secret: &str, steps_ahead: i64) -> String {
    let s = TotpSecret::from_base32(secret).unwrap();
    s.code_at_step(clock.now().timestamp().div_euclid(STEP_SECONDS) + steps_ahead)
}

async fn enroll(admin: &Client, clock: &FixedClock) -> String {
    let e = admin
        .post("/api/v1/session/totp/enrollment", json!({}))
        .await;
    assert_eq!(e.status, StatusCode::OK, "{:?}", e.body);
    let secret = e.body["secret"].as_str().unwrap().to_owned();
    assert!(
        e.body["otpauth_uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/Cooper:admin")
    );
    let confirm = admin
        .post(
            "/api/v1/session/totp/enrollment/confirm",
            json!({ "code": code(clock, &secret, 0) }),
        )
        .await;
    assert_eq!(confirm.status, StatusCode::NO_CONTENT, "{:?}", confirm.body);
    secret
}

#[sqlx::test(migrations = "../../migrations")]
async fn two_factor_sign_in_with_remembered_device(pool: PgPool) {
    let clock = frozen_clock();
    let (anon, admin) = setup_with_clock(&pool, clock.clone()).await;
    let secret = enroll(&admin, &clock).await;

    let stored: String =
        sqlx::query_scalar("SELECT totp_secret_encrypted FROM analysts WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        stored.starts_with("v1.") && !stored.contains(&secret),
        "secret is encrypted at rest"
    );

    let missing = anon
        .post(
            "/api/v1/session",
            json!({ "login": "admin", "password": ADMIN_PASSWORD }),
        )
        .await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        missing.body["type"],
        "https://cooper.dev/problems/totp-required"
    );

    let replay = anon
        .post(
            "/api/v1/session",
            json!({ "login": "admin", "password": ADMIN_PASSWORD, "totp_code": code(&clock, &secret, 0) }),
        )
        .await;
    assert_eq!(
        replay.body["type"], "https://cooper.dev/problems/totp-invalid",
        "the enrollment code cannot be reused"
    );

    clock.advance(Duration::seconds(STEP_SECONDS));
    let ok = anon.post("/api/v1/session", json!({
        "login": "admin", "password": ADMIN_PASSWORD, "totp_code": code(&clock, &secret, 0), "remember_device": true
    })).await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    let cookies: Vec<String> = ok
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    let device = cookies
        .iter()
        .find(|c| c.starts_with("cooper_trusted_device="))
        .expect("device cookie");
    assert!(device.contains("Path=/api/v1/session") && device.contains("HttpOnly"));

    let remembered = Client {
        cookie: Some(device.split(';').next().unwrap().to_owned()),
        ..anon.clone()
    };
    let skip = remembered
        .post(
            "/api/v1/session",
            json!({ "login": "admin", "password": ADMIN_PASSWORD }),
        )
        .await;
    assert_eq!(
        skip.status,
        StatusCode::OK,
        "remembered browser skips the code: {:?}",
        skip.body
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn required_two_factor_blocks_everything_but_enrollment(pool: PgPool) {
    sqlx::query("INSERT INTO system_settings (key, value, updated_at) VALUES ('security.require_totp', 'true', now())")
        .execute(&pool).await.unwrap();
    let clock = frozen_clock();
    let (_, admin) = setup_with_clock(&pool, clock.clone()).await;
    let session = admin.get("/api/v1/session").await;
    assert_eq!(session.body["totp_enrollment_required"], true);
    let blocked = admin.get("/api/v1/analysts").await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
    assert_eq!(
        blocked.body["type"],
        "https://cooper.dev/problems/totp-enrollment-required"
    );

    let secret = enroll(&admin, &clock).await;
    assert_eq!(admin.get("/api/v1/analysts").await.status, StatusCode::OK);
    let off = admin
        .send(
            Method::DELETE,
            "/api/v1/session/totp",
            Some(json!({ "code": code(&clock, &secret, 1) })),
        )
        .await;
    assert_eq!(
        off.status,
        StatusCode::CONFLICT,
        "cannot turn off what policy requires"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn repeated_failures_from_one_ip_are_banned(pool: PgPool) {
    let (anon, _) = setup(&pool).await;
    let attacker = Client {
        peer: Some("198.51.100.20:50000".parse().unwrap()),
        ..anon.clone()
    };
    for _ in 0..9 {
        let r = attacker
            .post(
                "/api/v1/session",
                json!({ "login": "admin", "password": "not the password" }),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let banned = attacker
        .post(
            "/api/v1/session",
            json!({ "login": "admin", "password": "not the password" }),
        )
        .await;
    assert_eq!(banned.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(banned.headers.get(header::RETRY_AFTER).is_some());
    let right = attacker
        .post(
            "/api/v1/session",
            json!({ "login": "admin", "password": ADMIN_PASSWORD }),
        )
        .await;
    assert_eq!(
        right.status,
        StatusCode::TOO_MANY_REQUESTS,
        "banned even with the right password"
    );

    let other = Client {
        peer: Some("198.51.100.21:50000".parse().unwrap()),
        ..anon
    };
    assert_eq!(
        other
            .post(
                "/api/v1/session",
                json!({ "login": "admin", "password": ADMIN_PASSWORD })
            )
            .await
            .status,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn api_keys_authenticate_with_capabilities_and_rate_limits(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let created = admin
        .post(
            "/api/v1/api-keys",
            json!({
                "name": "Monitoring", "capabilities": ["requesters.read"],
                "company_access": { "all": true }, "rate_limit_per_minute": 3
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let key = created.body["key"].as_str().unwrap().to_owned();
    let id = created.body["api_key"]["id"].as_str().unwrap().to_owned();
    assert!(key.starts_with("cpr_"));

    let listed = admin.get("/api/v1/api-keys").await;
    assert!(
        !listed.body.to_string().contains(&key),
        "the full key is never shown again"
    );

    let bearer = Client {
        headers: vec![("authorization".into(), format!("Bearer {key}"))],
        ..anon.clone()
    };
    assert_eq!(
        bearer.get("/api/v1/requesters").await.status,
        StatusCode::OK
    );
    let denied = bearer
        .post("/api/v1/teams", json!({ "name": "Nope" }))
        .await;
    assert_eq!(
        denied.status,
        StatusCode::FORBIDDEN,
        "no CSRF needed, but the Capability is"
    );
    // Limit 3 per minute: within at most 8 more requests one is refused, even if a minute
    // boundary resets the counter once.
    let mut limited = None;
    for _ in 0..8 {
        let r = bearer.get("/api/v1/requesters").await;
        if r.status == StatusCode::TOO_MANY_REQUESTS {
            limited = Some(r);
            break;
        }
    }
    let limited = limited.expect("rate limit applies");
    assert!(limited.headers.get(header::RETRY_AFTER).is_some());

    let wrong = Client {
        headers: vec![("authorization".into(), "Bearer cpr_deadbeef_nope".into())],
        ..anon.clone()
    };
    assert_eq!(
        wrong.get("/api/v1/requesters").await.status,
        StatusCode::UNAUTHORIZED
    );

    assert_eq!(
        admin
            .post(&format!("/api/v1/api-keys/{id}/revoke"), json!({}))
            .await
            .status,
        StatusCode::OK
    );
    sqlx::query("DELETE FROM api_key_usage")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        bearer.get("/api/v1/requesters").await.status,
        StatusCode::UNAUTHORIZED
    );

    let audits: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT change_set_id) FROM audit_entries WHERE aggregate_type = 'api_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 2, "creation and revocation are audited");
}

#[sqlx::test(migrations = "../../migrations")]
async fn only_administrators_manage_api_keys(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    admin.post("/api/v1/analysts", json!({
        "username": "desk", "email": "desk@default.example", "full_name": "Dee Desk", "password": "desk password 123"
    })).await;
    let desk = anon.sign_in("desk", "desk password 123").await;
    assert_eq!(
        desk.get("/api/v1/api-keys").await.status,
        StatusCode::FORBIDDEN
    );
}
