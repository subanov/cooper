# 0034. Contracts: per-Company Suppliers and Contracts, a calculated Notice Date, a Calendar event contract and once-only reminders

- **Status:** Accepted
- **Date:** 2026-11-11
- **Issue:** #182, #183, #184
- **Requirements:** FR-CON-001 to FR-CON-004

## Context

In FreeITSM, the Contracts module keeps Suppliers (legal and trading name, registration
and VAT numbers, a type and a status), their Contacts, and Contracts with a number, title,
Supplier, owner, status, start and end, notice period, Notice Date, value and currency,
payment schedule, cost centre, DMS link and data protection fields. Supplier Types,
Supplier Statuses, Contract Statuses and Payment Schedules are administrator lookups.

FreeITSM is single-tenant. Its Notice Date is a column the form fills in from the end
date and the notice period in the browser, so editing the end date in another way leaves
a stale Notice Date. The value is a free decimal with an optional currency, and nothing
stops DPIA details on a Contract that needs no DPIA. Reminders and Calendar entries do not
know about closed Contracts.

The Calendar context (FR-CAL-002) and Assets (Supplier and Contract links) are built in
parallel and need a stable way to learn about Contracts.

## Decision

- A new crate `cooper-contracts` (bounded context, AGENTS.md §3) behind the **Contracts**
  Module: `/contracts`, `/suppliers`, `/contacts`, `/supplier-types`, `/supplier-statuses`,
  `/contract-statuses`, `/payment-schedules`. Capabilities `contracts.read`,
  `contracts.write` and `contracts.configure`.
- **Lookups** (Supplier Type, Supplier Status, Contract Status, Payment Schedule) are
  instance-wide reference data with no seed values, deactivated rather than deleted. A
  Contract Status has the behaviour flag `is_closed`. Inactive rows cannot be chosen for a
  new value but stay on records that already use them.
- **Suppliers, Contacts and Contracts belong to a Company** (`tenant_id`, RLS). A Supplier's
  legal name and a Contract's number are unique within the Company (the number only among
  Contracts that are not deleted). A Contact belongs to one Supplier and its Company.
- **Who a Contract is with** is either a Supplier of the same Company, or the Company itself
  as customer, optionally with one of its Requesters as the contact. Owner Analysts,
  Requesters and Cost Centres are checked through the identity application API when they
  change.
- **Notice Date.** `NoticePeriod` is a value object: 0 to 3650 days or 0 to 120 months.
  `Contract::notice_date()` is the end date minus the period. Months keep the day of the
  month, clamped to the shorter month (31 May − 3 months = 28/29 February); a Contract that
  ends on the last day of a month gives notice on the last day of the earlier month
  (30 June − 1 month = 31 May). Properties are tested with proptest. The Notice Date is
  stored next to the period only so SQL can filter and remind by it; the domain always
  writes it.
- **Money** is `NUMERIC(14,2)` plus an ISO 4217 code (`Money`, `CurrencyCode`): the API
  takes and returns the amount as a decimal string, the domain keeps minor units, and SQL
  converts. A value needs a currency and the other way round.
- **Active** means not deleted and not in a closed Contract Status. Deleting a Contract is a
  soft delete (`deleted_at`).
- **Calendar contract.** Whenever a Contract is created or deleted, or its number, title,
  Notice Date, end date or active state changes (including when an Administrator opens or
  closes its Contract Status), Contracts appends `contract.dates_changed` to the outbox
  (aggregate type `contract`, aggregate id the Contract, tenant set) with the payload
  `{ "contract_id", "contract_number", "title", "notice_date": "YYYY-MM-DD" | null,
  "end_date": "YYYY-MM-DD" | null, "active": bool }`. The Calendar context owns the events
  it derives from it.
- **Reminders.** The `contract-reminders` job (hourly) reminds the owner of an active
  Contract when its Notice Date or end date falls from today up to
  `contracts.reminder_days` ahead (System Setting, default 30, 0 turns reminders off),
  "today" being the date in `general.time_zone`. `contract_reminders` records each
  reminder by Contract, date kind and date, in the same transaction as the
  `contract.reminder_due` event, so a reminder is sent once; a changed date is reminded
  again. The `ContractReminderNotifier` in the notifications context turns the event into
  a `contract_notice_due` or `contract_end_due` Notification linked by `contract_id`,
  unless the Contract has since been deleted or closed, changed owner or changed that date.
- **The list shows where the Notice Date stands** (`notice_status`: `none`, `later`,
  `upcoming`, `passed`), computed by the API with the same window, so the web UI holds no
  date rules. `?notice_due=true` lists the upcoming ones.
- **Public application API** for other contexts: `ContractsService::supplier_exists`,
  `contract_exists`, `supplier_summary` and `contract_summary`, each with a tenant scope.
- **Audit.** Every change of a lookup, Supplier, Contact or Contract is in the platform
  audit trail (`supplier`, `contact`, `contract`, and the lookup type);
  `GET /suppliers/{id}/history` and `GET /contracts/{id}/history` return it.

## Deviations from FreeITSM

- The Notice Date is always calculated by Cooper, never typed in.
- Suppliers and Contracts are per Company; legal names and Contract numbers are unique
  within it.
- A Contract is with a Supplier or with its Company (optionally a Requester), not both.
- Value and currency come together, as `NUMERIC(14,2)` and an ISO 4217 code.
- DPIA completion date and link need DPIA required; the start date cannot be after the end.
- Closed and deleted Contracts get no reminders and leave the Calendar.
- A Contact has one full name (Cooper's convention for people) and a validated email
  address.
- Contract Terms (FR-CON-005), links to Assets and Tasks (FR-CON-006) and RFPs (FR-CON-007)
  are not part of this decision.

## Consequences

- The Calendar and any later consumer depend on the `contract.dates_changed` payload; it
  must only grow compatibly.
- Changing `contracts.reminder_days` takes effect on the next job run; dates already
  reminded are not reminded again.
- Closing a Contract Status emits one event per Contract in that status, in the same
  transaction as the change.
