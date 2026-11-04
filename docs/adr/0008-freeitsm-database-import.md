# 0008. Offer a one-way FreeITSM importer, built after P1, as a separate CLI

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issue:** #9 (Appendix B, open question 2); implementation tracked as "FreeITSM MySQL database importer"
- **Evidence:** [docs/research/freeitsm-schema-and-seeds.md](../research/freeitsm-schema-and-seeds.md)

## Context

Organisations running FreeITSM will only move to Cooper if they can keep their history:
tickets, emails, attachments, assets, knowledge and changes. We studied FreeITSM 4.9.0
(commit `a30fa27`). The facts that shape the decision:

- **437 tables** (MySQL 8, InnoDB, `utf8mb4_unicode_ci`). Almost all have
  `INT AUTO_INCREMENT` keys.
- **No schema version number.** Upgrades are idempotent "verify and patch" runs
  (`api/system/db_verify.php`), so older installations can still carry legacy columns.
  For example, the string columns `tickets.status`/`priority` sit beside their lookup
  foreign keys. An importer must probe columns; it cannot rely on a version number.
- **Times** are `DATETIME` without a time zone. The database session runs in UTC, but
  PHP's default zone is Europe/London.
- **Email bodies** are in the database. **Attachments are files on disk** (relative
  paths under `tickets/attachments/`, plus change, war-room and file stores), so an
  import needs the database dump *and* the file tree.
- **Passwords** are bcrypt (`$2y$`) from PHP's `password_hash`.
- **Secrets** are AES-256-GCM `ENC:` values, keyed by a 64-hex-character key file. These
  include mailbox credentials and OAuth tokens, TOTP secrets, SSO/LDAP secrets and
  webhook secrets.
- **Denormalised or duplicated data:**
  - legacy `apikeys` (plaintext) vs `api_keys` (hashed);
  - `tickets.assigned_analyst_id` vs `owner_id`, which disagree on real installs;
  - `morningChecks_*` CamelCase tables that keep both a status ID and a label snapshot;
  - demo rows flagged `is_demo=1`;
  - `tenant_id` NULL meaning "Default company" on data tables.

## Options considered

1. **No migration path.** This blocks adoption by existing FreeITSM users.
2. **Live compatibility:** read FreeITSM's MySQL directly, or keep its schema. This
   contradicts AGENTS.md §1.1 (do not reproduce accidental design), and the schema keeps
   drifting.
3. **One-way, offline importer:** a separate CLI that reads a FreeITSM MySQL database,
   its attachment folders and its key file, then writes Cooper data through Cooper's
   application services. It can be re-run against a fresh Cooper database.

## Decision

Option 3: a `cooper-import-freeitsm` binary in `apps/`. Priority **Should**. It is built
**after P1** (the P4 issue), because it can only target aggregates that exist, and it
grows with each phase.

- **Scope per phase:**
  - P1 data: Companies, Analysts, Teams, Departments, Requesters, ticket reference data,
    Tickets, Emails, Notes, attachments and SLA settings.
  - Later phases add Problems, Changes, Knowledge, Assets and the rest.
- **Writes through application services** in batches, never with raw `INSERT`s into
  Cooper tables, so invariants, audit entries (Actor `Automation`, source `freeitsm-import`)
  and events stay correct. Event handlers that send email are suppressed during import.
- **IDs:** each FreeITSM `INT` key maps to a new UUID v7. The mapping is kept in an
  import-only table, `freeitsm_import_map(table, legacy_id, id)`, so re-runs are
  idempotent and links can be resolved.
- **Ticket numbers** are kept verbatim, and `[SDREF:…]` threading keeps working for
  emails already in flight.
- **Data repairs instead of copying flaws:**
  - `tenant_id` NULL → the "Default" Company.
  - Legacy string status/priority → lookup rows.
  - `owner_id` vs `assigned_analyst_id` → Assignee = `assigned_analyst_id`; Owner =
    `owner_id` only when set and different.
  - Demo rows are skipped.
  - Plaintext legacy `apikeys` are hashed (prefix + hash) or dropped, by an option.
- **Time:** `DATETIME` values are read as UTC. There is a `--php-timezone` option for
  columns known to be written with PHP `date()`.
- **Passwords:** bcrypt hashes are carried over. Cooper verifies bcrypt on the first
  sign-in and rehashes to Argon2id. This is the only place bcrypt is accepted.
- **Secrets:** decrypted with the FreeITSM key file (passed explicitly, never stored)
  and re-encrypted with Cooper's key. Where no key file is given, secrets are skipped and
  reported.
- **Output:** a report with counts per table, every skipped or repaired row and every
  warning. A `--dry-run` writes nothing.

## Consequences

- A new requirement is added: **FR-SYS-007** (Should, P4), "Import from FreeITSM".
- Cooper's seeded lookup rows carry a stable `key` besides their editable name (see
  the research notes: FreeITSM looks up Origins by English name, which breaks when an
  Administrator renames them). The importer maps FreeITSM rows by name **once**; after
  that, everything uses the key.
- Accepting bcrypt for migrated accounts is a deliberate, time-limited exception to the
  Argon2id rule. An Administrator report lists accounts still on bcrypt.
- The importer is tested against a fixture MySQL dump built from FreeITSM's own
  `database/freeitsm.sql` plus demo data, running in CI on demand (not on every PR).
