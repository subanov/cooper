# 0009. Shared `kernel`, `platform` and `http` crates and the dependency baseline

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issue:** #10, #11, #12, #13
- **Related:** [ADR-0001](0001-record-architecture-decisions.md)

## Context

AGENTS.md §3.1 lists a `kernel` crate for shared types with "no business rules", plus
the two binaries. The foundation also needs shared **infrastructure** that every context
uses the same way:

- the PostgreSQL pool and migrations;
- tenant-scoped transactions;
- the audit writer;
- the outbox;
- the job runner;
- the HTTP error format and middleware.

`kernel` must stay usable from `domain` modules, which may not depend on `sqlx` or
`axum` (AGENTS.md §3.2). Putting infrastructure there would break that rule.

## Decision

Create three shared library crates. Package names are prefixed `cooper-` to avoid
clashing with crates.io names such as `http`.

| Crate | Path | Contains | May depend on |
|---|---|---|---|
| `cooper-kernel` | `crates/kernel` | typed ID macro and shared IDs (`TenantId`, `AnalystId`, `RequesterId`, `ApiKeyId`), `TenantScope`, `Actor`, `Clock`, `DomainEvent`/`EventEnvelope`, `Cursor`/`Limit`/`Page`, `AuditRecord` | `serde`, `uuid`, `chrono`, `thiserror`, `base64`, `serde_json`. **No I/O, no sqlx, no axum.** |
| `cooper-platform` | `crates/platform` | config helpers, pool and migrations, `tenancy::begin`, `audit::record`, `outbox`, `jobs`, telemetry, test harness (`testing` feature) | `kernel`, `sqlx`, `tokio`, `tracing` |
| `cooper-http` | `crates/http` | RFC 9457 `Problem`, standard middleware stack | `axum`, `tower`, `tower-http` |

Rules:

- Context `domain` modules may use only `cooper-kernel`. Their `infrastructure` modules
  use `cooper-platform`, and their `api` modules use `cooper-http`.
- Shared crates contain **no business rules**. If something is about tickets, changes or
  any other domain concept, it belongs in that context.
- Shared, platform-owned tables (`audit_entries`, `outbox_events`, `background_jobs`,
  `background_job_runs`) are written only through `cooper-platform`. Contexts never query
  them directly. A single `audit_entries` table (keyed by aggregate type and ID) replaces
  FreeITSM's many `*_audit` tables. Each context still owns its audit *content*.

### Dependency baseline

Dependencies are declared once in `[workspace.dependencies]`, and versions are bumped by
Dependabot. The initial set and the reason for each:

| Dependency | Why |
|---|---|
| `tokio`, `axum`, `tower`, `tower-http`, `http` | AGENTS.md stack: async runtime, web framework, middleware |
| `sqlx` (postgres, rustls-ring, migrate, macros, uuid, chrono, json) | AGENTS.md stack: compile-time checked queries, migrations. **rustls only**, OpenSSL is banned in `deny.toml` |
| `serde`, `serde_json` | serialization |
| `thiserror` / `anyhow` | errors in libraries / binaries (AGENTS.md §4) |
| `uuid` (v7) | typed IDs |
| `chrono` | time ([ADR-0002](0002-use-chrono-for-time.md)) |
| `base64` | opaque pagination cursors |
| `async-trait` | object-safe async traits for `EventHandler` and `Job`, which are stored as `Arc<dyn …>` |
| `tracing`, `tracing-subscriber` | observability |
| `proptest` (dev) | property tests for value objects and time arithmetic |

Adding a dependency beyond this baseline needs a sentence in the PR explaining why.
`cargo deny` enforces the licence allow-list and advisories, and `cargo machete` flags
unused dependencies.

## Consequences

- AGENTS.md §3.1 is updated to show `crates/platform` and `crates/http`.
- Each new context crate starts from the same building blocks, so tenancy, audit and
  events are hard to get wrong.
- The shared crates are a coupling point. Changes to them get CODEOWNERS review.
