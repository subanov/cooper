# 0039. Tasks follow their linked tickets through ticket outbox events

- **Status:** Accepted
- **Date:** 2026-11-24
- **Issue:** —
- **Requirements:** FR-TSK-010, FR-TSK-004

## Context

Analysts link Tasks to tickets (FR-TSK-004), often to track the work an email asked for.
The Task then has to be moved by hand: to In Progress when the ticket is answered, to
Done when it is resolved. Service desks want this done for them. FreeITSM has no such
automation.

Statuses are reference data that administrators can rename or replace (FR-TSK-002), and
Tasks may not read the service desk's tables (AGENTS.md §3).

## Options considered

1. **Service desk calls the Tasks API when a ticket changes.** Couples the service desk to
   Tasks and puts the move in the ticket's transaction.
2. **Tasks react to the ticket events already in the outbox.** The service desk stays
   unaware of Tasks; a failed move is retried by the dispatcher.

## Decision

- Option 2. The `task-linked-tickets` handler in `cooper-tasks` (registered in
  `AppState::event_handlers`) reads `ticket.reply_sent`, `ticket.note_added` (only
  with an `author_id`, an Analyst), `ticket.status_changed` (between open statuses) as
  **work**, and `ticket.closed` as **resolution**. Automatic Template emails and
  `ticket.reopened` are ignored. The ticket is the event's aggregate; no payload type
  from the service desk crate is used.
- `TaskService::follow_ticket` runs as Automation and goes through `move_to`, so audit,
  Live Updates and repeat-on-completion work as for a manual move.
- **Work** moves an open Task that is still in the default status to the active, open
  status keyed `in_progress`. Anything an Analyst chose (Blocked, In Progress) stays.
- **Resolution** moves an open Task to the active *done* status keyed `done` when every
  ticket linked to it is closed. Ticket state comes from the new
  `Directory::ticket_is_closed` port (the service desk application API); tickets that are
  gone count as closed.
- Target statuses are found by their stable **keys**, never by name. If an administrator
  deactivated the keyed status or took away its flags, nothing moves and a warning
  reaches the System Log.
- System Setting `tasks.follow_linked_tickets` (Bool, default on). The settings adapter
  also reports it off while the Tasks Module is off.
- Reopening a ticket does not reopen its Tasks.

## Consequences

- Moves happen a dispatcher cycle after the ticket change, not in the same request.
- Redelivered events are harmless: a moved Task no longer qualifies.
- At most 200 open Tasks linked to one ticket are moved per event (the list limit).
- Choosing other target statuses would need a lookup-valued setting; keys cover the seed
  data and custom statuses can take over a key only through the database today.
