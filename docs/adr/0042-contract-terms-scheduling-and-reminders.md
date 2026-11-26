# 0042. Contract Terms, Task links to Contracts, scheduled work, the rota and Asset reminders

- **Status:** Accepted
- **Date:** 2026-11-29
- **Issue:** #185, #186 (Contracts); #162, #164, #188 (Service Desk and Calendar); #174 (Assets)
- **Requirements:** FR-CON-005, FR-CON-006, FR-TSK-004, FR-TKT-031, FR-SLA-008, FR-CAL-002, FR-AST-012

## Context

Contracts need configurable terms per Contract. Tasks should link to Contracts. Analysts
schedule work on tickets for an Owner, which should appear on the Calendar. A rota shows
who works when, and where, and who is on call. Asset owners should hear about warranty and
lease expiry before it happens.

## Decision

- **Contract Terms (FR-CON-005).** Tabs are instance-wide reference data (name unique,
  order, active). Each Contract holds a value per tab. Blank values are not kept. Only
  active tabs take values; a value cannot be set on an inactive tab, but stays where it is.
  Capabilities: `contracts.read` / `contracts.write`; tab changes need `contracts.configure`.
- **Tasks link to Contracts (FR-TSK-004, FR-CON-006).** `task_links` gains the kind
  `contract`. The target is checked through the Contracts application API, in the Task's
  Company. A Contract link appears with the Contract's number. Assets already link to
  Contracts (ADR-0034), so the Contract-side view of its linked Assets and Tasks is not
  built yet.
- **Scheduled work on tickets (FR-TKT-031).** Work has an Owner (an active Analyst), a
  span (whole days inclusive, or a time range), notes and an active flag. Cancelling keeps
  the record. Every change publishes `ticket.work_changed`. The Calendar generates events
  from it (see below). Capability: `tickets.update` to change, `tickets.read` to view.
- **Calendar for scheduled work (FR-CAL-002).** A system category "Tickets" with two
  generated all-day markers per work item: **Work starts** on the first day and **Work
  ends** on the last. Deviation: a multi-day span is shown as two markers, not one bar,
  because generated events are one day each (ADR-0036). Change windows, planned
  maintenance and task due dates are still not generated: their contexts do not publish
  dated events yet.
- **Rota (FR-SLA-008).** Shifts (a name, start and end time of day; a shift may run over
  midnight) and Rota Locations (name) are per Company reference data with no seeds. A Rota
  Entry places an active Analyst on a day, shift and location, and may be flagged on call.
  One entry per Analyst, day and shift. The rota view lists entries for a day or a range.
  Capabilities: `rota.read`, `rota.configure` (shifts and locations), `rota.manage` (entries).
- **Asset owners and reminders (FR-AST-012, #174).** An Asset may have an owner (an
  active Analyst), kept in `asset_owners` and set with `PUT /assets/{id}/owner`, so the
  Asset record itself does not change. The `asset-reminders` job (hourly) finds active
  Assets whose warranty or lease ends within 30 days and that have an owner. It records each
  reminder once per Asset, date kind and date, and appends `asset.reminder_due` in the same
  transaction. The notifier sends the owner an `asset_warranty_due` or `asset_lease_due`
  notification. Deviation: the 30-day window is fixed, not a System Setting (yet). Owners are
  not checked for being active when the reminder is sent.

## Consequences

- A Calendar source has three kinds now, and its database constraints and parsers list all
  of them (migration `20261201140000`). Adding a source means updating all of them.
- The rota's time of day is UTC. Calendar days for scheduled work are UTC days.
