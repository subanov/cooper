# 0043. Checklists, closure rules and procedures on tasks

- **Status:** Accepted
- **Date:** 2026-12-02
- **Issue:** #163, #165, #166
- **Requirements:** FR-TSK-007, FR-TSK-008, FR-TKT-032

## Context

Tasks and tickets need checklists built from templates. A template has a category and a
role. Its mandatory items can stop a ticket from closing. Knowledge Articles can be attached
to a task as procedures.

## Decision

- **A new bounded context, `cooper-checklists`**, owns Checklist Templates and the
  Checklists made from them. It does not depend on the ticket or task crates: it asks them
  through `TargetDirectory`, implemented by the server adapter.
- **Templates** are per Company, with a name (unique per Company), a category and a role.
  Each published version fixes the Closure Mode and the items; versions are never changed,
  and earlier ones are kept. Items have text, a mandatory flag and a requires-input flag.
- **Checklists copy a template version** when attached to a ticket or task. Later template
  changes do not reach them.
- **Closure Mode** is `advisory` or `blocking`. A blocking Checklist with an open mandatory
  item stops its ticket from closing. The ticket context asks the checklist guard in
  `change_status` and refuses with `ChecklistsOpen` (409). Advisory Checklists never block.
- **Completing an item** needs an answer when the item requires one. Reopening keeps the
  last answer for the history. Each change is audited.
- **Procedures (FR-TSK-007)** belong to the task context. Attaching copies the article's
  title and body at that moment, and records the version. Only published, non-archived
  articles of the same Company (or instance-wide ones) can be attached. Detaching removes
  the procedure, not the article.

## Consequences

- Closing a ticket reads the checklists of that ticket. Bulk close paths that bypass
  `change_status` are not covered yet and need the same guard.
- Checklist and procedure storage has row-level security like the other tenant tables.
- The ticket close guard is covered by the checklist domain tests; end-to-end tests were not
  run for this change.
