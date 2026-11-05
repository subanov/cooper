# AGENTS.md — Cooper

Guidance for AI coding agents (and humans) working in this repository. Read this file first,
then [docs/VOCABULARY.md](docs/VOCABULARY.md) and
[docs/FUNCTIONAL_REQUIREMENTS.md](docs/FUNCTIONAL_REQUIREMENTS.md).

## 1. What Cooper is

Cooper is an IT Service Management (ITSM) platform written in Rust. It reimplements
[FreeITSM](https://github.com/edmozley/freeitsm) (PHP/MySQL) and keeps its domain model and
domain language: tickets, analysts, requesters, changes, problems, assets, knowledge, morning
checks, service status, and so on. When this file says "FreeITSM parity", it means: the same
concepts, names and **user-facing intent**. It does not mean the same code, schema or quirks.
See §1.1.

### 1.1 FreeITSM is a reference for intent, not a codebase to port

- **Do not translate FreeITSM code** (PHP, SQL, JavaScript) line by line into Rust. Read it to
  understand *what problem a feature solves and why*. Then design that behaviour the Cooper
  way: DDD aggregates, typed values, PostgreSQL, idiomatic async Rust.
- **Look for intent** in this order: the FreeITSM UI and behaviour, the wiki and README, table
  and column comments, and only then the implementation. Write the intent down as a
  requirement in `docs/FUNCTIONAL_REQUIREMENTS.md` before you build it.
- **Do not reproduce bugs.** If FreeITSM behaves in a way that is clearly wrong
  (data loss, race conditions, missing validation, security holes, wrong SLA arithmetic,
  inconsistent state), implement the correct behaviour in Cooper.
- **Do not reproduce accidental design.** Examples: MySQL-specific workarounds, denormalised or
  legacy columns, duplicated tables (`apikeys` vs `api_keys`), mixed naming styles
  (`morningChecks_*`), stringly-typed enums, logic spread across UI scripts. Model these
  cleanly instead.
- **Keep what users rely on:** domain terms, workflows, seed reference data, and visible
  outcomes such as ticket numbers, SDREF threading and status semantics.
- **Record every deliberate deviation.** When Cooper fixes or intentionally changes FreeITSM
  behaviour, note it in the requirement (a "Deviation from FreeITSM:" line). For significant
  deviations, also write an ADR. When the intent is unclear, ask. Do not guess.

| Concern            | Choice                                                          |
|--------------------|-----------------------------------------------------------------|
| Language           | Rust (stable, edition 2024)                                     |
| Async runtime      | `tokio`                                                         |
| Web framework      | `axum` (with `tower` / `tower-http` middleware)                 |
| Web UI             | TypeScript + React 19 SPA in `web/`, served by `cooper-server` (ADR-0010) |
| Database           | PostgreSQL (18)                                                 |
| DB access          | `sqlx` (compile-time checked queries, migrations)               |
| Serialization      | `serde`                                                         |
| Errors             | `thiserror` in libraries, `anyhow` only in binaries/tests       |
| Observability      | `tracing`, `tracing-subscriber`, OpenTelemetry export optional  |
| Config             | environment variables (12-factor), parsed once at start-up      |
| Time               | `chrono` + `chrono-tz` (ADR-0002), always UTC in DB             |
| IDs                | UUID v7 wrapped in typed newtypes                               |

## 2. Ubiquitous language — non-negotiable

- Use the terms in [docs/VOCABULARY.md](docs/VOCABULARY.md) **verbatim** in type names, module
  names, API paths, DB tables, log messages and UI copy.
- Say `Analyst`, not `Agent`/`Operator`/`Technician`. Say `Requester`, not `Customer`/`Caller`.
  Say `Company` in UI and docs, and `Tenant` in code (`TenantId`). The vocabulary lists every
  forbidden synonym.
- If a concept is missing from the vocabulary, **add it there first** (same PR), then use it.
- Requirement IDs (`FR-TKT-012`) from the functional requirements go in commit messages,
  test names (where useful) and PR descriptions.

## 3. Architecture

Cooper is a **modular monolith** organised by **bounded context** (DDD). Each context is a
crate with strict layering. Contexts talk to each other only through their public
application API or through domain events, never by reading each other's tables.

### 3.1 Workspace layout (target)

```
cooper/
├── Cargo.toml                    # [workspace], shared deps in [workspace.dependencies]
├── AGENTS.md
├── docs/
│   ├── VOCABULARY.md
│   ├── FUNCTIONAL_REQUIREMENTS.md
│   └── adr/                      # Architecture Decision Records (NNNN-title.md)
├── migrations/                   # sqlx migrations, one sequence for the whole DB
├── crates/
│   ├── kernel/                   # shared kernel: ids, TenantId, Actor, Clock, DomainEvent,
│   │                             # pagination, audit records. NO business rules, NO I/O.
│   ├── platform/                 # shared infrastructure: config, pool + migrations, tenant-scoped
│   │                             # transactions, audit log, outbox, job runner (ADR-0009)
│   ├── http/                     # shared HTTP: problem+json, middleware stack (ADR-0009)
│   ├── identity/                 # Analysts, Requesters, Teams, Departments, Companies, RBAC, auth
│   ├── service-desk/             # Tickets, Mailboxes, Channels, SLA, Rota, Reply Templates
│   ├── problem-management/
│   ├── change-management/
│   ├── tasks/
│   ├── assets/                   # Assets, Inventory, Software, Licences
│   ├── cmdb/
│   ├── knowledge/
│   ├── service-status/
│   ├── morning-checks/
│   ├── contracts/                # Contracts, Suppliers, Contacts
│   ├── forms/
│   ├── calendar/
│   ├── workflow/                 # Workflows, Webhooks
│   ├── reporting/
│   └── notifications/            # outbound email, in-app notifications
└── apps/
    ├── cooper-server/            # axum HTTP server: wires contexts, routing, middleware
    └── cooper-worker/            # background jobs: mailbox polling, SLA clock, schedules
web/                              # Analyst console + Portal SPA (TypeScript, React; ADR-0010)
```

Create crates only when work on that context starts. Do not scaffold empty crates.

### 3.2 Layers inside a context crate

```
crates/service-desk/src/
├── lib.rs
├── domain/            # Aggregates, entities, value objects, domain events, repository TRAITS.
│                      # Pure Rust: no sqlx, no axum, no tokio, no I/O. Fully unit-testable.
├── application/       # Use cases (commands/queries), transaction boundaries, authorization
│                      # checks, event publishing. Depends on domain traits only.
├── infrastructure/    # sqlx repository impls, external clients (Graph, IMAP, SMTP), mappers.
└── api/               # axum Router + handlers + request/response DTOs for this context.
```

Dependency rule: `api → application → domain ← infrastructure`. The `domain` module must
never `use` anything from `infrastructure`, `api`, `sqlx` or `axum`.

### 3.3 DDD rules

- **Aggregates** protect their own invariants. Change state only through methods on the
  aggregate root (`ticket.assign(analyst, clock)`), never by setting public fields.
- An aggregate's fields are private. Expose read access through getters.
- **One aggregate per transaction.** To coordinate across aggregates, use domain events
  (handled in the same process; written to an outbox table when the change must survive
  a crash).
- Refer to other aggregates **by ID** (`AnalystId`), never by holding the object.
- **Value objects** are immutable and validate on construction: `EmailAddress`,
  `TicketNumber`, `Minutes`, `RiskScore`. Use the "parse, don't validate" approach:
  `TryFrom<String>` returns a typed value or an error.
- **Typed IDs**: `pub struct TicketId(Uuid);` One newtype per aggregate, generated by a
  shared macro in `kernel`.
- Domain methods that need the current time take a `&dyn Clock` (or `DateTime<Utc>`)
  argument. Never call `now()` inside the domain.
- Configurable lookups (statuses, priorities, ticket types, origins, and so on) are
  **reference data** owned by the context. Behaviour flags (`is_closed`, `pauses_sla`) live
  on the lookup row. Never hard-code a status name in logic.
- Domain events are past-tense facts: `TicketCreated`, `TicketAssigned`, `ChangeApproved`,
  `SlaBreached`.

## 4. Coding conventions

- `cargo fmt` is required. `cargo clippy --all-targets --all-features -- -D warnings` must pass.
- Workspace lints in the root `Cargo.toml`: `unsafe_code = "forbid"`, `clippy::unwrap_used`,
  `clippy::expect_used` (deny outside tests), `missing_docs` (warn for public items).
- **No `unwrap()`/`expect()`/`panic!`** in non-test code, except for start-up invariants that
  have a written justification.
- Errors: each layer has its own `thiserror` enum. Domain errors describe business rule
  violations (`TicketError::AlreadyClosed`). The API layer maps them to HTTP status codes in a
  single `IntoResponse` implementation per context. Never leak SQL errors to clients.
- Async: never block the runtime. Use `tokio::task::spawn_blocking` for CPU-heavy or blocking
  work (for example, attachment text extraction).
- No global mutable state. Share `AppState` (pool, config, clients) through `axum::extract::State`.
- Prefer `&str`/borrowing in signatures. Clone deliberately.
- Make domain logic synchronous and pure where possible. Make I/O async.
- Feature flags only for optional integrations (`graph`, `imap`, `ai`).
- Public items get rustdoc. A module-level `//!` doc explains the purpose of each context.
- Keep functions small. When a handler grows past roughly 30 lines, move logic into the
  application layer.

## 5. Database (PostgreSQL + sqlx)

- Migrations live in `/migrations`, are named `YYYYMMDDHHMMSS_description.sql`, and are
  **forward-only**. Never edit a migration that has been merged.
- Table names: `snake_case`, plural, prefixed by context where ambiguous
  (`change_statuses`, `ticket_statuses`). Reuse FreeITSM table names when they match the
  vocabulary and these conventions. Otherwise, normalise them (§1.1).
- Primary keys: `id UUID PRIMARY KEY` (UUID v7 generated in the application).
- Every tenant-scoped table has `tenant_id UUID NOT NULL` with an index. Every query in a
  repository filters by tenant. Consider Row-Level Security as a second layer of defence.
- Timestamps: `TIMESTAMPTZ`, stored in UTC, named `created_at`, `updated_at`, `closed_at`,
  `deleted_at` (soft delete where FreeITSM uses it).
- Use `sqlx::query!` / `query_as!` (compile-time checked). Commit the `.sqlx/` offline data
  so CI can build without a database (`cargo sqlx prepare --workspace`).
- Use enums stored as `TEXT` with a `CHECK` constraint for closed sets that are part of the
  domain model (for example `sensitivity`). Use lookup tables for admin-configurable sets
  (for example ticket statuses).
- Audit tables are **append-only** (no `UPDATE`/`DELETE` grants for the app role).
- Money: `NUMERIC(14,2)` plus an ISO-4217 currency code. Never use floats.

## 6. HTTP API (axum)

- REST + JSON under `/api/v1/...`. Resource names come from the vocabulary:
  `/api/v1/tickets`, `/api/v1/changes`, `/api/v1/morning-checks/results`.
- Authentication: session cookie for the web UI, API key (`Authorization: Bearer`) for
  integrations. Authorization uses RBAC capabilities `"<module>.<action>"`
  (`tickets.update`).
- Errors use RFC 9457 `application/problem+json`.
- Pagination: cursor-based (`?cursor=&limit=`), with a maximum limit of 200.
- Validate input at the boundary (DTO → value objects). Handlers stay thin.
- Middleware (`tower-http`): tracing, request-id, timeout, compression, CORS (config driven),
  body size limit.
- Keep an OpenAPI description current (`utoipa`).

### 6.1 Web UI (ADR-0010)

- The UI in `web/` talks **only** to `/api/v1` and the SSE endpoint. Missing data or
  actions are added to the API first (FR-API-001).
- **No business rules in TypeScript.** SLA, risk, permissions, validation and state
  transitions come from the API. Client-side checks only improve form feedback.
- The API client and types are **generated** from the OpenAPI description. Never edit
  `web/src/api/` by hand.
- Use the design system in `web/src/components/` and Vocabulary terms in routes,
  components and copy. All copy goes through the message catalogue.
- Every screen is keyboard-usable and passes axe-core (WCAG 2.1 AA).
- Browser sessions use the `HttpOnly` cookie plus CSRF token; never store tokens in
  `localStorage`.

## 7. Testing

| Layer          | How                                                                  |
|----------------|----------------------------------------------------------------------|
| domain         | Plain `#[test]` unit tests. No I/O. Cover every invariant.           |
| application    | Tests with in-memory fake repositories and a fixed `Clock`.          |
| infrastructure | `#[sqlx::test]` against a real Postgres (migrations applied).        |
| api            | `tower::ServiceExt::oneshot` against the Router, plus a test database. |

- Name tests by behaviour: `closing_ticket_requires_resolution_code_when_configured`.
- Use property tests (`proptest`) for value-object parsing and SLA time arithmetic.
- Use `insta` snapshots for API responses where helpful.
- Bug fixes come with a regression test.

## 8. Commands

```bash
cp .env.example .env                     # DATABASE_URL (owner) and COOPER_DATABASE_URL (app role)
docker compose up -d --wait postgres     # local database (add mailpit/greenmail with `up -d`)
cargo sqlx migrate run                   # apply migrations
cargo run -p cooper-server               # start the HTTP server (http://localhost:8080/health/ready)
cargo run -p cooper-worker               # start background jobs and the outbox dispatcher
docker compose --profile app up -d --build   # run migrate + server + worker from the Dockerfile
cargo test --workspace --all-features    # all tests (needs DATABASE_URL)
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo sqlx prepare --workspace -- --all-targets --all-features   # refresh .sqlx/ before committing
scripts/check-migrations.sh origin/main  # merged migrations must not change
cargo deny check && cargo machete        # advisories, licences, unused dependencies
```

`just check` runs the same checks as CI (see the `justfile`).

Building blocks every context uses (ADR-0004, ADR-0005, ADR-0009):

- `cooper_platform::tenancy::begin(&pool, &scope)` opens every transaction that touches
  tenant data. New tenant tables get `tenant_id`, an index, `FORCE ROW LEVEL SECURITY` and
  a `cooper_tenant_visible(tenant_id)` policy, plus an isolation test using
  `cooper_platform::testing`.
- `cooper_platform::audit::record(&mut tx, &AuditRecord)` in the same transaction as the
  change.
- `cooper_platform::outbox::append(&mut tx, &EventEnvelope)` for domain events; react to
  them with an `EventHandler` registered in `cooper-worker`.
- `cooper_platform::jobs::Job` for scheduled work, registered in `cooper-worker`.
- `cooper_http::Problem` for every API error.

## 9. Workflow for agents

1. Find the requirement ID(s) in `docs/FUNCTIONAL_REQUIREMENTS.md` for the task.
2. Check the vocabulary. Add any missing terms first.
3. If the feature exists in FreeITSM, study its intent (§1.1). Note any bugs or flaws you
   find and the correct behaviour Cooper will implement.
4. Design the domain model (aggregate, invariants, events) before writing SQL or handlers.
5. Implement inside-out: domain + tests → application → infrastructure → api.
6. Run fmt, clippy and tests. All must pass before you report the task as done.
7. Keep changes small and focused on one bounded context where possible.
8. Write an ADR for decisions that are hard to reverse (new crate, new dependency, data model
   shape, cross-context contract, significant deviation from FreeITSM).
9. Never commit secrets. `.env` is git-ignored. Use `.env.example` for documentation.

## 10. Security baseline

- Hash passwords with Argon2id. Support TOTP 2FA (FreeITSM parity).
- Store API keys as a prefix plus a hash. Show the full key only once.
- Store OAuth tokens and integration secrets encrypted at rest (key from environment).
- Every mutation records an audit entry with the actor (`Analyst`, `Requester`, `ApiKey` or
  `System`/automation).
- Enforce tenant isolation and `Sensitivity::Confidential` checks in the application layer,
  and cover them with tests.
- Sanitise HTML from email and knowledge articles (`ammonia`) before rendering.
