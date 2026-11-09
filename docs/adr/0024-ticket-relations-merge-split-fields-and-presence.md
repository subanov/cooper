# 0024. Ticket relations: Links, Merge and Split with undo, Number History, Type Rules, Ticket Fields and Presence

- **Status:** Accepted
- **Date:** 2026-11-07
- **Issue:** #97, #98, #99, #100, #101, #102, #103, #104, #106
- **Requirements:** FR-TKT-003, FR-TKT-008, FR-TKT-011, FR-TKT-012, FR-TKT-025, FR-TKT-026,
  FR-TKT-027, FR-TKT-028, FR-TKT-029, FR-TKT-041, FR-TKT-042
- **Related:** [ADR-0014](0014-service-desk-core.md),
  [ADR-0022](0022-live-updates-over-server-sent-events.md)

## Context

The P2 ticket features all relate tickets to each other or add per-ticket data that the
core aggregate (ADR-0014) does not hold. FreeITSM 4.10 implements them (studied at commit
`61163c1`: `includes/ticket_merge.php`, `ticket_split.php`, `ticket_links.php`,
`ticket_presence.php`, `ticket_snooze.php`, `ticket_type_rules.php`, and the
`ticket_fields*` tables). Its design notes are good and Cooper keeps their intent:

- a merged-away ticket must never disappear, because Requesters hold its SDREF;
- merges and splits must record exactly which rows moved, or undo is guesswork;
- presence is a heartbeat, not a session or a lock;
- a type rule must never stop a ticket from being created.

AGENTS.md allows one aggregate per transaction. Merge and split change two tickets and
move child rows between them.

## Decision

### Merge and Split are atomic across two tickets

Moving the conversation and closing the source are one business fact. Doing them in
separate transactions would show half-moved threads and need compensation for no gain.
The repository therefore performs a merge or split **in one transaction**. It locks both
tickets, moves rows by ID, saves the changed ticket(s), stores the link and writes a
`ticket_merges` / `ticket_splits` record with the moved IDs. This is the one documented
exception to "one aggregate per transaction". The aggregates still decide everything:
`Ticket::merge_into` and `Ticket::unmerge` hold the rules, and the repository only
persists them.

- **What moves:** emails, Notes, Time Entries and attachments (rows that describe the
  conversation). **What stays:** audit history, SLA events, Ticket Field values and links
  (they describe what happened to *that* ticket), as in FreeITSM.
- **Merge:** the source is closed with the first closed status and the `duplicate`
  Resolution Code (if it is not closed already). It keeps its number, gets
  `merged_into_id`, and is linked `duplicate_of` the target. Email threading follows
  `merged_into_id`, so replies to the old SDREF reach the survivor. The target must be
  open, in the same Company, and not merged away itself. **Undo** moves back the recorded
  rows still on the target and restores the source's previous status and closure exactly.
- **Split:** the Analyst picks emails and/or Notes. A new ticket copies the source's
  details, Queue and Assignee and gets a fresh number and SLA clock. A marker Note on the
  source says where the rows went, and the tickets are linked `related`. **Undo** moves
  the rows back, removes the marker Note and the link, and soft-deletes the new ticket.
  It is refused once the new ticket has activity of its own; merge it back instead.
- Capability `tickets.merge` covers merge, split and both undos.
- Deviation from FreeITSM: only the default merge policy (`survivor` + `thread`) exists.
  "Mint a new ticket" and HTML snapshots add options without a clear need.

### Ticket Links

`ticket_links` stores three kinds: `related` (symmetric, one row per pair), `duplicate`
(source duplicates target) and `parent` (source is parent of target). The API speaks
from the requested ticket's side (`duplicate_of`, `duplicated_by`, `parent_of`,
`child_of`). Partial unique indexes enforce one parent and one master per ticket. The
application refuses parent/child cycles using the ancestor chain. Links never cross
Companies. A listing omits links whose other ticket the caller cannot see, so a
Confidential ticket's existence is not leaked through a link.

### Ticket Number History

Renumbering (`tickets.configure`) takes the next number of the current format and moves
the old one to `ticket_number_history`, which is unique across all retired numbers.
Lookup by number, search and SDREF threading fall back to the history.

### Ticket Type Rules and Ticket Fields are instance-wide reference data

Ticket Types, Statuses and Categories are instance-wide in Cooper, so their rules and
custom fields are too.

- **Type Rules:** a Company is one of the conditions (`company_ids`), alongside channel
  (`email`, `portal`, `analyst`), Mailbox, Origin, Department, subject keywords and
  Requester domain. They run in `sort_order` when nobody chose a type. Failures are
  logged and mean "no type".
- **Ticket Fields:** they are typed (`text`, `long_text`, `number`, `date`, `dropdown`,
  `checkbox`), and the key and type never change. They can be limited to Ticket Types and
  are retired, never deleted. Values live in `ticket_field_values` (JSON validated by the
  field), written in the same transaction as ticket creation.
- **Requirements:** *required on create* applies to Analyst and API creation. On the
  Portal only fields marked *ask requester* are required, and only those may be set by a
  Requester. Email intake cannot ask, so it skips the check. *Required on close* blocks
  closing (FR-TKT-008) with a `fields-required` problem listing the keys.
- Deviation from FreeITSM: no per-Company fields or rules. Revisit if Companies need
  different forms.

### Ticket Presence

`PUT /tickets/{id}/presence` (every 10 s from the open ticket) upserts a heartbeat;
entries older than 30 s are ignored and a job prunes old rows. The server sends a Live
Update with topic `ticket_presence` only when what others see changes (someone arrives,
leaves or starts or stops writing). The SSE stream authorises it exactly like the ticket.
Presence is not audited: it is not a record, and "who looked at this ticket" would raise
privacy questions this feature does not answer.

### Snooze and bulk actions

Snooze also records who and when, and refuses dates more than 730 days ahead (typo
guard). Bulk actions accept up to 100 tickets. Each ticket runs through the normal use
case in its own transaction, and the response reports each outcome with the status code
the single change would have returned.

## Consequences

- Merge and split are the only cross-aggregate transactions. They lock two ticket rows
  briefly, which is negligible at ITSM volumes.
- Rows moved by a merge and later split or merged elsewhere stay where they are on undo
  (undo moves only rows still on the target). This is the safe choice, and the undo
  result shows what moved.
- Requirements on create and on close make Ticket Fields part of the creation and
  closing contracts. API clients can read `GET /ticket-fields` to know what to send.
- Linking tickets to Problems, Changes, Assets, CIs and Tasks (FR-TKT-030, #105) arrives
  with those contexts.
