# 0031. Morning Checks: per-Company Checks, Check Dates in the instance time zone, and once-only reminders

- **Status:** Accepted
- **Date:** 2026-11-09
- **Issue:** #144, #145, #146, #147, #148
- **Requirements:** FR-MCK-001 to FR-MCK-006

## Context

In FreeITSM, Morning Checks are a daily round of health checks ("Backups completed",
"Disk space"). An Analyst records each Check for a date as Green, Amber or Red; Amber and
Red need notes. Since discussion #64, Checks can be grouped, and a group or a Check is
routed to a Team or an Analyst. A result can be cleared ("I clicked green by accident"),
and a ticket or Task raised from a result is remembered as a link. A 30-day chart shows
the counts per status.

FreeITSM is single-tenant and takes "today" from the PHP server's clock
(`date('Y-m-d')`). It deletes Checks (with their results) and statuses (snapshotting
their label onto the results, which then need a normalisation tool). It stores any
`entity_id` a client sends as a result link, without checking that the ticket or Task
exists or who may see it. Its result upsert reads before it writes, so two Analysts
saving the same Check at once can collide.

## Decision

- A new crate `cooper-morning-checks` (bounded context, AGENTS.md §3), behind the
  **Morning Checks** Module. Every route lives under `/morning-checks`. Capabilities:
  `morning-checks.read`, `morning-checks.record` (results and their links) and
  `morning-checks.configure` (Check Groups, Checks, Check Statuses).
- **Reference data.** Check Statuses are instance-wide lookups with a colour and
  `requires_notes`, seeded Green, Amber, Red. Only the flag decides whether notes are
  required; no status name appears in code. Statuses are deactivated, never deleted.
- **Companies.** Check Groups and Checks belong to one Company. A Check's group is of
  the same Company (a composite foreign key enforces it). Results, Result Links and
  reminders take the Check's Company. RLS (`cooper_tenant_visible`) on every table.
- **Routing.** A group routes to a Team or an Analyst, never both. A Check is routed to
  its own Analyst, else its group's Analyst, else its group's Team (computed by
  `routing()`, never stored). Groups and Checks are deactivated, never deleted; the
  Round is every active Check whose group, if any, is active.
- **Check Dates.** A Check Date is a calendar day in the instance time zone, a new
  administration System Setting `general.time_zone` (IANA name, default `UTC`; chrono-tz,
  ADR-0002). An unknown name falls back to UTC with a warning in the System Log. A result
  may be recorded for today or an earlier day, never for a later one.
- **Check Results.** `CheckResult` is its own aggregate: one per Check per Check Date
  (a unique constraint). Recording again changes it and credits the Analyst as
  `modified_by`, keeping `recorded_by`. If two Analysts record at once, the loser of the
  insert race changes the winner's result. A result needs an active status, notes when
  the status requires them, and a Check in the Round. Clearing deletes the result and its
  Result Links (the audit trail keeps what it was). Events: `check_result.recorded`,
  `check_result.cleared`.
- **Result Links** to tickets and Tasks go through the service desk and Tasks
  application APIs, as the caller (adapters in `cooper-server`). The target must be
  visible to the caller and of the result's Company; lists hide links whose target the
  caller cannot see. `POST /morning-checks/results/{id}/follow-up-task` creates a Task
  through the Tasks API (routed like the Check, with its notes) and links it; it needs
  `tasks.create` and the Tasks Module. A ticket's related records (FR-TKT-030) list the
  results that link to it.
- **History and export.** `GET /morning-checks/history` is a grid of Checks × dates (at
  most 92 days) with the statuses and their colours. `GET /morning-checks/history/export`
  is CSV (at most 366 days and 50,000 rows), built with a new shared helper,
  `cooper_http::csv`: RFC 4180 quoting, and a `'` before any cell that starts with `=`,
  `+`, `-`, `@`, a tab or a carriage return, so a spreadsheet shows it as text.
- **Reminders.** Settings `morning_checks.reminders` (off by default),
  `morning_checks.reminder_time` (`HH:MM`, default 10:00) and
  `morning_checks.reminder_weekends`. The `morning-check-reminders` job (every five
  minutes) works out, once the time has come in the instance time zone, how many Checks
  in the Round are unrecorded per Check Group and Analyst (every active member of a
  routed Team). Each reminder is a row in `check_reminders`, unique per Company, date,
  group and Analyst, written with its `morning_checks.unrecorded` event in one
  transaction; a repeated run or a restarted worker finds the row and sends nothing. The
  notifications context's `CheckReminderNotifier` turns the event into a
  `checks_unrecorded` Notification, unless the Checks were recorded in the meantime.

## Deviations from FreeITSM

- **Nothing is deleted.** Checks, groups and statuses are deactivated, so the history
  keeps every Check and status, and no orphan normalisation is needed.
- **A group routes to a Team or an Analyst**, not both.
- **Dates come from the instance time zone**, never the server clock; no future dates.
- **Result Links are verified** (exists, visible, same Company) instead of trusting the
  client.
- **CSV export** instead of a printed PDF; the grid replaces the stacked chart and also
  shows results whose status is no longer active.
- **Reminders** are new (FR-MCK-006); FreeITSM has none.

## Consequences

- `general.time_zone` can be reused by other contexts that need calendar days
  (Calendar, reporting).
- `cooper_http::csv` is the shared building block for future exports.
- Reading the Round loads a Company's Checks and groups in one query each; that is fine
  for the tens to hundreds of Checks an instance has, but would need paging if Checks
  ever numbered in the thousands.
