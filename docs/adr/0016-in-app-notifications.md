# 0016. In-app Notifications from Domain Events, delivered idempotently

- **Status:** Accepted
- **Date:** 2026-11-10
- **Issue:** #28, #70
- **Requirements:** FR-GEN-006, FR-SLA-006, FR-TKT-010
- **Related:** [ADR-0005](0005-domain-events-and-transactional-outbox.md),
  [ADR-0015](0015-sla-clock.md)

## Decision

- A `cooper-notifications` crate owns the `notifications` table (one row per recipient,
  with `read_at`) and the bell API: list (newest first, unread filter), unread count,
  mark one or all read. Only Analysts have a bell.
- Notifications are created by **event handlers** on the outbox dispatcher, never by the
  contexts that cause them. The `ticket-notifier` handler reacts to `ticket.assigned`
  (the new Assignee), `ticket.note_added` (Analysts mentioned with `@username`) and
  `ticket.sla_warning` / `ticket.sla_breached` (recipients from the SLA Notification
  Rules: Assignee, Queue members, named Analysts).
- Handlers read event payloads by field name (the published event contract) and reach
  other contexts only through their application services, via ports implemented in
  `cooper-server`. `cooper-worker` builds the same `AppState` as the server to get them.
- **Idempotent delivery**: the outbox delivers at least once, so `(source_event_id,
  recipient_id)` is unique and inserts use `ON CONFLICT DO NOTHING`.
- The Analyst who caused an event is never notified about it. Inactive Analysts are
  skipped. Mention notifications do not include the ticket subject, because a mentioned
  Analyst might not be allowed to see a Confidential ticket.
- Requester replies (`reply_received`) are added with the email work.

## Consequences

- `cooper-worker` depends on `cooper-server`'s library for wiring.
- Live push (Server-Sent Events) to the browser comes with the web UI.
