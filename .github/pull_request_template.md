## What and why

<!-- One or two sentences. Use docs/VOCABULARY.md terms. -->

Requirements: <!-- FR-XXX-000, ... -->
Closes #<!-- issue -->

## Checklist (AGENTS.md §9)

- [ ] Vocabulary terms used verbatim; new terms added to `docs/VOCABULARY.md`
- [ ] FreeITSM intent studied; deviations recorded in the requirement (and an ADR if significant)
- [ ] Built inside-out: domain + tests → application → infrastructure → api
- [ ] Every mutation writes an Audit entry; tenant isolation covered by tests
- [ ] Migrations are new files only (never edit a merged migration)
- [ ] `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test --workspace` pass; `.sqlx/` refreshed (`just prepare`)
- [ ] No secrets; new configuration documented in `.env.example`
