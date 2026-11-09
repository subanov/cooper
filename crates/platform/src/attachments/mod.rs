//! Attachments (FR-GEN-007, ADR-0007, ADR-0033): a content-addressed blob store plus
//! metadata rows, text extraction for search (FR-EML-007) and orphan clean-up.
//!
//! Content is written by [`BlobWriter`] while it streams in: hashed with SHA-256, checked
//! against the size limit and spooled to a local temporary file. When complete it is
//! handed to the configured [`BlobBackend`] under `sha256/ab/cd/<hash>`: the local file
//! system ([`FilesystemBlobStore`]) or an S3-compatible bucket ([`S3BlobStore`]).
//! Identical files are stored once. The media type is detected from the content, never
//! taken from the client. Contexts decide who may attach to or read from their records;
//! this module only stores.

mod extract;
mod jobs;
mod s3;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, TenantId};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub use extract::{Extraction, ExtractionLimits, extract_text, html_to_text};
pub use jobs::{AttachmentTextExtraction, BlobOrphanCleanup};
pub use s3::{S3BlobStore, S3Config};

use crate::config::{self, ConfigError};

/// Storing or reading content failed.
#[derive(Debug, Error)]
pub enum BlobError {
    /// The content is larger than allowed.
    #[error("the file is larger than {limit} bytes")]
    TooLarge {
        /// The limit in bytes.
        limit: u64,
    },
    /// The content does not exist.
    #[error("content not found")]
    NotFound,
    /// File system failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The storage service failed (never contains credentials).
    #[error("{0}")]
    Backend(String),
}

/// A stored piece of content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    /// Hex SHA-256 of the content.
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
    /// Media type detected from the content.
    pub media_type: &'static str,
}

/// A piece of content in a backend listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobEntry {
    /// Hex SHA-256 (the content address).
    pub sha256: String,
    /// When it was last written (or written again by a duplicate upload).
    pub modified: DateTime<Utc>,
}

/// One page of a backend listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlobPage {
    /// Content on this page.
    pub blobs: Vec<BlobEntry>,
    /// Where the next page starts, if there is one.
    pub next: Option<String>,
}

/// Where content lives (ADR-0007): the port both backends implement.
#[async_trait]
pub trait BlobBackend: Send + Sync + std::fmt::Debug {
    /// `filesystem` or `s3`.
    fn kind(&self) -> &'static str;
    /// Stores the complete spooled file at `path` (consumed) as `sha256`. When the
    /// content exists already its modification time is refreshed, so the orphan
    /// clean-up's grace period starts again (ADR-0033).
    async fn put(&self, sha256: &str, path: &Path, size: u64) -> Result<(), BlobError>;
    /// Reads content.
    async fn get(&self, sha256: &str) -> Result<Vec<u8>, BlobError>;
    /// When content was last written, or `None` if it does not exist.
    async fn modified(&self, sha256: &str) -> Result<Option<DateTime<Utc>>, BlobError>;
    /// Deletes content (missing content is not an error).
    async fn delete(&self, sha256: &str) -> Result<(), BlobError>;
    /// Lists stored content page by page, starting with `page` `None`.
    async fn list(&self, page: Option<String>) -> Result<BlobPage, BlobError>;
}

/// `true` for a hex SHA-256.
fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The content address `sha256/ab/cd/<hash>` (callers check [`is_sha256`] first).
fn blob_key(sha256: &str) -> String {
    format!("sha256/{}/{}/{sha256}", &sha256[..2], &sha256[2..4])
}

/// The blob store: a local spool for uploads in progress and a backend for content.
/// Cheap to clone.
#[derive(Debug, Clone)]
pub struct BlobStore {
    spool: PathBuf,
    backend: Arc<dyn BlobBackend>,
}

impl BlobStore {
    /// A store that spools to `spool` and keeps content in `backend`.
    #[must_use]
    pub fn new(spool: impl Into<PathBuf>, backend: Arc<dyn BlobBackend>) -> Self {
        Self {
            spool: spool.into(),
            backend,
        }
    }

    /// Content on the local file system under `root` (spooling to `root/tmp`).
    #[must_use]
    pub fn filesystem(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self::new(root.join("tmp"), Arc::new(FilesystemBlobStore::new(root)))
    }

    /// The backend.
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn BlobBackend> {
        &self.backend
    }

    /// Where uploads in progress are spooled.
    #[must_use]
    pub fn spool_dir(&self) -> &Path {
        &self.spool
    }

    /// Starts writing new content of at most `limit` bytes.
    ///
    /// # Errors
    ///
    /// File system failures.
    pub async fn writer(&self, limit: u64) -> Result<BlobWriter, BlobError> {
        tokio::fs::create_dir_all(&self.spool).await?;
        let tmp = self.spool.join(Uuid::now_v7().to_string());
        let file = tokio::fs::File::create(&tmp).await?;
        Ok(BlobWriter {
            store: self.clone(),
            tmp,
            file,
            hasher: Sha256::new(),
            size: 0,
            limit,
            head: Vec::new(),
        })
    }

    /// Reads content by its hash.
    ///
    /// # Errors
    ///
    /// [`BlobError::NotFound`] or storage failures.
    pub async fn read(&self, sha256: &str) -> Result<Vec<u8>, BlobError> {
        if !is_sha256(sha256) {
            return Err(BlobError::NotFound);
        }
        self.backend.get(sha256).await
    }
}

/// Which backend to use, from the environment (ADR-0033).
#[derive(Debug, Clone)]
pub enum BlobStoreConfig {
    /// `COOPER_BLOB_STORE=filesystem` (default): content under `COOPER_BLOB_DIR`.
    Filesystem {
        /// Root directory.
        dir: PathBuf,
    },
    /// `COOPER_BLOB_STORE=s3`: content in a bucket, uploads spooled under
    /// `COOPER_BLOB_DIR/tmp`.
    S3 {
        /// Local spool root.
        dir: PathBuf,
        /// The bucket.
        config: S3Config,
    },
}

impl BlobStoreConfig {
    /// Reads `COOPER_BLOB_STORE`, `COOPER_BLOB_DIR` and, for S3, the `COOPER_S3_*`
    /// variables.
    ///
    /// # Errors
    ///
    /// Unknown backend or missing S3 settings.
    pub fn from_env() -> Result<Self, ConfigError> {
        let dir = config::parse_or("COOPER_BLOB_DIR", PathBuf::from("data/blobs"))?;
        match config::var("COOPER_BLOB_STORE")?.as_deref().map(str::trim) {
            None | Some("filesystem") => Ok(Self::Filesystem { dir }),
            Some("s3") => Ok(Self::S3 {
                dir,
                config: S3Config::from_env()?,
            }),
            Some(_) => Err(ConfigError::Invalid {
                name: "COOPER_BLOB_STORE",
                reason: "must be filesystem or s3".to_owned(),
            }),
        }
    }

    /// The store.
    ///
    /// # Errors
    ///
    /// The S3 client cannot be built.
    pub fn build(&self) -> Result<BlobStore, BlobError> {
        match self {
            Self::Filesystem { dir } => Ok(BlobStore::filesystem(dir)),
            Self::S3 { dir, config } => Ok(BlobStore::new(
                dir.join("tmp"),
                Arc::new(S3BlobStore::new(config.clone())?),
            )),
        }
    }
}

/// Content stored on the local file system (single host or a shared volume).
#[derive(Debug, Clone)]
pub struct FilesystemBlobStore {
    root: PathBuf,
}

impl FilesystemBlobStore {
    /// A store under `root` (created when needed).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, sha256: &str) -> Option<PathBuf> {
        is_sha256(sha256).then(|| self.root.join(blob_key(sha256)))
    }
}

fn modified_of(meta: &std::fs::Metadata) -> Result<DateTime<Utc>, BlobError> {
    Ok(DateTime::<Utc>::from(meta.modified()?))
}

#[async_trait]
impl BlobBackend for FilesystemBlobStore {
    fn kind(&self) -> &'static str {
        "filesystem"
    }

    async fn put(&self, sha256: &str, path: &Path, _size: u64) -> Result<(), BlobError> {
        let target = self.path_for(sha256).ok_or(BlobError::NotFound)?;
        if let Some(dir) = target.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        if tokio::fs::try_exists(&target).await? {
            tokio::fs::remove_file(path).await?;
            // Restart the orphan grace period of content that is attached again.
            tokio::task::spawn_blocking(move || {
                std::fs::File::options()
                    .append(true)
                    .open(&target)?
                    .set_modified(std::time::SystemTime::now())
            })
            .await
            .map_err(|e| BlobError::Backend(e.to_string()))??;
        } else {
            tokio::fs::rename(path, &target).await?;
        }
        Ok(())
    }

    async fn get(&self, sha256: &str) -> Result<Vec<u8>, BlobError> {
        let path = self.path_for(sha256).ok_or(BlobError::NotFound)?;
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(BlobError::NotFound),
            Err(e) => Err(e.into()),
        }
    }

    async fn modified(&self, sha256: &str) -> Result<Option<DateTime<Utc>>, BlobError> {
        let Some(path) = self.path_for(sha256) else {
            return Ok(None);
        };
        match tokio::fs::metadata(&path).await {
            Ok(meta) => Ok(Some(modified_of(&meta)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn delete(&self, sha256: &str) -> Result<(), BlobError> {
        let Some(path) = self.path_for(sha256) else {
            return Ok(());
        };
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// One page per first-level directory (`sha256/ab`), in order.
    async fn list(&self, page: Option<String>) -> Result<BlobPage, BlobError> {
        let base = self.root.join("sha256");
        let mut dirs = sorted_names(&base).await?;
        dirs.retain(|d| d.len() == 2 && page.as_deref().is_none_or(|p| d.as_str() >= p));
        let Some(first) = dirs.first().cloned() else {
            return Ok(BlobPage::default());
        };
        let mut blobs = Vec::new();
        for second in sorted_names(&base.join(&first)).await? {
            let dir = base.join(&first).join(&second);
            for name in sorted_names(&dir).await? {
                if !is_sha256(&name) {
                    continue;
                }
                if let Ok(meta) = tokio::fs::metadata(dir.join(&name)).await {
                    blobs.push(BlobEntry {
                        sha256: name,
                        modified: modified_of(&meta)?,
                    });
                }
            }
        }
        Ok(BlobPage {
            blobs,
            next: dirs.get(1).cloned(),
        })
    }
}

/// Names in a directory, sorted; empty when it does not exist.
async fn sorted_names(dir: &Path) -> Result<Vec<String>, BlobError> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if let Ok(name) = entry.file_name().into_string() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Content that may be deleted: last written before `cutoff` (the grace period that
/// protects uploads whose attachment row is not committed yet) and referenced by no
/// attachment.
#[must_use]
pub fn select_orphans(
    blobs: &[BlobEntry],
    referenced: &HashSet<String>,
    cutoff: DateTime<Utc>,
) -> Vec<String> {
    blobs
        .iter()
        .filter(|b| b.modified < cutoff && !referenced.contains(&b.sha256.to_ascii_lowercase()))
        .map(|b| b.sha256.clone())
        .collect()
}

/// Content being written. Call [`BlobWriter::finish`] to store it; dropping it leaves
/// only a temporary file in the spool, which the orphan clean-up removes.
#[derive(Debug)]
pub struct BlobWriter {
    store: BlobStore,
    tmp: PathBuf,
    file: tokio::fs::File,
    hasher: Sha256,
    size: u64,
    limit: u64,
    head: Vec<u8>,
}

impl BlobWriter {
    /// Appends a chunk.
    ///
    /// # Errors
    ///
    /// [`BlobError::TooLarge`] past the limit, or file system failures.
    pub async fn write(&mut self, chunk: &[u8]) -> Result<(), BlobError> {
        self.size += chunk.len() as u64;
        if self.size > self.limit {
            let _ = tokio::fs::remove_file(&self.tmp).await;
            return Err(BlobError::TooLarge { limit: self.limit });
        }
        if self.head.len() < 512 {
            let take = (512 - self.head.len()).min(chunk.len());
            self.head.extend_from_slice(&chunk[..take]);
        }
        self.hasher.update(chunk);
        self.file.write_all(chunk).await?;
        Ok(())
    }

    /// Completes the write and hands the content to the backend.
    ///
    /// # Errors
    ///
    /// Storage failures (the spooled file is removed).
    pub async fn finish(mut self) -> Result<StoredBlob, BlobError> {
        self.file.flush().await?;
        self.file.sync_all().await?;
        let sha256 = hex::encode(self.hasher.finalize());
        let media_type = sniff(&self.head, &self.tmp);
        drop(self.file);
        if let Err(e) = self.store.backend.put(&sha256, &self.tmp, self.size).await {
            let _ = tokio::fs::remove_file(&self.tmp).await;
            return Err(e);
        }
        Ok(StoredBlob {
            sha256,
            size: self.size,
            media_type,
        })
    }
}

/// Detects a media type from the first bytes. Unknown content is
/// `application/octet-stream`; text is only `text/plain` when it is valid UTF-8 without
/// control characters.
#[must_use]
pub fn sniff(head: &[u8], _name_hint: &Path) -> &'static str {
    const SIGNATURES: &[(&[u8], &str)] = &[
        (b"%PDF-", "application/pdf"),
        (b"\x89PNG\r\n\x1a\n", "image/png"),
        (b"\xff\xd8\xff", "image/jpeg"),
        (b"GIF87a", "image/gif"),
        (b"GIF89a", "image/gif"),
        (b"PK\x03\x04", "application/zip"),
        (b"\x1f\x8b", "application/gzip"),
        (
            b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1",
            "application/x-ole-storage",
        ),
    ];
    if let Some((_, media)) = SIGNATURES.iter().find(|(sig, _)| head.starts_with(sig)) {
        return media;
    }
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return "image/webp";
    }
    let text = match std::str::from_utf8(head) {
        Ok(t) => Some(t),
        // A multi-byte character may be cut at the end of the sample.
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&head[..e.valid_up_to()]).ok(),
        Err(_) => None,
    };
    match text {
        Some(t)
            if !t
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) =>
        {
            "text/plain"
        }
        _ => "application/octet-stream",
    }
}

/// Text extraction status of an attachment (FR-EML-007; fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionStatus {
    /// Not read yet (or waiting for a retry).
    Pending,
    /// All text extracted.
    Extracted,
    /// Text extracted up to the limit.
    Truncated,
    /// Larger than the extraction limit; not read.
    TooLarge,
    /// A format Cooper does not read.
    Unsupported,
    /// Could not be read.
    Failed,
}

/// What one extraction attempt found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionAttempt {
    /// The content was read.
    Read(Extraction),
    /// The attachment is larger than the extraction limit.
    TooLarge,
    /// Extraction took longer than the time limit.
    TimedOut,
    /// The content could not be fetched from the blob store (transient).
    ContentUnavailable(String),
}

/// Attempts after which content that cannot be fetched is `failed`.
pub const MAX_EXTRACTION_ATTEMPTS: u32 = 3;

impl ExtractionStatus {
    /// Every status.
    pub const ALL: [Self; 6] = [
        Self::Pending,
        Self::Extracted,
        Self::Truncated,
        Self::TooLarge,
        Self::Unsupported,
        Self::Failed,
    ];

    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Extracted => "extracted",
            Self::Truncated => "truncated",
            Self::TooLarge => "too_large",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
        }
    }

    /// Parses a storage name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Whether extraction is over for this attachment.
    #[must_use]
    pub const fn is_final(self) -> bool {
        !matches!(self, Self::Pending)
    }

    /// The status after an attempt (`attempts` counts this one), or `None` when the
    /// attachment is not pending: a final status never changes.
    #[must_use]
    pub fn after(self, attempt: &ExtractionAttempt, attempts: u32) -> Option<Self> {
        if self.is_final() {
            return None;
        }
        Some(match attempt {
            ExtractionAttempt::Read(Extraction::Text {
                truncated: false, ..
            }) => Self::Extracted,
            ExtractionAttempt::Read(Extraction::Text {
                truncated: true, ..
            }) => Self::Truncated,
            ExtractionAttempt::Read(Extraction::Unsupported) => Self::Unsupported,
            ExtractionAttempt::Read(Extraction::Failed(_)) | ExtractionAttempt::TimedOut => {
                Self::Failed
            }
            ExtractionAttempt::TooLarge => Self::TooLarge,
            ExtractionAttempt::ContentUnavailable(_) if attempts < MAX_EXTRACTION_ATTEMPTS => {
                Self::Pending
            }
            ExtractionAttempt::ContentUnavailable(_) => Self::Failed,
        })
    }
}

/// Attachment metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// Owning aggregate type, for example `ticket`.
    pub owner_type: String,
    /// Owning aggregate ID.
    pub owner_id: Uuid,
    /// File name as uploaded (sanitised).
    pub file_name: String,
    /// Detected media type.
    pub media_type: String,
    /// Size in bytes.
    pub size_bytes: i64,
    /// Content hash.
    pub sha256: String,
    /// Text extraction status (FR-EML-007).
    pub extraction_status: String,
    /// Who uploaded it.
    pub created_by: Actor,
    /// When.
    pub created_at: DateTime<Utc>,
}

/// A safe file name: path parts, control characters and reserved characters removed,
/// at most 255 characters; `file` when nothing is left.
#[must_use]
pub fn clean_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .take(255)
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_owned();
    if cleaned.is_empty() {
        "file".to_owned()
    } else {
        cleaned
    }
}

/// Stores attachment metadata in the caller's transaction (after the content).
///
/// # Errors
///
/// Database errors.
pub async fn insert(conn: &mut PgConnection, a: &Attachment) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO attachments (id, tenant_id, owner_type, owner_id, file_name, media_type, size_bytes, sha256,
                                  extraction_status, created_by_kind, created_by_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        a.id,
        a.tenant_id.as_uuid(),
        a.owner_type,
        a.owner_id,
        a.file_name,
        a.media_type,
        a.size_bytes,
        a.sha256,
        a.extraction_status,
        a.created_by.kind().as_str(),
        a.created_by.id(),
        a.created_at
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// Attachments of a record, oldest first (in the caller's tenant scope).
///
/// # Errors
///
/// Database errors or corrupt rows.
pub async fn of_owner(
    conn: &mut PgConnection,
    owner_type: &str,
    owner_id: Uuid,
) -> Result<Vec<Attachment>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT id, tenant_id, owner_type, owner_id, file_name, media_type, size_bytes, sha256, extraction_status,
                created_by_kind, created_by_id, created_at
         FROM attachments WHERE owner_type = $1 AND owner_id = $2 ORDER BY created_at, id",
        owner_type,
        owner_id
    )
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Attachment {
                id: r.id,
                tenant_id: TenantId::from_uuid(r.tenant_id),
                owner_type: r.owner_type,
                owner_id: r.owner_id,
                file_name: r.file_name,
                media_type: r.media_type,
                size_bytes: r.size_bytes,
                sha256: r.sha256,
                extraction_status: r.extraction_status,
                created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id)
                    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
                created_at: r.created_at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_from_content_not_names() {
        let p = Path::new("x.txt");
        assert_eq!(sniff(b"%PDF-1.7 ...", p), "application/pdf");
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nabc", p), "image/png");
        assert_eq!(sniff("Grüße\n".as_bytes(), p), "text/plain");
        assert_eq!(sniff(b"MZ\x90\x00\x03", p), "application/octet-stream");
        assert_eq!(
            sniff(&"é".as_bytes()[..1], p),
            "text/plain",
            "cut multi-byte character"
        );
    }

    #[test]
    fn file_names_are_cleaned() {
        assert_eq!(clean_file_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_file_name("C:\\Users\\me\\report.pdf"), "report.pdf");
        assert_eq!(clean_file_name("..."), "file");
        assert_eq!(clean_file_name("a<b>:\"c\".txt\u{0}"), "abc.txt");
    }

    #[tokio::test]
    async fn content_is_hashed_deduplicated_and_size_limited() {
        let dir = std::env::temp_dir().join(format!("cooper-blobs-{}", Uuid::now_v7()));
        let store = BlobStore::filesystem(&dir);
        let mut w = store.writer(1024).await.unwrap();
        w.write(b"hello ").await.unwrap();
        w.write(b"world").await.unwrap();
        let blob = w.finish().await.unwrap();
        assert_eq!(
            blob.sha256,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(blob.size, 11);
        assert_eq!(blob.media_type, "text/plain");
        assert_eq!(store.read(&blob.sha256).await.unwrap(), b"hello world");
        let mut again = store.writer(1024).await.unwrap();
        again.write(b"hello world").await.unwrap();
        assert_eq!(again.finish().await.unwrap().sha256, blob.sha256);

        let mut big = store.writer(4).await.unwrap();
        assert!(matches!(
            big.write(b"12345").await,
            Err(BlobError::TooLarge { limit: 4 })
        ));
        assert!(matches!(
            store.read("0".repeat(64).as_str()).await,
            Err(BlobError::NotFound)
        ));
        assert!(matches!(
            store.read("../../etc/passwd").await,
            Err(BlobError::NotFound)
        ));
        assert!(
            sorted_names(store.spool_dir()).await.unwrap().is_empty(),
            "nothing left in the spool"
        );
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn the_file_system_backend_lists_touches_and_deletes() {
        let dir = std::env::temp_dir().join(format!("cooper-blobs-{}", Uuid::now_v7()));
        let store = BlobStore::filesystem(&dir);
        let mut stored = Vec::new();
        for content in [b"one".as_slice(), b"two", b"three"] {
            let mut w = store.writer(64).await.unwrap();
            w.write(content).await.unwrap();
            stored.push(w.finish().await.unwrap().sha256);
        }
        let backend = store.backend();
        let mut listed = Vec::new();
        let mut page = None;
        loop {
            let p = backend.list(page).await.unwrap();
            listed.extend(p.blobs.into_iter().map(|b| b.sha256));
            match p.next {
                Some(next) => page = Some(next),
                None => break,
            }
        }
        listed.sort();
        let mut expected = stored.clone();
        expected.sort();
        assert_eq!(listed, expected);

        // A duplicate upload refreshes the modification time (grace period).
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let path = dir.join(blob_key(&stored[0]));
        std::fs::File::options()
            .append(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let before = backend.modified(&stored[0]).await.unwrap().unwrap();
        let mut w = store.writer(64).await.unwrap();
        w.write(b"one").await.unwrap();
        w.finish().await.unwrap();
        let after = backend.modified(&stored[0]).await.unwrap().unwrap();
        assert!(after > before + chrono::Duration::minutes(60));

        backend.delete(&stored[0]).await.unwrap();
        backend.delete(&stored[0]).await.unwrap();
        assert!(backend.modified(&stored[0]).await.unwrap().is_none());
        assert_eq!(backend.kind(), "filesystem");
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[test]
    fn orphans_are_old_and_unreferenced() {
        let now = Utc::now();
        let entry = |c: char, hours: i64| BlobEntry {
            sha256: c.to_string().repeat(64),
            modified: now - chrono::Duration::hours(hours),
        };
        let blobs = [entry('a', 48), entry('b', 48), entry('c', 1)];
        let referenced: HashSet<String> = ["b".repeat(64)].into_iter().collect();
        let cutoff = now - chrono::Duration::hours(24);
        assert_eq!(
            select_orphans(&blobs, &referenced, cutoff),
            ["a".repeat(64)],
            "b is referenced and c is inside the grace period"
        );
        assert!(select_orphans(&blobs, &referenced, now - chrono::Duration::hours(72)).is_empty());
    }

    #[test]
    fn extraction_status_moves_only_from_pending() {
        use ExtractionStatus as S;
        let text = |truncated| {
            ExtractionAttempt::Read(Extraction::Text {
                text: "x".into(),
                truncated,
            })
        };
        assert_eq!(S::Pending.after(&text(false), 1), Some(S::Extracted));
        assert_eq!(S::Pending.after(&text(true), 1), Some(S::Truncated));
        assert_eq!(
            S::Pending.after(&ExtractionAttempt::Read(Extraction::Unsupported), 1),
            Some(S::Unsupported)
        );
        assert_eq!(
            S::Pending.after(&ExtractionAttempt::Read(Extraction::Failed("x".into())), 1),
            Some(S::Failed)
        );
        assert_eq!(
            S::Pending.after(&ExtractionAttempt::TimedOut, 1),
            Some(S::Failed)
        );
        assert_eq!(
            S::Pending.after(&ExtractionAttempt::TooLarge, 1),
            Some(S::TooLarge)
        );
        let gone = ExtractionAttempt::ContentUnavailable("network".into());
        assert_eq!(S::Pending.after(&gone, 1), Some(S::Pending), "retried");
        assert_eq!(
            S::Pending.after(&gone, MAX_EXTRACTION_ATTEMPTS),
            Some(S::Failed),
            "until the attempts run out"
        );
        for done in S::ALL.into_iter().filter(|s| s.is_final()) {
            assert_eq!(done.after(&text(false), 1), None);
            assert_eq!(S::parse(done.as_str()), Some(done));
        }
    }
}
