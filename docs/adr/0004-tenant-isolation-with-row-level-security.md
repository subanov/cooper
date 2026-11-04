# 0004. Tenant isolation: explicit filters in repositories plus PostgreSQL Row-Level Security

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issue:** #4, #17
- **Requirements:** FR-GEN-002, FR-IAM-008, FR-IAM-009, FR-TKT-014

## Context

Cooper serves several Companies from one instance (multi-tenancy). An Analyst may access
one, several or all Companies, directly or through a Team (FR-IAM-009). A leak of one
Company's tickets to another is the worst bug Cooper can have.

In FreeITSM `tenant_id` is nullable and filtering is done by hand in each query.
"NULL means Default company" on data tables and "NULL means global" on configuration
tables, and nothing in the database stops a forgotten `WHERE`.

## Options considered

1. **Database per tenant / schema per tenant.** Strong isolation, but cross-Company
   Analysts, migrations across N schemas and connection pooling all get much harder.
2. **Shared tables with a `tenant_id` column, filtered in the application only.** This
   is simple, but a single missing filter leaks data.
3. **Shared tables with `tenant_id`, filtered in the application *and* enforced by
   PostgreSQL Row-Level Security (RLS).** Defence in depth.

## Decision

Option 3.

- Every tenant-scoped table has `tenant_id uuid NOT NULL` (instance-wide tables have no
  `tenant_id` column), an index on it, `ENABLE` and `FORCE ROW LEVEL SECURITY`, and a
  policy using the shared predicate `cooper_tenant_visible(tenant_id)`.
  - The only nullable cases are the platform's `audit_entries` and `outbox_events`, where
    `NULL` means "instance-wide record".
  - "Default company" is a real Company row, never `NULL`. This is a deviation from
    FreeITSM.
- `cooper_kernel::TenantScope` is the set of Companies a request or job may see:
  `Tenants(set)` or `All`. It is computed from the authenticated Actor (P1, FR-IAM-009).
- All tenant data access runs in a transaction opened by
  `cooper_platform::tenancy::begin(pool, &scope)`. This sets the transaction-local
  settings `cooper.tenant_scope` and `cooper.tenant_ids`, which the predicate reads.
  - With no scope set the predicate is false, so isolation **fails closed**.
  - Settings are transaction-local, so they never leak between pooled connections.
- Repositories **still filter by tenant explicitly** in every query (AGENTS.md §5). RLS
  is the second layer and catches mistakes; it is not the primary mechanism.
- The application connects as a **non-superuser role** (`cooper_app`; see
  `deploy/postgres/init/01-roles.sql`), because superusers and table owners bypass RLS.
  Migrations run as the owner role.
- `TenantScope::All` is reserved for Automation (background jobs, the outbox dispatcher)
  and Analysts with all-Company access.
- Tests use `cooper_platform::testing` (`grant_probe_role`, `begin_as_app`) to run
  queries as a non-privileged role, and assert that unfiltered queries see only rows in
  scope and that writes outside the scope are rejected.

## Consequences

- Every new tenant table needs the RLS boilerplate in its migration. Copy it from
  `20261104120000_tenant_isolation_and_audit.sql`, and add an isolation test with the
  harness.
- Cross-Company reporting for Administrators uses `TenantScope::All`, explicitly and
  visibly.
- `FORCE ROW LEVEL SECURITY` plus a non-owner app role means a misconfigured deployment
  that connects as the owner silently loses the second layer. The deployment docs and
  `.env.example` make the two roles explicit (`DATABASE_URL` for migrations,
  `COOPER_DATABASE_URL` for the application).
- RLS predicates add a small per-row cost. The P2 performance suite (1M tickets) will
  confirm that P95 stays under 300 ms.
- Confidential tickets (FR-TKT-014) are a separate, finer-grained rule enforced in the
  application layer. RLS does not cover them.
