//! Attachments (FR-GEN-007, ADR-0007): a content-addressed blob store plus metadata rows.
//!
//! Content is written by [`BlobWriter`] while it streams in: hashed with SHA-256, checked
//! against the size limit, written to a temporary file and moved into place under
//! `sha256/ab/cd/<hash>` only when complete. Identical files are stored once. The media
//! type is detected from the content, never taken from the client. Contexts decide who
//! may attach to or read from their records; this module only stores.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, TenantId};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

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
}

/// Content stored on the local file system (single host or a shared volume).
#[derive(Debug, Clone)]
pub struct FilesystemBlobStore {
    root: PathBuf,
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

impl FilesystemBlobStore {
    /// A store under `root` (created when needed).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, sha256: &str) -> Option<PathBuf> {
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(
            self.root
                .join("sha256")
                .join(&sha256[..2])
                .join(&sha256[2..4])
                .join(sha256),
        )
    }

    /// Starts writing new content of at most `limit` bytes.
    ///
    /// # Errors
    ///
    /// File system failures.
    pub async fn writer(&self, limit: u64) -> Result<BlobWriter, BlobError> {
        let tmp_dir = self.root.join("tmp");
        tokio::fs::create_dir_all(&tmp_dir).await?;
        let tmp = tmp_dir.join(Uuid::now_v7().to_string());
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
    /// [`BlobError::NotFound`] or file system failures.
    pub async fn read(&self, sha256: &str) -> Result<Vec<u8>, BlobError> {
        let path = self.path_for(sha256).ok_or(BlobError::NotFound)?;
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(BlobError::NotFound),
            Err(e) => Err(e.into()),
        }
    }
}

/// Content being written. Call [`BlobWriter::finish`] to store it; dropping it leaves
/// only a temporary file, which is never referenced.
#[derive(Debug)]
pub struct BlobWriter {
    store: FilesystemBlobStore,
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

    /// Completes the write and moves the content into place.
    ///
    /// # Errors
    ///
    /// File system failures.
    pub async fn finish(mut self) -> Result<StoredBlob, BlobError> {
        self.file.flush().await?;
        self.file.sync_all().await?;
        let sha256 = hex::encode(self.hasher.finalize());
        let target = self.store.path_for(&sha256).ok_or(BlobError::NotFound)?;
        if let Some(dir) = target.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        if tokio::fs::try_exists(&target).await? {
            tokio::fs::remove_file(&self.tmp).await?;
        } else {
            tokio::fs::rename(&self.tmp, &target).await?;
        }
        Ok(StoredBlob {
            sha256,
            size: self.size,
            media_type: sniff(&self.head, &self.tmp),
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
        let store = FilesystemBlobStore::new(&dir);
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
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}
