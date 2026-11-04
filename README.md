# Cooper

Cooper is an open IT Service Management (ITSM) platform written in Rust. It reimplements
[FreeITSM](https://github.com/edmozley/freeitsm) and keeps FreeITSM's domain model and
language, on a modern, type-safe stack.

## Features (planned)

- **Service Desk:** tickets, email-to-ticket (Microsoft Graph, Google, IMAP), SLA with
  calendars and pause states, reply templates, merge/split, CSAT.
- **Self-Service Portal:** Requesters raise and track tickets, read knowledge, check service status.
- **ITIL processes:** Problem Management (known errors), Change Management (CAB, risk, PIR),
  Knowledge, Tasks.
- **Operations:** Service Status page, Morning Checks, Calendar.
- **Estate & commercial:** Assets, Software & Licences, CMDB, Contracts & Suppliers, Forms.
- **Automation:** Workflows, Webhooks, REST API with scoped API keys.
- **Multi-company** (multi-tenant) with RBAC and audit trails on every record.

See [docs/FUNCTIONAL_REQUIREMENTS.md](docs/FUNCTIONAL_REQUIREMENTS.md) for the full list and
delivery phases.

## Tech stack

| Concern         | Choice                         |
|-----------------|--------------------------------|
| Language        | Rust (stable, edition 2024)    |
| Async runtime   | tokio                          |
| Web framework   | axum                           |
| Database        | PostgreSQL 16+ via sqlx        |
| Observability   | tracing                        |

Cooper is a modular monolith, organised by bounded context following Domain-Driven Design.
See [AGENTS.md](AGENTS.md) for the architecture and coding conventions.

## Getting started

> These steps describe the intended workflow. They will work once the workspace is scaffolded.

Prerequisites: Rust (via [rustup](https://rustup.rs)), Docker, and `sqlx-cli`
(`cargo install sqlx-cli --no-default-features --features rustls,postgres`).

```bash
cp .env.example .env                     # adjust settings
docker compose up -d postgres            # start a local database
cargo sqlx migrate run                   # apply migrations
cargo run -p cooper-server               # HTTP server
cargo run -p cooper-worker               # background jobs (mail polling, SLA, schedules)
```

Run the checks before you push:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Documentation

| Document | Purpose |
|---|---|
| [AGENTS.md](AGENTS.md) | Architecture, conventions and workflow for contributors and AI agents |
| [docs/VOCABULARY.md](docs/VOCABULARY.md) | Ubiquitous language: use these terms everywhere |
| [docs/FUNCTIONAL_REQUIREMENTS.md](docs/FUNCTIONAL_REQUIREMENTS.md) | What Cooper must do, with IDs, priorities and phases |
| `docs/adr/` | Architecture Decision Records |

## Contributing

1. Find the requirement ID for your change in the functional requirements.
2. Use the vocabulary terms. If a term is missing, add it in the same change.
3. Build inside-out: domain and tests, then application, infrastructure, api.
4. Reference requirement IDs (for example `FR-TKT-008`) in commits and pull requests.

## Acknowledgements

Cooper's domain model is based on [FreeITSM](https://github.com/edmozley/freeitsm) by Ed Mozley.
