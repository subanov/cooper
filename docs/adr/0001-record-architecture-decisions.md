# 0001. Record architecture decisions; build a modular monolith by bounded context

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issue:** #1

## Context

Cooper reimplements FreeITSM, a large PHP application with about 437 tables and logic
spread across pages, API scripts and JavaScript. We want a codebase that many people
(and AI agents) can change safely for years. Decisions made now, such as crate layout,
tenancy, events and storage, are expensive to reverse. Without a record, the reasons
behind them are lost.

## Options considered

1. **No written decisions.** Rely on code and PR discussions. Reasons get lost, and
   agents repeat old debates.
2. **Architecture Decision Records** (Michael Nygard's format) in the repository, next to
   the code they govern.

For the overall shape:

1. **Microservices per module.** Independent deployment, but distributed transactions,
   operational cost and network boundaries before we know where the seams are.
2. **Layered monolith** (one `models`, one `services` module). Simple at first, but
   modules entangle quickly. This is what FreeITSM looks like today.
3. **Modular monolith by bounded context.** One deployable, with each DDD context in its
   own crate, strict internal layering, and communication only through public
   application APIs or domain events.

## Decision

- We record decisions as ADRs in `docs/adr/NNNN-title.md`, using
  [0000-template.md](0000-template.md). The index is [README.md](README.md). Accepted
  ADRs are not rewritten; a new ADR supersedes them.
- Cooper is a **modular monolith organised by bounded context** (AGENTS.md §3). Each
  context is a crate under `crates/` with `domain`, `application`, `infrastructure` and
  `api` modules and the dependency rule `api → application → domain ← infrastructure`.
  Two binaries, `cooper-server` and `cooper-worker`, wire the contexts together.
- Contexts never read each other's tables. They refer to other aggregates by ID and
  coordinate through domain events ([ADR-0005](0005-domain-events-and-transactional-outbox.md)).
- Context crates are created only when work on that context starts.

## Consequences

- One deployment and one database keep operations simple, and the `cooper-server` can
  still scale horizontally because it is stateless.
- Crate boundaries make accidental coupling a compile error rather than a code-review
  finding.
- A context can be extracted into a service later if load or team structure demands it,
  because its contract is already explicit.
- Every hard-to-reverse change needs an ADR, which is a small cost per PR.
