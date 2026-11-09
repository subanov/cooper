//! Background Jobs of the attachment store (ADR-0033): text extraction for search
//! (FR-EML-007) and the clean-up of content no attachment references (ADR-0007).

use std::collections::HashSet;
use std::fmt::Write as _;

use async_trait::async_trait;
use chrono::{Duration, NaiveTime};
use cooper_kernel::TenantScope;
use uuid::Uuid;

use super::{
    BlobStore, Extraction, ExtractionAttempt, ExtractionLimits, ExtractionStatus, extract_text,
    select_orphans,
};
use crate::jobs::{Job, JobContext, JobError, Schedule};
use crate::tenancy;

/// Extracts the text of pending attachments so ticket search finds it (FR-EML-007).
/// Every 30 seconds by default, a batch at a time; the reading itself runs on the
/// blocking thread pool with a size and time limit.
#[derive(Debug, Clone)]
pub struct AttachmentTextExtraction {
    store: BlobStore,
    limits: ExtractionLimits,
    batch: i64,
}

impl AttachmentTextExtraction {
    /// A job reading content from `store` with the default limits.
    #[must_use]
    pub fn new(store: BlobStore) -> Self {
        Self {
            store,
            limits: ExtractionLimits::default(),
            batch: 20,
        }
    }

    /// The same job with other limits.
    #[must_use]
    pub const fn with_limits(mut self, limits: ExtractionLimits) -> Self {
        self.limits = limits;
        self
    }

    async fn attempt(
        &self,
        sha256: &str,
        size: i64,
        media_type: String,
        file_name: String,
    ) -> ExtractionAttempt {
        if u64::try_from(size).unwrap_or(u64::MAX) > self.limits.max_input_bytes {
            return ExtractionAttempt::TooLarge;
        }
        let bytes = match self.store.read(sha256).await {
            Ok(b) => b,
            Err(e) => return ExtractionAttempt::ContentUnavailable(e.to_string()),
        };
        let limits = self.limits;
        let work = tokio::task::spawn_blocking(move || {
            extract_text(&bytes, &media_type, &file_name, &limits)
        });
        match tokio::time::timeout(self.limits.timeout, work).await {
            Ok(Ok(extraction)) => ExtractionAttempt::Read(extraction),
            Ok(Err(_)) => ExtractionAttempt::Read(Extraction::Failed("the reader crashed".into())),
            Err(_) => ExtractionAttempt::TimedOut,
        }
    }
}

#[async_trait]
impl Job for AttachmentTextExtraction {
    fn name(&self) -> &'static str {
        "attachment-text-extraction"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(30)
    }

    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError> {
        let now = ctx.clock.now();
        // Attachments of every Company: the job works for all of them.
        let mut tx = tenancy::begin(&ctx.pool, &TenantScope::All).await?;
        let rows = sqlx::query!(
            "SELECT id, sha256, size_bytes, media_type, file_name, extraction_attempts FROM attachments
             WHERE extraction_status = 'pending' AND (extraction_retry_at IS NULL OR extraction_retry_at <= $1)
             ORDER BY created_at, id LIMIT $2 FOR UPDATE SKIP LOCKED",
            now,
            self.batch
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
        for r in rows {
            let attempts = u32::try_from(r.extraction_attempts).unwrap_or(0) + 1;
            let attempt = self
                .attempt(&r.sha256, r.size_bytes, r.media_type, r.file_name)
                .await;
            let Some(status) = ExtractionStatus::Pending.after(&attempt, attempts) else {
                continue;
            };
            let (text, error) = match attempt {
                ExtractionAttempt::Read(Extraction::Text { text, .. }) => (Some(text), None),
                ExtractionAttempt::Read(Extraction::Failed(e))
                | ExtractionAttempt::ContentUnavailable(e) => (None, Some(e)),
                ExtractionAttempt::TimedOut => (None, Some("timed out".to_owned())),
                ExtractionAttempt::Read(Extraction::Unsupported) | ExtractionAttempt::TooLarge => {
                    (None, None)
                }
            };
            if let Some(e) = &error {
                tracing::warn!(attachment = %r.id, error = %e, "attachment text not extracted");
            }
            let retry_at = (status == ExtractionStatus::Pending)
                .then(|| now + Duration::minutes(5 * i64::from(attempts)));
            let finished_at = status.is_final().then_some(now);
            let mut tx = tenancy::begin(&ctx.pool, &TenantScope::All).await?;
            sqlx::query!(
                "UPDATE attachments SET extraction_status = $2, extracted_text = $3, extraction_error = $4,
                        extraction_attempts = $5, extraction_retry_at = $6, extracted_at = $7
                 WHERE id = $1 AND extraction_status = 'pending'",
                r.id,
                status.as_str(),
                text,
                error,
                i32::try_from(attempts).unwrap_or(i32::MAX),
                retry_at,
                finished_at
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            *counts.entry(status.as_str()).or_default() += 1;
        }
        if counts.is_empty() {
            return Ok(None);
        }
        let mut message = String::from("attachments read:");
        for (status, n) in counts {
            let _ = write!(message, " {status} {n}");
        }
        Ok(Some(message))
    }
}

/// Deletes content no attachment references, and abandoned uploads in the spool, once
/// they are older than the grace period (ADR-0007, ADR-0033). Daily at 04:30 UTC by
/// default.
#[derive(Debug, Clone)]
pub struct BlobOrphanCleanup {
    store: BlobStore,
    grace: Duration,
}

impl BlobOrphanCleanup {
    /// A job for `store` keeping unreferenced content for `grace` (at least an hour).
    #[must_use]
    pub fn new(store: BlobStore, grace: Duration) -> Self {
        Self {
            store,
            grace: grace.max(Duration::hours(1)),
        }
    }
}

#[async_trait]
impl Job for BlobOrphanCleanup {
    fn name(&self) -> &'static str {
        "attachment-orphan-cleanup"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::Daily {
            at: NaiveTime::from_hms_opt(4, 30, 0).unwrap_or(NaiveTime::MIN),
        }
    }

    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError> {
        let cutoff = ctx.clock.now() - self.grace;
        let backend = self.store.backend();
        let (mut deleted, mut kept) = (0usize, 0usize);
        let mut page = None;
        loop {
            let listing = backend.list(page).await?;
            let old: Vec<String> = listing
                .blobs
                .iter()
                .filter(|b| b.modified < cutoff)
                .map(|b| b.sha256.to_ascii_lowercase())
                .collect();
            if !old.is_empty() {
                let mut tx = tenancy::begin(&ctx.pool, &TenantScope::All).await?;
                let referenced: HashSet<String> = sqlx::query_scalar!(
                    "SELECT DISTINCT sha256 FROM attachments WHERE sha256 = ANY($1)",
                    &old
                )
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .collect();
                tx.commit().await?;
                kept += referenced.len();
                for sha256 in select_orphans(&listing.blobs, &referenced, cutoff) {
                    // Written again since it was listed: a new upload may be about to
                    // reference it.
                    if backend.modified(&sha256).await?.is_some_and(|m| m < cutoff) {
                        backend.delete(&sha256).await?;
                        deleted += 1;
                    }
                }
            }
            match listing.next {
                Some(next) => page = Some(next),
                None => break,
            }
        }
        let spooled = clean_spool(&self.store, cutoff).await?;
        Ok(Some(format!(
            "deleted {deleted} unreferenced files and {spooled} abandoned uploads; {kept} old files still referenced"
        )))
    }
}

/// Removes spooled uploads older than `cutoff` (left by failed or abandoned uploads).
async fn clean_spool(
    store: &BlobStore,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> Result<usize, JobError> {
    let mut entries = match tokio::fs::read_dir(store.spool_dir()).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let mut removed = 0;
    while let Some(entry) = entries.next_entry().await? {
        let is_upload = entry
            .file_name()
            .to_str()
            .is_some_and(|n| Uuid::parse_str(n).is_ok());
        let meta = entry.metadata().await?;
        if is_upload
            && meta.is_file()
            && chrono::DateTime::<chrono::Utc>::from(meta.modified()?) < cutoff
        {
            tokio::fs::remove_file(entry.path()).await?;
            removed += 1;
        }
    }
    Ok(removed)
}
