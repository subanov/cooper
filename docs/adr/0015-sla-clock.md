# 0015. SLA clock: calendars, due times derived from creation, and once-only warnings and breaches

- **Status:** Accepted
- **Date:** 2026-11-09
- **Issue:** #65, #66, #67, #68, #69, #70, #71
- **Requirements:** FR-SLA-001 to FR-SLA-007
- **Related:** [ADR-0002](0002-use-chrono-for-time.md), [ADR-0014](0014-service-desk-core.md)

## Context

FreeITSM computes SLA due dates in PHP with ad-hoc pause handling, and its status seeder
can leave "pauses SLA" off (see FR-TKT-004). SLA arithmetic is easy to get wrong around
weekends, Holidays and daylight-saving changes.

## Decision

- **SLA Calendars** (working hours per weekday, an IANA time zone, Holidays) live in the
  service desk. Working-time arithmetic (`add_working_minutes`,
  `working_minutes_between`) is a pure domain function: each local day's window is
  converted to UTC separately with `chrono-tz`, so DST changes are exact. Property tests
  check that adding *n* minutes and measuring back gives *n*, and monotonicity.
- A Priority's targets count against its own calendar, else the default calendar, else
  24×7. The calendar ID is stored on the ticket; later calendar edits do not move due
  times of open tickets.
- **Due times are derived, not accumulated**: `due = created_at + target + paused working
  minutes`, and the warning point uses `target × sla.warning_percent / 100` (default
  80). Pausing records `sla_paused_at`; resuming adds the paused working minutes and
  recomputes. A Priority change recomputes with the new targets. This keeps the clock
  correct whatever order events happen in.
- **Response** is satisfied by the first Note visible to the Requester (replies by email
  join with the email work); **resolution** by closing.
- The **`sla-monitor`** job (every minute) finds passed warning and due points of running
  clocks and inserts `sla_events (ticket, target, trigger)` with `ON CONFLICT DO NOTHING`;
  only a new row emits `ticket.sla_warning` / `ticket.sla_breached` to the outbox, in the
  same transaction. Each event is therefore raised at most once, even with several
  workers.
- **SLA Notification Rules** select recipients (Assignee, Queue members, named Analysts)
  per trigger and target; delivery happens in the Notifications work, which handles the
  SLA events.
- Ticket responses include SLA state per target (`on_track`, `warning`, `breached`,
  `paused`, `met`, `met_late`, `none`) and seconds remaining.

## Consequences

- New dependency: `chrono-tz` (planned in ADR-0002).
- `SystemClock` returns microsecond precision, matching `timestamptz`, so values echoed
  to clients equal stored values.
