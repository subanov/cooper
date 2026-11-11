# Cooper Functional Requirements

Cooper reimplements FreeITSM in Rust (tokio + axum + PostgreSQL). This document lists
**what** Cooper must do. [AGENTS.md](../AGENTS.md) covers **how** it is built. All terms
come from [VOCABULARY.md](VOCABULARY.md).

## How to read this document

- **ID**: `FR-<AREA>-<NNN>`. IDs are stable: never renumber, and mark removed requirements
  as *Withdrawn*.
- **Priority** (MoSCoW): **M** = Must, **S** = Should, **C** = Could, **W** = Won't (for now).
- **Phase**: the delivery phase the requirement is planned for.
  - **P1 — Core Service Desk:** Identity, Tickets, Email intake, SLA, Portal (basic), Audit.
  - **P2 — ITIL Processes:** Problems, Changes, Knowledge, Tasks, Service Status, Morning Checks.
  - **P3 — Estate & Commercial:** Assets, Software, CMDB, Contracts, Calendar, Forms.
  - **P4 — Automation & Insight:** Workflows, Webhooks, Integrations, Reporting, AI, extra channels.
- "Configurable" means an Administrator manages the values at runtime (reference data),
  without a deployment.

| Code | Area |
|---|---|
| GEN | Cross-cutting |
| IAM | Identity, access & tenancy |
| TKT | Tickets |
| EML | Mailboxes & email intake |
| CHN | Other channels |
| SLA | SLA & rota |
| PRT | Self-Service Portal |
| PRB | Problem Management |
| CHG | Change Management |
| TSK | Tasks, Checklists & Projects |
| KNW | Knowledge |
| SST | Service Status |
| MCK | Morning Checks |
| AST | Assets & Software |
| CMD | CMDB |
| CON | Contracts & Suppliers |
| CAL | Calendar |
| FRM | Forms |
| WFL | Workflow, Webhooks & Integrations |
| RPT | Reporting & Dashboards |
| AI  | AI features |
| SYS | System administration |
| API | REST API |

---

## 1. Cross-cutting (GEN)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-GEN-001 | Every create, update or delete of a domain record writes an append-only **Audit** entry: actor, timestamp, field, old value, new value. | M | P1 |
| FR-GEN-002 | Every record that belongs to a Company is isolated by **Tenant**. An Analyst sees only the Companies they have access to (directly or through a Team). | M | P1 |
| FR-GEN-003 | Lists support filtering, sorting, full-text search and cursor pagination. | M | P1 |
| FR-GEN-004 | Records support soft delete where FreeITSM does (tickets, assets). Soft-deleted records are hidden by default and an Administrator can restore them. | S | P1 |
| FR-GEN-005 | Each Module can be turned on or off. A disabled module is hidden from navigation and its API returns 404. Deviation from FreeITSM: the Tickets Module is always on (ADR-0012). | S | P1 |
| FR-GEN-006 | Users receive in-app **Notifications** (the bell) for assignments, mentions, replies and SLA events. | S | P1 |
| FR-GEN-007 | Attachments can be uploaded to tickets, changes, problems, tasks, articles and contracts, with a configurable size limit. Content is stored once per SHA-256 on the file system or in an S3-compatible bucket (configuration); content no attachment references is deleted after a grace period (ADR-0007, ADR-0033). | M | P1 |
| FR-GEN-008 | Users can save **Table Views** (columns, filters, sort) per list. | C | P4 |
| FR-GEN-009 | The UI and email texts support localisation. English is the default. | C | P4 |
| FR-GEN-010 | **Live Updates**: open Analyst screens refresh within seconds when a ticket they show changes (created, assigned, status or Priority changed, closed, reopened, Note, email, time, attachment, SLA Warning or Breach), without polling. Analysts are told only about tickets they may see (Company access and Confidential rules). Live Updates carry no ticket data; the screen re-reads it through the API (ADR-0022). A slow fallback refresh covers anything a Live Update cannot (ADR-0022). | S | P1 |

## 2. Identity, Access & Tenancy (IAM)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-IAM-001 | Administrators can create, update and deactivate **Analysts** (name, email, username, admin flag, active flag). | M | P1 |
| FR-IAM-002 | Analysts sign in with username/email and password (Argon2id). Sessions expire after a configurable period. Deviation from FreeITSM: sessions have an idle timeout and an absolute lifetime, tokens are stored hashed, and every unsafe request needs a CSRF token (ADR-0011). | M | P1 |
| FR-IAM-003 | Analysts can turn on **TOTP two-factor authentication**. Administrators can require it. Trusted devices can be remembered. Deviation from FreeITSM: a code is accepted only once (ADR-0013). | S | P1 |
| FR-IAM-004 | Password reset by emailed one-time token. | M | P1 |
| FR-IAM-005 | Repeated failed logins from an IP address cause a temporary ban. | S | P1 |
| FR-IAM-006 | Administrators manage **Teams** and Team membership. | M | P1 |
| FR-IAM-007 | Administrators manage **Departments** (name, default Sensitivity) and link them to the Teams that serve them. | M | P1 |
| FR-IAM-008 | Administrators manage **Companies** (name, Ticket Code, Sender Domains). A "Default" company exists on install. Deviation from FreeITSM: a Sender Domain belongs to one Company only and can never be a Freemail Domain; the default Company cannot be deactivated. | M | P1 |
| FR-IAM-009 | Administrators grant Analysts or Teams access to specific Companies or to all Companies. Deviation from FreeITSM: Administrators always see every Company; non-Administrators can only grant access within their own scope (ADR-0011). | M | P1 |
| FR-IAM-010 | RBAC: Administrators define **Roles** as sets of **Capabilities** (`<module>.<action>`) and assign them to Analysts or Teams. Every API endpoint checks a Capability. Roles may only contain Capabilities a context declares, and a non-Administrator can only grant Capabilities they hold. | M | P1 |
| FR-IAM-011 | Administrators and Directory Sync manage **Requesters** (name, email, username, Manager, Cost Centre, Company, active flag). Deactivation is recorded with a timestamp. | M | P1 |
| FR-IAM-012 | SSO for Analysts and Requesters through **OIDC**. Authorization code flow with PKCE, single-use state and nonce; ID tokens must be RS256 or ES256 signed by the provider's JWKS. A subject is linked once; a first sign-in links by verified email only when the provider allows it. SSO never creates Analysts and only signs in Requesters who already exist. The identity provider is responsible for the second factor (ADR-0025). | S | P2 |
| FR-IAM-013 | LDAP/Active Directory authentication and **Directory Sync** of Requesters and groups, with run history (running/ok/stopped/failed). | S | P3 |
| FR-IAM-014 | SSO group → Team mapping. | C | P3 |
| FR-IAM-015 | **Manager Grants** give a Requester visibility of other Requesters' tickets (everyone, user, group, department, reports). Exclusions (a Requester, User Group or Cost Centre) always win; everything stays within the manager's Company; the feature is off until switched on. Deviation from FreeITSM: Requesters have no free-text department in Cooper, so the `department` grant is a `cost_centre` grant (ADR-0025). | S | P2 |
| FR-IAM-016 | **User Groups** of Analysts and/or Requesters, with optional membership expiry. Inactive groups and expired memberships grant nothing. | S | P2 |

## 3. Tickets (TKT)

### 3.1 Lifecycle

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-TKT-001 | An Analyst can create a **Ticket** with subject, description, Requester, Department, Ticket Type, Category, Priority, Origin, Queue (Team), Assignee and Sensitivity. | M | P1 |
| FR-TKT-002 | Each ticket gets a unique **Ticket Number** from a configurable format (prefix per department or type, `{COMPANY}` token, zero-padded sequence). Cooper tokens: `{COMPANY}`, `{TYPE}`, `{YEAR}`, `{SEQ:n}`, one sequence per Company; a per-Department prefix is not supported yet (ADR-0014). | M | P1 |
| FR-TKT-003 | When the number format changes or a ticket is renumbered, previous numbers are kept in Ticket Number History and still resolve to the ticket. Renumbering (`tickets.configure`) takes the next number of the current format; a retired number is never reused (ADR-0024). | S | P2 |
| FR-TKT-004 | **Ticket Statuses** are configurable, each with `is_closed`, `pauses_sla` and `is_default` flags. Seeded with Open (default), In Progress, On Hold (pauses SLA), Awaiting Response (pauses SLA), Closed. Exactly one status is the default. Deviation from FreeITSM: the seed always sets `pauses_sla` (FreeITSM's verification seeder omits it, so those installs never pause the SLA). | M | P1 |
| FR-TKT-005 | **Ticket Priorities** are configurable, each with SLA response minutes, resolution minutes and SLA Calendar. Seeded with Low, Normal (default), High, Critical, Urgent. | M | P1 |
| FR-TKT-006 | **Ticket Types**, **Origins** and **Resolution Codes** are configurable, with the FreeITSM seeds in the Vocabulary. Seeded rows have a stable `key` that code uses instead of the name. Deviation from FreeITSM: FreeITSM finds channel Origins by English name, so renaming one breaks channel intake. | M | P1 |
| FR-TKT-007 | **Categories** are configurable as a tree of at most 3 levels. A category can be limited to Ticket Types and marked visible on the Portal. | M | P1 |
| FR-TKT-008 | Closing a ticket (moving it to a closed status) records the closed timestamp. It requires a **Resolution Code**, and a **Closure Category** when configured. It also requires every Ticket Field marked *required on close*. | M | P1 |
| FR-TKT-009 | A closed ticket can be reopened. Reopening clears the closed timestamp and is audited. A Requester reply to a closed ticket reopens it or creates a new linked ticket (configurable). | M | P1 |
| FR-TKT-010 | A ticket can be assigned to a Team (Queue) and/or an Analyst. The Analyst must belong to the Team when both are set (configurable). Assignment changes notify the new assignee. | M | P1 |
| FR-TKT-011 | **Ticket Type Rules** set the default Ticket Type from channel, mailbox, origin, department, subject keywords, requester domain or company. Rules are evaluated in order. A chosen type always wins and a failing rule never stops a ticket being created. Deviation from FreeITSM: rules are instance-wide reference data with a Company condition, not per-Company rows, and there is no separate default-type setting (ADR-0024). | S | P2 |
| FR-TKT-012 | Analysts can **Snooze** a ticket until a date and time (at most 730 days ahead), with a reason. The ticket records who snoozed it and when. Snoozed tickets are hidden from default views and reappear automatically; the SLA clock keeps running. | S | P2 |
| FR-TKT-013 | A ticket can be flagged **First Time Fix** and **Training Provided**. | S | P1 |
| FR-TKT-014 | A ticket can be **Confidential**. Only Analysts with the matching capability, or in the Department's Teams, can see confidential tickets. Confidential content is excluded from search for everyone else. | M | P1 |

### 3.2 Collaboration & content

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-TKT-020 | Analysts add **Notes** to a ticket. Notes are internal by default and can be made visible to the Requester. | M | P1 |
| FR-TKT-021 | Analysts send **Replies** to the Requester (and CC recipients) by email from the ticket's Mailbox. The subject contains the **SDREF** tag. The Analyst's Signature is appended. | M | P1 |
| FR-TKT-022 | Analysts can insert **Reply Templates** with placeholders (requester name, ticket number, …). | S | P1 |
| FR-TKT-023 | Analysts record **Time Entries** (minutes, note, date) on a ticket. The ticket shows total time. | S | P1 |
| FR-TKT-024 | The ticket timeline shows emails, notes, status and assignment changes, and audit events, in chronological order. | M | P1 |
| FR-TKT-025 | **Ticket Presence**: the ticket shows which other Analysts are viewing or replying to it, live. Presence is a heartbeat that goes stale after 30 seconds; it warns and never locks, and it is not audited (ADR-0024). | C | P2 |
| FR-TKT-026 | **Merge** a ticket into another: emails, notes and attachments move, and the source ticket is closed and points to the target. Merge can be undone. Time Entries move too; replies to the merged-away ticket's SDREF reach the target. Deviation from FreeITSM: only its default policy (the target survives, messages move) is offered, and the target must be open (ADR-0024). | S | P2 |
| FR-TKT-027 | **Split** selected emails or notes into a new ticket. Split can be undone. The new ticket copies the source's details and Queue and is linked as related; undo is refused once the new ticket has activity of its own (ADR-0024). | C | P2 |
| FR-TKT-028 | **Ticket Links** between tickets: related, duplicate, parent/child. Links stay within one Company, a ticket has at most one parent and one master, parent/child cycles are refused, and links to tickets an Analyst may not see are hidden from them. | S | P2 |
| FR-TKT-029 | Administrators define custom **Ticket Fields** (typed: text, number, date, dropdown, checkbox, …) with *required on create*, *required on close* and *ask requester* options. A field's key and type never change; fields are retired, not deleted. Emailed tickets cannot be asked and skip *required on create*. Deviation from FreeITSM: fields are instance-wide, not per Company (ADR-0024). | S | P2 |
| FR-TKT-030 | Tickets can be linked to Assets, CIs, Problems, Changes and Tasks. Each context owns its links; `GET /tickets/{id}/related` gathers Problems, Changes, Tasks, Knowledge Articles and Morning Check Results (ADR-0031). Assets and CIs join with their contexts (ADR-0029). | S | P2 |
| FR-TKT-031 | Analysts can schedule work on a ticket (start, end, all-day) for an **Owner**. The work appears on the Calendar. | C | P3 |
| FR-TKT-032 | **Checklists** from templates can be attached to a ticket. The template's Closure Mode decides whether incomplete mandatory items block closure. | C | P3 |
| FR-TKT-033 | After closure, the Requester receives a **CSAT** request. Ratings are stored per ticket and reported. | C | P4 |

### 3.3 Views

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-TKT-040 | Built-in views: My tickets, My team's tickets (Queue), Unassigned, All open, Snoozed, Recently closed. | M | P1 |
| FR-TKT-041 | Ticket search by number (current or historical), subject, body, requester, email address and extracted attachment text. | M | P1 |
| FR-TKT-042 | Bulk actions on selected tickets: assign, change status or priority, merge, close. Up to 100 tickets; each is changed in its own transaction with the usual checks and its outcome is reported. | S | P2 |

## 4. Mailboxes & Email Intake (EML)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-EML-001 | Administrators configure **Mailboxes**: provider `microsoft` (Graph, auth mode `delegated` or `app_only`), `google` or `imap`/`smtp`, with default Department, Origin, Company and Team. Microsoft `delegated` and Google Mailboxes are connected by **Mailbox Consent**: the Administrator signs in to the account at the provider (authorization code with PKCE, single-use state, redirect URI from configuration); the refresh token is stored encrypted and never returned (ADR-0020, ADR-0033). | M | P1 |
| FR-EML-002 | A background job polls each active Mailbox on a configurable interval and imports new emails. | M | P1 |
| FR-EML-003 | An inbound email whose subject contains a valid **SDREF** tag (or matches a known thread or message id) is added to the existing ticket. Otherwise it creates a new ticket. | M | P1 |
| FR-EML-004 | The Requester is matched by sender address. Unknown senders create a Requester (configurable) or are rejected. The Company is set from **Sender Domains**. **Freemail Domains** never set a Company. | M | P1 |
| FR-EML-005 | The **Mailbox Whitelist** allows or blocks senders by address or domain. Rejected emails are deleted, moved or kept (configurable) and logged. | S | P1 |
| FR-EML-006 | After import, the mailbox message is deleted, moved or marked read (configurable). | M | P1 |
| FR-EML-007 | Attachments and inline images are stored. Text is extracted asynchronously for search, with extraction status recorded. Plain text, HTML and Office Open XML documents are read, with size, length and time limits; a file whose content cannot be fetched is retried, then `failed` (ADR-0033). Deviation from FreeITSM: there is no Apache Tika tier, so PDF and images are `unsupported`. | S | P2 |
| FR-EML-008 | Every outbound send attempt is logged (success or failure, error). Failed sends are retried. | M | P1 |
| FR-EML-009 | **Email Templates** send automated emails on events (ticket created, assigned, replied, closed), matched by recipient address or domain. Deviation from FreeITSM: only the most specific template is sent, and never twice for one event (ADR-0019). | S | P1 |
| FR-EML-010 | Mailbox health (last poll, last error) is visible to Administrators and raises a notification on failure. | S | P1 |
| FR-EML-011 | Loop protection: auto-replies, bounces and Cooper's own outbound mail do not create tickets. Cooper also skips duplicates by Message-ID, so a message read twice never creates two tickets (ADR-0018). | M | P1 |
| FR-EML-012 | OAuth tokens and mailbox passwords are encrypted at rest. Deviation from FreeITSM: keys are versioned and rotatable (ADR-0013). | M | P1 |

## 5. Other Channels (CHN)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-CHN-001 | **Web Chat Widget**, embeddable in websites. Conversations become tickets. Messages come from a visitor, an analyst ("agent" sender), the system or AI. | C | P4 |
| FR-CHN-002 | Web chat **AI mode**: `assist` (suggests answers to Analysts) or `deflect` (answers visitors from Knowledge before a ticket is created). | C | P4 |
| FR-CHN-003 | Messaging channels (WhatsApp through Twilio or Meta, Telegram, Slack, Microsoft Teams) create and update tickets. | C | P4 |
| FR-CHN-004 | **Monitoring Events** (for example Zabbix) create, update or resolve tickets, with a severity mapping. | C | P4 |
| FR-CHN-005 | Telephony (3CX) call records can be linked to Requesters and tickets. | W | — |

## 6. SLA & Rota (SLA)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-SLA-001 | Each ticket gets **Response** and **Resolution** due times from its Priority when it is created, and again when the Priority changes. | M | P1 |
| FR-SLA-002 | SLA time counts only within the **SLA Calendar**'s working hours (per weekday, time zone) and skips **Holidays**. | M | P1 |
| FR-SLA-003 | SLA time pauses while the ticket is in a status with `pauses_sla` and resumes when it leaves that status. Due times shift by the time spent paused. Cooper derives due times from creation time, targets and total paused working time (ADR-0015). | M | P1 |
| FR-SLA-004 | The first Analyst reply (or note visible to the Requester) satisfies the Response target. Closure satisfies the Resolution target. | M | P1 |
| FR-SLA-005 | A background job raises **SLA Warning** (configurable threshold) and **SLA Breach** events. Each event is raised at most once per ticket, target and trigger. | M | P1 |
| FR-SLA-006 | **SLA Notification Rules** decide who is notified (assignee, team, specific Analysts) for warning or breach, for response, resolution or both. | S | P1 |
| FR-SLA-007 | Ticket lists show SLA state (on track, warning, breached, paused) and time remaining. | M | P1 |
| FR-SLA-008 | **Rota**: Administrators define Shifts and Rota Locations (Office, WFH). Analysts are scheduled through Rota Entries, which can be flagged On Call. A rota view shows who is working today. | C | P3 |

## 7. Self-Service Portal (PRT)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-PRT-001 | Requesters sign in to the Portal (local password, SSO, or Self-Registration with email confirmation). Deviation from FreeITSM: one Portal Account per email address acts for every active Requester with that address (switchable in the session); passwords are only ever set through an emailed one-time link (Portal Invitation, reset or Self-Registration), so an address is confirmed before it can sign in; Self-Registration is off by default and limited to Sender Domains (never Freemail Domains). SSO follows with FR-IAM-012 (P2). See ADR-0021. | M | P1 |
| FR-PRT-002 | Requesters raise tickets by choosing from the **Portal Catalogue** (portal-visible Categories and Forms) and filling in any *ask requester* Ticket Fields. The Catalogue holds active, portal-visible Categories whose ancestors are active and portal-visible too; a choice is required whenever the Catalogue is not empty. The ticket gets the Portal Origin, the signed-in Requester and Company (never taken from the request), the Category's first active allowed Ticket Type and the default Priority, and is left unassigned. Forms (FR-FRM) and *ask requester* Ticket Fields join the Catalogue when those features are built. | M | P1 |
| FR-PRT-003 | Requesters see their own tickets (open and closed), read the public timeline (replies and public notes), reply and attach files. A Requester sees only tickets where they are the Requester, in the Company the Portal session acts for (Confidential ones included, since they are about them; deleted ones never); any other ticket answers "not found". The public timeline holds public Notes and replies, never internal Notes, time or field history. A Portal reply is recorded like an emailed reply (inbound message with the SDREF subject), so it notifies the Assignee and reopens a closed ticket. Requesters see their own uploads and files that arrived by email; files Analysts attach stay internal until attachments get a public flag. | M | P1 |
| FR-PRT-004 | Managers see tickets of Requesters covered by their Manager Grants. Read-only (replying and attachments stay with each Requester); Confidential tickets only when configured; a Requester's directory Manager covers their reports (direct or whole chain, configurable). | S | P2 |
| FR-PRT-005 | Requesters search and read Knowledge Articles with `customer` or `public` audience that they are allowed to see. | S | P2 |
| FR-PRT-006 | The Portal shows current **Service Status** and Planned Maintenance: active shared Services and those of the Company the session acts for, scheduled and started Planned Maintenance, and, when `service_status.show_incidents` is on, unresolved and recently resolved (`service_status.recent_days`) Incidents that have a public update, with their public updates only and no authors. Deviation from FreeITSM: Incident display is one switch, off by default, with no separate "all incidents" mode (ADR-0030). | S | P2 |
| FR-PRT-007 | Requesters receive Portal notifications and can set notification preferences. Portal Notifications tell a Requester that their ticket was logged, has a Reply or public Note, or was closed (never about their own actions), in the Company the Portal session acts for. Notification Preferences turn each kind on or off in-app and by email (the Email Template of the event); Analyst Replies always go out (ADR-0033). Deviation from FreeITSM: Manager notifications about their reports' tickets are not included. | C | P2 |

## 8. Problem Management (PRB)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-PRB-001 | Analysts create a **Problem** (Problem Number, title, description, status, priority, assigned Analyst). Problem Numbers are `PRB-00001`, … across the instance (ADR-0026). | M | P2 |
| FR-PRB-002 | Problem Statuses and Priorities are configurable. | M | P2 |
| FR-PRB-003 | One or more incident tickets can be linked to a Problem. The link is visible from both sides. | M | P2 |
| FR-PRB-004 | Analysts record a **Root Cause** and a **Workaround**, and can flag the Problem as a **Known Error**. A Known Error needs a Root Cause or a Workaround. | M | P2 |
| FR-PRB-005 | A Known Error's Workaround can be published as a Knowledge Article. | S | P2 |
| FR-PRB-006 | When a Problem is resolved, linked tickets can be bulk-updated or notified. Cooper offers closing them (with a Resolution Code) or adding a Note (public Notes reach the Requester on the Portal); each ticket passes the usual service desk checks and reports its outcome (ADR-0026). | C | P2 |
| FR-PRB-007 | Problems have notes and a full audit trail. | M | P2 |

## 9. Change Management (CHG)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-CHG-001 | Analysts create a **Change** (numbered `CHG-00001`) with title, Change Type, Priority, Impact, Category, Requested By, Assigned To, Approver, Work Window, Outage Window, description, Reason for Change, Risk Evaluation, Test Plan and Rollback Plan. Requested By is an Analyst, as in FreeITSM (Requester means a Portal user in Cooper). ADR-0027. | M | P2 |
| FR-CHG-002 | Change Types are configurable, each with an Approval Route (`review` or `emergency`). Seeded with Standard, Normal (default), Emergency. Deviation from FreeITSM: the seeded Emergency type uses the `emergency` route (FreeITSM seeds it with `review`, so emergency changes wait for approval). | M | P2 |
| FR-CHG-003 | Change Statuses are configurable. Each maps to a fixed **Stage** (draft, awaiting, approved, implementing, done, stopped) and has an `is_closed` flag. Seeded with the 10 FreeITSM statuses. | M | P2 |
| FR-CHG-004 | Status transitions follow the stage order. Changes in `draft` are editable (also `awaiting` and `approved`, see FR-CHG-007). Moving to `awaiting` submits the Change for approval. Moving to `implementing` requires approval, except for **Standard** changes and the `emergency` route (approved afterwards). Only a decision enters `approved`. | M | P2 |
| FR-CHG-005 | Risk assessment: Risk Likelihood (1–5) × Risk Impact Score (1–5) gives a **Risk Score**, mapped to a **Risk Level** through configurable thresholds (seed: Low ≤ 4, Medium 5–9, High 10–15, Very High 16–20, Critical 21–25). Deviation from FreeITSM: thresholds are reference data, computed in one place (FreeITSM hard-codes them in PHP and again in JavaScript). | S | P2 |
| FR-CHG-006 | **CAB**: when `cab_required`, CAB Members (some required) vote approve or reject with a comment. The **CAB Approval Type** (`all` or `majority` of required members) decides the outcome; a rejection by a required member sends the Change back to Draft, optional members only advise. Without a CAB, the Approver decides (or, with none named, any Analyst with `changes.approve`). Deviation from FreeITSM: the CAB is fixed once submitted, votes cannot be overwritten within a round, and a rejection always returns to Draft (ADR-0027). | M | P2 |
| FR-CHG-007 | Votes are append-only. Editing an approved Change's material content starts a new **Approval Round** and invalidates earlier approval. | M | P2 |
| FR-CHG-008 | Changes have a checklist, comments, attachments, links to tickets, problems, CIs and other changes, and a full audit trail. Links to CIs follow the CMDB (P3). | S | P2 |
| FR-CHG-009 | Every saved version of long-text fields is kept as a **Text Revision** and can be compared. | S | P2 |
| FR-CHG-010 | **Change Templates** (versioned, limited to Teams) pre-fill new Changes. | S | P2 |
| FR-CHG-011 | After completion or failure, a **PIR** is recorded: successful (yes/no), actual start and end, lessons learned, follow-up. | S | P2 |
| FR-CHG-012 | A **Change Calendar** shows Changes by Work and Outage Windows and highlights overlaps. | S | P2 |
| FR-CHG-013 | Administrators can configure the change form layout (sections, field order, visibility). | C | P4 |
| FR-CHG-014 | Interested parties (requester, approver, CAB, assignee) are notified on submission, approval, rejection, schedule and completion. | M | P2 |

## 10. Tasks, Checklists & Projects (TSK)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-TSK-001 | Analysts create **Tasks** (title, description, status, priority, start and due date, estimate) assigned to an Analyst, Team, Supplier or Contact. Assigning an Analyst notifies them. Supplier and Contact assignees follow the Contracts context (FR-CON-001) (ADR-0029). | M | P2 |
| FR-TSK-002 | Task Statuses and Priorities are configurable. | M | P2 |
| FR-TSK-003 | Tasks support **Subtasks**, **Tags**, **Collaborators**, comments, time entries and an audit trail. Deviation from FreeITSM: one level of Subtasks (ADR-0029). | S | P2 |
| FR-TSK-004 | Tasks can be linked to a ticket, change, contract or project. Contract links follow the Contracts context, project links Projects (FR-TSK-009) (ADR-0029). | S | P2 |
| FR-TSK-005 | **Board** (Kanban) view with drag-and-drop between statuses, plus a list view. | S | P2 |
| FR-TSK-006 | **Recurrence**: daily, weekly (weekdays), monthly (date or nth weekday), yearly. Repeats on completion or on a fixed schedule, with an end condition. Occurrences are real Tasks; only completing (a Done status) repeats, cancelling does not. | S | P2 |
| FR-TSK-007 | Knowledge Articles can be attached as **Procedures** (snapshotted at attach time). | C | P3 |
| FR-TSK-008 | **Checklist Templates** (versioned, with category and role) contain items that can be mandatory or require input. They can be applied to tasks and tickets. | S | P3 |
| FR-TSK-009 | **Projects** built on tasks: method (Simple, Staged, Agile), stages with gates, milestones, RAID log, decision log, budget. | W | — |

## 11. Knowledge (KNW)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-KNW-001 | Analysts create and edit **Articles** (title, rich HTML body, tags, folder, owner, review date). HTML is sanitised. | M | P2 |
| FR-KNW-002 | Each save creates an immutable **Article Version**. Previous versions can be viewed and restored. | M | P2 |
| FR-KNW-003 | Articles can be published or unpublished, and archived. Deviation from FreeITSM: new Articles start unpublished, archiving also unpublishes, and restoring brings an Article back unpublished (ADR-0028). | M | P2 |
| FR-KNW-004 | Each Article has an **Audience** (`internal`, `customer`, `public`) and an optional Company (empty means shared with all Companies). Only callers who see every Company create shared content. | M | P2 |
| FR-KNW-005 | **Folders** form a tree. Folders and Articles can be restricted through ACLs for analysts, teams, requesters or user groups, with permission inheritance. Open nodes deny the named, Restricted nodes admit only the named; every Folder above an Article must admit the reader; `knowledge.manage` is the audited administrator floor. Deviation from FreeITSM: only the `containers` permission model (no install-wide `filing` switch) (ADR-0028). | S | P2 |
| FR-KNW-006 | Full-text search across Articles that respects permissions. | M | P2 |
| FR-KNW-007 | Articles past their Review Date are listed for their Owner and notified. | S | P2 |
| FR-KNW-008 | Analysts can link Articles to tickets and insert them into replies. | S | P2 |
| FR-KNW-009 | The knowledge audit records create, edit, view, delete, restore, move, permission changes and admin override. View counts are tracked. | S | P2 |
| FR-KNW-010 | **Knowledge Gap** analysis clusters closed tickets that no Article covers. | C | P4 |

## 12. Service Status (SST)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-SST-001 | Administrators manage the **Services** shown on the status page (name, description, order, active). A Service belongs to one Company or is shared with every Company; only callers who see every Company manage shared Services. Deviation from FreeITSM: Services are deactivated, never deleted, so incident history keeps its Services (ADR-0030). | M | P2 |
| FR-SST-002 | **Impact Levels** are configurable, each with severity order and *counts as downtime*. Seeded with Major Outage, Partial Outage, Degraded, Maintenance, Operational (default), No Disruption. Deviation from FreeITSM: severity orders are unique, so "worst" always has one answer, and the default (the all-clear) is active and never counts as downtime. | M | P2 |
| FR-SST-003 | **Incident Statuses** are configurable, with an `is_resolved` flag. Seeded with Investigating (default), Identified, Monitoring, 3rd Party, Resolved. The default is active and not resolved. | M | P2 |
| FR-SST-004 | Analysts open a **Service Incident** that affects one or more Services, each with an Impact Level. The Incident takes the Company of its Services, which are all of one Company or all shared. Opening it records the first Incident Update. Deviation from FreeITSM: an Incident cannot mix Companies, and Incidents are never deleted (they are the availability record) (ADR-0030). | M | P2 |
| FR-SST-005 | Analysts post **Incident Updates** (internal by default, or public). Each update snapshots the status and the impact on each service. An update is only recorded when it says something new (a status, impact or text change). Entering a resolved status stamps the resolution time; leaving it clears it. The wording and visibility of a posted update can be corrected in place; its status and impact snapshot cannot. | M | P2 |
| FR-SST-006 | A Service's current status is the worst Impact Level among its unresolved incidents, or Operational (the default Impact Level). Deviation from FreeITSM: computed in one place on the server (FreeITSM computes it again in each page's JavaScript). | M | P2 |
| FR-SST-007 | **Planned Maintenance** with planned start and end and affected Services. State moves scheduled → started → finished (or cancelled). When it starts, it becomes an Incident. The `planned-maintenance` job starts it at its planned start (the Incident is backdated to it, with a public update) and finishes it at its planned end (resolving the Incident unless someone already did); Analysts can also start or finish it by hand. Only scheduled maintenance can be edited or cancelled. Deviation from FreeITSM: the planned end is required, and the started Incident's status is the first open non-default Incident Status in order (no name is hard-coded) (ADR-0030). | S | P2 |
| FR-SST-008 | Availability reporting per Service, based on *counts as downtime*. | S | P4 |
| FR-SST-009 | Optional public (unauthenticated) status page. Off by default (`service_status.public_page`). It shows only shared Services, their current status, Planned Maintenance and, when `service_status.show_incidents` is on, Incidents that have a public update with their public updates (never authors or internal updates). It reads with an empty Company scope, so Row-Level Security keeps every Company's own data out (ADR-0030). | C | P2 |

## 13. Morning Checks (MCK)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-MCK-001 | Administrators manage **Check Groups** (assigned Team or Analyst) and **Checks** (name, group, assigned Analyst, order), per Company. A Check is routed to its own Analyst, else its group's Analyst, else its group's Team. Groups and Checks are deactivated, never deleted; an inactive group takes its Checks out of the Round. Deviation from FreeITSM: Checks and groups belong to a Company; a group routes to a Team *or* an Analyst, not both; nothing is deleted, so history keeps its Checks (ADR-0031). | M | P2 |
| FR-MCK-002 | **Check Statuses** are configurable, each with a colour and *requires notes*. Seeded with Green, Amber (notes required), Red (notes required). Deviation from FreeITSM: statuses are deactivated, never deleted, so results never lose their status (no orphaned labels to normalise). | M | P2 |
| FR-MCK-003 | For a **Check Date** (default today), Analysts record one **Check Result** per Check: a status, plus notes where required. The result is unique per check per date. Edits record who modified it, and a result can be cleared (the Check is unrecorded again; its Result Links go, the tickets and Tasks stay). Check Dates are calendar days in the instance Time Zone (`general.time_zone`). Deviation from FreeITSM: the date is never the server's local day, never after today, only active statuses and Checks in the Round can be recorded, and two Analysts recording at once cannot create two results (ADR-0031). | M | P2 |
| FR-MCK-004 | From a result, an Analyst can create or link a follow-up ticket or task (**Result Link**). A follow-up Task can be created from the result in one step (routed like the Check, carrying its notes); tickets and Tasks are linked through their contexts' APIs, of the result's Company only, and links to records the caller cannot see are hidden. A ticket's related records list the results that link to it. Deviation from FreeITSM: the target must exist, be visible and be of the same Company (FreeITSM stores any ID it is given). | S | P2 |
| FR-MCK-005 | History view: a grid of checks × dates (at most 92 days), colour-coded. Results can be exported (CSV, at most 366 days; cells a spreadsheet would run as formulas are defused). Deviation from FreeITSM: the export is CSV rather than a printed PDF, and the grid shows results under inactive statuses too (FreeITSM's 30-day chart drops them). | S | P2 |
| FR-MCK-006 | Notify the assigned Analyst or Team when checks are still unrecorded at a configurable time (`morning_checks.reminders`, `morning_checks.reminder_time`, `morning_checks.reminder_weekends`). The `morning-check-reminders` job sends one reminder per Check Date, Check Group and Analyst (every active member for a Team), never twice, and none when the Checks were recorded before it is delivered. | C | P2 |

## 14. Assets & Software (AST)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-AST-001 | Analysts manage **Assets**: hostname, Service Tag, Asset Tag (unique per Company), Asset Type, Asset Status, Location, Supplier, Cost Centre, purchase date and cost, warranty expiry, lease start and expiry. | M | P3 |
| FR-AST-002 | Asset Types, Asset Statuses (with `is_retired`) and Locations (tree) are configurable. | M | P3 |
| FR-AST-003 | Custom **Asset Fields** are grouped into **Field Sets** assigned to Asset Types. Fields can be required, unique, searchable or shown in lists. | S | P3 |
| FR-AST-004 | **Check Out / Check In** of an Asset to a Holder (Requester or Analyst), with a logged history. | M | P3 |
| FR-AST-005 | **Retire** an Asset, with a date and reason. Retired assets are excluded from active counts. | M | P3 |
| FR-AST-006 | Full asset history (audit) and links to tickets, contracts and CIs. | M | P3 |
| FR-AST-007 | **Asset Import** from CSV or API, with column mappings, preview mode, a write mode (fill or overwrite), handling for missing assets (ignore, flag, deactivate) and a per-row outcome log. | S | P3 |
| FR-AST-008 | **Inventory** push API for an agent or script to report hardware, disks, network adapters and installed software. | S | P3 |
| FR-AST-009 | Inventory connectors: Microsoft Intune, Proxmox, VMware vCenter / Cloud Director, Hyper-V. | C | P4 |
| FR-AST-010 | **Reconciliation**: possible duplicates or conflicts become Review Items (open, merged, different, ignored, resolved). **Protected Fields** are never overwritten by imports. | C | P4 |
| FR-AST-011 | Print **Asset Labels** with QR codes. Scanning a code opens the Asset. | C | P4 |
| FR-AST-012 | Warranty and lease expiry create Calendar Events and notifications ahead of time. The Calendar Events come from `asset.dates_changed` (FR-CAL-002, ADR-0034). | S | P3 |
| FR-AST-013 | **Software** inventory: distinct Applications (name, publisher) and Installations per Asset with version. | S | P3 |
| FR-AST-014 | **Licences** (type, key, quantity, renewal date, notice period, cost, currency, status), with compliance shown as installations vs. quantity. | S | P3 |
| FR-AST-015 | Asset and software dashboards (by type, status, OS, manufacturer, model; top installed; publisher distribution). | C | P4 |

## 15. CMDB (CMD)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-CMD-001 | Administrators define **CMDB Classes** with typed properties (text, number, date, boolean, dropdown, object_ref). | S | P3 |
| FR-CMD-002 | Analysts manage **CIs** (class, name, properties, parent, Company, planned flag). | S | P3 |
| FR-CMD-003 | **Relationship Types** (verb, inverse verb, Impact Direction) and **Relationships** between CIs. | S | P3 |
| FR-CMD-004 | Impact analysis: given a CI, list the CIs and Services affected, following the Impact Directions. | S | P3 |
| FR-CMD-005 | Link Assets to CIs (**Asset–CI Link**, time-bounded). Link CIs to tickets and changes. | S | P3 |
| FR-CMD-006 | Visual relationship graph of a CI. | C | P4 |

## 16. Contracts & Suppliers (CON)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-CON-001 | Manage **Suppliers** (legal name, trading name, registration number, VAT number, type, status) and their **Contacts**. | M | P3 |
| FR-CON-002 | Manage **Contracts**: number, title, supplier (or customer Company/Requester), owner, status, start and end, notice period, value, currency, payment schedule, cost centre, DMS link. | M | P3 |
| FR-CON-003 | Cooper calculates the **Notice Date**. Calendar Events and notifications are generated ahead of the notice date and end date. The Calendar Events come from `contract.dates_changed` (FR-CAL-002, ADR-0034). | M | P3 |
| FR-CON-004 | Data protection fields: personal data transferred, DPIA required, DPIA completed date and link. | S | P3 |
| FR-CON-005 | Configurable **Contract Terms** tabs with values per contract. | C | P3 |
| FR-CON-006 | Link Contracts to Assets and Tasks. | S | P3 |
| FR-CON-007 | **RFP** builder (documents, requirements extraction, supplier scoring). | W | — |

## 17. Calendar (CAL)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-CAL-001 | An IT **Calendar** with manual events and colour-coded **Calendar Categories**. Each Calendar Event belongs to a Company and has a title, description, category, optional location, and either whole days (start and end date) or a start and end time; the end is never before the start. Calendar Categories have a unique name, a `#rrggbb` colour and an active flag; only active, non-system categories take new events; no categories are seeded. Analysts list the events of a range of days (at most 366) of the Companies they see, optionally for one Company or category, and view the Calendar a month at a time. Deviation from FreeITSM: events belong to a Company; the end must not be before the start; all-day events are dates and timed events UTC instants, never server-local times (ADR-0034). | M | P3 |
| FR-CAL-002 | The calendar shows generated events: change windows, planned maintenance, ticket scheduled work, task due dates, warranty, lease and contract dates. Contract Notice Dates and end dates, and asset warranty and lease expiry, are generated from the Contracts and Assets contexts' `contract.dates_changed` and `asset.dates_changed` events: one all-day event per date, in a system Calendar Category per source, removed when the date is cleared or the record is inactive. Generated events are read-only and link to their source (ADR-0034). Change windows, planned maintenance, scheduled work and task due dates are not generated yet. | S | P3 |
| FR-CAL-003 | Analysts can **Enrol** to sync their ticket work and task dates to Microsoft 365 or Google calendars (push), or subscribe to an iCal feed. | C | P4 |

## 18. Forms (FRM)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-FRM-001 | Administrators build **Forms** with typed **Form Fields** (label, options, required, layout). Forms are versioned. | S | P3 |
| FR-FRM-002 | Forms can be Portal-visible and limited to audiences (users, groups, departments). | S | P3 |
| FR-FRM-003 | Submitting a Form runs **Submission Actions**: create a ticket and/or task with mapped values. | S | P3 |
| FR-FRM-004 | Optional approval: an Approver or approval Team, with required approval and rejection counts, an option to exclude the Requester, and approval rounds. | S | P3 |
| FR-FRM-005 | Requesters can save **Drafts**. | C | P3 |
| FR-FRM-006 | **Form Collections**: time-boxed campaigns of submissions. | C | P4 |

## 19. Workflow, Webhooks & Integrations (WFL)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-WFL-001 | Administrators define **Workflows**: Trigger Event (for example `ticket.created`, `ticket.updated`, `sla.breached`, `change.approved`), Conditions (field, operator, value) and ordered Actions (set field, assign, add note, send email, call webhook, create task). | S | P4 |
| FR-WFL-002 | Workflows are scoped to a Company and can be enabled or disabled. | S | P4 |
| FR-WFL-003 | Each run is recorded as a **Workflow Execution** with status and step log. Workflows can be **Dry Run**. Recursive triggering is prevented. | S | P4 |
| FR-WFL-004 | Time-based triggers fire exactly once per record. | S | P4 |
| FR-WFL-005 | Outbound **Webhooks** with formats (generic JSON, Slack, Teams, Discord). Retries with backoff (up to 6 attempts), then `dead`. Delivery history is visible. | S | P4 |
| FR-WFL-006 | **Integration Links** to Jira, GitHub, GitLab and Azure DevOps issues, with status sync. | C | P4 |

## 20. Reporting & Dashboards (RPT)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-RPT-001 | Ticket dashboard widgets: by status, priority, department, type, analyst, origin; first time fix rate; created and closed per day and month; created vs closed. Counts also by Queue and Company, optionally only open or closed tickets and only those created in a period. Days and months are calendar days in the instance Time Zone (`general.time_zone`). Tickets the caller cannot open (other Companies, Confidential tickets outside their Departments) are never counted; deleted and merged-away tickets are left out. Deviation from FreeITSM: buckets follow the instance time zone instead of UTC; the First Time Fix Rate counts only tickets closed in the period, with no rate when none were closed; widget layouts are not stored (the UI arranges the widgets) (ADR-0032). | M | P2 |
| FR-RPT-002 | SLA compliance report (response and resolution met or breached, by priority, team and period). Covers tickets created in the period; each ticket counts in the SLA state its own page shows (met, met late, breached, running, no target), and compliance is met ÷ (met + breached), with no value when nothing is decided. Also by Company. Deviation from FreeITSM: FreeITSM has no SLA compliance report; this one reuses the service desk's stored due times and never recomputes SLA time (ADR-0032). | M | P2 |
| FR-RPT-003 | **Watchtower**: configurable overview tiles counting selected ticket statuses and priorities, task statuses and morning check statuses. **Watchtower Items** choose the open statuses counted one by one (default: every active open status; the total is the sum of the rows), the high Priorities (default: those ranked after the default Priority) and the Check Statuses that need attention (default: all but the first); unassigned tickets, overdue Tasks and Tasks due today are counted too. Scope *all* or *mine*. A section is left out when its Module is off or the caller may not read it. Deviation from FreeITSM: one capability (`reporting.configure`) for the Watchtower Items; card layout is the UI's; tiles for Modules Cooper does not have yet (Projects, Contracts, Assets, …) follow those Modules (ADR-0032). | S | P2 |
| FR-RPT-004 | Change, problem, asset, morning check and service availability reports. | S | P4 |
| FR-RPT-005 | Export any report or list to CSV. Every report has an `…/export` with the same parameters; the ticket list exports with the filters of `GET /tickets` (at most 10,000 tickets); Morning Check history already exports (FR-MCK-005). Cells are RFC 4180 and protected against formula injection (`cooper_http::csv`). Exports of other lists follow their Modules. | M | P2 |
| FR-RPT-006 | **Report Packs**: saved report designs that can be shared with Analysts or Teams. | C | P4 |
| FR-RPT-007 | Time-entry report per Analyst, Team, Company and period. Adds ticket and Task Time Entries by work date (Team: the ticket's Queue or the Task's Team); Task time only with `tasks.read` and the Tasks Module on. Time on tickets the caller cannot open is not counted (ADR-0032). | S | P2 |

## 21. AI Features (AI)

All AI features are optional and use a provider and key that the Administrator configures.

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-AI-001 | Generate a versioned **AI Summary** of a ticket. | C | P4 |
| FR-AI-002 | **Knowledge Q&A**: answer questions from Articles (embeddings in PostgreSQL / pgvector), respecting audience and ACLs. | C | P4 |
| FR-AI-003 | Suggest replies, clean up reply text, and draft root cause analyses for Problems. | C | P4 |

## 22. System Administration (SYS)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-SYS-001 | Administrators manage System Settings, Modules (availability) and all reference data (statuses, priorities, types, …) through the UI. | M | P1 |
| FR-SYS-002 | **Background Jobs** (mailbox polling, SLA, imports, syncs, recurrence) have schedules (interval, daily, off) and run history with outcome and duration. Each job runs on only one worker at a time. | M | P1 |
| FR-SYS-003 | System log viewer (errors, warnings, integration failures). | S | P1 |
| FR-SYS-004 | First-run setup creates the first Administrator, the default Company and seed reference data. | M | P1 |
| FR-SYS-005 | Optional demo data loader for evaluation. | C | P4 |
| FR-SYS-006 | Health and readiness endpoints for operations. | M | P1 |
| FR-SYS-007 | **Import from FreeITSM**: a one-way, re-runnable importer reads a FreeITSM MySQL database, its attachment folders and (optionally) its encryption key file, and writes Cooper data through application services, with a dry-run mode and a report. See ADR-0008. | S | P4 |

## 23. REST API (API)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-API-001 | Every UI capability is available through a versioned REST/JSON API (`/api/v1`). | M | P1 |
| FR-API-002 | **API Keys**, which Administrators manage: prefix plus hash storage, the full key shown once, per-key Capabilities, allowed Companies, rate limit per minute, expiry, last-used timestamp. Deviation from FreeITSM: no plaintext key table (ADR-0013). | M | P1 |
| FR-API-003 | The API is documented with OpenAPI. | S | P1 |
| FR-API-004 | Errors use `application/problem+json`. | M | P1 |

---

## Appendix A — Key non-functional expectations

These are not functional requirements, but they constrain the design:

- **Performance:** P95 < 300 ms for ticket list and detail endpoints with 1M tickets.
- **Availability:** stateless `cooper-server` instances that can scale horizontally. Background
  jobs are coordinated through PostgreSQL (advisory locks / `SKIP LOCKED`).
- **Security:** OWASP ASVS L2 baseline, tenant isolation tests, secrets encrypted at rest.
- **Data:** PostgreSQL 18, forward-only migrations, point-in-time recovery supported.
- **Accessibility:** UI targets WCAG 2.1 AA.

## Appendix B — Open questions

Resolved questions stay listed with their answer.

1. ~~Exact FreeITSM seed values~~ **Resolved:** confirmed against FreeITSM 4.9.0 and
   recorded in the Vocabulary. Evidence and FreeITSM bugs found:
   [research/freeitsm-schema-and-seeds.md](research/freeitsm-schema-and-seeds.md).
   FreeITSM ships no seeds for Asset Types/Statuses or Contract/Supplier lookups.
2. ~~Migration path from FreeITSM~~ **Resolved:** yes, as a one-way importer
   (FR-SYS-007, [ADR-0008](adr/0008-freeitsm-database-import.md)).
3. ~~UI technology~~ **Resolved:** a TypeScript React SPA served by `cooper-server`,
   talking only to `/api/v1`
   ([ADR-0010](adr/0010-typescript-react-spa-for-analyst-console-and-portal.md), which
   supersedes [ADR-0003](adr/0003-server-rendered-ui-with-askama-and-htmx.md)). The public
   status page stays server-rendered.
