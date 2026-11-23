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
| Web UI          | TypeScript + React SPA (ADR-0010) |
| Database        | PostgreSQL 18 via sqlx         |
| Observability   | tracing                        |

Cooper is a modular monolith, organised by bounded context following Domain-Driven Design.
See [AGENTS.md](AGENTS.md) for the architecture and coding conventions.

## Getting started

Prerequisites: Rust (via [rustup](https://rustup.rs); the toolchain version is pinned in
`rust-toolchain.toml`), Docker with Compose, and `sqlx-cli`
(`cargo install sqlx-cli --locked --no-default-features --features rustls,postgres`).
[`just`](https://just.systems) is optional.

```bash
cp .env.example .env                     # adjust settings
docker compose up -d --wait postgres     # start a local database
cargo sqlx migrate run                   # apply migrations
cargo run -p cooper-server               # HTTP server: http://localhost:8080/health/ready
cargo run -p cooper-worker               # background jobs and domain event dispatch
```

On a fresh database, create the default Company and the first Administrator once
(FR-SYS-004), then sign in. The API is described at `/api/v1/openapi.json`.

```bash
curl -X POST http://localhost:8080/api/v1/setup -H 'content-type: application/json' -d '{
  "company_name": "Default", "ticket_code": "SD", "username": "admin",
  "email": "admin@example.com", "full_name": "Administrator", "password": "change me please"}'
```

### Web UI

The Analyst console (`/`) and the Self-Service Portal (`/portal`) are one React SPA in
[`web/`](web) ([ADR-0010](docs/adr/0010-typescript-react-spa-for-analyst-console-and-portal.md)).
It needs Node (version in `web/.node-version`) and [pnpm](https://pnpm.io) 12.

```bash
just web-install        # pnpm install --frozen-lockfile in web/
just web-dev            # Vite on http://localhost:5173, proxying /api to cooper-server
just web-build          # build web/dist; cooper-server serves it on http://localhost:8080
just web-check          # typecheck, Biome, generated API client check, Vitest
just web-api            # after changing the Rust API: regenerate web/openapi.json and types
```

Release builds (and the Docker image) embed `web/dist` into `cooper-server`; debug builds
read it from disk. Without a build, the API works and non-API paths answer `404`.

`docker compose up -d` also starts Mailpit (captures outbound mail, UI on
<http://localhost:8025>) and GreenMail (IMAP/SMTP test server). Mailpit relays every message on to GreenMail, and Roundcube (webmail
on <http://localhost:8026>) lets you sign in as any address with any password: write to a
Mailbox address to open a ticket and read Cooper's replies in the same inbox.

To run Cooper itself in containers instead (migrations, server and worker, built from the
[Dockerfile](Dockerfile)):

```bash
docker compose --profile app up -d --build   # or: just up-app
```

Images: `runtime` (default; `cooper-server` and `cooper-worker` on distroless, non-root, start
the worker with `/usr/local/bin/cooper-worker`) and `migrate` (`cooper-server migrate` as the
owner role). Probe `GET /health/live` and `GET /health/ready`. Tagged releases (`vX.Y.Z`)
publish signed `linux/amd64` and `linux/arm64` images to GitHub Container Registry.

For production, start from the single-host deployment in
[deploy/single-host/](deploy/single-host/compose.yaml) (Caddy with automatic HTTPS,
PostgreSQL with pgBackRest point-in-time recovery): see the
[deployment](docs/runbooks/deployment.md) and [backup](docs/runbooks/backup-and-restore.md)
runbooks.

Run the checks before you push (`just check`, and `just web-check` when `web/` changed).
`just check` runs against its own PostgreSQL (`cooper-test-postgres`, role `cooper_test`),
never the compose database:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full workflow.

## Documentation

| Document | Purpose |
|---|---|
| [AGENTS.md](AGENTS.md) | Architecture, conventions and workflow for contributors and AI agents |
| [docs/VOCABULARY.md](docs/VOCABULARY.md) | Ubiquitous language: use these terms everywhere |
| [docs/FUNCTIONAL_REQUIREMENTS.md](docs/FUNCTIONAL_REQUIREMENTS.md) | What Cooper must do, with IDs, priorities and phases |
| [docs/adr/](docs/adr/README.md) | Architecture Decision Records |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Set-up, workflow, commit style |
| [SECURITY.md](SECURITY.md) | How to report vulnerabilities |
| [docs/runbooks/](docs/runbooks/) | Deploying, upgrading, backups and restore, releasing, key rotation |

## Contributing

1. Find the requirement ID for your change in the functional requirements.
2. Use the vocabulary terms. If a term is missing, add it in the same change.
3. Build inside-out: domain and tests, then application, infrastructure, api.
4. Reference requirement IDs (for example `FR-TKT-008`) in commits and pull requests.

## Acknowledgements

Cooper's domain model is based on [FreeITSM](https://github.com/edmozley/freeitsm) by Ed Mozley.
