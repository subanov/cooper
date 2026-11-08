# Cooper Vocabulary (Ubiquitous Language)

This is the domain language of Cooper. It follows the terms used by FreeITSM. Use these terms
exactly in code, database, API, UI and conversation. Each section is one **bounded context**.

**Conventions**

- **Term** is the canonical name. *Code* is the Rust type or DB table name.
- **Avoid** lists synonyms that must not be used for this concept.
- Seed values marked **(seed)** are the defaults FreeITSM ships. Administrators can change
  them (they are reference data, not code). Values marked **(fixed)** are a closed set in
  the domain model.
- ⚠ means the exact FreeITSM value has not been confirmed yet. Confirm it against the
  FreeITSM schema before you implement it. Seed values were confirmed against FreeITSM
  4.9.0; see [research/freeitsm-schema-and-seeds.md](research/freeitsm-schema-and-seeds.md).

---

## 1. Shared Kernel & Identity

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Analyst** | A member of IT staff who signs in to Cooper to work tickets, changes, problems, tasks and so on. Can be an administrator. | `Analyst`, `analysts` | agent, technician, operator, engineer, staff user |
| **Administrator** | An Analyst with the `is_admin` flag. Has every Capability and sees every Company. At least one active Administrator must always exist. | `Analyst::is_admin` | superuser, root |
| **Requester** | A person who asks IT for help: the end user who raises tickets, usually through email or the Self-Service Portal. FreeITSM stores requesters in the `users` table; Cooper uses `requesters`. | `Requester`, `requesters` | customer, caller, client, contact (Contact means a supplier contact) |
| **Manager** | A Requester whom other Requesters report to (`manager_id`). Managers can see tickets of their reports, as granted by Manager Grants. | `Requester::manager_id` | supervisor |
| **Manager Grant** | A rule that gives a Requester visibility of other Requesters' tickets. Grant types (fixed): `everyone`, `user`, `group`, `department`, `reports`. | `ManagerGrant` | |
| **Team** | A group of Analysts. Tickets, tasks and checks are assigned to teams. A team is also a ticket **Queue**. | `Team`, `teams` | group (in the Analyst sense), squad |
| **Department** | The service area that a ticket belongs to (for example IT, HR, Facilities). Has a default Sensitivity. Linked to the Teams that serve it. | `Department`, `departments` | division |
| **Company** | A customer organisation served by this Cooper instance (multi-tenancy). Has a Ticket Code used in ticket numbers. Called **Company** in the UI and **Tenant** in code. | `Tenant`, `TenantId`, `tenants` | organisation, account, client |
| **Ticket Code** | A short code for a Company, used as the `{COMPANY}` token in ticket numbers. | `Tenant::ticket_code` | |
| **Cost Centre** | A financial unit within a Company that Requesters and Assets can belong to. Has a code unique within the Company. | `CostCentre`, `cost_centres` | |
| **Role** | A named set of Capabilities assigned to Analysts or Teams (RBAC). An Analyst's Capabilities are the union of their own Roles and their active Teams' Roles. | `Role`, `rbac_roles` | permission group |
| **Capability** | A permission string in the form `<module>.<action>`, for example `tickets.update`. | `Capability` | right, privilege |
| **Module** | A functional area of Cooper (fixed set): Tickets, Portal, Problems, Changes, Tasks, Assets, CMDB, Knowledge, Service Status, Morning Checks, Contracts, Forms, Calendar, Workflow, Reporting. An administrator can turn each one on or off, except Tickets. | `Module`, `modules` | app, plugin |
| **Auth Provider** | An external identity source: `oidc`, `ldap` or `carddav`. | `AuthProvider` | |
| **Directory Sync** | A scheduled import of Requesters and groups from an Auth Provider. Run status (fixed): `running`, `ok`, `stopped`, `failed`. | `DirectorySyncRun` | |
| **Session** | A signed-in browser session of an Analyst: an `HttpOnly` cookie holding a random token (stored only as a hash) plus a CSRF token for unsafe requests. Expires when idle and after an absolute lifetime (ADR-0011). | `Session`, `analyst_sessions` | login (as a noun) |
| **Principal** | The authenticated caller of a request: its Actor, Capabilities and Tenant Scope. Built by the identity context, checked by every context. | `Principal` | user context |
| **API Key** | A credential for REST integrations (`cpr_<prefix>_<secret>`), with Capabilities, Company access, a rate limit per minute and an expiry. Stored as a hash; shown once. | `ApiKey`, `api_keys` | token (ambiguous) |
| **Two-Factor Sign-In** | A six-digit code from an authenticator app (TOTP) asked for after the password. Administrators can require it. | `TotpSecret`, `Analyst::totp` | MFA (in UI copy), OTP |
| **Trusted Device** | A browser remembered for 30 days after a two-factor sign-in, so it skips the code. | `TrustedDevice`, `analyst_trusted_devices` | remembered computer |
| **IP Ban** | A temporary block of sign-ins from one IP address after repeated failures. | `ip_bans` | lockout (an account lockout is different) |
| **Actor** | Whoever performed an action: an Analyst, a Requester, an API Key or **Automation** (system). Recorded in every audit entry. | `Actor` | user (ambiguous) |
| **Sensitivity** | Confidentiality of a ticket or department (fixed): `normal`, `confidential`. Confidential items are visible only to authorised Analysts. | `Sensitivity` (kernel) | privacy level |
| **Company Access** | Which Companies an Analyst or Team may see: all Companies, specific Companies, or none. An Analyst's Tenant Scope is the union of their own and their active Teams' access. | `CompanyAccess`, `company_access_grants` | tenant membership |
| **System Setting** | A typed configuration value (bool, integer, text or choice) declared by a context with a default; Administrators change it. | `SettingDefinition`, `Setting`, `system_settings` | option, config key |
| **System Log** | Warnings and errors from cooper-server and cooper-worker, stored for Administrators to read in Cooper. | `system_log_entries` | error log |
| **Audit** | An append-only record of who changed what and when: Actor, timestamp, field, old value, new value. Every aggregate has an audit trail (ticket audit, change audit, …), stored in one platform table keyed by aggregate type and ID (ADR-0009). | `AuditRecord`, `audit_entries` | history log |
| **Tenant Scope** | The set of Companies a request or job may see: specific Companies, or all. Every tenant query runs inside one; the database enforces it with Row-Level Security (ADR-0004). | `TenantScope` | tenant context, org filter |
| **Domain Event** | A past-tense fact recorded by an aggregate (`TicketCreated`, `ChangeApproved`), with a stable dotted type (`ticket.created`). The way contexts react to each other. | `DomainEvent`, `EventEnvelope` | message, notification (Notification is the in-app bell) |
| **Notification** | An entry in an Analyst's bell (in-app), about an assignment, a mention, a Requester reply or an SLA Warning or Breach. Kinds (fixed): `ticket_assigned`, `mentioned`, `reply_received`, `sla_warning`, `sla_breached`. | `Notification`, `notifications` | alert, message |
| **Live Update** | A signal pushed to a signed-in Analyst's browser that a record they can see (for now a ticket) has changed, so the screen fetches it again. Carries only which record changed, never its data. Sent over Server-Sent Events (ADR-0022). | `LiveChange`, `/api/v1/live` | push message, notification (Notification is the in-app bell), websocket event |
| **Mention** | `@username` in a Note; the mentioned Analyst gets a Notification. | `mentions()` | tag |
| **Outbox** | The table where Domain Events are stored in the same transaction as the change, then delivered at least once to Event Handlers by the worker (ADR-0005). | `outbox_events`, `EventHandler` | queue (generic) |

---

## 2. Service Desk (Tickets)

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Ticket** | The core record of a request for help or report of a fault. The Ticket Type classifies it (incident, service request, …). | `Ticket`, `tickets` | case, issue, call, job |
| **Ticket Number** | The human-readable, unique reference of a ticket, built from the `tickets.number_format` setting (tokens `{COMPANY}`, `{TYPE}`, `{YEAR}`, `{SEQ:n}`; one sequence per Company). Old numbers remain valid after renumbering (Ticket Number History). | `NumberFormat`, `tickets.ticket_number` | ref, ID (ID is the internal UUID) |
| **SDREF** | The tag `[SDREF:<ticket number>]` placed in the subject of outbound emails. When a reply arrives, Cooper uses this tag to thread it onto the existing ticket. | `SdRef` | |
| **Ticket Status** | Lifecycle state of a ticket. Flags: `is_closed`, `pauses_sla`, `is_default`. Seed: **Open** (default), **In Progress**, **On Hold** (pauses SLA), **Awaiting Response** (pauses SLA), **Closed** (closed). | `TicketStatus`, `ticket_statuses` | state, stage |
| **Ticket Priority** | Urgency of a ticket. Carries the SLA targets: response minutes, resolution minutes and SLA Calendar. Seed: **Low**, **Normal** (default), **High**, **Critical**, **Urgent**. | `TicketPriority`, `ticket_priorities` | severity (Severity is used for monitoring events) |
| **Ticket Type** | Classifies a ticket. Seed: **Incident**, **Service Request**, **Question**. There is no default flag: Ticket Type Rules pick the default. | `TicketType`, `ticket_types` | kind |
| **Ticket Type Rule** | A rule that picks a ticket's default Type from its channel, mailbox, origin, department, subject words, requester domain or company. | `TicketTypeRule` | |
| **Origin** | How the ticket arrived. Seed: **Email**, **Phone**, **Portal**, **Walk-up**; **WhatsApp**, **Web chat**, **Slack** and **Telegram** are seeded when that Channel is enabled. Each seeded Origin has a stable `key`, so renaming it does not break Channels. A Mailbox can stamp a default Origin. | `Origin`, `ticket_origins` | source, channel (Channel is the technical intake) |
| **Category** | A hierarchical classification (up to 3 levels) of what the ticket was **reported as**. Can be limited to a Ticket Type and shown on the Portal. | `TicketCategory` | topic |
| **Closure Category** | The Category that the ticket **turned out to be**, set at closure. | `Ticket::closure_category_id` | |
| **Resolution Code** | **How the ticket ended**. Seed: Fixed remotely, Fixed on site, Hardware replaced, Configuration change, Training given, Access granted, No fault found, Duplicate, Withdrawn, Referred to supplier. | `ResolutionCode` | outcome, close code |
| **Queue** | The Team a ticket is assigned to (`assigned_team_id`). | `Ticket::assigned_team_id` | bucket, pool |
| **Assignee** | The Analyst a ticket is assigned to (`assigned_analyst_id`). | `Ticket::assigned_analyst_id` | owner (Owner means calendar owner) |
| **Owner** | The Analyst whose calendar shows the scheduled work for a ticket. | `Ticket::owner_id` | |
| **Scheduled Work** | A planned work window on a ticket (`work_start`, `work_end`, `all_day`). Synced to the Owner's calendar. | `WorkWindow` | appointment |
| **Note** | A comment added to a ticket. Notes are **internal** by default (only Analysts see them). | `TicketNote` | comment, update |
| **Reply** | An outbound message to the Requester on a ticket (email or chat). | `Reply` | response |
| **Time Entry** | Minutes an Analyst spent on a ticket (or task). | `TimeEntry` | timesheet line |
| **Snooze** | Hide a ticket from active views until a set time, with a reason. | `Ticket::snooze()` | park, defer |
| **Merge** | Combine one ticket into another (`merged_into_id`). Can be undone. | `TicketMerge` | |
| **Split** | Create a new ticket from part of an existing one. Can be undone. | `TicketSplit` | |
| **Ticket Link** | A relationship between tickets. Relation type (fixed): `related`, `duplicate`, `parent`. | `TicketLink` | |
| **First Time Fix** | Flag: the ticket was resolved on first contact. | `Ticket::first_time_fix` | FCR |
| **Training Provided** | Flag: IT training was given as part of the ticket. | `Ticket::it_training_provided` | |
| **Ticket Field** | An administrator-defined custom field on tickets. Can be required on create or on close, and can be asked of the Requester. | `TicketField` | custom attribute |
| **View** | A built-in ticket list: My tickets (`mine`), My team's tickets (`queue`), Unassigned, All open (`open`), Snoozed, Recently closed (7 days), Deleted (Administrators). | `ViewName` | filter preset |
| **Ticket Presence** | Live indicator showing which Analysts are viewing or replying to a ticket (collision detection). | `TicketPresence` | |
| **CSAT** | Customer satisfaction rating a Requester gives after closure. | `CsatResponse` | survey score |
| **Reply Template** | A canned reply an Analyst can insert. | `ReplyTemplate` | canned response, macro |
| **Email Template** | An automated email sent on a ticket event (created, assigned, replied, closed). Can be matched by the recipient's address or domain; the most specific one is sent. | `EmailTemplate` | notification template |
| **Signature** | An Analyst's email signature. | `AnalystSignature` | |
| **Ticket Checklist** | A checklist attached to a ticket from a Checklist Template. | `TicketChecklist` | |
| **Workspace** | A ticket used as a personal work area (`is_workspace`). | `Ticket::is_workspace` | |
| **AI Summary** | An AI-generated, versioned summary of a ticket. | `TicketAiSummary` | |

### 2.1 Intake: Mailboxes & Channels

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Mailbox** (Target Mailbox) | An email account that Cooper polls to create and update tickets and sends replies from. Provider: `microsoft` (Graph), `google`, `imap`. Auth mode for Graph: `delegated` or `app_only`. | `Mailbox`, `target_mailboxes` | inbox, email account |
| **Email** | An inbound or outbound message stored against a ticket. Direction (fixed): `Inbound`, `Outbound`. Status starts at `New`. | `Email`, `emails` | message (generic) |
| **Mailbox Whitelist** | Addresses or domains allowed (or blocked) from creating tickets. | `MailboxWhitelistEntry` | allow-list |
| **Rejected Email** | An inbound email that did not pass the whitelist or rules. The default action is delete. | — | spam |
| **Attachment** | A file attached to an email or ticket. Cooper extracts its text for search. Extraction status (fixed): `pending`, `extracted`, `truncated`, `too_large`, `unsupported`, `failed`. | `Attachment` | |
| **Channel** | A non-email intake: WhatsApp, Telegram, Slack, Microsoft Teams, Web Chat. | `MessagingChannel` | |
| **Web Chat Widget** | An embeddable chat for websites. AI mode (fixed): `assist`, `deflect`. Message sender (fixed): `visitor`, `ai`, `agent`, `system`. | `WebchatWidget` | |
| **Sender Domain** | An email domain mapped to a Company, used to set the tenant on inbound tickets. | `TenantDomain` | |
| **Freemail Domain** | A public email domain (gmail.com and similar) that must never map to a Company. | `FreemailDomain` | |

### 2.2 SLA & Rota

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **SLA** | Service Level Agreement targets for a ticket: **Response** and **Resolution** time in minutes, taken from its Priority. | `SlaTarget` | |
| **SLA Calendar** | Working hours (per weekday, 1 = Mon … 7 = Sun), time zone and **Holidays** that SLA time is counted against. | `SlaCalendar` | business hours |
| **SLA Pause** | SLA time stops while the ticket is in a status with `pauses_sla`. | — | stop the clock |
| **SLA Warning** | Notification that an SLA target is about to be missed. | `SlaTrigger::Warning` | |
| **SLA Breach** | An SLA target was missed. | `SlaTrigger::Breach`, event `SlaBreached` | violation |
| **SLA Notification Rule** | Who is notified, for which trigger (`warning`/`breach`) and target (`response`/`resolution`/`both`). | `SlaNotificationRule` | |
| **Rota** | The staff shift plan: **Shifts**, **Rota Locations** (seed: Office (default), WFH) and **Rota Entries** (who works when, `is_on_call`). | `RotaShift`, `RotaEntry`, `RotaLocation` | schedule (ambiguous) |
| **On Call** | Flag on a Rota Entry: the Analyst is the out-of-hours contact. | `RotaEntry::is_on_call` | standby |

---

## 3. Problem Management

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Problem** | The root cause behind one or more incidents (tickets). Has a Problem Number, status, priority and assigned Analyst. | `Problem`, `problems` | |
| **Problem Number** | Human-readable reference of a Problem. | `ProblemNumber` | |
| **Root Cause** | The investigated underlying cause of the Problem. | `Problem::root_cause` | RCA (RCA is the activity) |
| **Workaround** | A temporary way to restore service while the root cause is not fixed. | `Problem::workaround` | |
| **Known Error** | A Problem with a documented root cause and/or workaround (`is_known_error`). | `Problem::is_known_error` | |
| **Problem Status / Priority** | Configurable lookups. Status seed: **New** (default), **Investigating**, **Root Cause Identified**, **Known Error**, **Resolved** (closed), **Closed** (closed). Priority seed: **Low**, **Medium** (default), **High**, **Critical**. | `ProblemStatus`, `ProblemPriority` | |

---

## 4. Change Management

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Change** | A planned modification to IT services or infrastructure, with risk assessment, approval, schedule and review. | `Change`, `changes` | RFC, change request (in code) |
| **Change Type** | Seed: **Standard** (pre-approved), **Normal** (default), **Emergency**. Each type has an **Approval Route** (fixed): `review` (approve before work, default) or `emergency` (approve afterwards). Seeded Emergency uses `emergency`. | `ChangeType` | |
| **Change Status** | Each status belongs to a **Stage** and may be closed. Seed (status → stage): Draft → draft (default), Submitted → draft, Pending Approval → awaiting, Approved → approved, Rejected → stopped (closed), Scheduled → implementing, In Progress → implementing, Completed → done (closed), Failed → done (closed), Cancelled → stopped (closed). | `ChangeStatus`, `ChangeStage` | |
| **Change Stage** | Fixed lifecycle phase that groups statuses: `draft`, `awaiting`, `approved`, `implementing`, `done`, `stopped`. | `ChangeStage` | |
| **Change Priority** | Seed: Low, Medium (default), High, Critical. | `ChangePriority` | |
| **Change Impact** | Seed: Low, Medium (default), High. | `ChangeImpact` | |
| **Change Category** | Configurable classification of changes. | `ChangeCategory` | |
| **Change Template** | A versioned template that pre-fills a Change. Can be limited to Teams. | `ChangeTemplate` | |
| **Requester / Assigned To / Approver** | The people on a Change: who asked, who implements, who approves. | `Change::{requester_id, assigned_to_id, approver_id}` | |
| **Work Window** | Planned implementation start and end. | `Change::{work_start, work_end}` | |
| **Outage Window** | Planned service downtime start and end. | `Change::{outage_start, outage_end}` | |
| **Reason for Change** | Business justification. | `Change::reason_for_change` | |
| **Risk Evaluation** | Narrative risk assessment, plus a **Risk Likelihood** × **Risk Impact Score** = **Risk Score**, mapped to a **Risk Level**. Likelihood and Impact Score are 1–5 (Very low, Low, Medium, High, Very high). Risk Level seed (score bands): **Low** ≤ 4, **Medium** 5–9, **High** 10–15, **Very High** 16–20, **Critical** 21–25. | `RiskAssessment` | |
| **Test Plan** | How the change will be verified. | `Change::test_plan` | |
| **Rollback Plan** | How to revert if the change fails. | `Change::rollback_plan` | backout plan |
| **CAB** | Change Advisory Board: the members who vote on a Change. Members can be required. **CAB Approval Type** (fixed) sets the rule for passing: `all` required members approve (default) or `majority` of required members approve. One rejection sends the Change back. | `CabMember`, `Change::cab_required` | committee |
| **Vote** | A CAB member's decision, with a comment. Votes are append-only and are never updated. | `ChangeApprovalVote` | |
| **Approval Round** | One cycle of approval. A Change that is edited after approval starts a new round. | `ChangeApproval::round_no` | |
| **PIR** | Post-Implementation Review: was the change successful, actual start and end, lessons learned, follow-up. | `PostImplementationReview` | retrospective |
| **Text Revision** | Every saved version of a Change's long-text fields. | `ChangeTextRevision` | |
| **Change Calendar** | A view of Changes by Work and Outage Windows. | — | forward schedule |

---

## 5. Tasks & Projects

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Task** | A unit of work with status, priority, start and due date, and an assignee (Analyst, Team, Supplier or Contact). Can link to a ticket, change, contract or project. Supports **Subtasks** (`parent_task_id`). | `Task`, `tasks` | to-do, job |
| **Task Status / Priority** | Configurable lookups. Status seed: **To Do** (default), **In Progress**, **Blocked**, **Done** (closed), **Cancelled** (closed). Priority seed: **Low**, **Medium** (default), **High**, **Urgent**. | `TaskStatus`, `TaskPriority` | |
| **Board** | Kanban view of Tasks by status (`board_position`). | — | |
| **Recurrence** | A repeat rule for a Task (daily, weekly, monthly, yearly). Repeats **on completion** or on a **fixed schedule**. | `TaskRecurrence` | |
| **Collaborator** | An extra Analyst involved in a Task. | `TaskCollaborator` | watcher |
| **Tag** | A free label on Tasks. | `TaskTag` | |
| **Procedure** | A Knowledge article attached to a Task, frozen as a snapshot. | `TaskProcedure` | |
| **Project** | An IT project built on Tasks. Method: Simple, Staged (PRINCE2-style) or Agile. Has stages, stage gates, milestones, a RAID log, a decision log, a budget and benefits. | `Project` | programme |
| **Stage / Stage Gate** | A project phase and the go/no-go decision at its end. | `ProjectStage` | |
| **RAID Log** | Risks, Assumptions, Issues, Dependencies of a project. | `RaidItem` | |

---

## 6. Checklists

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Checklist Template** | A reusable, versioned list of items with a Category and a Role. Its **Closure Mode** controls whether open items block closing a ticket or task. | `ChecklistTemplate` | |
| **Checklist Item** | A step. Can be **mandatory** (critical) or require input, and has a result mode. | `ChecklistItem` | |

---

## 7. Assets

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Asset** | A physical or virtual IT item (PC, laptop, server, VM, phone …). Key fields: hostname, Service Tag (serial), Asset Tag, type, status, location, supplier, cost centre, purchase, warranty and lease dates. | `Asset`, `assets` | device, hardware, CI (CI is CMDB) |
| **Asset Tag** | The per-company label identifier stuck on the item. Also printable as a QR label. | `AssetTag` | |
| **Service Tag** | The manufacturer serial number. | `ServiceTag` | serial |
| **Asset Type** | Configurable classification. Linked to a CMDB Class and to Field Sets. No seed values (Administrators create them). | `AssetType` | |
| **Asset Status** | Configurable status. `is_retired` means the asset has left the estate. No seed values (Administrators create them). | `AssetStatus` | |
| **Retire** | Remove an Asset from the estate, with a date and reason. | `Asset::retire()` | dispose, delete |
| **Location** | Hierarchical physical location. | `AssetLocation` | site |
| **Asset Field / Field Set** | Custom fields, grouped into Field Sets and assigned to Asset Types. | `AssetField`, `AssetFieldSet` | |
| **Holder** | The Requester or Analyst who currently has an Asset. | `AssetHolder` | owner, user |
| **Check Out / Check In** | Give an Asset to, or take it back from, a Holder. Logged. | `AssetCheckout` | lend |
| **Asset Connection** | A physical attachment between assets (for example a monitor to a PC). | `AssetConnection` | |
| **Inventory Source** | Where asset data comes from: agent/REST push, Intune, Proxmox, VMware Cloud Director, Hyper-V, vCenter. | `InventorySource` | discovery |
| **Asset Import** | A CSV or API import with mappings. Run modes: `live`, `preview`. Entry actions (fixed): create, update, unchanged, conflict, skip, error, deactivate. | `AssetImportProfile`, `AssetImportRun` | |
| **Review Item** | A possible duplicate or conflict found by reconciliation. Status (fixed): open, merged, different, ignored, resolved. | `AssetReviewItem` | |
| **Protected Field** | An asset field that imports must not overwrite. | `AssetProtectedField` | |
| **Asset Label** | A printable label design (with QR code). | `AssetLabelDesign` | |

### 7.1 Software

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Software Application** | A distinct installed product (display name, publisher), found by inventory. | `SoftwareApp` | program |
| **Installation** | An Application on a specific Asset, with a version. | `SoftwareInstallation` | |
| **Licence** | An entitlement: type, key, quantity, renewal date, notice period, cost, status. Use British spelling in code and UI. | `SoftwareLicence` | license |

---

## 8. CMDB

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Configuration Item (CI)** | An object in the CMDB (service, server, application, database …). Can be **planned**. | `CmdbObject` (alias `ConfigurationItem`) | asset (Assets are a separate context) |
| **CMDB Class** | The type of a CI, with typed Class Properties: text, number, date, boolean, dropdown, object_ref. | `CmdbClass`, `CmdbClassProperty` | |
| **Relationship** | A directed link between CIs, with a Relationship Type (verb / inverse verb, for example "runs on" / "hosts"). | `CmdbRelationship`, `CmdbRelationshipType` | |
| **Impact Direction** | How failure propagates along a relationship type (fixed): `none`, `to_from`, `from_to`. | `ImpactDirection` | |
| **Asset–CI Link** | Links an Asset to the CI it realises, for a period of time. | `AssetCiLink` | |

---

## 9. Knowledge

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Article** | A knowledge base document (HTML body) with author, owner, version, tags, folder, review date and view count. | `KnowledgeArticle` | page, doc, KB entry |
| **Folder** | Hierarchical container of Articles. Can restrict access and inherit permissions. | `KnowledgeFolder` | category |
| **Audience** | Who an Article is for (fixed, ascending): `internal`, `customer`, `public`. | `Audience` | visibility |
| **Published / Archived** | Article lifecycle flags. | `KnowledgeArticle::{is_published, is_archived}` | draft (use "unpublished") |
| **Article Version** | An immutable previous version of an Article. | `KnowledgeArticleVersion` | |
| **Review Date** | When an Article is due for review. | `next_review_date` | expiry |
| **Knowledge ACL** | An access rule on a folder or article for a principal (`analyst`, `team`, `user`, `user_group`). | `KnowledgeAcl` | |
| **User Group** | A named group of Analysts and/or Requesters, with optional membership expiry. Used for knowledge and learning permissions. | `UserGroup` | |
| **Knowledge Gap** | A cluster of tickets that no Article covers yet. | `KnowledgeGapCluster` | |
| **Knowledge Q&A** | AI answers grounded in Articles (embeddings). | — | chatbot |

---

## 10. Service Status

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Service** | A business or IT service whose status is published. | `StatusService` | system |
| **Service Incident** | A published disruption that affects one or more Services. Separate from a Ticket. | `StatusIncident` | outage (Outage is an impact level) |
| **Incident Status** | Seed: **Investigating** (default), **Identified**, **Monitoring**, **3rd Party**, **Resolved** (resolved). | `ServiceIncidentStatus` | |
| **Impact Level** | Effect on a Service, ordered by severity. Seed: **Major Outage** (1, downtime), **Partial Outage** (2, downtime), **Degraded** (3, downtime), **Maintenance** (4), **Operational** (5, default), **No Disruption** (6). | `ImpactLevel` | |
| **Counts as Downtime** | Impact Level flag used for availability reporting. | `ImpactLevel::counts_as_downtime` | |
| **Incident Update** | A timestamped update on a Service Incident, with a snapshot of the affected services. Internal by default. | `StatusIncidentUpdate` | post |
| **Planned Maintenance** | A scheduled service window. State (fixed): `scheduled`, `started`, `finished`, `cancelled`. Becomes an Incident when it starts. | `PlannedMaintenance` | |

---

## 11. Morning Checks

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Morning Checks** | The daily round of infrastructure health checks that Analysts record. | module `morning_checks` | health checks |
| **Check Group** | A group of Checks, assigned to a Team or Analyst. | `CheckGroup` | |
| **Check** | One item to verify each day (for example "Backups completed"). Has an assigned Analyst. | `Check` | test |
| **Check Status** | Seed: **Green** (OK), **Amber** (requires notes), **Red** (requires notes). Colours #28a745, #ffc107, #dc3545. | `CheckStatus` | RAG (acceptable in UI), pass/fail |
| **Check Result** | The recorded Status and Notes of one Check on one **Check Date**. Unique per check per date. | `CheckResult` | |
| **Result Link** | Links a Check Result to a follow-up ticket or task. | `CheckResultLink` | |

---

## 12. Calendar

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Calendar Event** | An entry on the IT calendar, with a Category (colour). Can be generated, for example from asset warranty expiry or contract notice dates. | `CalendarEvent` | appointment |
| **Calendar Category** | Colour-coded event category. | `CalendarCategory` | |
| **Calendar Connection** | A link to an external calendar (Microsoft, Google). | `CalendarConnection` | |
| **Enrolment** | An Analyst's calendar sync setting. Mode (fixed): `off`, `push`, `feed`. Task mode (fixed): `off`, `work`, `due`, `both`. | `CalendarEnrolment` | subscription |

---

## 13. Contracts & Suppliers

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Contract** | An agreement with a Supplier (or a customer): number, title, owner, status, start and end, notice period and **Notice Date**, value, currency, payment schedule, cost centre, DMS link, data protection fields (personal data transferred, DPIA required or completed). | `Contract` | agreement, deal |
| **Supplier** | An external vendor: legal name, trading name, registration number, VAT number, type, status. | `Supplier` | vendor |
| **Contact** | A person at a Supplier. | `Contact` | (never use for Requesters) |
| **Contract Status / Supplier Type / Supplier Status / Payment Schedule** | Configurable lookups. No seed values (Administrators create them). | — | |
| **Contract Terms** | Structured terms, organised in tabs. | `ContractTermTab`, `ContractTermValue` | |
| **Notice Date** | The last date to give notice: contract end minus notice period. | `Contract::notice_date()` | |
| **RFP** | Request for Proposal builder: documents, extracted requirements, conflicts, invited suppliers, scores. | `Rfp` | tender |

---

## 14. Forms

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Form** | An administrator-built, versioned form (fields, layout). Can appear on the Portal, require approval and run **Submission Actions** (for example create a ticket or task). | `Form` | survey |
| **Form Field** | A typed field with a label, options and required flag. | `FormField` | |
| **Submission** | One completed Form, with approval status and round, and the ticket or task it created. | `FormSubmission` | response, entry |
| **Form Approval** | Approval by an Approver or approval Team, with required approval and rejection counts. Can exclude the Requester. | — | |
| **Form Collection** | A time-boxed campaign that collects Submissions. | `FormCollection` | |
| **Draft** | A Submission saved but not yet submitted. | `FormDraft` | |

---

## 15. Workflow & Integrations

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Workflow** | An automation rule: **Trigger Event** + **Conditions** (`field`, `op`, `value`) + **Actions** (`type`, `args`). Company-scoped. | `Workflow` | rule, automation (generic) |
| **Workflow Execution** | One run of a workflow. Status (fixed): `running`, `success`, `failed`, `skipped`, `aborted`. Supports **Dry Run**. | `WorkflowExecution` | |
| **Webhook** | An outbound HTTP notification. Delivery status (fixed): `pending`, `delivering`, `delivered`, `failed`, `dead`. Message formats: slack, teams, discord, teams_workflows. | `WebhookDelivery` | callback |
| **Integration Link** | A link from a Cooper record to an external issue (Jira, GitHub, GitLab, Azure DevOps). Status category (fixed): `todo`, `in_progress`, `done`, `cancelled`. | `IntegrationLink` | |
| **Monitoring Event** | An alert from monitoring (for example Zabbix) that creates, updates or resolves tickets. Severity 0–5 (Not classified … Disaster). | `MonitoringEvent` | |
| **Background Job** | A scheduled job (mailbox polling, SLA checks, syncs). Runs on one worker at a time. Schedule mode (fixed): `interval`, `daily`, `off`. Each run is recorded with outcome (fixed): `running`, `succeeded`, `failed`, `abandoned`, and duration. | `Job`, `Schedule`, `background_jobs`, `background_job_runs` | cron |

---

## 16. Self-Service Portal

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Self-Service Portal** | The web interface for Requesters: raise and track tickets, fill in Forms, read customer and public Articles, see Service Status. | `portal` | customer portal, help center (UI may say "Help Centre") |
| **Portal Catalogue** | The Categories and Forms a Requester can choose from to raise a ticket: active and marked `portal_visible`, with every parent Category also active and visible. | `portal_catalogue` | service catalog |
| **Portal Account** | The Portal sign-in of one email address: a password that acts for every active Requester with that address, in any Company. Created when the person first sets a password through an emailed link (invitation, reset or Self-Registration). | `PortalAccount`, `portal_accounts` | user account, login |
| **Portal Invitation** | An email an Analyst sends a Requester with a link to set a Portal password (valid 7 days). | `PortalLinkPurpose::Invitation` | |
| **Self-Registration** | A person whose email belongs to a Company's Sender Domain asks for a Portal Account; the emailed link confirms the address, sets the password and creates the Requester. Off by default (`portal.self_registration`). | `PortalLinkPurpose::Registration` | sign-up |

---

## 17. Reporting & Dashboards

| Term | Definition | Code | Avoid |
|---|---|---|---|
| **Dashboard** | A per-module set of **Widgets** (for example Tickets by status, First time fix rate, Created vs closed monthly). | `Dashboard`, `DashboardWidget` | |
| **Watchtower** | The cross-module overview showing counts of selected ticket statuses, priorities, task statuses and morning check statuses. | `WatchtowerItem` | home, overview |
| **Report Pack** | A saved, shareable report design. | `ReportPack` | |
| **Table View** | A saved column and filter configuration of a list. | `TableView` | saved filter |

---

## 18. Out of initial scope (FreeITSM modules recognised but not yet modelled)

LMS (courses, lessons, competency tests), Process Mapper, Network Mapper, War Room (team
chat), Domains (domain/SSL/DNS monitoring), Recordings (screen recordings), Files
(documents/DMS links). Add their terms here before you implement them.
