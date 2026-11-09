# 0032. Reporting: owning contexts count in SQL as the caller, Reporting composes; no read model

- **Status:** Accepted
- **Date:** 2026-11-09
- **Issue:** #149, #150, #151, #152, #153
- **Requirements:** FR-RPT-001, FR-RPT-002, FR-RPT-003, FR-RPT-005, FR-RPT-007
- **Related:** [ADR-0004](0004-tenant-isolation-with-row-level-security.md), [ADR-0005](0005-domain-events-and-transactional-outbox.md), [ADR-0015](0015-sla-clock.md), [ADR-0031](0031-morning-checks-context.md)

## Context

Reports aggregate data that other contexts own: tickets (statuses, Priorities,
Departments, Types, Origins, Queues, Assignees, SLA results, First Time Fix, Time
Entries), Tasks and Morning Checks. AGENTS.md §3 forbids reading another context's tables;
contexts talk through application APIs or domain events.

In FreeITSM, the ticket dashboard (`api/tickets/get_ticket_widget_data.php`) builds SQL
over `tickets` and joins every lookup and `analysts` directly, buckets by `DATE()` in UTC
and counts First Time Fix as "Yes/No" over every filtered ticket, open ones included.
Widget layouts are stored per Analyst. The Watchtower (`includes/watchtower_queries.php`)
reads every module's tables in one function; "Watchtower settings" choose which ticket
statuses, high Priorities, task statuses and "attention" check statuses the cards count,
with defaults (all open statuses; Priorities above the default; all but the first check
status). Its comments record bugs fixed over time: totals that silently counted more
than the rows shown, cards that looked up hard-coded status names, scope filters that
turned `LEFT JOIN`s into inner joins. There is no SLA compliance report. Table export
(`api/tickets/table_export.php`) writes whatever the grid shows.

Two shapes were possible:

- **(a) Query APIs on the owners.** Each owning context exposes aggregate queries in its
  application API (counts grouped by one dimension, done in SQL inside the owner), and
  Reporting composes them through ports implemented by adapters in `cooper-server`.
- **(b) A Reporting read model** fed by domain events from the outbox.

(b) would need every event to carry every reported attribute, a backfill for existing
data, and a second copy of the Confidential, Company and SLA rules that could drift from
the service desk's. Reports would also lag the outbox.

## Decision

- **Approach (a).** A new crate `cooper-reporting`, behind the **Reporting** Module
  (route prefixes `/reports` and `/watchtower`). It owns no business data, only the
  Watchtower Items.
- **The owner counts, as the caller.** New application methods count in SQL with
  `GROUP BY` and return one tally per group, never rows:
  - service desk: `ticket_tally` (by status, Priority, Department, Type, Origin, Queue,
    Assignee, Company, or day/month of creation or closure, with the First Time Fix count
    per group), `sla_tally` and `time_tally` (port `TicketStatistics`);
  - Tasks: `open_tally` (open top-level Tasks per status, overdue, due today) and
    `time_tally`;
  - Morning Checks: `round_tally` (today's Round, recorded results per status, optionally
    only Checks routed to the caller).

  Each takes the caller's `Principal`, checks the owner's read Capability, narrows to one
  Company only within the caller's scope, opens the transaction with
  `tenancy::begin` (RLS), and filters exactly like the owner's lists. Reports therefore
  need `reporting.read` **and** the data's own read Capability.
- **Confidential rule.** Ticket statistics use the same visibility predicate as ticket
  lists: a Confidential ticket is counted only for Analysts with
  `tickets.view_confidential` or a Team serving its Department. It is left out of every
  figure for anyone else — counts, Department rows, SLA results, First Time Fix and
  Time Entries on it — so no report can reveal that it exists. Deleted tickets are left
  out; merged-away tickets too (their work continues on the target).
- **Time zone.** Days and months are calendar days in the System Setting
  `general.time_zone` (ADR-0031). Reporting turns a period of local dates into a
  half-open range of instants (`start_of_day`, which handles a skipped or repeated
  midnight); the owner buckets with `date_trunc(unit, ts AT TIME ZONE tz)`. Work dates of
  Time Entries are already dates. Empty buckets are filled in Reporting so charts have
  every day or month.
- **SLA compliance reuses the service desk's results.** `sla_tally` classifies each
  ticket's stored due and done times (`response_due_at`/`responded_at`,
  `resolution_due_at`/`closed_at`, `sla_paused_at`) with the case analysis of
  `Ticket::sla_state`, at the same `now`; a storage test checks the two agree for every
  combination. Buckets: met, met late, breached (open, past due), running (on track,
  warning, paused), no target. Compliance = met ÷ (met + met late + breached); no value
  when nothing is decided. Tickets are selected by creation date.
- **First Time Fix Rate** = tickets closed in the period with the flag ÷ tickets closed
  in the period; no value when none were closed.
- **Labels.** Owners return IDs. Reporting resolves names through a `Labels` port: lookups
  (with their order and flags) from their owners, and names of Analysts, Teams,
  Departments and Companies from identity, read as Automation (names are not sensitive;
  only the counted IDs come from the caller's figures). Rows of reference data keep
  their admin-defined order; `(none)` comes last.
- **Watchtower.** Tiles: open tickets per chosen open status (the total is the sum of
  the rows shown), high-priority, unassigned (everybody's, whatever the scope), open
  Tasks per chosen status with overdue and due today (today in the instance time zone),
  and today's Round with results per Check Status and those needing attention. Scope
  `all` or `mine`. A section is `null` when its Module is off or the caller lacks its
  read Capability, instead of failing the whole page. **Watchtower Items** are one
  instance-wide table (`watchtower_items`, like the statuses it names; no `tenant_id`,
  so no RLS), replaced as a whole by `PUT /watchtower/items` with `reporting.configure`,
  validated against the owners' reference data (existing values; open statuses only),
  and audited. No rows of a kind means the default rule.
- **CSV.** Every report has `…/export` with the same parameters, built with
  `cooper_http::csv`. Rows are bounded by the period (at most 366 days by day, 1,830
  otherwise). `GET /reports/ticket-list/export` takes the filters of `GET /tickets`,
  pages through `list_tickets` as the caller and refuses more than 10,000 tickets
  (narrow the filters) rather than truncating silently.
- **Indexes** (migration `20261123100000_reporting.sql`): tickets by `(tenant_id,
  closed_at)` for closed in a period, open tickets by `(tenant_id, status_id,
  priority_id)`, and Time Entries by `(tenant_id, work_date)`. Created-in-period uses the
  existing `(tenant_id, created_at)` list index. Checked with `EXPLAIN ANALYZE` on
  800,000 tickets: bitmap index scans, about a millisecond per Company and month.

## Deviations from FreeITSM

- Buckets follow the instance time zone, not UTC.
- The First Time Fix Rate counts closed tickets only, with no rate for an empty period.
- Widget layouts are not stored: the API serves widget data and the UI arranges it.
  Saved, shareable designs are Report Packs (FR-RPT-006, P4).
- An SLA compliance report is new, and reuses the service desk's stored SLA results.
- Watchtower Items have one capability (`reporting.configure`), not separate ones for
  cards and counts; tiles for Modules Cooper does not have yet follow those Modules.
- Exports are CSV from the API with formula-injection protection, not a dump of the grid.

## Consequences

- Each new report needs a query method on its owner; the owner keeps its rules in one
  place, and a report can never see more than the caller's lists.
- Report latency is that of a few `GROUP BY` queries; there is no lag and no backfill.
- Cross-context joins (for example tickets by Requester's Cost Centre) are not possible
  in one query; they would need the owner to accept IDs from the other context, or a
  read model later.
