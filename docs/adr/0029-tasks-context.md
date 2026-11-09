# 0029. Tasks: real occurrences, a pure recurrence engine, and ticket-side related records

- **Status:** Accepted
- **Date:** 2026-11-08
- **Issue:** #126, #127, #128, #129, #130, #105
- **Requirements:** FR-TSK-001 to FR-TSK-006, FR-TKT-030

## Context

In FreeITSM, a Task has a status, a priority, dates, an assignee, Subtasks, tags,
collaborators, comments, time and links. Its recurrence is modelled on Microsoft
Planner. A recurrence is a **series, not a template**: each occurrence is a real Task.
It repeats either **on completion** (counted from the day the Task was finished) or on
a **fixed schedule** (whether or not the previous occurrence is done; the backlog is
the point). All recurrence arithmetic works on calendar dates.

In FreeITSM, Tasks also have Supplier and Contact assignees and links to Contracts and
Projects. Cooper does not have those contexts yet.

## Decision

- A new crate `cooper-tasks` (bounded context, AGENTS.md §3), behind the **Tasks**
  Module (`/tasks`, `/task-statuses`, `/task-priorities`).
- **Task aggregate.** A Task has a title, description, status, priority, start and due
  dates (dates, not instants), an estimate in minutes, an assigned Analyst and/or Team,
  tags, a parent (for a Subtask), a Board position and its series.
- **Statuses** are reference data. Each has `is_closed`, plus `is_done` to tell a
  completion (Done) apart from an abandonment (Cancelled): only a completion repeats.
  The default status must be open.
- **Subtasks are Tasks.** They take their parent's Company, and there is only one level:
  a Subtask cannot have Subtasks of its own.
- **The Board.** It has one column per active status, in status order, showing
  top-level Tasks by `board_position`. Moving a card to a status and an index renumbers
  that column in steps of 1024, in one transaction.
- **The recurrence engine** (`recurrence.rs`) is pure and is tested with proptest:
  - The next date is always later than the base date, fits the rule, and is the first
    date that fits (checked against brute force for interval 1).
  - Frequencies: daily; weekly on chosen ISO weekdays; monthly on a day (clamped to the
    month length, or the last day); monthly on the nth or last weekday, skipping months
    that have no such day; yearly. Each has an interval.
  - End conditions: never, on a date, or after N occurrences in total (the first Task
    counts).
  - The rule is stored as JSON and validated before it is saved.
- **On completion.** Moving a Task into a Done status creates the next occurrence in the
  same transaction. The next occurrence is a copy of the work, open again: the start
  date keeps the same lead time before the due date, collaborators are copied, Subtasks
  are copied as open Subtasks, and `previous_occurrence_id` points back. When the end
  condition is reached, the series becomes inactive.
- **Fixed schedule.** The `task-recurrence` job creates every occurrence whose date has
  come, up to 50 per series per run, using the newest occurrence as the template.
- **Assignment.** Assigning an Analyst raises `task.assigned`. The `TaskNotifier` sends a
  `task_assigned` Notification (with `task_id`), unless the Task has been reassigned or
  closed by then.
- **Links** to tickets and Changes of the same Company, reached through those contexts'
  application APIs. Lists hide links to records the caller cannot see. Filters
  `?ticket_id=` and `?change_id=` give the other side of a link.
- **Related records on a ticket (FR-TKT-030).** `GET /tickets/{id}/related` lives in
  `cooper-server`, the composition root. It calls the Problems, Changes, Tasks and
  Knowledge application APIs as the caller. A context is left out when its Module is off
  or the caller lacks its read Capability. The service desk crate therefore depends on
  none of them, and every link stays owned by the context it belongs to. Assets and CIs
  will be added when those contexts exist.
- **Audit.** Every change goes to the platform audit trail (`task`), including
  collaborators, comments, time and links. `GET /tasks/{id}/history` returns it. Tasks
  publish the `task` Live Updates topic.

## Deviations from FreeITSM

- **Supplier and Contact assignees, and links to Contracts and Projects, are deferred.**
  The Contracts context (FR-CON) and Projects (FR-TSK-009) do not exist yet. Adding them
  later is additive.
- **One level of Subtasks**, so Board and list views have one meaning.
- **Only completion repeats.** Cancelling a Task never creates the next occurrence.
- **Copying is fixed.** Occurrences always copy the description, assignee, tags,
  collaborators and Subtasks, and never copy links or attachments. FreeITSM's
  per-series copy switches, period tracking and evidence export are not included.

## Consequences

- The date engine can be reused (Morning Checks, Calendar) without touching storage.
- A fixed-schedule series that has been paused for a long time catches up 50
  occurrences per run, which bounds each job run.
