import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axeViolations, mockFetch, renderApp } from "@/test/render";

afterEach(() => {
  vi.unstubAllGlobals();
});

const me = { id: "a1", full_name: "Ada Analyst" };
const session = {
  analyst: me,
  csrf_token: "csrf-1",
  is_admin: false,
  capabilities: ["tickets.read", "tickets.update"],
  all_companies: true,
  company_ids: [],
  totp_enrollment_required: false,
};
const lookup = (id: string, name: string, extra: Record<string, unknown> = {}) => ({
  id,
  name,
  is_active: true,
  is_closed: false,
  is_default: false,
  pauses_sla: false,
  sort_order: 0,
  ...extra,
});
const target = (state: string, remaining: number | null = null) => ({
  state,
  due_at: remaining === null ? null : "2026-11-07T15:00:00Z",
  remaining_seconds: remaining,
});

function ticket(id: string, number: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    number,
    subject: `Subject of ${number}`,
    description: "The printer on floor 2 is jammed.",
    company_id: "c1",
    requester_id: "r1",
    status_id: "s-open",
    priority_id: "p-high",
    analyst_id: null,
    team_id: "t1",
    sensitivity: "normal",
    created_at: "2026-11-06T09:00:00Z",
    updated_at: "2026-11-06T10:00:00Z",
    first_time_fix: false,
    training_provided: false,
    sla: {
      paused_at: null,
      responded_at: null,
      response: target("warning", 20 * 60),
      resolution: target("on_track", 5 * 3600),
    },
    ...overrides,
  };
}

const t1 = ticket("tk1", "INC-0001");
const t2 = ticket("tk2", "INC-0002", {
  analyst_id: "a1",
  sla: {
    paused_at: null,
    responded_at: "2026-11-06T09:10:00Z",
    response: target("met"),
    resolution: target("breached", -90 * 60),
  },
});

function api(extra: Parameters<typeof mockFetch>[0] = {}) {
  return mockFetch({
    "/api/v1/session": { status: 200, body: session },
    "/api/v1/modules": { status: 200, body: [{ key: "tickets", is_enabled: true }] },
    "/api/v1/ticket-statuses": {
      status: 200,
      body: [lookup("s-open", "Open"), lookup("s-closed", "Closed", { is_closed: true })],
    },
    "/api/v1/ticket-priorities": { status: 200, body: [lookup("p-high", "High")] },
    "/api/v1/resolution-codes": { status: 200, body: [lookup("rc1", "Fixed")] },
    "/api/v1/analysts": { status: 200, body: { items: [me] } },
    "/api/v1/teams": { status: 200, body: { items: [{ id: "t1", name: "Service Desk" }] } },
    "/api/v1/requesters": {
      status: 200,
      body: { items: [{ id: "r1", full_name: "Rita Requester", email: "rita@acme.example" }] },
    },
    "GET /api/v1/tickets": { status: 200, body: { items: [t1, t2], next_cursor: null } },
    "GET /api/v1/tickets/tk1": { status: 200, body: t1 },
    "GET /api/v1/tickets/tk2": { status: 200, body: t2 },
    "/api/v1/tickets/tk1/timeline": {
      status: 200,
      body: [
        {
          kind: "change",
          at: "2026-11-06T09:00:00Z",
          actor: { kind: "automation" },
          data: { action: "created", changes: [] },
        },
        {
          kind: "email",
          at: "2026-11-06T09:01:00Z",
          actor: { kind: "requester", id: null },
          data: {
            direction: "inbound",
            from: "rita@acme.example",
            to: ["help@cooper.example"],
            subject: "Printer jammed",
            text_body: "Please help.",
          },
        },
        {
          kind: "change",
          at: "2026-11-06T09:05:00Z",
          actor: { kind: "analyst", id: "a1" },
          data: {
            action: "updated",
            changes: [
              { field: "status_id", old_value: "s-new", new_value: "s-open" },
              { field: "response_due_at", old_value: null, new_value: "2026-11-07T15:00:00Z" },
            ],
          },
        },
        {
          kind: "note",
          at: "2026-11-06T09:06:00Z",
          actor: { kind: "analyst", id: "a1" },
          data: { body: "Checked the tray.", is_public: false },
        },
      ],
    },
    "/api/v1/tickets/tk2/timeline": { status: 200, body: [] },
    ...extra,
  });
}

describe("ticket list (FR-TKT-040, FR-SLA-007)", () => {
  it("shows the view's tickets with SLA state and time remaining, and passes axe-core", async () => {
    const calls = api();
    const { container } = renderApp("/tickets");
    const table = await screen.findByRole("table", { name: "Tickets: My tickets" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("INC-0001");
    expect(rows[0]).toHaveTextContent("Warning· 20m left");
    expect(rows[1]).toHaveTextContent("Breached· 1h 30m overdue");
    await waitFor(() => expect(rows[0]).toHaveTextContent("Rita Requester"));
    expect(rows[0]).toHaveTextContent("Unassigned");
    expect(rows[1]).toHaveTextContent("Ada Analyst");
    const list = calls.find((c) => c.path === "/api/v1/tickets");
    expect(list).toBeDefined();
    const requesters = calls.find((c) => c.path === "/api/v1/requesters");
    expect(requesters).toBeDefined();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("keeps the view and search in the URL", async () => {
    const urls: string[] = [];
    api({
      "GET /api/v1/tickets": (request) => {
        urls.push(new URL(request.url).search);
        return { status: 200, body: { items: [], next_cursor: null } };
      },
    });
    const user = userEvent.setup();
    renderApp("/tickets");
    await user.click(await screen.findByRole("link", { name: "Unassigned" }));
    await screen.findByText("No tickets in this view.");
    await user.type(screen.getByRole("searchbox", { name: "Search tickets" }), "printer");
    await user.click(screen.getByRole("button", { name: "Search" }));
    await screen.findByText("No tickets match the search.");
    expect(urls.at(-1)).toContain("view=unassigned");
    expect(urls.at(-1)).toContain("q=printer");
    expect(screen.getByRole("link", { name: "Unassigned" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });
});

describe("ticket detail and keyboard triage (FR-TKT-024)", () => {
  it("opens tickets with j/k and shows the timeline in order", async () => {
    api();
    const { container } = renderApp("/tickets");
    await screen.findByRole("table");
    fireEvent.keyDown(window, { key: "j" });
    const heading = await screen.findByRole("heading", { name: "Subject of INC-0001" });
    await waitFor(() => expect(heading).toHaveFocus());
    const items = await waitFor(() => {
      const section = screen.getByRole("region", { name: "Timeline" });
      const list = [...section.querySelectorAll(":scope > ol > li")];
      expect(list.length).toBeGreaterThanOrEqual(4);
      return list;
    });
    expect(items[0]).toHaveTextContent("Created the ticket");
    expect(items[1]).toHaveTextContent("Email received");
    expect(items[1]).toHaveTextContent("Please help.");
    await waitFor(() => expect(items[2]).toHaveTextContent("Status: Open"));
    expect(items[2]).not.toHaveTextContent("due");
    expect(items[3]).toHaveTextContent("Internal Note");
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.keyDown(window, { key: "j" });
    expect(await screen.findByRole("heading", { name: "Subject of INC-0002" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "k" });
    expect(await screen.findByRole("heading", { name: "Subject of INC-0001" })).toBeInTheDocument();
  });

  it("assigns to me with `a`, keeping the Queue, with the CSRF token", async () => {
    let current: Record<string, unknown> = t1;
    const calls = api({
      "GET /api/v1/tickets/tk1": () => ({ status: 200, body: current }),
      "PUT /api/v1/tickets/tk1/assignment": (_req, body) => {
        current = { ...t1, ...(body as object) };
        return { status: 200, body: current };
      },
    });
    renderApp("/tickets/tk1");
    await screen.findByRole("heading", { name: "Subject of INC-0001" });
    await waitFor(() => expect(screen.getByRole("button", { name: "Assign to me" })).toBeEnabled());
    fireEvent.keyDown(window, { key: "a" });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Assign to me" })).toBeDisabled(),
    );
    const put = calls.find((c) => c.method === "PUT" && c.path.endsWith("/assignment"));
    expect(put?.body).toEqual({ analyst_id: "a1", team_id: "t1" });
    expect(put?.headers.get("X-CSRF-Token")).toBe("csrf-1");
  });

  it("asks for a Resolution Code when closing and shows the server's problem", async () => {
    const calls = api({
      "POST /api/v1/tickets/tk1/status": {
        status: 422,
        body: {
          type: "https://cooper.dev/problems/invalid",
          status: 422,
          detail: "closure_category_id: is required",
        },
      },
    });
    const user = userEvent.setup();
    renderApp("/tickets/tk1");
    await screen.findByRole("heading", { name: "Subject of INC-0001" });
    await waitFor(() => expect(screen.getByRole("option", { name: "Closed" })).toBeInTheDocument());
    await user.selectOptions(screen.getByLabelText("Status"), "s-closed");
    await user.selectOptions(screen.getByLabelText("Resolution Code"), "rc1");
    await user.click(screen.getByRole("button", { name: "Change Status" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("closure_category_id: is required");
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({
      status_id: "s-closed",
      resolution_code_id: "rc1",
    });
  });

  it("adds an internal Note with `n` and replies by email", async () => {
    const calls = api({
      "POST /api/v1/tickets/tk1/notes": { status: 201, body: { id: "n1" } },
      "POST /api/v1/tickets/tk1/replies": { status: 201, body: { id: "e1" } },
    });
    const user = userEvent.setup();
    renderApp("/tickets/tk1");
    await screen.findByRole("heading", { name: "Subject of INC-0001" });
    fireEvent.keyDown(window, { key: "n" });
    const box = screen.getByRole("textbox", { name: "Note" });
    await waitFor(() => expect(box).toHaveFocus());
    await user.type(box, "Ordered a new roller.");
    await user.click(screen.getByRole("button", { name: "Add Note" }));
    expect(await screen.findByText("Saved to the timeline.")).toBeInTheDocument();
    expect(calls.find((c) => c.path.endsWith("/notes"))?.body).toEqual({
      body: "Ordered a new roller.",
      is_public: false,
    });

    await user.click(screen.getByRole("radio", { name: "Reply to Requester" }));
    await user.type(screen.getByRole("textbox", { name: "Reply to the Requester" }), "On it.");
    await user.click(screen.getByRole("button", { name: "Send reply" }));
    await waitFor(() =>
      expect(calls.find((c) => c.path.endsWith("/replies"))?.body).toEqual({ body: "On it." }),
    );
  });

  it("says when a ticket cannot be seen", async () => {
    api({ "GET /api/v1/tickets/missing": { status: 404 } });
    renderApp("/tickets/missing");
    expect(await screen.findByRole("alert")).toHaveTextContent("This ticket does not exist");
  });
});
