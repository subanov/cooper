# 0030. Service Status: shared or per-Company Services, a computed Current Status, and a public page read with no Company

- **Status:** Accepted
- **Date:** 2026-11-09
- **Issue:** #139, #140, #141, #142, #143, #109
- **Requirements:** FR-SST-001 to FR-SST-007, FR-SST-009, FR-PRT-006

## Context

In FreeITSM, Service Status is a board of Services. Analysts open Service Incidents
that affect Services with an Impact Level, and post updates to them. The Portal shows
each Service with a colour. Since discussion #99, the Portal can also show Incidents
with their *external* updates. That is off by default, because Incident titles were
written for Analysts. Planned Maintenance (3.2.0) waits as "Upcoming". At its planned
start it becomes an ordinary Incident, backdated to that start. At its planned end that
Incident is resolved, unless somebody resolved it already.

FreeITSM is single-tenant. Cooper serves several Companies from one instance, and
FR-SST-009 asks for a page that anyone can read without signing in. Without a clear rule
about Companies, that page could leak one Company's outages to the world, or to another
Company.

## Decision

- A new crate `cooper-service-status` (bounded context, AGENTS.md §3), behind the
  **Service Status** Module. Every route lives under `/service-status`, plus
  `/portal/service-status` and `/public/service-status`. Capabilities:
  `service-status.read`, `service-status.update` (Incidents, updates, Planned
  Maintenance) and `service-status.configure` (Services, Impact Levels, Incident
  Statuses).
- **Reference data.** Impact Levels (unique severity order, *counts as downtime*, one
  default, which is the all-clear) and Incident Statuses (`is_resolved`, one default,
  which is open) are instance-wide lookups, seeded as in FreeITSM. Behaviour comes only
  from these flags and orders. No status or level name appears in code.
- **Companies.** A Service belongs to one Company or is **shared** (`tenant_id` NULL),
  as Knowledge Articles are (ADR-0028). Only callers who see every Company manage shared
  records. A Service Incident and a Planned Maintenance take the Company of their
  Services. All their Services must be of one Company, or all shared, so an Incident can
  never put one Company's Service in front of another Company. RLS on every table admits
  `tenant_id IS NULL OR cooper_tenant_visible(tenant_id)`.
- **Service Incident aggregate.** It has a title, an Incident Status, the Impact Level on
  each affected Service, `started_at` and `resolved_at`. Opening it records the first
  Incident Update. Each later update snapshots the status and every impact. An update is
  only appended when it says something new: a status change, an impact change or text.
  Entering a resolved status stamps `resolved_at`; leaving it clears it. A posted update's
  wording and visibility can be corrected in place. Its snapshot cannot, because it
  records what was true at that moment. Updates are internal unless marked public.
  Events: `status_incident.opened`, `.updated`, `.resolved`.
- **Current Status** (FR-SST-006) is a pure domain function. It returns the worst
  (lowest severity order) Impact Level among a Service's unresolved Incidents, or the
  default Impact Level. It is computed on every read and never stored, so it cannot go
  stale. The Analyst board, the Portal and the public page all use the same function.
- **Planned Maintenance aggregate.** It has a title, a public description, a planned
  start and end, and affected Services with Impact Levels. State (fixed): `scheduled →
  started → finished`, or `scheduled → cancelled`. Only scheduled maintenance can be
  edited or cancelled.
  - **Starting** opens its Service Incident in the same transaction. The state change
    is a compare-and-set on `state = 'scheduled'`, so the job and an Analyst can never
    start it twice. The Incident is backdated to the planned start (never forward: an
    early start by hand starts now). It gets the first active, open, non-default
    Incident Status in order ("Identified" in the seed; the default would say "nobody
    knows what is happening"). Its first update is public and carries the description,
    because the maintenance was announced in advance. The Incident is made as the
    Analyst who scheduled the maintenance.
  - **Finishing** resolves that Incident with the first active resolved status, backdated
    to the planned end, unless somebody already resolved it. Their resolution stands.
  - The `planned-maintenance` Background Job (every minute) starts what is due and
    finishes what has ended. Analysts can also start or finish by hand.
  - Creating the Incident while changing the Planned Maintenance breaks the "one
    aggregate per transaction" rule on purpose. A started maintenance without its
    Incident (or two Incidents for one maintenance) would be the inconsistent state
    FreeITSM guards against with its claim-then-create step. A unique
    `planned_maintenance_id` on the Incident enforces the pairing.
- **Portal (FR-PRT-006).** `GET /portal/service-status`, for Portal sessions only. It
  shows active shared Services and those of the Company the session acts for, each with
  its Current Status, plus scheduled and started Planned Maintenance. When
  `service_status.show_incidents` is on, it also shows Incidents that are unresolved or
  were resolved within `service_status.recent_days` and that have at least one public
  update. It shows only their public updates, never authors or internal updates.
- **Public Status Page (FR-SST-009).** `GET /public/service-status` needs no sign-in and
  answers 404 unless `service_status.public_page` is on (default off). **Tenancy
  decision:** it shows only shared records, and it reads them in a transaction whose
  Tenant Scope is *empty*. Row-Level Security then admits only rows with no Company. A
  bug in a query filter therefore still cannot reveal a Company's Service, Incident or
  maintenance: the database refuses them. The response uses its own DTOs. These carry
  only names, Impact Levels, public update text, times and maintenance windows, never
  IDs of Analysts or Companies. A per-Company public page (addressed by a slug) is left
  for later. It would need an explicit opt-in per Company.
- **Audit.** Every change goes to the platform audit trail (`status_service`,
  `status_incident`, `planned_maintenance`, `service_status_lookup`), with the Company
  of the record, or none for shared records and reference data.

## Deviations from FreeITSM

- **Services are deactivated, Incidents are never deleted.** FreeITSM deletes both, and
  that loses the availability record (FR-SST-008) and the audit trail.
- **Unique severity orders.** Otherwise "the worst Impact Level" can be ambiguous. The
  default level is the all-clear and never counts as downtime.
- **An Incident does not mix Companies** (FreeITSM has no Companies).
- **The planned end is required.** FreeITSM allows an open-ended maintenance, but FR-SST-007
  asks for both. Finishing early by hand is still possible.
- **The started status is chosen by order and flags**, not as "the first status that is
  not the starting one" by ID.
- **Current Status is computed once, on the server.** FreeITSM works it out again in each
  page's JavaScript.
- **One switch for showing Incidents** to Requesters and the public, with
  `service_status.recent_days` for resolved ones. FreeITSM's `all` mode (every Incident
  ever) is not included.

## Consequences

- Availability reporting (FR-SST-008, P4) can work from `status_incidents` (start,
  resolution) and the update snapshots, together with *counts as downtime*.
- A shared Service's status can only be changed by callers who see every Company. A
  Company that needs its own view of a shared platform gets a Company Service.
- The worker must run the `planned-maintenance` job for maintenance to start on time.
  Without the job, Analysts start it by hand.
