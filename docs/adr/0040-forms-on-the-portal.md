# 0040. Forms on the Portal: audiences, approval rounds, Submission Actions and Drafts

- **Status:** Accepted
- **Date:** 2026-11-29
- **Issue:** #189 to #193 (Forms group); #190 needs Requesters in Departments
- **Requirements:** FR-FRM-002 to FR-FRM-005, FR-PRT-002, FR-IAM-011

## Context

A Form (ADR-0039) is only useful once a Requester can fill it in: it is offered on the
Self-Service Portal to some people, its answers are checked, the Requester can save a
Draft, and the Submission can open a ticket or create a task. Some Forms need an approval
first.

## Decision

- **Portal visibility and audiences (FR-FRM-002).** A Form is on the Portal when it is
  active and marked visible. Its audiences are Requesters, User Groups or Departments. With
  no audience, every active Requester of the Company is offered the Form. Requesters have
  a Department (new `requesters.department_id`, set through the Requester API, same
  Company's Department table: identity change in migration `20261129110000`). Visibility
  is decided in the domain (`Form::offered_to`); a Requester outside the audience gets 404,
  not 403, so the Form's existence is not revealed.
- **Approval rounds (FR-FRM-004).** An approval rule names Analysts or one Team, and sets
  required approvals and rejections (1 to 50 each). A Submission is decided in a round: its
  votes count only within that round. Enough approvals approve it; enough rejections return
  it to the Requester, who amends it and resubmits, which starts round n + 1. Votes are never
  changed. Analysts with the Requester's e-mail address are excluded when the rule says so
  (`exclude_requester`). Analysts need `forms.approve`.
- **Submission Actions (FR-FRM-003).** A Form can open a ticket (through the Portal
  Catalogue path, as the Requester) and create a task (as an automation principal limited
  to `tasks.create` in the Company). Subject and description are templates with
  `{field_key}` placeholders, checked against the current version when the settings are
  saved. A Submission without approval runs its actions at once; with approval, the actions
  run when the final approval is cast. Actions run once: a failure keeps the Submission
  approved (not completed) and the vote or submission is kept; `POST
  /form-submissions/{id}/actions` retries. The actions span contexts and are not atomic:
  if the task fails after the ticket was opened, the ticket remains and the retry opens
  another one. This is a known limitation, recorded here rather than hidden.
- **Drafts (FR-FRM-005).** One Draft per Requester and Form, validated against the fields
  without requiring them. Submitting removes the Draft.
- **Settings are current configuration, not versioned.** Only the fields are versioned
  (ADR-0039). A Submission records the version its answers were given against.
- **Contexts.** Forms reads people and teams through its own `Directory` port and writes
  tickets and tasks through `SubmissionTargets`; both are implemented in
  `apps/cooper-server/src/forms_adapters.rs`, so Forms never reads another context's tables.
- **Capabilities.** `forms.read`, `forms.write`, `forms.approve`. Requesters use the
  `/portal/forms` and `/portal/submissions` routes with a Portal session.
- **Privacy.** Requesters never see votes, approver identities or the Form settings; they see
  the status and the comments of the current round's rejections.

## Consequences

- Staff see Submissions per Form, newest first, with all votes.
- The ticket path is exercised only by compilation in the test suite; the task path has an
  end-to-end test. A ticket test needs a Portal Catalogue Category and is still to add.
