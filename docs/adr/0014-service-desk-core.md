# 0014. Service desk core: reference data, Ticket Numbers, ticket lifecycle, visibility and search

- **Status:** Accepted
- **Date:** 2026-11-08
- **Issue:** #25, #26, #39, #40, #41, #42, #43, #44, #45, #46, #49, #50, #51, #52
- **Requirements:** FR-TKT-001, -002, -004 to -010, -012 to -014, -020, -023, -024, -040,
  -041, FR-GEN-003, FR-GEN-004
- **Related:** [ADR-0005](0005-domain-events-and-transactional-outbox.md),
  [ADR-0006](0006-postgresql-full-text-search.md),
  [ADR-0011](0011-identity-context-sessions-and-rbac.md)

## Decision

A new `cooper-service-desk` crate (`crates/service-desk`).

- **Reference data** (Statuses, Priorities, Types, Origins, Resolution Codes) is
  instance-wide, seeded from the Vocabulary with stable `key`s and fixed UUIDs, and
  modelled as one `Lookup` type with per-kind flags (`is_closed`, `pauses_sla`,
  `is_default`, SLA minutes, Type number prefix). Flags that do not apply to a kind are
  dropped; a closed status can be neither default nor SLA-pausing. Making a row the
  default moves the flag in one transaction (a partial unique index allows only one).
  Categories form a tree of at most three levels and may be limited to Ticket Types.
- **Ticket Numbers** come from the `tickets.number_format` System Setting with tokens
  `{COMPANY}`, `{TYPE}`, `{YEAR}` and exactly one `{SEQ:n}`. Each Company has its own
  sequence in `ticket_number_counters`, incremented with `INSERT … ON CONFLICT … RETURNING`
  in its own transaction: concurrent creates never get the same value; a failed create
  leaves a gap. A unique index on the number is the last line of defence.
- **Lifecycle**: entering a closed status needs a Resolution Code (and a Closure Category
  when `tickets.require_closure_category`) and sets `closed_at`; leaving it reopens the
  ticket and clears those fields. Closing and reopening need `tickets.close`.
- **Assignment**: Queue (Team) and Assignee; with `tickets.assignee_must_be_team_member`
  (default on) the Assignee must belong to the Queue.
- **Cross-context reads** (Requesters, Departments, Teams, Analysts, Ticket Codes) go
  through a `Directory` port implemented in `cooper-server` on top of the identity
  context's application service, never its tables.
- **Visibility** is part of every query (ADR-0006): tenant filter plus RLS, soft-deleted
  rows hidden unless an Administrator asks, and Confidential tickets visible only with
  `tickets.view_confidential` or to members of Teams serving the ticket's Department.
- **Lists** combine a built-in view (`mine`, `queue`, `unassigned`, `open`, `snoozed`,
  `recently_closed`, `deleted`) with filters, full-text search (number prefix, subject and
  description `tsvector`, Note text) and keyset pagination on `created_at`, `updated_at`
  or the number. The list query is assembled with `sqlx::QueryBuilder` because its
  filters are optional; values are always bound, never interpolated.
- **Timeline** merges Notes, Time Entries and the ticket's audit history.
- Changes write audit entries and outbox events (`ticket.created`, `ticket.assigned`,
  `ticket.status_changed`, `ticket.closed`, `ticket.reopened`,
  `ticket.priority_changed`) in the same transaction, for the SLA clock and
  Notifications.

## Consequences

- Text search uses `simple` + `unaccent` via an `IMMUTABLE` wrapper
  (`cooper_unaccent`) so the search vector can be a generated column.
- Department-specific number prefixes are not supported yet (types have a prefix);
  emails and attachment text join the timeline and search with the email work.
