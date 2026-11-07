# Architecture Decision Records

Decisions that are hard to reverse: new crates or dependencies, data model shapes,
cross-context contracts and significant deviations from FreeITSM (AGENTS.md §9). Copy
[0000-template.md](0000-template.md), take the next number, and link the issue. Never
rewrite an accepted ADR: supersede it with a new one.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions; modular monolith by bounded context | Accepted |
| [0002](0002-use-chrono-for-time.md) | Use `chrono` + `chrono-tz` for time | Accepted |
| [0003](0003-server-rendered-ui-with-askama-and-htmx.md) | Server-rendered UI with askama + htmx | Superseded by 0010 |
| [0004](0004-tenant-isolation-with-row-level-security.md) | Tenant isolation: explicit filters plus Row-Level Security | Accepted |
| [0005](0005-domain-events-and-transactional-outbox.md) | Domain events through a transactional outbox | Accepted |
| [0006](0006-postgresql-full-text-search.md) | PostgreSQL full-text search | Accepted |
| [0007](0007-attachment-storage.md) | Attachments in a blob store (filesystem or S3), metadata in PostgreSQL | Accepted |
| [0008](0008-freeitsm-database-import.md) | Offer a one-way FreeITSM importer, after P1 | Accepted |
| [0009](0009-shared-crates-and-dependency-baseline.md) | Shared `kernel`, `platform`, `http` crates and the dependency baseline | Accepted |
| [0010](0010-typescript-react-spa-for-analyst-console-and-portal.md) | Analyst console and Portal as a TypeScript React SPA served by `cooper-server` | Accepted |
| [0011](0011-identity-context-sessions-and-rbac.md) | Identity context: Companies, Analysts, Requesters, browser sessions and RBAC | Accepted |
| [0012](0012-administration-context.md) | Administration context: Module switches, typed System Settings, job schedules, system log | Accepted |
| [0013](0013-two-factor-ip-bans-api-keys-and-encryption-at-rest.md) | Two-factor sign-in, IP bans, API keys and encryption of secrets at rest | Accepted |
| [0014](0014-service-desk-core.md) | Service desk core: reference data, Ticket Numbers, lifecycle, visibility and search | Accepted |
| [0015](0015-sla-clock.md) | SLA clock: calendars, derived due times, once-only warnings and breaches | Accepted |
| [0016](0016-in-app-notifications.md) | In-app Notifications from Domain Events, delivered idempotently | Accepted |
| [0017](0017-outbound-email-and-attachments.md) | Outbound email through a queue with a send log; file-system attachments first | Accepted |
| [0018](0018-email-intake.md) | Email intake: Mailboxes, threading, matching and loop protection | Accepted |
| [0019](0019-email-templates.md) | Email Templates sent from ticket events, most specific match wins | Accepted |
| [0020](0020-microsoft-graph-and-gmail-mailboxes.md) | Microsoft Graph and Gmail Mailboxes on the shared intake pipeline | Accepted |
| [0021](0021-portal-accounts.md) | Portal Accounts per email address, set-password links and Portal sessions | Accepted |
| [0023](0023-deployment-observability-and-release-pipeline.md) | Signed multi-arch image, separate migrate step, pgBackRest PITR, Prometheus metrics and OTLP traces, release pipeline | Accepted |
