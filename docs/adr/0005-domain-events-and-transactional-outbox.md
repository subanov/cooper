# 0005. Domain events through a transactional outbox

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issue:** #5, #16
- **Requirements:** FR-GEN-006, FR-EML-009, FR-SLA-005, FR-CAL-002, FR-WFL-001, FR-WFL-005

## Context

AGENTS.md allows one aggregate per transaction and requires contexts to coordinate
through domain events. Many features react to events:

- notifications and email templates on ticket events (FR-GEN-006, FR-EML-009);
- SLA warnings (FR-SLA-005);
- generated calendar events (FR-CAL-002);
- workflows and webhooks (FR-WFL-001, FR-WFL-005).

Reactions must not be lost if the process crashes after the transaction commits, and
they must not happen if the transaction rolls back.

## Options considered

1. **In-process publish after commit** (for example a `tokio::broadcast` channel). Simple,
   but events are lost on a crash between commit and publish, and there is no retry.
2. **Message broker** (RabbitMQ, NATS, Kafka). Durable, but it adds infrastructure and
   still needs an outbox to avoid dual writes.
3. **Transactional outbox in PostgreSQL, dispatched by `cooper-worker`.** The event row
   commits atomically with the state change. A dispatcher delivers it to in-process
   handlers and retries on failure.

## Decision

Option 3. The implementation is in `cooper_kernel::event` and `cooper_platform::outbox`.

- Aggregates record **past-tense** events (`TicketCreated`). Each event type implements
  `DomainEvent`, which supplies:
  - a stable dotted `EVENT_TYPE` (`ticket.created`), part of the cross-context contract,
    which must never be renamed once released;
  - an `AGGREGATE_TYPE`;
  - the aggregate ID and the tenant ID.
- The application layer wraps each event in an `EventEnvelope` (ID, type, aggregate,
  tenant, Actor, `occurred_at`, JSON payload). It calls `outbox::append(&mut tx, …)` in
  the **same transaction** as the aggregate change.
- `cooper-worker` runs `outbox::Dispatcher`. It claims due rows with
  `FOR UPDATE SKIP LOCKED`, so several workers can run safely, and delivers each event
  to every registered `EventHandler` whose `handles(event_type)` matches.
- Delivery is **at least once**:
  - Handlers must be idempotent and should de-duplicate by `EventEnvelope::id`.
  - On failure, the attempt count and error are recorded and the event is retried after
    `2^attempts` seconds, capped at 1 hour.
  - After `max_attempts` (default 10) the event is marked `dead_at` and needs operator
    attention.
- There is no global ordering guarantee across retries. Handlers that care about order
  compare `occurred_at` or the aggregate state.
- Events without handlers are marked published. The outbox is a delivery queue, not an
  event store. Published rows can be pruned later.
- The dispatcher runs with `TenantScope::All`. Handlers that write tenant data open their
  own scoped transaction with the event's `tenant_id`.

## Consequences

- No new infrastructure is needed. Throughput is bounded by PostgreSQL, which is ample
  for ITSM volumes, and the partial index on pending rows keeps polling cheap.
- Latency is up to one worker tick (default 1 s). If that proves too slow,
  `LISTEN/NOTIFY` can wake the dispatcher without changing the contract.
- Outbound integrations (webhooks, P4) are themselves event handlers with their own
  delivery log, so a slow webhook does not block other handlers' retries for long.
- Dead events need visibility. The system log viewer (FR-SYS-003, P1) will list them.
