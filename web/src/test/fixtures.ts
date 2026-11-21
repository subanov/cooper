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

const AT = "2026-03-01T09:00:00Z";

export function task(overrides: Partial<Schemas["TaskDto"]> = {}): Schemas["TaskDto"] {
  return {
    id: "00000000-0000-7000-8000-0000000000k1",
    company_id: COMPANY_ID,
    title: "Replace the backup tapes",
    description: "",
    status_id: "ts-todo",
    is_closed: false,
    priority_id: "tp-normal",
    tags: ["backup"],
    board_position: 1,
    created_at: AT,
    updated_at: AT,
    ...overrides,
  };
}

export function asset(overrides: Partial<Schemas["AssetDto"]> = {}): Schemas["AssetDto"] {
  return {
    id: "00000000-0000-7000-8000-0000000000s1",
    company_id: COMPANY_ID,
    asset_tag: "LT-0042",
    hostname: "ada-laptop",
    is_active: true,
    created_at: AT,
    updated_at: AT,
    ...overrides,
  };
}

export function contract(overrides: Partial<Schemas["ContractDto"]> = {}): Schemas["ContractDto"] {
  return {
    id: "00000000-0000-7000-8000-0000000000d1",
    company_id: COMPANY_ID,
    contract_number: "C-2026-01",
    title: "Printer lease",
    description: "",
    party: "supplier",
    is_active: true,
    notice_status: "upcoming",
    notice_date: "2026-05-01",
    end_date: "2026-08-01",
    personal_data_transferred: false,
    dpia_required: false,
    created_at: AT,
    updated_at: AT,
    ...overrides,
  } as Schemas["ContractDto"];
}

export function article(
  overrides: Partial<Schemas["KnowledgeArticleDto"]> = {},
): Schemas["KnowledgeArticleDto"] {
  return {
    id: "00000000-0000-7000-8000-0000000000e1",
    title: "Reset your password",
    body: "<p>Open the sign-in page and choose <strong>Forgot</strong>.</p>",
    body_text: "Open the sign-in page and choose Forgot.",
    tags: ["password"],
    audience: "public",
    is_published: true,
    is_archived: false,
    version: 2,
    view_count: 12,
    access: { all: true },
    created_at: AT,
    updated_at: AT,
    ...overrides,
  } as Schemas["KnowledgeArticleDto"];
}

/** Task reference data and a Board with one Task. */
export const TASK_API = {
  "GET /api/v1/task-statuses": {
    status: 200,
    body: [
      {
        id: "ts-todo",
        name: "To do",
        colour: "#6b7280",
        is_closed: false,
        is_active: true,
        sort_order: 0,
      },
      {
        id: "ts-done",
        name: "Done",
        colour: "#16a34a",
        is_closed: true,
        is_active: true,
        sort_order: 1,
      },
    ],
  },
  "GET /api/v1/task-priorities": {
    status: 200,
    body: [{ id: "tp-normal", name: "Normal", is_active: true, is_default: true, sort_order: 0 }],
  },
  "GET /api/v1/tasks/board": {
    status: 200,
    body: [
      {
        status: { id: "ts-todo", name: "To do", colour: "#6b7280" },
        tasks: [task()],
        has_more: false,
      },
      { status: { id: "ts-done", name: "Done", colour: "#16a34a" }, tasks: [], has_more: false },
    ],
  },
} as const;
