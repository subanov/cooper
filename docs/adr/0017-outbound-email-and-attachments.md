# 0017. Outbound email through a queue with a send log; attachments on the file system first

- **Status:** Accepted
- **Date:** 2026-11-11
- **Issue:** #29, #33, #47, #48, #62
- **Requirements:** FR-GEN-007, FR-IAM-004, FR-TKT-021, FR-TKT-022, FR-EML-008
- **Related:** [ADR-0007](0007-attachment-storage.md), [ADR-0016](0016-in-app-notifications.md)

## Decision

### Outbound email (FR-EML-008)

- Contexts never speak SMTP. The notifications context owns `outbound_emails` (a queue
  with status `queued`/`sent`/`dead`) and `outbound_email_attempts` (one row per send
  attempt with its error).
- Replies are queued by the `reply-mailer` handler from `ticket.reply_sent` events, so
  the reply and its email are committed with the ticket change; the email is queued at
  least once and the unique `Message-ID` makes repeats harmless. Password reset mail is
  queued directly by an adapter of the identity `ResetMailer` port.
- The `email-sender` job (every 10 s) claims due emails with `FOR UPDATE SKIP LOCKED`,
  sends them through a `MailTransport` (SMTP via `lettre` with rustls, configured by
  `COOPER_SMTP_URL`; Mailbox-specific transports come with Mailboxes), logs every
  attempt and retries failures with the outbox backoff; after 8 attempts the email is
  `dead`. Without `COOPER_SMTP_URL` email stays queued.
- `From` comes from the `email.from_address` System Setting until Mailboxes send their
  own; `Message-ID`s use its domain. Replies carry `In-Reply-To` and `References` of the
  ticket's earlier emails.

### Replies, Signatures and Reply Templates

- A Reply goes to the Requester (plus Cc), with `[SDREF:<number>]` in the subject and
  the Analyst's Signature after `-- `. It is stored as a ticket email (timeline,
  search) and satisfies the SLA response.
- Reply Templates support `{requester_name}`, `{ticket_number}`, `{ticket_subject}` and
  `{analyst_name}`; unknown placeholders are rejected when the template is saved.

### Attachments (FR-GEN-007)

- `cooper-platform` streams uploads into a content-addressed file-system blob store
  (`COOPER_BLOB_DIR`; a shared volume for several servers): hashed while written, size
  checked against `attachments.max_size_mb`, moved into place only when complete,
  deduplicated by SHA-256. Media types are sniffed from content; file names are cleaned.
  Downloads always use `Content-Disposition: attachment` and `nosniff`.
- The S3-compatible backend and the clean-up job for unreferenced content from
  ADR-0007 follow as separate issues.

### Password reset (FR-IAM-004)

- `POST /password-reset` always answers `202`. Active Analysts get a link with a random
  token valid for 60 minutes; only its hash is stored, a new request replaces older
  tokens, and a token works once. Completing a reset ends every session and forgets
  trusted devices. The link uses the `general.base_url` setting.

## Consequences

- New dependency: `lettre` (rustls, no OpenSSL). `axum` gains the `multipart` feature.
