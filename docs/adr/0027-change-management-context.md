# 0027. Change Management: stages, CAB approval rounds, versioned templates and a calendar

- **Status:** Accepted
- **Date:** 2026-11-07
- **Issue:** #116, #117, #118, #119, #120, #121, #122, #123, #124, #125
- **Requirements:** FR-CHG-001 to FR-CHG-012, FR-CHG-014

## Context

FreeITSM's change module has configurable types and statuses grouped into fixed stages, a
risk matrix, CAB voting with `all`/`majority` rules, approval rounds that restart after
edits, templates, a checklist, comments, attachments, links, text revisions, a
Post-Implementation Review and a calendar. The rules are spread over PHP endpoints and
JavaScript: risk thresholds exist in both, the approval outcome is recomputed in several
scripts, and the seeded Emergency type waits for approval like a Normal one.

## Decision

- A new crate `cooper-change-management` (bounded context, AGENTS.md §3). It owns its
  tables. Its Module is **Changes** (`/changes`, `/change-calendar`, `/change-types`,
  `/change-statuses`, `/change-priorities`, `/change-impacts`, `/change-categories`,
  `/change-risk-levels`, `/change-templates`). Switching the Module off answers 404.
- **Change** aggregate. It holds the details (title, type, priority, impact, category,
  Requested By, Assigned To, Approver, Work and Outage Windows, the five long texts, risk
  likelihood and impact score, CAB settings), plus the status and its Stage, the current
  Approval Round, `approved_at`, the template version it came from, the PIR, and
  `closed_at`. Numbers are `CHG-00001`, taken from one instance-wide sequence.
- **Stages are code; statuses are data.** `check_stage_move` is the only place where the
  order is defined: draft ↔ awaiting, approved → implementing → done, any open stage →
  stopped, stopped → draft. Only a decision enters `approved`; moving the status there by
  hand is refused. A **Standard** type (`is_standard`) may go draft → implementing. The
  `emergency` route may go awaiting → implementing, and its votes are still taken while
  it is under way or done.
- **Approval.** Entering `awaiting` opens a new round. Who decides:
  - With `cab_required`, the CAB decides. Its members are set while the Change is a
    draft; submitting needs at least one.
  - Otherwise the named Approver decides.
  - Without an Approver, any Analyst with `changes.approve` decides.

  Each decider votes once per round, and votes are append-only (a trigger rejects
  `UPDATE` and `DELETE`). A rejection needs a comment. `Deciders::outcome` is a pure
  function. A rejection by anyone who counts (the Approver, a required member, or any
  member when none is marked required) rejects. Optional members only advise. `all`
  needs every counted member, and `majority` needs more than half. An approval moves
  the Change to the first `approved` status (unless it is already under way). A
  rejection moves it to the first `draft` status. Votes are serialised per Change with
  `SELECT … FOR UPDATE`, so two concurrent last votes cannot both miss the outcome.
- **Material edits restart approval** (FR-CHG-007). An edit in `awaiting` or `approved`
  is material when it changes what the deciders judged: the title, type, impact, a window,
  the risk factors, a long text or the CAB settings. Such an edit moves the Change back to
  `awaiting` with a new round and a `change.submitted` event. People, priority and
  category are not material. Earlier rounds and their
  votes stay readable (`GET /changes/{id}/votes?round=`).
- **Risk Levels are reference data** (`change_risk_levels`). The bands must cover scores
  1–25 exactly once. The API returns the score and the level name with every Change, so
  the UI computes nothing.
- **Text Revisions** store each saved value of the five long-text fields. A line diff
  (`/revisions/{field}/compare`) is computed in the domain.
- **Change Templates** are instance-wide and optionally limited to Teams. Saving
  different content makes a new version (`change_template_versions`). A Change records
  the template and version it came from, and later template edits do not touch it.
  Template values fill only the fields the Analyst left empty. Analysts see the active
  templates of their Teams; configurers see all of them.
- **Calendar.** `GET /change-calendar?from=&to=` (at most 366 days) returns the Changes
  whose Work or Outage Window touches the period, without stopped ones, plus every pair
  of different Changes whose windows overlap.
- **Links** to tickets, Problems and other Changes in the same Company. Tickets and
  Problems are reached only through their contexts' application APIs (the `Directory`
  port in `cooper-server`), so their visibility rules apply. The ticket's side is
  `GET /changes?ticket_id=`. CI links follow the CMDB (P3).
- **Attachments** use the platform attachments table (`owner_type = 'change'`) and blob
  store. The size limit is the `attachments.max_size_mb` System Setting.
- **Events and Notifications** (FR-CHG-014). `change.submitted`, `change.approved`,
  `change.rejected`, `change.scheduled` and `change.completed` go through the outbox. The
  `ChangeNotifier` sends `change_submitted` to the deciders, and the other four to
  Requested By and Assigned To (never to the Analyst who acted). Notifications get a
  `change_id`. Live Updates publish the `change` topic. The SSE endpoint forwards it to
  Analysts with `changes.read` in the Change's Company.
- **Tenancy.** Changes and everything hanging off them are tenant-scoped with RLS.
  Reference data and templates are instance-wide, like the service desk's.

## Deviations from FreeITSM

- **Requested By** is an Analyst (`requested_by_id`), as in FreeITSM. Cooper does not call
  it "Requester", because Requester means a Portal user in the vocabulary.
- The seeded Emergency type uses the `emergency` route (FR-CHG-002).
- Risk thresholds live in one table, not in PHP and JavaScript.
- A rejection always returns the Change to its first draft status. FreeITSM's optional
  "reject to status" setting is not carried over: a rejected Change has to be revised
  and resubmitted, and Cancelled is a separate, deliberate move.
- The CAB is fixed once the Change is submitted. FreeITSM lets members change in the
  middle of a round, which changes the outcome rule after people have voted.
- Votes are final within a round. FreeITSM lets a member overwrite their vote. Cooper
  keeps votes append-only (FR-CHG-007), so a member who changes their mind comments, and
  the Change is revised into a new round.
- The form layout designer (FR-CHG-013, P4) is not part of this decision.

## Consequences

- Other contexts can rely on `change.*` events and on `GET /changes?ticket_id=` /
  `?problem_id=` without reading change tables.
- Adding a status never needs code; adding a Stage does, on purpose.
- The approval outcome has one implementation with unit tests; the API and the
  notifications reuse it.
