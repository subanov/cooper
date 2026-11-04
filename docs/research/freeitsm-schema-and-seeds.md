# FreeITSM schema and seed data: research notes

Sources:

- **Version studied:** FreeITSM 4.9.0, commit `a30fa27`
  ([edmozley/freeitsm](https://github.com/edmozley/freeitsm)).
- **Issues:** #8 (seed values), #9 (migration path).
- **Abbreviations:** `SQL` = `database/freeitsm.sql`; `DBV` = `api/system/db_verify.php`.

## How FreeITSM seeds reference data

FreeITSM has two install paths, and they do **not** seed the same tables:

- **Fresh install (Docker):** runs `SQL` as the init script.
- **"Database Verification"** (`setup/index.php` → `DBV`): creates missing tables and
  seeds any empty lookup table.

Ticket Types, Origins, Problem Statuses and Problem Priorities are seeded **only** by
`DBV`.

Cooper seeds everything in one forward-only migration per context, so both paths give the
same result.

## Confirmed seed values

| Lookup | Values (flags) | FreeITSM source |
|---|---|---|
| Ticket Types | Incident (10), Service Request (20), Question (30). No default flag; Ticket Type Rules choose the default | DBV:902-905 |
| Origins | Email, Phone, Portal, Walk-up (10-40), plus WhatsApp, Web chat, Slack, Telegram when the channel is set up | DBV:916-920, 1605-1636 |
| Problem Statuses | New (default), Investigating, Root Cause Identified, Known Error, Resolved (closed), Closed (closed) | DBV:2668-2675 |
| Problem Priorities | Low, Medium (default), High, Critical | DBV:2682-2686 |
| Task Statuses | To Do (default), In Progress, Blocked, Done (closed), Cancelled (closed) | SQL:5764-5769 |
| Task Priorities | Low, Medium (default), High, Urgent | SQL:5783-5787 |
| Asset Types / Asset Statuses | **None.** Admin UI only. Demo data: Laptop, Desktop, Monitor / Active, In Storage, Retired | `demo-data/assets.json` |
| Contract Statuses, Supplier Types, Supplier Statuses, Payment Schedules | **None.** Admin UI only. Demo data exists | `demo-data/contracts.json` |
| Change Risk Levels | Hard-coded bands on likelihood × impact (each 1-5): Low ≤ 4, Medium 5-9, High 10-15, Very High 16-20, Critical 21-25 | `includes/services/changes.php:1717-1724` and duplicated in JS |
| Approval Route | `review` (default), `emergency` (approve afterwards) | SQL:4366 |
| CAB Approval Type | `all` (default), `majority`; any rejection sends the change back | SQL:4528, `changes.php:1102-1105` |

These already-documented seeds were re-checked and match FreeITSM:

- Ticket Statuses and Ticket Priorities;
- the 10 Resolution Codes;
- Change Types, the 10 Change Statuses with their stages, Change Priorities and Change
  Impacts;
- Incident Statuses and Impact Levels;
- Check Statuses and Rota Locations.

## FreeITSM bugs and accidental design Cooper does not copy

| Finding | Cooper behaviour | Recorded in |
|---|---|---|
| The `DBV` ticket-status seeder never sets `pauses_sla`, so On Hold and Awaiting Response do not pause the SLA on installs seeded that way | The seed always sets `pauses_sla` | FR-TKT-004 |
| Channel intake finds Origins by English name (`ucfirst(channel)`); renaming the row breaks channel tickets | Seeded rows have a stable `key` | FR-TKT-006 |
| The seeded Emergency change type gets `approval_route = 'review'`, so emergency changes wait for approval until an admin fixes it | The Emergency seed uses `emergency` | FR-CHG-002 |
| Risk banding is hard-coded twice (PHP and JS) | Thresholds are reference data, computed in the domain | FR-CHG-005 |
| The demo "Retired" asset status does not set `is_retired` | No seed; the flag is validated by the admin UI | — |
| Comment "1 = worst, 5 = best" on Impact Levels is stale (there are 6) | — | — |

## Facts for the migration path (ADR-0008)

**Schema size and versioning**

- 437 tables, MySQL 8 InnoDB, `utf8mb4_unicode_ci`.
- Keys are `INT AUTO_INCREMENT`.
- There is no schema version: upgrades are idempotent verify-and-patch runs, so
  importers must probe columns.

**Tenant and time handling**

- `tenant_id` is nullable. NULL means "Default company" on data tables and "global" on
  configuration tables.
- `DATETIME` columns have no time zone. The DB session is UTC, but PHP's default zone is
  Europe/London.

**Content and files**

- Email bodies are in the database.
- Attachments are files on disk, with paths relative to `tickets/attachments/`, plus the
  change, war-room and file stores.

**Credentials and secrets**

- Passwords are bcrypt `$2y$` (`password_hash`).
- Secrets are AES-256-GCM `ENC:` values keyed by a 64-hex-character key file. They
  include mailbox credentials and OAuth tokens, TOTP, SSO/LDAP and webhook secrets.

**Duplicated and denormalised data**

- `apikeys` (plaintext) vs `api_keys` (hashed).
- `tickets.assigned_analyst_id` vs `owner_id`, which disagree on real installs.
- Legacy string status and priority columns next to their lookup foreign keys.
- `morningChecks_*` CamelCase tables.
- Demo rows flagged `is_demo = 1`.
