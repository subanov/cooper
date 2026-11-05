# 0018. Email intake: Mailboxes, threading, matching and loop protection

- **Status:** Accepted
- **Date:** 2026-11-12
- **Issue:** #53, #56, #57, #58, #59, #60, #61, #64, #91
- **Requirements:** FR-EML-001 to FR-EML-006, FR-EML-010 to FR-EML-012, FR-TKT-009
- **Related:** [ADR-0013](0013-two-factor-ip-bans-api-keys-and-encryption-at-rest.md),
  [ADR-0017](0017-outbound-email-and-attachments.md)

## Decision

- **Mailboxes** belong to the service desk: provider (`imap` now; `microsoft` and
  `google` follow), connection settings, a write-only secret encrypted with the keyring
  (`mailbox.secret:<id>` purpose, re-encrypted after rotation by
  `mailbox-secrets-reencrypt`), defaults for new tickets (Company, Department, Origin,
  Queue), what to do after import (delete, move, mark read), unknown-sender and
  rejected-email actions, and a Whitelist of addresses or domains (block entries always
  win; with allow entries, the sender must match one; subdomains match).
- The **`mailbox-poll`** job (every minute, one worker at a time) reads up to 50 unread
  messages per active Mailbox with `BODY.PEEK[]`, imports each, then applies the
  disposition. A message whose import fails is left unread and retried. IMAP uses TLS
  (rustls) or plain for test servers; STARTTLS is not supported (use port 993).
- **Intake order** for each message:
  1. **Loop protection** (FR-EML-011): Cooper's own addresses, bounces (`mailer-daemon`,
     `postmaster`), `Auto-Submitted` other than `no`, `Precedence: bulk/junk/list`,
     auto-reply headers and delivery reports are logged as Rejected Emails and never
     create tickets.
  2. **Duplicates**: an inbound `Message-ID` already stored is skipped (a missing one is
     derived from the content), so re-reading a message never creates a second ticket.
  3. **Whitelist** (FR-EML-005).
  4. **Threading** (FR-EML-003): `[SDREF:<number>]` first, then `In-Reply-To` and
     `References` against the ticket's stored emails.
  5. **Company and Requester** (FR-EML-004): the ticket's Company when threaded; else
     the Company whose Sender Domain matches (never a Freemail Domain); else the
     Company of an existing Requester with that address; else the Mailbox default.
     Unknown senders get a Requester, or are rejected, per Mailbox.
  6. **Append or create**: a reply to a closed ticket reopens it (FR-TKT-009) and the
     Assignee gets a `reply_received` Notification; otherwise a new ticket is created
     with the Mailbox defaults and the `email` Origin. Attachments are stored.
- **Health** (FR-EML-010): each poll records last poll, last success, last error and
  failures in a row; the third failure in a row emits `mailbox.poll_failed`, and
  Administrators get a `mailbox_failing` Notification.
- CI runs GreenMail as a service so the IMAP poll is tested against a real server.

## Consequences

- New dependencies: `async-imap` (tokio), `mail-parser`, `tokio-rustls`, `webpki-roots`.
- FR-TKT-009's alternative ("create a new linked ticket") needs Ticket Links (P2).
