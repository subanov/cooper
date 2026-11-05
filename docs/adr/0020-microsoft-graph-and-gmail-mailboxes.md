# 0020. Microsoft Graph and Gmail Mailboxes

- **Status:** Accepted
- **Date:** 2026-11-13
- **Issue:** #54, #55
- **Requirements:** FR-EML-001, FR-EML-002, FR-EML-006, FR-EML-012
- **Related:** [ADR-0013](0013-two-factor-ip-bans-api-keys-and-encryption-at-rest.md),
  [ADR-0018](0018-email-intake.md)

## Context

Many service desks run on Microsoft 365 or Google Workspace, where basic-auth IMAP is
disabled. FreeITSM reads Microsoft 365 Mailboxes through Graph with `delegated` or
`app_only` auth and also supports Google. Cooper already has the intake pipeline
(ADR-0018); only the transport differs.

## Decision

- **Same pipeline, new clients.** `GraphClient` and `GmailClient` implement the existing
  `MailboxClient`/`MailboxSession` ports. Both fetch messages as **raw MIME** (Graph
  `/messages/{id}/$value`, Gmail `format=raw`), so parsing, loop protection, threading and
  matching are shared with IMAP.
- **Feature flags** `graph` and `google` on `cooper-service-desk`, forwarded by
  `cooper-server` and on by default. A build without them reports "the provider is not
  enabled in this build" in Mailbox health.
- **HTTP:** `reqwest` 0.13 with rustls on the `ring` provider and the Mozilla roots
  (`webpki-roots`), as for IMAP and SMTP: no OpenSSL, no aws-lc build. Timeouts: 15 s
  connect, 30 s per request. Error text from the provider (`error_description`,
  `error.message`) goes to Mailbox health, never tokens or URLs.
- **Credentials** are the Mailbox's encrypted secret (`mailbox.secret:<id>`, ADR-0013),
  sent as a write-only `credentials` object and checked on save: `client_id` and
  `client_secret` always; `tenant` for Microsoft; `refresh_token` for Microsoft
  `delegated` and for Google.
- **Microsoft Graph**
  - `app_only`: client credentials (`https://graph.microsoft.com/.default`), reading
    `/users/{address}`. Administrators should restrict the app with an application
    access policy to the Mailbox.
  - `delegated`: refresh token grant (`Mail.ReadWrite offline_access`), reading `/me`.
    Microsoft **rotates** refresh tokens; the new one is stored (audited as a rotation)
    before any message is handled.
  - Unread messages oldest first (`receivedDateTime` in both `$filter` and `$orderby`, as
    Graph requires). Folders are top-level display names; `INBOX` is the Inbox.
    Dispositions: delete (to Deleted Items), mark read, or mark read and move.
- **Gmail:** refresh token grant (scope `gmail.modify` at consent). The folder is a label
  (`INBOX` or a label name, case-insensitive). Unread messages in the label are imported
  oldest first. Dispositions: trash, remove `UNREAD`, or add the target label and remove
  `UNREAD` and the source label.

## Consequences

- Deviation from FreeITSM: Cooper does not yet run the OAuth consent (authorization code)
  flow itself; Administrators paste a refresh token obtained from the provider. The
  consent flow comes with the Mailbox administration screens (#247).
- Google service accounts with domain-wide delegation are not supported; one consenting
  account per Mailbox is.
- Sending from Microsoft or Google Mailboxes (Graph `sendMail`, Gmail `send`) is not part
  of this decision; outbound email still goes through SMTP (ADR-0017).
- New dependency: `reqwest` (MIT/Apache-2.0), optional.
