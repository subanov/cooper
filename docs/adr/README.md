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
| [0022](0022-live-updates-over-server-sent-events.md) | Live Updates: PostgreSQL `NOTIFY` in the writing transaction, Server-Sent Events that only invalidate caches | Accepted |
| [0023](0023-deployment-observability-and-release-pipeline.md) | Signed multi-arch image, separate migrate step, pgBackRest PITR, Prometheus metrics and OTLP traces, release pipeline | Accepted |
| [0024](0024-ticket-relations-merge-split-fields-and-presence.md) | Ticket relations: Links, Merge and Split with undo, Number History, Type Rules, Ticket Fields and Presence | Accepted |
| [0025](0025-user-groups-manager-grants-and-oidc.md) | User Groups, Manager Grants for the Portal, and OIDC single sign-on verified with ring | Accepted |
| [0026](0026-problem-management-context.md) | Problem Management as its own context, reaching tickets through the service desk API | Accepted |
| [0027](0027-change-management-context.md) | Change Management: stages, CAB approval rounds, versioned templates and a calendar | Accepted |
| [0028](0028-knowledge-context.md) | Knowledge: one read decision, access lists with object polarity, shared Articles | Accepted |
| [0029](0029-tasks-context.md) | Tasks: real occurrences, a pure recurrence engine, and ticket-side related records | Accepted |
| [0030](0030-service-status-context.md) | Service Status: shared or per-Company Services, a computed Current Status, and a public page read with no Company | Accepted |
| [0031](0031-morning-checks-context.md) | Morning Checks: per-Company Checks, Check Dates in the instance time zone, and once-only reminders | Accepted |
| [0032](0032-reporting-context.md) | Reporting: owning contexts count in SQL as the caller, Reporting composes; no read model | Accepted |
| [0033](0033-attachments-storage-extraction-and-mailbox-consent.md) | S3 blob store over SigV4, background text extraction, Mailbox Consent with PKCE, and Portal Notifications | Accepted |
| [0034](0034-assets.md) | Assets: retirement on the Asset, one open Check Out, exact money, and Suppliers by ID | Accepted |
| [0035](0035-contracts.md) | Contracts: per-Company Suppliers and Contracts, a calculated Notice Date, a Calendar event contract and once-only reminders | Accepted |
| [0036](0036-calendar-context.md) | Calendar: per-Company events, instance-wide categories, and read-only events generated from other contexts' outbox events | Accepted |
| [0037](0037-suggested-replies-with-an-ai-provider.md) | Suggested Replies through a provider port in the service desk, Gemini configured by environment | Accepted |
| [0038](0038-editable-suggested-reply-instructions.md) | Editable Suggested Reply Instructions with fixed rules Cooper always adds | Accepted |
| [0039](0039-forms.md) | Forms: per-Company, versioned Forms with typed Form Fields | Accepted |
| [0040](0040-forms-on-the-portal.md) | Forms on the Portal: audiences, approval rounds, Submission Actions and Drafts | Accepted |
| [0041](0041-asset-extras.md) | Asset extras: custom fields, software inventory, licences and CSV import | Accepted |
| [0043](0043-checklists-and-procedures.md) | Checklists, closure rules and procedures on tasks | Accepted |
| [0042](0042-contract-terms-scheduling-and-reminders.md) | Contract Terms, Task links to Contracts, scheduled work, the rota and Asset reminders | Accepted |
