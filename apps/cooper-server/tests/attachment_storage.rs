//! Attachment text extraction feeding ticket search (FR-EML-007, FR-TKT-041) and the
//! clean-up of content no attachment references (ADR-0007, ADR-0033).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

mod common;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use common::{ADMIN_PASSWORD, Client, test_settings};
use cooper_http::HttpConfig;
use cooper_kernel::SystemClock;
use cooper_platform::attachments::{AttachmentTextExtraction, BlobOrphanCleanup, BlobStore};
use cooper_platform::jobs::{Job, JobContext};
use cooper_server::{AppState, SecuritySettings, router};
use http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

/// An Administrator's client whose server keeps content in `blobs`.
async fn admin_with(pool: &PgPool, blobs: &BlobStore) -> Client {
    let state = AppState::with_settings(
        pool.clone(),
        Arc::new(SystemClock),
        SecuritySettings {
            secure_cookies: false,
            blobs: blobs.clone(),
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
    anon.sign_in("admin", ADMIN_PASSWORD).await
}

fn temp_store() -> (std::path::PathBuf, BlobStore) {
    let dir = std::env::temp_dir().join(format!("cooper-blobs-{}", uuid::Uuid::now_v7()));
    (dir.clone(), BlobStore::filesystem(dir))
}

fn ctx(pool: &PgPool) -> JobContext {
    JobContext {
        pool: pool.clone(),
        clock: Arc::new(SystemClock),
    }
}

async fn ticket(admin: &Client, subject: &str) -> String {
    let company = admin.get("/api/v1/companies").await.body["items"][0]["id"].clone();
    let reply = admin
        .post(
            "/api/v1/tickets",
            json!({ "company_id": company, "subject": subject }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body["id"].as_str().unwrap().to_owned()
}

async fn attach(admin: &Client, ticket: &str, name: &str, content: &[u8]) -> Value {
    let boundary = "cooperboundary42";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let (status, _, bytes) = admin
        .send_raw(
            Method::POST,
            &format!("/api/v1/tickets/{ticket}/attachments"),
            &format!("multipart/form-data; boundary={boundary}"),
            body,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

async fn search(admin: &Client, text: &str) -> Vec<String> {
    let reply = admin.get(&format!("/api/v1/tickets?q={text}")).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    reply.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_owned())
        .collect()
}

async fn statuses(admin: &Client, ticket: &str) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = admin
        .get(&format!("/api/v1/tickets/{ticket}/attachments"))
        .await
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["file_name"].as_str().unwrap().to_owned(),
                a["extraction_status"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    list.sort();
    list
}

#[sqlx::test(migrations = "../../migrations")]
async fn extracted_attachment_text_is_found_by_ticket_search(pool: PgPool) {
    let (dir, blobs) = temp_store();
    let admin = admin_with(&pool, &blobs).await;
    let t = ticket(&admin, "Printer problem").await;
    let other = ticket(&admin, "Something else").await;
    attach(&admin, &t, "log.txt", b"spooler error zanzibar at 10:02").await;
    attach(
        &admin,
        &t,
        "page.html",
        b"<html><body><p>Kyoto</p><script>marrakesh()</script></body></html>",
    )
    .await;
    attach(&admin, &other, "photo.png", b"\x89PNG\r\n\x1a\nnot really").await;

    assert!(search(&admin, "zanzibar").await.is_empty(), "not read yet");
    assert_eq!(
        statuses(&admin, &t).await,
        [
            ("log.txt".to_owned(), "pending".to_owned()),
            ("page.html".to_owned(), "pending".to_owned())
        ]
    );

    let job = AttachmentTextExtraction::new(blobs.clone());
    let message = job.run(&ctx(&pool)).await.unwrap().unwrap();
    assert!(message.contains("extracted 2"), "{message}");
    assert!(
        job.run(&ctx(&pool)).await.unwrap().is_none(),
        "nothing left"
    );

    assert_eq!(search(&admin, "zanzibar").await, std::slice::from_ref(&t));
    assert_eq!(search(&admin, "kyoto").await, std::slice::from_ref(&t));
    assert!(
        search(&admin, "marrakesh").await.is_empty(),
        "scripts are not text"
    );
    assert_eq!(
        statuses(&admin, &t).await,
        [
            ("log.txt".to_owned(), "extracted".to_owned()),
            ("page.html".to_owned(), "extracted".to_owned())
        ]
    );
    assert_eq!(
        statuses(&admin, &other).await,
        [("photo.png".to_owned(), "unsupported".to_owned())]
    );
    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[sqlx::test(migrations = "../../migrations")]
async fn missing_content_is_retried_then_failed(pool: PgPool) {
    let (dir, blobs) = temp_store();
    let admin = admin_with(&pool, &blobs).await;
    let t = ticket(&admin, "Lost file").await;
    let a = attach(&admin, &t, "gone.txt", b"vanished content").await;
    let sha: String = sqlx::query_scalar("SELECT sha256 FROM attachments WHERE id = $1::uuid")
        .bind(a["id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    blobs.backend().delete(&sha).await.unwrap();
    let job = AttachmentTextExtraction::new(blobs.clone());
    for _ in 0..3 {
        job.run(&ctx(&pool)).await.unwrap();
        // Retries wait a few minutes; let the next run pick it up now.
        sqlx::query("UPDATE attachments SET extraction_retry_at = NULL")
            .execute(&pool)
            .await
            .unwrap();
    }
    let (status, attempts, error): (String, i32, Option<String>) = sqlx::query_as(
        "SELECT extraction_status, extraction_attempts, extraction_error FROM attachments WHERE id = $1::uuid",
    )
    .bind(a["id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((status.as_str(), attempts), ("failed", 3));
    assert!(error.unwrap().contains("not found"));
    let _ = tokio::fs::remove_dir_all(dir).await;
}

fn age(dir: &std::path::Path, sha: &str, hours: u64) {
    let path = dir
        .join("sha256")
        .join(&sha[..2])
        .join(&sha[2..4])
        .join(sha);
    std::fs::File::options()
        .append(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(hours * 3600))
        .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn orphan_cleanup_deletes_only_old_unreferenced_content(pool: PgPool) {
    let (dir, blobs) = temp_store();
    let admin = admin_with(&pool, &blobs).await;
    let t = ticket(&admin, "Keep my file").await;
    attach(&admin, &t, "kept.txt", b"referenced content").await;
    let kept: String = sqlx::query_scalar("SELECT sha256 FROM attachments")
        .fetch_one(&pool)
        .await
        .unwrap();
    let store = |content: &'static [u8]| {
        let blobs = blobs.clone();
        async move {
            let mut w = blobs.writer(1024).await.unwrap();
            w.write(content).await.unwrap();
            w.finish().await.unwrap().sha256
        }
    };
    // Written, but the attachment row never committed.
    let orphan = store(b"abandoned upload").await;
    let fresh = store(b"upload still in flight").await;
    age(&dir, &kept, 72);
    age(&dir, &orphan, 72);
    // A spooled upload that was never finished.
    let spool = blobs.spool_dir().join(uuid::Uuid::now_v7().to_string());
    std::fs::write(&spool, b"partial").unwrap();
    std::fs::File::options()
        .append(true)
        .open(&spool)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(72 * 3600))
        .unwrap();

    let job = BlobOrphanCleanup::new(blobs.clone(), chrono::Duration::hours(24));
    let message = job.run(&ctx(&pool)).await.unwrap().unwrap();
    assert!(
        message.contains("deleted 1 unreferenced files and 1 abandoned uploads"),
        "{message}"
    );
    let backend = blobs.backend();
    assert!(backend.modified(&orphan).await.unwrap().is_none());
    assert!(
        backend.modified(&kept).await.unwrap().is_some(),
        "referenced"
    );
    assert!(
        backend.modified(&fresh).await.unwrap().is_some(),
        "inside the grace period"
    );
    assert!(!spool.exists());

    // Content attached again restarts its grace period (deduplicated uploads).
    age(&dir, &fresh, 72);
    attach(&admin, &t, "again.txt", b"upload still in flight").await;
    job.run(&ctx(&pool)).await.unwrap();
    assert!(backend.modified(&fresh).await.unwrap().is_some());
    let _ = tokio::fs::remove_dir_all(dir).await;
}
