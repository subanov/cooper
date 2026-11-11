# 0034. Assets: retirement on the Asset, one open Check Out, exact money, and Suppliers by ID

- **Status:** Accepted
- **Date:** 2026-11-11
- **Issue:** #167, #169, #170, #171
- **Requirements:** FR-AST-001, FR-AST-002, FR-AST-004, FR-AST-005, FR-AST-006

## Context

FreeITSM keeps an `assets` table with hostname, Service Tag, type and status lookups,
locations, a supplier and cost centre, purchase, warranty and lease dates, a history of
who had the asset and a status that can mean "disposed". Cooper is multi-Company, uses
DDD aggregates and keeps one audit trail for every aggregate (ADR-0004, ADR-0005).

Three things needed a decision:

1. How "retired" is represented. FreeITSM only has a status, so an asset can be
   "disposed" without a date or reason, and leave or re-enter the estate by editing a
   drop-down.
2. Suppliers belong to the Contracts context, which is built in parallel.
3. Purchase cost is money; Cooper stores money as `NUMERIC(14,2)` plus a currency, and
   `sqlx` is built without a decimal type.

## Decision

- A new crate `cooper-assets` behind the **Assets** Module (`/assets`, `/asset-types`,
  `/asset-statuses`, `/asset-locations`). Capabilities `assets.read`, `assets.create`,
  `assets.update` (edit, Check Out / Check In, Retire, link) and `assets.configure`
  (reference data). Requesters never reach it.
- **Reference data.** Asset Types, Asset Statuses (`is_retired`, optional colour) and
  Locations are instance-wide, like the other contexts' lookups, with no seed values.
  Rows are deactivated, never deleted; an Asset keeps a value that became inactive, but
  inactive values cannot be newly chosen. **Locations** form a tree (`parent_id`): no
  loops, at most six levels, sibling names unique. A Location filter includes the
  Locations below it.
- **Asset aggregate.** Hostname, Service Tag and Asset Tag are optional but at least one
  is required; the Asset Tag is unique per Company, case-insensitively (a partial unique
  index). The lease cannot end before it starts. The Cost Centre must belong to the
  Asset's Company (checked through the identity API, plus a foreign key).
- **Retirement is recorded on the Asset** (`retired_on`, `retirement_reason`, who and
  when), not inferred from the status. `Asset::retire` needs a reason, refuses an Asset
  that is checked out or already retired, and optionally moves it to an active
  `is_retired` Asset Status; without one, the in-estate status is cleared. A retired
  status can only be reached through Retire, and a retired Asset keeps a retired status.
  `retired_on IS NULL` defines **active**: lists leave retired Assets out unless
  `include_retired` (or `retired_only`) is asked for, and `GET /assets/summary` counts
  active Assets per type and status with retired ones apart. Retirement is final in
  this release.
- **Check Out / Check In.** `asset_checkouts` is the log; the open row (no
  `checked_in_at`) is the current Holder, a Requester of the Asset's Company or an
  active Analyst. A partial unique index allows one open row per Asset, and Check Out
  and Retire lock the Asset row (`SELECT … FOR UPDATE`) so the two cannot race. Check
  In closes the row only if it is still open.
- **Money.** `Amount` keeps hundredths in an `i64` and parses decimal strings with at
  most two places; the API exchanges `"1299.00"` strings, never floats. SQL converts
  with `$n::bigint / 100.0` and `(purchase_cost * 100)::bigint`, so no decimal crate is
  needed. The currency is an ISO-4217 code (three upper-case letters); cost and currency
  are both present or both absent.
- **Suppliers and Contracts by ID.** `supplier_id` has no foreign key: the Contracts
  context owns Suppliers. The application checks a Supplier through the `Directory`
  port (`supplier_exists`), and Contract links through `link_target`; the adapter in
  `apps/cooper-server/src/adapters.rs` asks the Contracts application API (ADR-0035):
  the Supplier must belong to the Asset's Company, and a linked Contract must belong to
  it too and be readable by the caller (`contracts.read`), and is labelled with its
  Contract Number. Ticket links go through the service desk API: the ticket must be
  visible to the caller and belong to the Asset's Company.
- **Links** (`asset_links`) are `ticket` or `contract`. Links to CIs arrive with the
  CMDB (FR-CMD-005).
- **History.** Every mutation writes the Asset's audit entries in the same transaction:
  field changes, `holder` for Check Out / Check In, `retired_on` and
  `retirement_reason`, `ticket_linked` and the like. `GET /assets/{id}/history` returns
  them; `GET /assets/{id}/checkouts` returns the Check Out log.
- **Domain Events** through the outbox: `asset.checked_out`, `asset.checked_in`,
  `asset.retired`, and `asset.dates_changed` whenever the warranty or lease expiry is
  set, changed or cleared (on create or edit) or the Asset is retired. Its payload is
  `{asset_id, asset_tag, hostname, warranty_expires_on, lease_expires_on, active}`; the
  Company travels in the envelope. The Calendar (FR-AST-012) consumes it later; nothing
  consumes these events yet.

## Consequences

- Retired Assets cannot drift back into the estate by a status edit, and every
  retirement has a date and a reason.
- One Check Out per Asset holds even under concurrent requests.
- The Contracts integration is two adapter methods, so Assets never read Contracts
  tables.
- Custom Asset Fields, imports, inventory, software, licences and labels (FR-AST-003,
  -007 to -015) are not part of this decision.
