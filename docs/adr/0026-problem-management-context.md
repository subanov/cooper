# 0026. Problem Management as its own context, reaching tickets through the service desk API

- **Status:** Accepted
- **Date:** 2026-11-07
- **Issue:** #111, #112, #113, #115
- **Requirements:** FR-PRB-001, FR-PRB-002, FR-PRB-003, FR-PRB-004, FR-PRB-006, FR-PRB-007

## Context

Problems are the root causes behind incidents. FreeITSM keeps `problems`,
`problem_statuses`, `problem_priorities`, `problem_notes` and `problem_tickets`, numbers
Problems `PRB-#####` from the row ID, and shows linked tickets from both sides.

## Decision

- A new crate `cooper-problem-management` (bounded context, AGENTS.md §3) with the usual
  layers. It owns its tables; its Module is **Problems** (`/problems`,
  `/problem-statuses`, `/problem-priorities`), and switching the Module off answers 404.
- **Problem** aggregate: title, description, status, priority, assigned Analyst, Root
  Cause, Workaround and Known Error. A Known Error must have a Root Cause or a Workaround
  (a flag with nothing behind it helps nobody). Entering a closed status resolves the
  Problem (`closed_at`, event `problem.resolved` with the linked ticket IDs); leaving one
  reopens it.
- **Numbers** `PRB-00001`, from one PostgreSQL sequence across the instance, as in
  FreeITSM (sequential, never reused, independent of row IDs).
- **Reference data:** Problem Statuses and Priorities with the FreeITSM seeds and stable
  keys; exactly one default each (partial unique index; making a row the default clears
  the old one).
- **Tenancy:** Problems, Notes and links are tenant-scoped with RLS. Creating needs sight
  of the Company, and links stay within it.
- **Tickets through the service desk's application API only** (`Tickets` port,
  implemented in `cooper-server`): to see a ticket (Company access and Confidential rules
  apply), and to act on it. Linking requires seeing the ticket. Lists of linked tickets
  leave out those the caller cannot see. The ticket's side is `GET /problems?ticket_id=`,
  so the Tickets Module never depends on the Problems Module. `problem_tickets` keeps a
  foreign key to `tickets` for integrity only; no Problem code reads ticket rows.
- **Acting on linked tickets** (FR-PRB-006): once the Problem is resolved, close every
  linked ticket (with a Resolution Code) or add a Note (internal or public). Each ticket
  goes through the normal service desk use case and its outcome is reported, so
  Confidential, closed or required-field rules still apply.
- **History:** every change is audited per field. `GET /problems/{id}/history` returns
  the audit trail and Notes are separate (FR-PRB-007).
- Capabilities: `problems.read`, `problems.create`, `problems.update`,
  `problems.configure`.

## Consequences

- Publishing a Known Error's Workaround as a Knowledge Article (FR-PRB-005, #114) comes
  with the Knowledge context.
- Changes link to Problems (FR-CHG) through the same pattern: a port to the Problem
  application API.
