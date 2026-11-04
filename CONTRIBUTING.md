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
cargo test --workspace                   # or: just test
```

`docker compose up -d` also starts Mailpit (outbound mail capture, UI on
<http://localhost:8025>) and GreenMail (IMAP/SMTP test server for mailbox intake).

## Workflow

1. Pick an issue. Each feature issue lists its requirement IDs (`FR-*`) and dependencies.
2. Branch from `main`: `feat/<short-name>`, `fix/<short-name>` or `chore/<short-name>`.
3. Use the [vocabulary](docs/VOCABULARY.md) verbatim. Add missing terms in the same PR.
4. Build inside-out: domain and tests → application → infrastructure → api.
5. Database changes: add a **new** migration (`cargo sqlx migrate add <description>`).
   Never edit a merged migration; CI rejects it.
6. Run `just check` (fmt, clippy, tests, `.sqlx` freshness, migration guard). After
   changing a query, run `just prepare` and commit `.sqlx/`.
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
