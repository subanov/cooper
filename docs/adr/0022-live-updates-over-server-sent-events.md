# 0022. Deliver Live Updates with PostgreSQL `NOTIFY` and Server-Sent Events that only invalidate caches

- **Status:** Accepted
- **Date:** 2026-11-07
- **Issue:** none yet (follow-up of [ADR-0010](0010-typescript-react-spa-for-analyst-console-and-portal.md), "Live updates")
- **Requirements:** FR-GEN-010, FR-GEN-002, FR-TKT-040, FR-TKT-024, FR-SLA-007
- **Related:** [ADR-0004](0004-tenant-isolation-with-row-level-security.md),
  [ADR-0005](0005-domain-events-and-transactional-outbox.md),
  [ADR-0011](0011-identity-context-sessions-and-rbac.md),
  [ADR-0014](0014-service-desk-core.md)

## Context

ADR-0010 chose Server-Sent Events (SSE) from axum for live updates, with one rule: an
event only tells TanStack Query which cache is stale; it carries no state for the client
to merge. Until now the ticket list and detail screens poll every 60 s.

Constraints:

- `cooper-server` and `cooper-worker` are separate processes, and there can be several of
  each. Ticket changes are written by both: Analysts through the API (server), email
  intake and the SLA monitor (worker). Every server must hear about every change.
- Not every ticket change is a Domain Event. Editing details, logging time, adding an
  attachment, snoozing or deleting writes an Audit entry but no outbox row (ADR-0014).
  The screens still need to refresh.
- Tenant isolation (FR-GEN-002, ADR-0004) and Confidential tickets (ADR-0014) apply to
  pushes exactly as to reads: an Analyst must not learn that a ticket they cannot see
  exists or changed.
- No new infrastructure (ADR-0005 rejected a broker for the same reason).

## Options considered

1. **Outbox handler in `cooper-worker` that calls `pg_notify`.** Reuses ADR-0005, but adds
   up to one dispatcher tick of latency, stops when the worker is down, and misses
   changes that raise no Domain Event.
2. **Trigger on `tickets` (and child tables) that calls `pg_notify`.** Catches every write,
   but puts behaviour in SQL, is easy to forget for new tables, and fires for bulk
   maintenance (imports, backfills) that should not page every browser.
3. **`pg_notify` from Rust in the writing transaction (chosen).** `cooper_platform::live::publish`
   is called by `outbox::append` for every Domain Event and by the ticket repository for
   writes without one. `NOTIFY` is transactional: it is delivered only on commit and is
   dropped on rollback, like the outbox row.
4. **Redis / NATS pub-sub.** Rejected: new infrastructure for a signal PostgreSQL already
   provides.
5. **WebSockets instead of SSE.** Rejected by ADR-0010: the stream is one-way, SSE works
   through proxies as plain HTTP, reconnects by itself and uses the session cookie.

## Decision

Option 3.

### Publishing

- `cooper_platform::live::publish(&mut tx, &LiveChange)` runs
  `SELECT pg_notify('cooper_live', '{"tenant_id":…,"topic":"ticket","id":…}')`.
  The payload names the Company, the topic (the aggregate type) and the ID. Nothing else.
- `outbox::append` publishes a `LiveChange` for every event that belongs to a Company, so
  every context gets Live Updates for its Domain Events for free (including the worker's
  SLA Warnings and Breaches and email intake).
- The service desk ticket repository also publishes on insert, update, Note, Time Entry,
  email and attachment writes. PostgreSQL drops duplicate payloads within one
  transaction, so a write that does both sends one notification.

### Fan-out in `cooper-server`

- `cooper_platform::live::LiveFeed` holds one `PgListener` (one pooled connection) per
  server process and re-broadcasts notifications on a `tokio::sync::broadcast` channel.
  It starts on the first subscriber, so `cooper-worker`, which builds the same
  `AppState`, never opens it.
- If the listener connection drops, notifications sent meanwhile are lost. After it
  reconnects the feed broadcasts `Resync`. A subscriber that falls behind the broadcast
  buffer also gets `Resync`.

### The endpoint: `GET /api/v1/live` (`text/event-stream`)

- **Session cookie only.** API keys and Portal sessions get `401`; integrations use
  webhooks (FR-WFL-005) and the Portal can get its own stream later.
- **Filtered per connection, in the application layer.** For each change the server
  checks, as the connected Analyst:
  1. the Company is in their tenant scope (cheap, no query);
  2. the topic is one the server knows how to authorise. Unknown topics are dropped
     (fail closed). Today only `ticket`;
  3. `ServiceDesk::can_see_ticket`: `tickets.read`, Company access and the Confidential
     rule, through the same repository query (and Row-Level Security) as `GET
     /tickets/{id}`. Soft-deleted tickets count as visible, so a delete reaches the
     Analysts who had the ticket on screen.
- Changes that arrive together are de-duplicated before the checks. Bursts larger than
  256 changes become one `resync`.
- **Re-validated every 60 s.** The stream re-reads the session and the Analyst's Roles
  and Company access without extending the session's idle timeout. A signed-out,
  expired, revoked or deactivated session ends the stream; changed access applies
  within a minute. An open tab therefore does not keep a session alive.
- **Events** (the data never contains record fields):
  - `ready`: sent once the feed is listening. On every `ready` after the first, the
    client refetches, because it may have missed changes while disconnected;
  - `invalidate` with `{"topic":"ticket","id":"…"}`;
  - `resync` with `{}`: refetch everything live data covers.
- Keep-alive comments every 15 s. Compression skips `text/event-stream`, and the request
  timeout covers only the response head, so neither cuts the stream. The stream ends on
  server shutdown so graceful shutdown does not wait for open tabs.

### Web

- `useLiveUpdates()` in the Analyst layout opens one `EventSource('/api/v1/live')`. On
  `invalidate` for a ticket it invalidates `["tickets", "list"]` and the queries of that
  ticket (`["tickets", "detail", id]`, `["tickets", "timeline", id]`); on `resync` or a
  reconnect it invalidates `["tickets"]`. If the browser gives up (for example after a
  `401`), it reconnects with backoff.
- Ticket queries drop the 60 s polling to a 5-minute fallback refresh (plus the existing
  refetch on window focus). It covers what pushes cannot:
  - a ticket that becomes invisible to an Analyst (made Confidential) is never pushed to
    them, so it stays on their screen until the next refetch, which then removes it;
  - the SLA time remaining counts down without any write. SLA state changes (Warning,
    Breach) are pushed by the SLA monitor's events, but the minutes shown may lag by up
    to five minutes while the window stays focused.

## Consequences

- Latency is the commit plus a visibility query per change and connection; no worker
  tick. The cost is one query per change per open Analyst console, which is ample for
  ITSM volumes; the de-duplication keeps bursts cheap. If it ever is not, the Visibility
  can be cached per connection between re-validations.
- `NOTIFY` holds a global lock briefly at commit and its queue is 8 GB; neither matters
  at ITSM write rates. Bulk imports (ADR-0008) should not call `publish`.
- New contexts get Live Updates by appending Domain Events; to show them they add a topic
  check next to `ticket` in `cooper-server` and an invalidation in the web hook. Ticket
  Presence (FR-TKT-025) and the bell (FR-GEN-006) can reuse the endpoint.
- Revisit if several regions or a connection pooler in transaction mode (PgBouncer)
  are introduced: `LISTEN` needs a session-mode connection.
