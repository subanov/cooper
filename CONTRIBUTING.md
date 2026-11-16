# Contributing to Cooper

Thank you for helping. Read [AGENTS.md](AGENTS.md) first: it defines the architecture,
conventions and workflow. This page covers the practical steps.

## Set up

Prerequisites: [rustup](https://rustup.rs) (the toolchain is pinned in
`rust-toolchain.toml` and installs automatically), Docker with Compose,
[`just`](https://just.systems) (optional) and `sqlx-cli`:

```bash
cargo install sqlx-cli --locked --no-default-features --features rustls,postgres
cp .env.example .env
docker compose up -d --wait postgres     # or: just db
cargo sqlx migrate run                   # or: just migrate
just test                                # tests in their own PostgreSQL (see below)
```

Tests and `just check` do not use the compose database. `just test-db` (run
automatically by `just test`, `lint`, `prepare`, `sqlx-check` and `coverage`) starts a
separate container, `cooper-test-postgres`, on port 55432 (`COOPER_TEST_POSTGRES_PORT`)
with its own role and database, `cooper_test`, on a tmpfs, and applies the migrations.
Your development data in the compose `cooper` database is never touched by tests, and the
`_sqlx_test_*` databases tests create never appear there. `just test-db-down` removes it.

`docker compose up -d` also starts Mailpit (outbound mail capture, UI on
<http://localhost:8025>) and GreenMail (IMAP/SMTP test server for mailbox intake).

### Web UI (`web/`)

The Analyst console and the Portal are a React SPA ([ADR-0010](docs/adr/0010-typescript-react-spa-for-analyst-console-and-portal.md)).
You need Node (version in `web/.node-version`) and pnpm 12 (`npm install -g pnpm@12`).
Browser tests run in the official Playwright container, so you only need Docker for them,
and screenshots match CI.

```bash
just web-install      # pnpm install --frozen-lockfile
just web-dev          # http://localhost:5173, proxies /api to `just server`
just web-check        # typecheck, Biome, API client up to date, Vitest (+ axe-core)
just web-build        # web/dist, served by cooper-server on :8080
just web-e2e          # Playwright + axe-core against a running server (see below)
just web-visual       # Storybook screenshots in light and dark themes
```

For `just web-e2e`, start `just server` with `COOPER_BIND_ADDR=0.0.0.0:8080` after
`just web-build`. From WSL or Docker Desktop also set
`COOPER_E2E_URL=http://host.docker.internal:8080`.

Rules for `web/`:

- **API types are generated.** After changing the Rust API, run `just web-api` and commit
  `web/openapi.json` and `web/src/api/schema.d.ts`. The Rust test
  `committed_openapi_document_is_current` and `pnpm api:check` fail otherwise.
- **Visual changes need new baselines.** When a component's look changes on purpose, run
  `just web-visual-update`, look at the new images in `web/visual/__screenshots__/` and
  commit them.
- **Dependencies.** pnpm only installs versions that are at least 7 days old, and blocks
  install scripts unless `web/pnpm-workspace.yaml` allows them. Explain every new runtime
  dependency in the PR. CI runs `pnpm audit` (high and critical advisories fail the build).

## Workflow

1. Pick an issue. Each feature issue lists its requirement IDs (`FR-*`) and dependencies.
2. Branch from `main`: `feat/<short-name>`, `fix/<short-name>` or `chore/<short-name>`.
3. Use the [vocabulary](docs/VOCABULARY.md) verbatim. Add missing terms in the same PR.
4. Build inside-out: domain and tests → application → infrastructure → api.
5. Database changes: add a **new** migration (`cargo sqlx migrate add <description>`).
   Never edit a merged migration; CI rejects it.
6. Run `just check` (fmt, clippy, tests, `.sqlx` freshness, migration guard). After
   changing a query, run `just prepare` and commit `.sqlx/`. When `web/` changed, also run
   `just web-check` (and `just web-visual` for component changes).
7. Open a pull request using the template. Reference requirement IDs and `Closes #N`.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/) with the requirement ID
where there is one:

```
feat(tickets): close tickets with resolution code (FR-TKT-008)
fix(sla): skip holidays when computing due times (FR-SLA-002)
chore(ci): cache sqlx-cli
```

## Decisions

Write an ADR in `docs/adr/` for anything hard to reverse: a new crate or dependency, a
data model shape, a cross-context contract or a significant deviation from FreeITSM. Copy
`docs/adr/0000-template.md`.

## Security

Report vulnerabilities privately; see [SECURITY.md](SECURITY.md).
