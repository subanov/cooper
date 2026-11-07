//! User Groups (FR-IAM-016), Manager Grants with the Portal manager view (FR-IAM-015,
//! FR-PRT-004) and OIDC single sign-on (FR-IAM-012) end to end (ADR-0025). The OIDC
//! tests run a small identity provider in the test process.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Form, State};
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{Client, setup};
use http::{StatusCode, header};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use sqlx::PgPool;

const PORTAL_PASSWORD: &str = "my portal password";

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

async fn default_company(admin: &Client) -> Value {
    admin.get("/api/v1/companies").await.body["items"][0]["id"].clone()
}

async fn requester(admin: &Client, company: &Value, email: &str, extra: Value) -> Value {
    let mut body = json!({ "company_id": company, "full_name": email, "email": email });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let r = admin.post("/api/v1/requesters", body).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.body
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

async fn portal_for(pool: &PgPool, anon: &Client, admin: &Client, requester: &Value) -> Client {
    let email = requester["email"].as_str().unwrap();
    let r = admin
        .post(
            &format!("/api/v1/requesters/{}/portal-invitation", id(requester)),
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    let token = link_token(pool, email).await;
    let r = anon
        .post(
            "/api/v1/portal/password",
            json!({ "token": token, "password": PORTAL_PASSWORD }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = anon
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
    Client {
        cookie: Some(cookie),
        csrf: Some(r.body["csrf_token"].as_str().unwrap().to_owned()),
        ..anon.clone()
    }
}

async fn ticket_for(admin: &Client, company: &Value, requester: &Value, extra: Value) -> String {
    let mut body = json!({ "company_id": company, "subject": format!("Ticket of {}", requester["email"]),
                           "requester_id": requester["id"] });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let r = admin.post("/api/v1/tickets", body).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.body["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_groups_hold_analysts_and_requesters_with_expiry(pool: PgPool) {
    let (_, admin) = setup(&pool).await;
    let company = default_company(&admin).await;
    let sam = requester(&admin, &company, "sam@default.example", json!({})).await;
    let me = admin.get("/api/v1/session").await.body["analyst"]["id"].clone();

    let g = admin
        .post("/api/v1/user-groups", json!({ "name": "Finance readers" }))
        .await;
    assert_eq!(g.status, StatusCode::CREATED, "{:?}", g.body);
    assert_eq!(
        admin
            .post("/api/v1/user-groups", json!({ "name": "finance READERS" }))
            .await
            .status,
        StatusCode::CONFLICT
    );
    let path = format!("/api/v1/user-groups/{}/members", id(&g.body));
    let set = admin
        .put(
            &path,
            json!({ "members": [
            { "kind": "requester", "id": sam["id"], "expires_at": "2030-01-01T00:00:00Z" },
            { "kind": "analyst", "id": me }
        ] }),
        )
        .await;
    assert_eq!(set.status, StatusCode::OK, "{:?}", set.body);
    assert_eq!(set.body.as_array().unwrap().len(), 2);
    assert_eq!(
        admin
            .put(
                &path,
                json!({ "members": [{ "kind": "requester", "id": uuid::Uuid::now_v7() }] })
            )
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let history = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM audit_entries WHERE aggregate_type = 'user_group'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(history >= 2, "creation and membership changes are audited");
}

async fn enable_managers(admin: &Client, confidential: bool) {
    for (key, value) in [
        ("portal.managers_enabled", json!(true)),
        ("portal.managers_confidential", json!(confidential)),
    ] {
        let r = admin
            .put(
                &format!("/api/v1/settings/{key}"),
                json!({ "value": value }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{key}: {:?}", r.body);
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn managers_read_the_tickets_their_grants_cover_and_nothing_else(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let company = default_company(&admin).await;
    let other = admin
        .post(
            "/api/v1/companies",
            json!({ "name": "Other", "ticket_code": "OTH", "sender_domains": ["other.example"] }),
        )
        .await
        .body["id"]
        .clone();
    let cc = admin
        .post(
            "/api/v1/cost-centres",
            json!({ "company_id": company, "code": "SALES", "name": "Sales" }),
        )
        .await;
    assert_eq!(cc.status, StatusCode::CREATED, "{:?}", cc.body);

    let boss = requester(&admin, &company, "boss@default.example", json!({})).await;
    let report = requester(
        &admin,
        &company,
        "report@default.example",
        json!({ "manager_id": boss["id"] }),
    )
    .await;
    let seller = requester(
        &admin,
        &company,
        "seller@default.example",
        json!({ "cost_centre_id": cc.body["id"] }),
    )
    .await;
    let griever = requester(
        &admin,
        &company,
        "griever@default.example",
        json!({ "cost_centre_id": cc.body["id"] }),
    )
    .await;
    let stranger = requester(&admin, &other, "stranger@other.example", json!({})).await;

    let grants = format!("/api/v1/requesters/{}/manager-grants", id(&boss));
    for body in [
        json!({ "kind": "cost_centre", "target_id": cc.body["id"] }),
        json!({ "kind": "requester", "target_id": griever["id"], "is_exclusion": true }),
    ] {
        let r = admin.post(&grants, body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }
    assert_eq!(
        admin
            .post(
                &grants,
                json!({ "kind": "requester", "target_id": stranger["id"] })
            )
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "grants never cross Companies"
    );
    assert_eq!(
        admin
            .post(&grants, json!({ "kind": "everyone", "is_exclusion": true }))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let report_ticket = ticket_for(&admin, &company, &report, json!({})).await;
    let seller_ticket = ticket_for(&admin, &company, &seller, json!({})).await;
    let griever_ticket = ticket_for(&admin, &company, &griever, json!({})).await;
    let secret = ticket_for(
        &admin,
        &company,
        &seller,
        json!({ "sensitivity": "confidential" }),
    )
    .await;
    ticket_for(&admin, &other, &stranger, json!({})).await;

    let portal = portal_for(&pool, &anon, &admin, &boss).await;
    let listed = |body: &Value| -> Vec<String> {
        let mut ids: Vec<String> = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_owned())
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(
        portal.get("/api/v1/portal/managed-tickets").await.body["items"],
        json!([]),
        "Managers are off by default"
    );
    assert_eq!(
        portal
            .get(&format!("/api/v1/portal/tickets/{report_ticket}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    enable_managers(&admin, false).await;
    let mut expected = vec![report_ticket.clone(), seller_ticket.clone()];
    expected.sort();
    assert_eq!(
        listed(&portal.get("/api/v1/portal/managed-tickets").await.body),
        expected
    );
    assert_eq!(
        portal
            .get(&format!("/api/v1/portal/tickets/{seller_ticket}"))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        portal
            .get(&format!("/api/v1/portal/tickets/{report_ticket}/timeline"))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        portal
            .get(&format!("/api/v1/portal/tickets/{griever_ticket}"))
            .await
            .status,
        StatusCode::NOT_FOUND,
        "the exclusion wins over the Cost Centre grant"
    );
    assert_eq!(
        portal
            .get(&format!("/api/v1/portal/tickets/{secret}"))
            .await
            .status,
        StatusCode::NOT_FOUND,
        "Confidential tickets are hidden from managers by default"
    );
    assert_eq!(
        portal
            .post(
                &format!("/api/v1/portal/tickets/{seller_ticket}/replies"),
                json!({ "body": "Any news?" })
            )
            .await
            .status,
        StatusCode::NOT_FOUND,
        "managers read; they do not reply for others"
    );

    enable_managers(&admin, true).await;
    expected.push(secret.clone());
    expected.sort();
    assert_eq!(
        listed(&portal.get("/api/v1/portal/managed-tickets").await.body),
        expected
    );
}

// --- OIDC ------------------------------------------------------------------------------

#[derive(Clone)]
struct Idp {
    issuer: String,
    key: Arc<EcdsaKeyPair>,
    /// What the authorization request carried, keyed by state: (nonce, code challenge).
    pending: Arc<Mutex<HashMap<String, (String, String)>>>,
    /// The next ID token's claims (merged over the defaults).
    claims: Arc<Mutex<Value>>,
}

impl Idp {
    fn sign(&self, payload: &Value) -> String {
        let h = URL_SAFE_NO_PAD
            .encode(json!({ "alg": "ES256", "kid": "k1", "typ": "JWT" }).to_string());
        let p = URL_SAFE_NO_PAD.encode(payload.to_string());
        let sig = self
            .key
            .sign(&SystemRandom::new(), format!("{h}.{p}").as_bytes())
            .unwrap();
        format!("{h}.{p}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
    }
}

async fn start_idp() -> Idp {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let key = Arc::new(
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let idp = Idp {
        issuer: issuer.clone(),
        key,
        pending: Arc::new(Mutex::new(HashMap::new())),
        claims: Arc::new(Mutex::new(json!({}))),
    };
    let point = idp.key.public_key().as_ref().to_vec();
    let jwks = json!({ "keys": [{ "kty": "EC", "crv": "P-256", "kid": "k1", "use": "sig",
        "x": URL_SAFE_NO_PAD.encode(&point[1..33]), "y": URL_SAFE_NO_PAD.encode(&point[33..65]) }] });
    let discovery = json!({ "issuer": issuer, "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"), "jwks_uri": format!("{issuer}/jwks") });
    let app = axum::Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(move || async move { Json(discovery) }),
        )
        .route("/jwks", get(move || async move { Json(jwks) }))
        .route("/token", post(token))
        .with_state(idp.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    idp
}

async fn token(
    State(idp): State<Idp>,
    Form(form): Form<HashMap<String, String>>,
) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    // The code is the state the test passes through, so the IdP knows which request.
    let code = form.get("code").cloned().unwrap_or_default();
    let Some((nonce, challenge)) = idp.pending.lock().unwrap().remove(&code) else {
        return (StatusCode::BAD_REQUEST, "unknown code").into_response();
    };
    let verifier = form.get("code_verifier").cloned().unwrap_or_default();
    if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != challenge {
        return (StatusCode::BAD_REQUEST, "PKCE mismatch").into_response();
    }
    let now = chrono::Utc::now().timestamp();
    let mut claims = json!({ "iss": idp.issuer, "aud": "cooper-test", "sub": "subject-1", "exp": now + 300,
        "iat": now, "nonce": nonce, "email": "admin@default.example", "email_verified": true });
    for (k, v) in idp.claims.lock().unwrap().as_object().unwrap() {
        claims[k] = v.clone();
    }
    Json(json!({ "access_token": "x", "token_type": "Bearer", "id_token": idp.sign(&claims) }))
        .into_response()
}

async fn provider(admin: &Client, idp: &Idp, audience: &str) -> String {
    let r = admin
        .post(
            "/api/v1/sso-providers",
            json!({ "display_name": "Test IdP", "issuer_url": idp.issuer,
            "client_id": "cooper-test", "client_secret": "s3cret", "audience": audience }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(
        r.body.get("client_secret").is_none(),
        "the secret is write-only"
    );
    id(&r.body).to_owned()
}

/// Starts a sign-in and plays the provider: returns the state to use as the code.
async fn authorize(anon: &Client, idp: &Idp, provider: &str, query: &str) -> String {
    let r = anon
        .get(&format!("/api/v1/sso/{provider}/start?{query}"))
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{:?}", r.body);
    let location = r.headers[header::LOCATION].to_str().unwrap();
    assert!(
        location.starts_with(&format!("{}/authorize?", idp.issuer)),
        "{location}"
    );
    let url = reqwest_free_parse(location);
    assert_eq!(url["code_challenge_method"], "S256");
    assert_eq!(url["client_id"], "cooper-test");
    let state = url["state"].clone();
    idp.pending.lock().unwrap().insert(
        state.clone(),
        (url["nonce"].clone(), url["code_challenge"].clone()),
    );
    state
}

fn reqwest_free_parse(url: &str) -> HashMap<String, String> {
    let query = url.split_once('?').unwrap().1;
    query
        .split('&')
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap();
            let decode = |s: &str| {
                let s = s.replace('+', " ");
                let mut out = Vec::new();
                let bytes = s.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i] == b'%' {
                        out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                        i += 3;
                    } else {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
                String::from_utf8(out).unwrap()
            };
            (decode(k), decode(v))
        })
        .collect()
}

#[sqlx::test(migrations = "../../migrations")]
async fn analysts_sign_in_with_oidc_and_links_are_remembered(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let idp = start_idp().await;
    let provider = provider(&admin, &idp, "analysts").await;

    let options = anon
        .get("/api/v1/sso/providers?audience=analysts")
        .await
        .body;
    assert_eq!(options[0]["display_name"], "Test IdP");
    assert_eq!(
        anon.get("/api/v1/sso/providers?audience=requesters")
            .await
            .body,
        json!([])
    );

    let state = authorize(&anon, &idp, &provider, "return_to=%2Ftickets%3Fview%3Dmine").await;
    let r = anon
        .get(&format!("/api/v1/sso/callback?state={state}&code={state}"))
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{:?}", r.body);
    assert_eq!(r.headers[header::LOCATION], "/tickets?view=mine");
    let cookie = r.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert!(cookie.starts_with("cooper_session="));
    let me = Client {
        cookie: Some(cookie),
        ..anon.clone()
    };
    let session = me.get("/api/v1/session").await;
    assert_eq!(session.status, StatusCode::OK);
    assert_eq!(session.body["analyst"]["username"], "admin");

    // The state is single use.
    assert_eq!(
        anon.get(&format!("/api/v1/sso/callback?state={state}&code={state}"))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    // Once linked, the subject signs in even when the provider no longer verifies email.
    *idp.claims.lock().unwrap() = json!({ "email_verified": false });
    let state = authorize(&anon, &idp, &provider, "").await;
    let r = anon
        .get(&format!("/api/v1/sso/callback?state={state}&code={state}"))
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(r.headers[header::LOCATION], "/");

    // An unknown subject with an unverified address is refused: SSO never guesses.
    *idp.claims.lock().unwrap() = json!({ "sub": "someone-else", "email_verified": false });
    let state = authorize(&anon, &idp, &provider, "").await;
    let r = anon
        .get(&format!("/api/v1/sso/callback?state={state}&code={state}"))
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(r.headers.get(header::SET_COOKIE).is_none());

    // A token for another sign-in (wrong nonce) or another client is refused.
    for claims in [
        json!({ "nonce": "replayed" }),
        json!({ "aud": "someone-else" }),
    ] {
        *idp.claims.lock().unwrap() = claims;
        let state = authorize(&anon, &idp, &provider, "").await;
        assert_eq!(
            anon.get(&format!("/api/v1/sso/callback?state={state}&code={state}"))
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
    }

    // Requesters cannot use an Analysts-only provider.
    assert_eq!(
        anon.get(&format!("/api/v1/sso/{provider}/start?audience=requesters"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requesters_sign_in_to_the_portal_with_oidc_and_redirects_stay_local(pool: PgPool) {
    let (anon, admin) = setup(&pool).await;
    let company = default_company(&admin).await;
    requester(&admin, &company, "sam@default.example", json!({})).await;
    let idp = start_idp().await;
    let provider = provider(&admin, &idp, "both").await;
    *idp.claims.lock().unwrap() = json!({ "sub": "sam", "email": "Sam@Default.example" });

    let state = authorize(
        &anon,
        &idp,
        &provider,
        "audience=requesters&return_to=https%3A%2F%2Fevil.example",
    )
    .await;
    let r = anon
        .get(&format!("/api/v1/sso/callback?state={state}&code={state}"))
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{:?}", r.body);
    assert_eq!(
        r.headers[header::LOCATION],
        "/portal",
        "never an open redirect"
    );
    let set_cookie = r.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(
        set_cookie.starts_with("cooper_portal_session=")
            && set_cookie.contains("Path=/api/v1/portal")
    );
    let portal = Client {
        cookie: Some(set_cookie.split(';').next().unwrap().to_owned()),
        ..anon.clone()
    };
    assert_eq!(
        portal.get("/api/v1/portal/session").await.status,
        StatusCode::OK
    );

    // An address without an active Requester gets nothing.
    *idp.claims.lock().unwrap() = json!({ "sub": "nobody", "email": "nobody@default.example" });
    let state = authorize(&anon, &idp, &provider, "audience=requesters").await;
    assert_eq!(
        anon.get(&format!("/api/v1/sso/callback?state={state}&code={state}"))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    // The provider list for Administrators never shows the secret.
    let listed = admin.get("/api/v1/sso-providers").await.body;
    assert!(!listed.to_string().contains("s3cret"));
    let stored: Option<String> = sqlx::query_scalar("SELECT client_secret_enc FROM sso_providers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!stored.unwrap().contains("s3cret"), "encrypted at rest");
}
