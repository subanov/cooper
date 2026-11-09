# 0033. S3 blob store over SigV4, background text extraction, Mailbox Consent with PKCE, and Portal Notifications

- **Status:** Accepted
- **Date:** 2026-11-09
- **Issue:** #107, #110, #242, #247
- **Requirements:** FR-GEN-007, FR-EML-001, FR-EML-007, FR-EML-012, FR-TKT-041, FR-PRT-007
- **Related:** [ADR-0007](0007-attachment-storage.md), [ADR-0013](0013-two-factor-ip-bans-api-keys-and-encryption-at-rest.md),
  [ADR-0016](0016-in-app-notifications.md), [ADR-0017](0017-outbound-email-and-attachments.md),
  [ADR-0019](0019-email-templates.md), [ADR-0020](0020-microsoft-graph-and-gmail-mailboxes.md),
  [ADR-0021](0021-portal-accounts.md), [ADR-0025](0025-user-groups-manager-grants-and-oidc.md)

## Context

ADR-0007 planned two blob store backends and a clean-up job; P1 shipped only the file
system (ADR-0017). Attachment text was meant to feed ticket search (FR-EML-007,
FR-TKT-041) but no text was extracted, so every attachment stayed `pending`.
Administrators had to paste refresh tokens for Microsoft `delegated` and Google Mailboxes
(ADR-0020), and Requesters had no notifications on the Portal (FR-PRT-007).

FreeITSM intent, read from its wiki and schema:

- `attachment_text` keeps the text of each attachment durably (status, extractor, text,
  characters), so re-indexing never reads a file twice. Tier 1 reads plain text and
  Office Open XML in PHP; tier 2 sends PDFs and images to an optional Apache Tika server.
- Mailboxes are connected by the OAuth consent (authorization code) flow run by FreeITSM
  itself.
- `portal_notifications` is a bell on the Portal; FreeITSM fills it mainly for Managers
  (new tickets of their team).

## Decision

### Blob store backends (FR-GEN-007, #242)

- `cooper_platform::attachments::BlobStore` is a local **spool** plus a `BlobBackend`
  port (`put`, `get`, `modified`, `delete`, paged `list`) with two implementations:
  `FilesystemBlobStore` and `S3BlobStore`. Uploads still stream into the spool while
  they are hashed, size-checked and sniffed; only complete content reaches the backend,
  at the content address `sha256/ab/cd/<hash>` (with an optional key prefix on S3).
- Chosen at start-up: `COOPER_BLOB_STORE=filesystem` (default, `COOPER_BLOB_DIR`) or
  `s3` with `COOPER_S3_ENDPOINT`, `COOPER_S3_REGION`, `COOPER_S3_BUCKET`,
  `COOPER_S3_ACCESS_KEY_ID`, `COOPER_S3_SECRET_ACCESS_KEY` (a `Secret`, never logged),
  `COOPER_S3_PREFIX` and `COOPER_S3_PATH_STYLE` (MinIO, SeaweedFS, Ceph). Objects are
  private; downloads keep going through Cooper's checks.
- **No SDK.** Requests are signed with AWS Signature Version 4 over `hmac`, `sha2` and
  `reqwest` (all already in the tree; `reqwest` gains its `stream` feature so the spooled
  file is uploaded as a stream). The payload hash is the content address we already
  computed. Unit tests use the AWS documentation's signing examples; an integration test
  runs against any S3 service when `COOPER_TEST_S3_ENDPOINT` is set (SeaweedFS in
  `docker compose --profile s3`) and is skipped otherwise. `aws-sdk-s3` (hundreds of
  crates, its own TLS stack), `rust-s3` and `object_store` (a second `reqwest` major)
  were rejected for their weight.
- **Orphan clean-up:** the `attachment-orphan-cleanup` Background Job (daily, 04:30 UTC)
  lists the backend page by page and deletes content that no `attachments` row (of any
  Company) references and that was last written before the grace period
  (`COOPER_BLOB_ORPHAN_GRACE_HOURS`, default 24, at least 1). It also removes abandoned
  spool files. Deduplication would otherwise race with it: an upload of content that
  already exists **rewrites** it (S3 `PUT` again) or **touches** it (file system), so
  its modification time restarts the grace period before the attachment row is written;
  the job re-reads the modification time right before each delete. The residual window
  (an upload touching content between that re-read and the delete) is milliseconds on a
  job that runs once a day, and only for content unreferenced for a whole grace period.

### Text extraction (FR-EML-007, #107)

- The `attachment-text-extraction` Background Job (every 30 s, 20 at a time) claims
  `pending` attachments of every Company, reads their content and extracts text on the
  blocking thread pool (`spawn_blocking`) under limits: 20 MiB input (`too_large`),
  100 000 characters kept (`truncated`), 64 MiB unpacked from a document, 30 s.
- Formats: plain text; HTML (tags removed with `ammonia`, scripts and styles dropped);
  Office Open XML (`.docx` body, headers, footers and footnotes, `.xlsx` shared strings,
  `.pptx` slides) read with the `zip` crate (deflate only, `zlib-rs` backend) and a tag
  stripper. Everything else, PDF and images included, is `unsupported`. Nothing in an
  attachment is executed or followed.
- Status transitions are a pure function: only `pending` moves; a read gives
  `extracted`, `truncated`, `unsupported` or `failed`; content that cannot be fetched
  is retried (5, then 10 minutes later) and is `failed` after 3 attempts. The status,
  error, attempts and time are stored on the attachment; status is in the API.
- Text is stored on `attachments.extracted_text` with a generated `tsvector`; ticket
  search adds the text of the ticket's attachments to its `OR` list. Extraction is
  derived data written by automation, so it is not audited.

### Mailbox Consent (FR-EML-001, FR-EML-012, #247)

- `POST /api/v1/mailboxes/{id}/consent` (Mailbox administration, Analyst session)
  returns the provider's authorize URL: Microsoft identity platform v2
  (`Mail.ReadWrite offline_access`, the Mailbox's directory tenant) or Google
  (`gmail.modify`, `access_type=offline`, `prompt=consent`), with `login_hint`, a
  random `state` and a PKCE `S256` challenge, as for OIDC sign-in (ADR-0025).
- `mailbox_consents` keeps only the SHA-256 of the state, the verifier, the Mailbox,
  the Administrator and an expiry ten minutes ahead. The callback
  (`GET /api/v1/mailboxes/consent/callback`) deletes the row as it reads it (one use),
  requires the same Administrator's session, exchanges the code with the verifier and
  stores the refresh token in the Mailbox's credentials, encrypted for
  `mailbox.secret:<id>` (ADR-0013), audited as "account connected by consent". The
  existing re-encryption job covers it; the API never returns it. Token refresh on use,
  with Microsoft's rotation, is unchanged (ADR-0020).
- The redirect URI is `<COOPER_PUBLIC_URL>/api/v1/mailboxes/consent/callback`, falling
  back to the `general.base_url` System Setting; never derived from the request.
- Credentials of `delegated` and Google Mailboxes may now be saved without a refresh
  token; polling an unconnected Mailbox reports "not connected yet" in its health.
- Provider endpoints are configuration (`OAuthEndpoints`), so tests run the whole flow
  against a fake token endpoint.

### Portal Notifications and Notification Preferences (FR-PRT-007, #110)

- The notifications context owns `portal_notifications` and
  `portal_notification_preferences`, both per Requester and Company with RLS. Kinds
  (fixed): `ticket_created`, `ticket_replied`, `ticket_closed`.
- The `portal-notifier` handler turns `ticket.created`, `ticket.reply_sent`, public
  `ticket.note_added` and `ticket.closed` into a Portal Notification for the ticket's
  Requester, once per event (unique source event), never for the Requester's own action,
  and only when the kind is on in-app.
- Preferences are per kind, `in_app` and `email`, all on by default, changed by the
  Requester (audited). The email preference governs the **Email Templates** sent to the
  Requester (FR-EML-009) through a service-desk port implemented by `cooper-server`;
  Analyst Replies are the conversation and always go out.
- `/api/v1/portal/notifications` (list, unread count, mark read, mark all read) and
  `/api/v1/portal/notification-preferences` (get, put) take the Requester and Company
  from the Portal session; anything else is "not found".

## Consequences

- New dependency: `zip` 9.1 (MIT), deflate only, pinned in `Cargo.lock` to a release
  older than seven days (the crate releases often). `reqwest` gains the `stream`
  feature; `cooper-platform` now depends on `reqwest`, `hmac`, `ammonia`,
  `tokio-rustls`, `webpki-roots` and `zip`.
- Deviation from FreeITSM: no Apache Tika tier; PDF and images are `unsupported` until
  a vetted pure-Rust reader is chosen (or a sandboxed external extractor, with its own
  ADR).
- Deviation from FreeITSM: Portal Notifications tell Requesters about their own tickets
  with per-kind preferences; notifications to Managers about their reports' tickets are
  not part of this decision.
- The "Connect account" button on a Mailbox screen comes with the Mailbox administration
  screens of the web UI; the callback redirects to `/?mailbox_consent=connected`.
- Backups now cover the bucket (versioning or replication) when S3 is used (ADR-0007).
