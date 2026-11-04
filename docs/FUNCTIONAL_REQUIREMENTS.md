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
| FR-GEN-005 | Each Module can be turned on or off. A disabled module is hidden from navigation and its API returns 404. | S | P1 |
| FR-GEN-006 | Users receive in-app **Notifications** (the bell) for assignments, mentions, replies and SLA events. | S | P1 |
| FR-GEN-007 | Attachments can be uploaded to tickets, changes, problems, tasks, articles and contracts, with a configurable size limit. | M | P1 |
| FR-GEN-008 | Users can save **Table Views** (columns, filters, sort) per list. | C | P4 |
| FR-GEN-009 | The UI and email texts support localisation. English is the default. | C | P4 |

## 2. Identity, Access & Tenancy (IAM)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-IAM-001 | Administrators can create, update and deactivate **Analysts** (name, email, username, admin flag, active flag). | M | P1 |
| FR-IAM-002 | Analysts sign in with username/email and password (Argon2id). Sessions expire after a configurable period. | M | P1 |
| FR-IAM-003 | Analysts can turn on **TOTP two-factor authentication**. Administrators can require it. Trusted devices can be remembered. | S | P1 |
| FR-IAM-004 | Password reset by emailed one-time token. | M | P1 |
| FR-IAM-005 | Repeated failed logins from an IP address cause a temporary ban. | S | P1 |
| FR-IAM-006 | Administrators manage **Teams** and Team membership. | M | P1 |
| FR-IAM-007 | Administrators manage **Departments** (name, default Sensitivity) and link them to the Teams that serve them. | M | P1 |
| FR-IAM-008 | Administrators manage **Companies** (name, Ticket Code, Sender Domains). A "Default" company exists on install. | M | P1 |
| FR-IAM-009 | Administrators grant Analysts or Teams access to specific Companies or to all Companies. | M | P1 |
| FR-IAM-010 | RBAC: Administrators define **Roles** as sets of **Capabilities** (`<module>.<action>`) and assign them to Analysts or Teams. Every API endpoint checks a Capability. | M | P1 |
| FR-IAM-011 | Administrators and Directory Sync manage **Requesters** (name, email, username, Manager, Cost Centre, Company, active flag). Deactivation is recorded with a timestamp. | M | P1 |
| FR-IAM-012 | SSO for Analysts and Requesters through **OIDC**. | S | P2 |
| FR-IAM-013 | LDAP/Active Directory authentication and **Directory Sync** of Requesters and groups, with run history (running/ok/stopped/failed). | S | P3 |
| FR-IAM-014 | SSO group → Team mapping. | C | P3 |
| FR-IAM-015 | **Manager Grants** give a Requester visibility of other Requesters' tickets (everyone, user, group, department, reports). | S | P2 |
| FR-IAM-016 | **User Groups** of Analysts and/or Requesters, with optional membership expiry. | S | P2 |

## 3. Tickets (TKT)

### 3.1 Lifecycle

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-TKT-001 | An Analyst can create a **Ticket** with subject, description, Requester, Department, Ticket Type, Category, Priority, Origin, Queue (Team), Assignee and Sensitivity. | M | P1 |
| FR-TKT-002 | Each ticket gets a unique **Ticket Number** from a configurable format (prefix per department or type, `{COMPANY}` token, zero-padded sequence). | M | P1 |
| FR-TKT-003 | When the number format changes or a ticket is renumbered, previous numbers are kept in Ticket Number History and still resolve to the ticket. | S | P2 |
| FR-TKT-004 | **Ticket Statuses** are configurable, each with `is_closed`, `pauses_sla` and `is_default` flags. Seeded with Open (default), In Progress, On Hold (pauses SLA), Awaiting Response (pauses SLA), Closed. Exactly one status is the default. Deviation from FreeITSM: the seed always sets `pauses_sla` (FreeITSM's verification seeder omits it, so those installs never pause the SLA). | M | P1 |
| FR-TKT-005 | **Ticket Priorities** are configurable, each with SLA response minutes, resolution minutes and SLA Calendar. Seeded with Low, Normal (default), High, Critical, Urgent. | M | P1 |
| FR-TKT-006 | **Ticket Types**, **Origins** and **Resolution Codes** are configurable, with the FreeITSM seeds in the Vocabulary. Seeded rows have a stable `key` that code uses instead of the name. Deviation from FreeITSM: FreeITSM finds channel Origins by English name, so renaming one breaks channel intake. | M | P1 |
| FR-TKT-007 | **Categories** are configurable as a tree of at most 3 levels. A category can be limited to Ticket Types and marked visible on the Portal. | M | P1 |
| FR-TKT-008 | Closing a ticket (moving it to a closed status) records the closed timestamp. It requires a **Resolution Code**, and a **Closure Category** when configured. It also requires every Ticket Field marked *required on close*. | M | P1 |
| FR-TKT-009 | A closed ticket can be reopened. Reopening clears the closed timestamp and is audited. A Requester reply to a closed ticket reopens it or creates a new linked ticket (configurable). | M | P1 |
| FR-TKT-010 | A ticket can be assigned to a Team (Queue) and/or an Analyst. The Analyst must belong to the Team when both are set (configurable). Assignment changes notify the new assignee. | M | P1 |
| FR-TKT-011 | **Ticket Type Rules** set the default Ticket Type from channel, mailbox, origin, department, subject keywords, requester domain or company. Rules are evaluated in order. | S | P2 |
| FR-TKT-012 | Analysts can **Snooze** a ticket until a date and time, with a reason. Snoozed tickets are hidden from default views and reappear automatically. | S | P2 |
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
| FR-TKT-025 | **Ticket Presence**: the ticket shows which other Analysts are viewing or replying to it, live. | C | P2 |
| FR-TKT-026 | **Merge** a ticket into another: emails, notes and attachments move, and the source ticket is closed and points to the target. Merge can be undone. | S | P2 |
| FR-TKT-027 | **Split** selected emails or notes into a new ticket. Split can be undone. | C | P2 |
| FR-TKT-028 | **Ticket Links** between tickets: related, duplicate, parent/child. | S | P2 |
| FR-TKT-029 | Administrators define custom **Ticket Fields** (typed: text, number, date, dropdown, checkbox, …) with *required on create*, *required on close* and *ask requester* options. | S | P2 |
| FR-TKT-030 | Tickets can be linked to Assets, CIs, Problems, Changes and Tasks. | S | P2 |
| FR-TKT-031 | Analysts can schedule work on a ticket (start, end, all-day) for an **Owner**. The work appears on the Calendar. | C | P3 |
| FR-TKT-032 | **Checklists** from templates can be attached to a ticket. The template's Closure Mode decides whether incomplete mandatory items block closure. | C | P3 |
| FR-TKT-033 | After closure, the Requester receives a **CSAT** request. Ratings are stored per ticket and reported. | C | P4 |

### 3.3 Views

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-TKT-040 | Built-in views: My tickets, My team's tickets (Queue), Unassigned, All open, Snoozed, Recently closed. | M | P1 |
| FR-TKT-041 | Ticket search by number (current or historical), subject, body, requester, email address and extracted attachment text. | M | P1 |
| FR-TKT-042 | Bulk actions on selected tickets: assign, change status or priority, merge, close. | S | P2 |

## 4. Mailboxes & Email Intake (EML)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-EML-001 | Administrators configure **Mailboxes**: provider `microsoft` (Graph, auth mode `delegated` or `app_only`), `google` or `imap`/`smtp`, with default Department, Origin, Company and Team. | M | P1 |
| FR-EML-002 | A background job polls each active Mailbox on a configurable interval and imports new emails. | M | P1 |
| FR-EML-003 | An inbound email whose subject contains a valid **SDREF** tag (or matches a known thread or message id) is added to the existing ticket. Otherwise it creates a new ticket. | M | P1 |
| FR-EML-004 | The Requester is matched by sender address. Unknown senders create a Requester (configurable) or are rejected. The Company is set from **Sender Domains**. **Freemail Domains** never set a Company. | M | P1 |
| FR-EML-005 | The **Mailbox Whitelist** allows or blocks senders by address or domain. Rejected emails are deleted, moved or kept (configurable) and logged. | S | P1 |
| FR-EML-006 | After import, the mailbox message is deleted, moved or marked read (configurable). | M | P1 |
| FR-EML-007 | Attachments and inline images are stored. Text is extracted asynchronously for search, with extraction status recorded. | S | P2 |
| FR-EML-008 | Every outbound send attempt is logged (success or failure, error). Failed sends are retried. | M | P1 |
| FR-EML-009 | **Email Templates** send automated emails on events (ticket created, assigned, replied, closed), matched by recipient address or domain. | S | P1 |
| FR-EML-010 | Mailbox health (last poll, last error) is visible to Administrators and raises a notification on failure. | S | P1 |
| FR-EML-011 | Loop protection: auto-replies, bounces and Cooper's own outbound mail do not create tickets. | M | P1 |
| FR-EML-012 | OAuth tokens and mailbox passwords are encrypted at rest. | M | P1 |

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
| FR-SLA-003 | SLA time pauses while the ticket is in a status with `pauses_sla` and resumes when it leaves that status. Due times shift by the time spent paused. | M | P1 |
| FR-SLA-004 | The first Analyst reply (or note visible to the Requester) satisfies the Response target. Closure satisfies the Resolution target. | M | P1 |
| FR-SLA-005 | A background job raises **SLA Warning** (configurable threshold) and **SLA Breach** events. Each event is raised at most once per ticket, target and trigger. | M | P1 |
| FR-SLA-006 | **SLA Notification Rules** decide who is notified (assignee, team, specific Analysts) for warning or breach, for response, resolution or both. | S | P1 |
| FR-SLA-007 | Ticket lists show SLA state (on track, warning, breached, paused) and time remaining. | M | P1 |
| FR-SLA-008 | **Rota**: Administrators define Shifts and Rota Locations (Office, WFH). Analysts are scheduled through Rota Entries, which can be flagged On Call. A rota view shows who is working today. | C | P3 |

## 7. Self-Service Portal (PRT)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-PRT-001 | Requesters sign in to the Portal (local password, SSO, or Self-Registration with email confirmation). | M | P1 |
| FR-PRT-002 | Requesters raise tickets by choosing from the **Portal Catalogue** (portal-visible Categories and Forms) and filling in any *ask requester* Ticket Fields. | M | P1 |
| FR-PRT-003 | Requesters see their own tickets (open and closed), read the public timeline (replies and public notes), reply and attach files. | M | P1 |
| FR-PRT-004 | Managers see tickets of Requesters covered by their Manager Grants. | S | P2 |
| FR-PRT-005 | Requesters search and read Knowledge Articles with `customer` or `public` audience that they are allowed to see. | S | P2 |
| FR-PRT-006 | The Portal shows current **Service Status** and Planned Maintenance. | S | P2 |
| FR-PRT-007 | Requesters receive Portal notifications and can set notification preferences. | C | P2 |

## 8. Problem Management (PRB)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-PRB-001 | Analysts create a **Problem** (Problem Number, title, description, status, priority, assigned Analyst). | M | P2 |
| FR-PRB-002 | Problem Statuses and Priorities are configurable. | M | P2 |
| FR-PRB-003 | One or more incident tickets can be linked to a Problem. The link is visible from both sides. | M | P2 |
| FR-PRB-004 | Analysts record a **Root Cause** and a **Workaround**, and can flag the Problem as a **Known Error**. | M | P2 |
| FR-PRB-005 | A Known Error's Workaround can be published as a Knowledge Article. | S | P2 |
| FR-PRB-006 | When a Problem is resolved, linked tickets can be bulk-updated or notified. | C | P2 |
| FR-PRB-007 | Problems have notes and a full audit trail. | M | P2 |

## 9. Change Management (CHG)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-CHG-001 | Analysts create a **Change** with title, Change Type, Priority, Impact, Category, Requester, Assigned To, Approver, Work Window, Outage Window, description, Reason for Change, Risk Evaluation, Test Plan and Rollback Plan. | M | P2 |
| FR-CHG-002 | Change Types are configurable, each with an Approval Route (`review` or `emergency`). Seeded with Standard, Normal (default), Emergency. Deviation from FreeITSM: the seeded Emergency type uses the `emergency` route (FreeITSM seeds it with `review`, so emergency changes wait for approval). | M | P2 |
| FR-CHG-003 | Change Statuses are configurable. Each maps to a fixed **Stage** (draft, awaiting, approved, implementing, done, stopped) and has an `is_closed` flag. Seeded with the 10 FreeITSM statuses. | M | P2 |
| FR-CHG-004 | Status transitions follow the stage order. Changes in `draft` are editable. Moving to `awaiting` submits the Change for approval. Moving to `implementing` requires approval, except for **Standard** changes. | M | P2 |
| FR-CHG-005 | Risk assessment: Risk Likelihood (1–5) × Risk Impact Score (1–5) gives a **Risk Score**, mapped to a **Risk Level** through configurable thresholds (seed: Low ≤ 4, Medium 5–9, High 10–15, Very High 16–20, Critical 21–25). Deviation from FreeITSM: thresholds are reference data, computed in one place (FreeITSM hard-codes them in PHP and again in JavaScript). | S | P2 |
| FR-CHG-006 | **CAB**: when `cab_required`, CAB Members (some required) vote approve or reject with a comment. The **CAB Approval Type** (`all` or `majority` of required members) decides the outcome; one rejection sends the Change back. | M | P2 |
| FR-CHG-007 | Votes are append-only. Editing an approved Change's material content starts a new **Approval Round** and invalidates earlier approval. | M | P2 |
| FR-CHG-008 | Changes have a checklist, comments, attachments, links to tickets, problems, CIs and other changes, and a full audit trail. | S | P2 |
| FR-CHG-009 | Every saved version of long-text fields is kept as a **Text Revision** and can be compared. | S | P2 |
| FR-CHG-010 | **Change Templates** (versioned, limited to Teams) pre-fill new Changes. | S | P2 |
| FR-CHG-011 | After completion or failure, a **PIR** is recorded: successful (yes/no), actual start and end, lessons learned, follow-up. | S | P2 |
| FR-CHG-012 | A **Change Calendar** shows Changes by Work and Outage Windows and highlights overlaps. | S | P2 |
| FR-CHG-013 | Administrators can configure the change form layout (sections, field order, visibility). | C | P4 |
| FR-CHG-014 | Interested parties (requester, approver, CAB, assignee) are notified on submission, approval, rejection, schedule and completion. | M | P2 |

## 10. Tasks, Checklists & Projects (TSK)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-TSK-001 | Analysts create **Tasks** (title, description, status, priority, start and due date, estimate) assigned to an Analyst, Team, Supplier or Contact. | M | P2 |
| FR-TSK-002 | Task Statuses and Priorities are configurable. | M | P2 |
| FR-TSK-003 | Tasks support **Subtasks**, **Tags**, **Collaborators**, comments, time entries and an audit trail. | S | P2 |
| FR-TSK-004 | Tasks can be linked to a ticket, change, contract or project. | S | P2 |
| FR-TSK-005 | **Board** (Kanban) view with drag-and-drop between statuses, plus a list view. | S | P2 |
| FR-TSK-006 | **Recurrence**: daily, weekly (weekdays), monthly (date or nth weekday), yearly. Repeats on completion or on a fixed schedule, with an end condition. | S | P2 |
| FR-TSK-007 | Knowledge Articles can be attached as **Procedures** (snapshotted at attach time). | C | P3 |
| FR-TSK-008 | **Checklist Templates** (versioned, with category and role) contain items that can be mandatory or require input. They can be applied to tasks and tickets. | S | P3 |
| FR-TSK-009 | **Projects** built on tasks: method (Simple, Staged, Agile), stages with gates, milestones, RAID log, decision log, budget. | W | — |

## 11. Knowledge (KNW)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-KNW-001 | Analysts create and edit **Articles** (title, rich HTML body, tags, folder, owner, review date). HTML is sanitised. | M | P2 |
| FR-KNW-002 | Each save creates an immutable **Article Version**. Previous versions can be viewed and restored. | M | P2 |
| FR-KNW-003 | Articles can be published or unpublished, and archived. | M | P2 |
| FR-KNW-004 | Each Article has an **Audience** (`internal`, `customer`, `public`) and an optional Company (empty means shared with all Companies). | M | P2 |
| FR-KNW-005 | **Folders** form a tree. Folders and Articles can be restricted through ACLs for analysts, teams, requesters or user groups, with permission inheritance. | S | P2 |
| FR-KNW-006 | Full-text search across Articles that respects permissions. | M | P2 |
| FR-KNW-007 | Articles past their Review Date are listed for their Owner and notified. | S | P2 |
| FR-KNW-008 | Analysts can link Articles to tickets and insert them into replies. | S | P2 |
| FR-KNW-009 | The knowledge audit records create, edit, view, delete, restore, move, permission changes and admin override. View counts are tracked. | S | P2 |
| FR-KNW-010 | **Knowledge Gap** analysis clusters closed tickets that no Article covers. | C | P4 |

## 12. Service Status (SST)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-SST-001 | Administrators manage the **Services** shown on the status page. | M | P2 |
| FR-SST-002 | **Impact Levels** are configurable, each with severity order and *counts as downtime*. Seeded with Major Outage, Partial Outage, Degraded, Maintenance, Operational (default), No Disruption. | M | P2 |
| FR-SST-003 | **Incident Statuses** are configurable, with an `is_resolved` flag. Seeded with Investigating (default), Identified, Monitoring, 3rd Party, Resolved. | M | P2 |
| FR-SST-004 | Analysts open a **Service Incident** that affects one or more Services, each with an Impact Level. | M | P2 |
| FR-SST-005 | Analysts post **Incident Updates** (internal or public). Each update snapshots the status and the impact on each service. | M | P2 |
| FR-SST-006 | A Service's current status is the worst Impact Level among its unresolved incidents, or Operational. | M | P2 |
| FR-SST-007 | **Planned Maintenance** with planned start and end and affected Services. State moves scheduled → started → finished (or cancelled). When it starts, it becomes an Incident. | S | P2 |
| FR-SST-008 | Availability reporting per Service, based on *counts as downtime*. | S | P4 |
| FR-SST-009 | Optional public (unauthenticated) status page. | C | P2 |

## 13. Morning Checks (MCK)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-MCK-001 | Administrators manage **Check Groups** (assigned Team or Analyst) and **Checks** (name, group, assigned Analyst, order). | M | P2 |
| FR-MCK-002 | **Check Statuses** are configurable, each with a colour and *requires notes*. Seeded with Green, Amber (notes required), Red (notes required). | M | P2 |
| FR-MCK-003 | For a **Check Date** (default today), Analysts record one **Check Result** per Check: a status, plus notes where required. The result is unique per check per date. Edits record who modified it. | M | P2 |
| FR-MCK-004 | From a result, an Analyst can create or link a follow-up ticket or task (**Result Link**). | S | P2 |
| FR-MCK-005 | History view: a grid of checks × dates, colour-coded. Results can be exported. | S | P2 |
| FR-MCK-006 | Notify the assigned Analyst or Team when checks are still unrecorded at a configurable time. | C | P2 |

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
| FR-AST-012 | Warranty and lease expiry create Calendar Events and notifications ahead of time. | S | P3 |
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
| FR-CON-003 | Cooper calculates the **Notice Date**. Calendar Events and notifications are generated ahead of the notice date and end date. | M | P3 |
| FR-CON-004 | Data protection fields: personal data transferred, DPIA required, DPIA completed date and link. | S | P3 |
| FR-CON-005 | Configurable **Contract Terms** tabs with values per contract. | C | P3 |
| FR-CON-006 | Link Contracts to Assets and Tasks. | S | P3 |
| FR-CON-007 | **RFP** builder (documents, requirements extraction, supplier scoring). | W | — |

## 17. Calendar (CAL)

| ID | Requirement | Pri | Phase |
|---|---|---|---|
| FR-CAL-001 | An IT **Calendar** with manual events and colour-coded **Calendar Categories**. | M | P3 |
| FR-CAL-002 | The calendar shows generated events: change windows, planned maintenance, ticket scheduled work, task due dates, warranty, lease and contract dates. | S | P3 |
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
| FR-RPT-001 | Ticket dashboard widgets: by status, priority, department, type, analyst, origin; first time fix rate; created and closed per day and month; created vs closed. | M | P2 |
| FR-RPT-002 | SLA compliance report (response and resolution met or breached, by priority, team and period). | M | P2 |
| FR-RPT-003 | **Watchtower**: configurable overview tiles counting selected ticket statuses and priorities, task statuses and morning check statuses. | S | P2 |
| FR-RPT-004 | Change, problem, asset, morning check and service availability reports. | S | P4 |
| FR-RPT-005 | Export any report or list to CSV. | M | P2 |
| FR-RPT-006 | **Report Packs**: saved report designs that can be shared with Analysts or Teams. | C | P4 |
| FR-RPT-007 | Time-entry report per Analyst, Team, Company and period. | S | P2 |

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
| FR-API-002 | **API Keys**, which Administrators manage: prefix plus hash storage, the full key shown once, per-key Capabilities, allowed Companies, rate limit per minute, expiry, last-used timestamp. | M | P1 |
| FR-API-003 | The API is documented with OpenAPI. | S | P1 |
| FR-API-004 | Errors use `application/problem+json`. | M | P1 |

---

## Appendix A — Key non-functional expectations

These are not functional requirements, but they constrain the design:

- **Performance:** P95 < 300 ms for ticket list and detail endpoints with 1M tickets.
- **Availability:** stateless `cooper-server` instances that can scale horizontally. Background
  jobs are coordinated through PostgreSQL (advisory locks / `SKIP LOCKED`).
- **Security:** OWASP ASVS L2 baseline, tenant isolation tests, secrets encrypted at rest.
- **Data:** PostgreSQL 16+, forward-only migrations, point-in-time recovery supported.
- **Accessibility:** UI targets WCAG 2.1 AA.

## Appendix B — Open questions

Resolved questions stay listed with their answer.

1. ~~Exact FreeITSM seed values~~ **Resolved:** confirmed against FreeITSM 4.9.0 and
   recorded in the Vocabulary. Evidence and FreeITSM bugs found:
   [research/freeitsm-schema-and-seeds.md](research/freeitsm-schema-and-seeds.md).
   FreeITSM ships no seeds for Asset Types/Statuses or Contract/Supplier lookups.
2. ~~Migration path from FreeITSM~~ **Resolved:** yes, as a one-way importer
   (FR-SYS-007, [ADR-0008](adr/0008-freeitsm-database-import.md)).
3. ~~UI technology~~ **Resolved:** server-rendered with askama + htmx
   ([ADR-0003](adr/0003-server-rendered-ui-with-askama-and-htmx.md)).
