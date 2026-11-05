# 0011. Identity context: Companies, Analysts, Requesters, browser sessions and RBAC

- **Status:** Accepted
- **Date:** 2026-11-05
- **Issue:** #30, #31, #35, #36, #37, #38, #78, #80
- **Requirements:** FR-GEN-001, FR-GEN-002, FR-IAM-001, FR-IAM-002, FR-IAM-006 to
  FR-IAM-011, FR-SYS-004, FR-API-001, FR-API-003, FR-API-004
- **Related:** [ADR-0004](0004-tenant-isolation-with-row-level-security.md),
  [ADR-0009](0009-shared-crates-and-dependency-baseline.md),
  [ADR-0010](0010-typescript-react-spa-for-analyst-console-and-portal.md)

## Context

Every other P1 context needs to know **who** is calling, **which Companies** they may
see and **what** they may do. FreeITSM spreads this over `analysts`, `users`,
`tenants`, `analyst_tenants`, `rbac_*` tables and PHP session variables, with
permission checks in page scripts. ADR-0010 requires a cookie session plus CSRF token for
the browser and API keys for integrations.

## Decision

### Crate and model

A new `cooper-identity` crate (`crates/identity`) owns:

| Aggregate | Table(s) | Tenant-scoped |
|---|---|---|
| Company | `tenants`, `tenant_domains` (Sender Domains) | rows filtered by RLS on `id` |
| Analyst | `analysts`, `analyst_sessions` | no (instance-wide) |
| Team | `teams`, `team_members` | no |
| Department | `departments`, `department_teams` | no |
| Role | `rbac_roles`, `rbac_role_capabilities`, `rbac_role_assignments` | no |
| Company access | `company_access_grants` | no |
| Requester | `requesters` | yes (`tenant_id`, RLS) |
| Cost Centre | `cost_centres` | yes (`tenant_id`, RLS) |

FreeITSM stores Requesters in `users`; Cooper calls the table `requesters` so the
vocabulary term is used verbatim (AGENTS.md §2).

### Principal

`cooper-kernel` gains `Principal` (the authenticated caller: `Actor`, admin flag,
`Capability` set, `TenantScope`) and `Capability` (validated `<module>.<action>`).
Both are pure. `cooper-http` gains the `Authenticated` extractor, which reads the
`Principal` placed in request extensions by the identity authentication middleware and
returns `401` when it is missing.

- An **Administrator** has every Capability and the `All` tenant scope.
- Other Analysts get the union of the Capabilities of Roles assigned to them directly
  or through their Teams, and the union of Company access granted directly or through
  their Teams. A grant with no Company means **all Companies** (FR-IAM-009).
- Each context declares its Capabilities as constants. `cooper-server` assembles the
  **Capability catalogue**; identity rejects Roles that name unknown Capabilities.
- Reading reference people data (Analysts, Teams, Departments, Companies in scope) only
  requires being signed in, because every Analyst needs it to assign work. Changing it
  needs `<module>.manage`.

### Browser sessions (FR-IAM-002)

- Passwords are hashed with **Argon2id** (`argon2` crate defaults) on a blocking thread.
- Sign-in creates an `analyst_sessions` row. The browser gets a random 256-bit token in
  the `cooper_session` cookie (`HttpOnly`, `Secure` unless disabled for local HTTP,
  `SameSite=Lax`, `Path=/`). Only its SHA-256 hash is stored.
- Sessions expire after an **idle timeout** (default 8 hours) and an **absolute
  lifetime** (default 7 days), both configurable. Deactivating an Analyst or changing
  their password revokes their sessions.
- Each session has a random **CSRF token**, returned by sign-in and `GET /api/v1/session`.
  Unsafe methods (`POST`, `PUT`, `PATCH`, `DELETE`) authenticated by cookie must send it
  in `X-CSRF-Token`. Bearer API-key requests (#79) do not use cookies and are exempt.
- Wrong username, wrong password and inactive Analyst all return the same `401` so the
  response does not reveal which accounts exist. A dummy hash is verified for unknown
  usernames to keep timing similar.

### Persistence pattern

Application services depend on repository traits defined in `domain`. A repository
write takes the aggregate **and** its `AuditRecord`s (and events, when any) and stores
them in **one transaction**, so application tests use in-memory fakes while the
"audit in the same transaction" rule (FR-GEN-001) holds in production.

### First-run setup (FR-SYS-004)

`GET /api/v1/setup` reports whether setup is needed; `POST /api/v1/setup` creates the
default Company and the first Administrator, and only succeeds while no Analyst exists
(checked under an advisory lock, so two concurrent requests cannot both succeed).
Instance-wide seed data comes from migrations.

### OpenAPI (FR-API-003)

Handlers are annotated with `utoipa` and registered through `utoipa-axum`'s
`OpenApiRouter`, so the document cannot drift from the routes. It is served at
`/api/v1/openapi.json` and is the input for the generated web client (ADR-0010).

## Consequences

- New dependencies: `argon2` (password hashing), `sha2` (token hashes), `getrandom`
  (token generation), `utoipa` and `utoipa-axum` (OpenAPI).
- Every later context protects its endpoints with `Authenticated` and
  `principal.require(capability)`, and opens transactions with `principal.scope()`.
- TOTP (#32), IP bans (#34), password reset (#33), API keys (#79) and Portal sign-in (#72)
  extend this model without changing it.

### Deviations from FreeITSM

- Session tokens are random, stored hashed, and bound to a CSRF token; FreeITSM uses PHP
  sessions without CSRF protection on most forms.
- Administrators always see all Companies; FreeITSM relies on `analyst_tenants` rows
  even for admins, which can hide data from the people who configure the system.
- Sender Domains are unique across Companies and can never be a Freemail Domain.
