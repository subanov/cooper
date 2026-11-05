# 0007. Store attachment content in a blob store (filesystem or S3-compatible); keep metadata in PostgreSQL

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issue:** #7
- **Requirements:** FR-GEN-007, FR-EML-007, FR-AST-011; Appendix A (stateless servers, PITR)

## Context

Attachments come from emails, inline images, tickets, changes, problems, tasks,
articles and contracts (FR-GEN-007). They can be large and numerous, and text is
extracted from them for search (FR-EML-007).

`cooper-server` must be stateless and horizontally scalable (Appendix A). FreeITSM
stores files on the web server's disk under per-module folders, with the path in the
database. That ties files to one host, and they are not covered by database backups.

## Options considered

1. **In PostgreSQL** (`bytea` or large objects). This gives transactional consistency and
   a single backup. But the database grows quickly, backups and point-in-time recovery
   (PITR) slow down, replication traffic goes up, and large objects need their own
   clean-up.
2. **Local filesystem only.** Simple, but it breaks horizontal scaling unless a shared
   volume is mounted.
3. **A blob store abstraction** with filesystem (single host or shared volume) and
   S3-compatible backends (AWS S3, MinIO, Azure via an S3 gateway, and similar). Metadata
   lives in PostgreSQL.

## Decision

Option 3.

- A `BlobStore` port with `put`, `get` (streaming), `delete` and `exists`. There are two
  implementations:
  - `filesystem`: a root directory, for development and single-host installs;
  - `s3`: endpoint, bucket and credentials from the environment, for everything else.
  The backend is chosen by configuration at start-up.
- Content is **content-addressed** by SHA-256, at the key `sha256/ab/cd/<hash>`.
  - Identical files (for example a logo in every email signature) are stored once.
  - Keys are immutable, which makes caching and backups simple.
- An `attachments` table holds the tenant, owner (aggregate type and ID), file name,
  media type, size, SHA-256, extraction status and created by/at. It is tenant-scoped
  with RLS like every other tenant table.
- Uploads are streamed. The configurable size limit (FR-GEN-007) is enforced while
  streaming, and the media type is detected from content rather than trusted from the
  client.
- The metadata row is written in the aggregate's transaction **after** the blob is
  stored. A periodic background job deletes blobs that no metadata row references and
  that are older than a grace period, so failed transactions leave no permanent garbage.
- Downloads go through Cooper, which checks tenant, Confidential and ACL rules, and are
  served with `Content-Disposition: attachment` and a safe content type. There are no
  public bucket URLs.

## Consequences

- Backups are now two things: the PostgreSQL PITR backup and the blob store, with
  versioning or replication on S3, or a filesystem snapshot. The backup runbook (P1)
  covers both.
- Antivirus scanning, if a Company needs it, can hook in after `put` as an extraction
  step.
- The importer from FreeITSM ([ADR-0008](0008-freeitsm-database-import.md)) reads
  FreeITSM's attachment folders and writes them through the same `BlobStore`.

## Implementation note

P1 implements the file-system backend ([ADR-0017](0017-outbound-email-and-attachments.md)); the S3 backend and the orphan clean-up job are tracked separately.
