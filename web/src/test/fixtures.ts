import type { Schemas } from "@/lib/api";

export const COMPANY_ID = "00000000-0000-7000-8000-0000000000c1";
export const ANALYST_ID = "00000000-0000-7000-8000-0000000000a1";

export function analyst(overrides: Partial<Schemas["AnalystDto"]> = {}): Schemas["AnalystDto"] {
  return {
    id: ANALYST_ID,
    username: "ada",
    email: "ada@example.com",
    full_name: "Ada Lovelace",
    is_active: true,
    is_admin: false,
    has_password: true,
    totp_enabled: false,
    signature: "",
    role_ids: [],
    company_access: { all: true },
    created_at: "2026-01-01T09:00:00Z",
    updated_at: "2026-01-01T09:00:00Z",
    ...overrides,
  };
}

/** An Analyst session with the given Capabilities. */
export function session(
  capabilities: string[] = [],
  overrides: Partial<Schemas["SessionDto"]> = {},
): Schemas["SessionDto"] {
  return {
    analyst: analyst(),
    capabilities,
    company_ids: [COMPANY_ID],
    all_companies: false,
    csrf_token: "csrf-analyst",
    expires_at: "2099-01-01T00:00:00Z",
    idle_expires_at: "2099-01-01T00:00:00Z",
    is_admin: false,
    totp_enrollment_required: false,
    ...overrides,
  };
}

export const MODULES: Schemas["ModuleDto"][] = [
  "tickets",
  "tasks",
  "assets",
  "calendar",
  "knowledge",
  "service-status",
  "morning-checks",
  "contracts",
].map((key) => ({ key, name: key, is_enabled: true, can_disable: key !== "tickets" }));

export function lookup(
  id: string,
  name: string,
  overrides: Partial<Schemas["LookupDto"]> = {},
): Schemas["LookupDto"] {
  return {
    id,
    name,
    is_active: true,
    is_closed: false,
    is_default: false,
    pauses_sla: false,
    sort_order: 0,
    ...overrides,
  };
}

export function ticket(overrides: Partial<Schemas["TicketDto"]> = {}): Schemas["TicketDto"] {
  return {
    id: "00000000-0000-7000-8000-000000000071",
    number: "ACM-INC-0042",
    subject: "Printer on floor 2 is offline",
    description: "Paper stuck in tray 2.",
    company_id: COMPANY_ID,
    status_id: "s-open",
    priority_id: "p-normal",
    sensitivity: "normal",
    first_time_fix: false,
    training_provided: false,
    created_at: "2026-03-01T09:00:00Z",
    updated_at: "2026-03-01T10:00:00Z",
    sla: {
      response: { state: "on_track", remaining_seconds: 3600, due_at: "2026-03-01T11:00:00Z" },
      resolution: { state: "on_track", remaining_seconds: 86400, due_at: "2026-03-02T09:00:00Z" },
    },
    ...overrides,
  };
}

/** The API answers every screen needs for a signed-in Analyst. */
export function analystApi(capabilities: string[], overrides: Partial<Schemas["SessionDto"]> = {}) {
  return {
    "GET /api/v1/session": { status: 200, body: session(capabilities, overrides) },
    "GET /api/v1/modules": { status: 200, body: MODULES },
    "GET /api/v1/notifications/unread-count": { status: 200, body: { unread: 0 } },
    "GET /api/v1/notifications": { status: 200, body: { items: [] } },
    "GET /api/v1/ticket-statuses": {
      status: 200,
      body: [lookup("s-open", "Open"), lookup("s-closed", "Closed", { is_closed: true })],
    },
    "GET /api/v1/ticket-priorities": {
      status: 200,
      body: [lookup("p-normal", "Normal", { is_default: true })],
    },
    "GET /api/v1/ticket-types": { status: 200, body: [] },
    "GET /api/v1/ticket-origins": { status: 200, body: [] },
    "GET /api/v1/resolution-codes": { status: 200, body: [lookup("r-fixed", "Fixed")] },
    "GET /api/v1/ticket-categories": { status: 200, body: [] },
    "GET /api/v1/analysts": { status: 200, body: { items: [analyst()] } },
    "GET /api/v1/teams": { status: 200, body: { items: [] } },
    "GET /api/v1/companies": {
      status: 200,
      body: {
        items: [
          {
            id: COMPANY_ID,
            name: "Acme",
            ticket_code: "ACM",
            sender_domains: [],
            is_active: true,
            is_default: true,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
          },
        ],
      },
    },
    "GET /api/v1/departments": { status: 200, body: { items: [] } },
    "GET /api/v1/requesters": { status: 200, body: { items: [] } },
  } as const;
}
