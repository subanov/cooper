//! Suggested Replies (FR-AI-003, ADR-0037): the API with a stub AI provider.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use common::{ADMIN_PASSWORD, Client, test_settings};
use cooper_http::HttpConfig;
use cooper_kernel::SystemClock;
use cooper_server::{AppState, SecuritySettings, router};
use cooper_service_desk::domain::{
    ReplySuggester, SuggestError, SuggestedReply, SuggestedReplyRequest,
};
use http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

/// Answers with a fixed text, or fails, and remembers the last request.
#[derive(Debug, Default)]
struct StubSuggester {
    fail: Option<SuggestError>,
    last: Mutex<Option<SuggestedReplyRequest>>,
}

#[async_trait]
impl ReplySuggester for StubSuggester {
    async fn suggest(
        &self,
        request: &SuggestedReplyRequest,
    ) -> Result<SuggestedReply, SuggestError> {
        *self.last.lock().unwrap() = Some(request.clone());
        match &self.fail {
            Some(err) => Err(err.clone()),
            None => SuggestedReply::new("Hello, please restart Outlook.", "stub-model"),
        }
    }
}

/// An Administrator's client whose server uses `suggester`.
async fn admin_with(pool: &PgPool, suggester: Option<Arc<StubSuggester>>) -> (Client, Client) {
    let state = AppState::with_settings(
        pool.clone(),
        Arc::new(SystemClock),
        SecuritySettings {
            secure_cookies: false,
            reply_suggester: suggester.map(|s| s as Arc<dyn ReplySuggester>),
            ..test_settings()
        },
    )
    .unwrap();
    let anon = Client {
        app: router(state, &HttpConfig::default()),
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
    (anon, admin)
}

async fn ticket(admin: &Client) -> String {
    let company = admin.get("/api/v1/companies").await.body["items"][0]["id"].clone();
    let reply = admin
        .post(
            "/api/v1/tickets",
            json!({
                "company_id": company,
                "subject": "Outlook keeps asking for my password",
                "description": "Since this morning Outlook asks every few minutes."
            }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body["id"].as_str().unwrap().to_owned()
}

/// An Analyst with only `tickets.read` in the default Company.
async fn reader(anon: &Client, admin: &Client) -> Client {
    let company = admin.get("/api/v1/companies").await.body["items"][0]["id"].clone();
    let created = admin
        .post(
            "/api/v1/analysts",
            json!({
                "username": "reader", "email": "reader@default.example", "full_name": "Reader",
                "password": "analyst password 1"
            }),
        )
        .await;
    let id = created.body["id"].as_str().unwrap().to_owned();
    let role = admin
        .post(
            "/api/v1/roles",
            json!({ "name": "Reader role", "capabilities": ["tickets.read"] }),
        )
        .await;
    admin
        .put(
            &format!("/api/v1/analysts/{id}/roles"),
            json!({ "role_ids": [role.body["id"]] }),
        )
        .await;
    admin
        .put(
            &format!("/api/v1/analysts/{id}/company-access"),
            json!({ "all": false, "company_ids": [company] }),
        )
        .await;
    anon.sign_in("reader", "analyst password 1").await
}

#[sqlx::test(migrations = "../../migrations")]
async fn suggests_a_reply_from_the_thread_including_internal_notes(pool: PgPool) {
    let stub = Arc::new(StubSuggester::default());
    let (_, admin) = admin_with(&pool, Some(stub.clone())).await;
    let id = ticket(&admin).await;
    admin
        .post(
            &format!("/api/v1/tickets/{id}/notes"),
            json!({ "body": "Token cache looks corrupt" }),
        )
        .await;

    let detail = admin.get(&format!("/api/v1/tickets/{id}")).await;
    assert_eq!(detail.body["can_suggest_reply"], true);

    let reply = admin
        .post(
            &format!("/api/v1/tickets/{id}/suggested-reply"),
            json!({ "guidance": "ask them to restart" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body["text"], "Hello, please restart Outlook.");
    assert_eq!(reply.body["model"], "stub-model");

    let sent = stub.last.lock().unwrap().clone().unwrap();
    let content = sent.content();
    assert!(content.contains("Outlook keeps asking for my password"));
    assert!(content.contains("Since this morning Outlook asks every few minutes."));
    assert!(content.contains("internal note"));
    assert!(content.contains("Token cache looks corrupt"));
    assert!(content.contains("Analyst writing the Reply: Ada Admin"));
    assert!(content.contains("<guidance>\nask them to restart\n</guidance>"));

    // Nothing was sent or recorded: the timeline has only the Note.
    let emails = admin.get(&format!("/api/v1/tickets/{id}/emails")).await;
    assert_eq!(emails.body.as_array().unwrap().len(), 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn suggestions_are_off_without_a_provider(pool: PgPool) {
    let (_, admin) = admin_with(&pool, None).await;
    let id = ticket(&admin).await;
    let detail = admin.get(&format!("/api/v1/tickets/{id}")).await;
    assert_eq!(detail.body["can_suggest_reply"], false);
    let reply = admin
        .post(&format!("/api/v1/tickets/{id}/suggested-reply"), json!({}))
        .await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        reply.body["type"],
        "https://cooper.dev/problems/suggestions-off"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn suggesting_needs_tickets_update(pool: PgPool) {
    let stub = Arc::new(StubSuggester::default());
    let (anon, admin) = admin_with(&pool, Some(stub.clone())).await;
    let id = ticket(&admin).await;
    let reader = reader(&anon, &admin).await;
    let detail = reader.get(&format!("/api/v1/tickets/{id}")).await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.body["can_suggest_reply"], false);
    let reply = reader
        .post(&format!("/api/v1/tickets/{id}/suggested-reply"), json!({}))
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert!(
        stub.last.lock().unwrap().is_none(),
        "the provider was not called"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn provider_failures_are_problems(pool: PgPool) {
    let busy = Arc::new(StubSuggester {
        fail: Some(SuggestError::Unavailable("overloaded".into())),
        ..StubSuggester::default()
    });
    let (_, admin) = admin_with(&pool, Some(busy)).await;
    let id = ticket(&admin).await;
    let reply = admin
        .post(&format!("/api/v1/tickets/{id}/suggested-reply"), json!({}))
        .await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        reply.body["type"],
        "https://cooper.dev/problems/ai-unavailable"
    );

    let too_long = "x".repeat(4_001);
    let reply = admin
        .post(
            &format!("/api/v1/tickets/{id}/suggested-reply"),
            json!({ "guidance": too_long }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.body["field"], "guidance");
}
