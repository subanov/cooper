# 0034. Calendar: per-Company events, instance-wide categories, and read-only events generated from other contexts' outbox events

- **Status:** Accepted
- **Date:** 2026-11-11
- **Issue:** #187, #184 (Calendar half)
- **Requirements:** FR-CAL-001, FR-CAL-002 (contract, warranty and lease dates), FR-CON-003, FR-AST-012

## Context

FreeITSM has an IT calendar: manual events (title, description, start and end, an
all-day flag, a location) in colour-coded categories. Events are shared by everyone,
times are stored as server-local datetimes, and nothing stops an event from ending before
it starts. Contract and asset dates are not on the calendar.

Cooper serves several Companies from one instance, so the calendar must not show one
Company's events to another. FR-CON-003 and FR-AST-012 ask for Calendar Events at
contract Notice Dates and end dates, and at asset warranty and lease expiry. The
Contracts and Assets contexts are built in parallel. Contexts may not read each other's
tables (AGENTS.md §3).

## Options considered

1. **Read other contexts' dates at query time** through their application APIs. The
   calendar would always be current, with no stored copy. But every calendar read would
   fan out to every source context, the calendar would depend on their crates, and a
   source whose Module is off or broken would break the calendar.
2. **Generated events stored in the calendar, kept in step by outbox events.** Each
   source context publishes its record's full current dates. A Calendar handler upserts
   or removes the generated events. Reads stay local and fast, and the only contract is
   an event payload. The copy is eventually consistent, and the handler must cope with
   redelivery and reordering.

## Decision

Option 2.

- A new crate `cooper-calendar` (bounded context, AGENTS.md §3), behind the **Calendar**
  Module. Routes: `/calendar/events` and `/calendar/categories`. Capabilities:
  `calendar.read`, `calendar.create` (manual events), `calendar.update` (edit and delete
  manual events) and `calendar.configure` (Calendar Categories).
- **Calendar Categories** are instance-wide reference data, like Task Statuses: a name
  (unique, case-insensitive), a `Colour` value object (`#rrggbb`, stored in lower case)
  and an active flag. There is no seed. Administrators create their own. Cooper creates
  one **system category** per source kind (`contracts`, `assets`) the first time it needs
  one. If an Administrator already has that name, Cooper adds "(generated)". System
  categories can be renamed and recoloured. They cannot be deactivated, and manual events
  cannot use them.
- **Calendar Events** belong to one Company (`tenant_id`, RLS, an isolation test). An
  `EventTiming` is either **all-day** (`start_date..=end_date`, dates with no time zone:
  a day is the same day everywhere) or **timed** (`starts_at..ends_at`, UTC instants).
  The end is never before the start. That rule is checked in the domain and again in a
  `CHECK` constraint. A list request names a range of days (`from`, `to`, at most
  366 days) and returns at most 2000 events, with a `truncated` flag. Timed events are
  matched on their UTC days. The web UI asks for one extra day on each side and places
  events on the viewer's local days.
- **Generated events.** The handler `calendar-generated-events` (`GeneratedEventsSync`)
  handles:
  - `contract.dates_changed`: `{ contract_id, contract_number, title, notice_date,
    end_date, active }`. It produces "Notice date: <number> <title>" and "Contract ends:
    <number> <title>".
  - `asset.dates_changed`: `{ asset_id, asset_tag, hostname, warranty_expires_on,
    lease_expires_on, active }`. It produces "Warranty expires: <asset tag>" and
    "Lease expires: <asset tag>", or the hostname when there is no tag.

  The Calendar parses these payloads with its own DTOs. It never depends on the
  Contracts or Assets crates. Each payload is the record's full current state. The
  handler computes the wanted events: one all-day event per date that is set, and none
  when the record is inactive. It diffs them against the stored events, which are keyed
  by `(Company, source kind, source id, source date)`, and inserts, updates or deletes
  them in one transaction. The System (Automation) writes and audits each change.
- **Idempotency and order.** `calendar_sources` stores, per source, the last applied
  event's `(occurred_at, id)`, its watermark. An event at or below the watermark is a
  no-op. This covers redelivery, and an older event that arrives late cannot undo a newer
  one, even after the newer one removed every generated event. The watermark is updated
  with a compare-and-set, so two workers syncing the same source at once cannot
  interleave. The loser gets a conflict and the outbox retries it.
- Generated events are **read-only** in the API (`409 rule-violated` on edit or
  delete). They carry their source (`kind`, `id`, `date`), and the UI links to
  `/contracts/<id>` or `/assets/<id>`.
- Every write announces a Live Update (`calendar_event`), and the web UI refetches the
  calendar.

Deviations from FreeITSM: events belong to a Company; the end must not be before the
start; all-day events are dates and timed events are UTC instants, never server-local
times; contract and asset dates appear as generated, read-only events.

## Consequences

- The Calendar learns about new sources through event contracts only. Change windows,
  Planned Maintenance, Scheduled Work and Task due dates (the rest of FR-CAL-002) can
  follow the same pattern: a `SourceKind`, the dates it carries and a system category.
- The generated copy is eventually consistent. Until the worker dispatches the outbox,
  the calendar shows the old dates.
- Contracts or assets that existed before their context published events have no
  generated events until their next change. A source context can backfill by publishing
  an event for every record.
- Deleting a Contract or Asset must be published as `active: false`, which removes its
  events.
- Notifications ahead of these dates (the other half of FR-CON-003 and FR-AST-012) are
  not part of this decision.
