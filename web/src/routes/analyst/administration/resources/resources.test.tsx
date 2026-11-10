import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axeViolations, mockFetch, renderApp } from "@/test/render";

afterEach(() => {
  vi.unstubAllGlobals();
});

const session = {
  analyst: { id: "a1", full_name: "Ada Admin" },
  csrf_token: "csrf-1",
  is_admin: true,
  capabilities: [],
  all_companies: true,
  company_ids: [],
  totp_enrollment_required: false,
};
const base = {
  "/api/v1/session": { status: 200, body: session },
  "/api/v1/modules": { status: 200, body: [{ key: "tickets", is_enabled: true }] },
};
const lookup = (id: string, name: string, over: Record<string, unknown> = {}) => ({
  id,
  name,
  is_active: true,
  is_closed: false,
  is_default: false,
  pauses_sla: false,
  sort_order: 0,
  number_prefix: null,
  response_minutes: null,
  resolution_minutes: null,
  sla_calendar_id: null,
  ...over,
});
const company = {
  id: "c1",
  name: "Acme",
  ticket_code: "ACM",
  sender_domains: ["acme.example"],
  is_default: true,
  is_active: true,
};

describe("Administration reference data", () => {
  it("lists Ticket Statuses and adds a closed Status", async () => {
    const rows = [lookup("s1", "Open", { is_default: true })];
    const calls = mockFetch({
      ...base,
      "GET /api/v1/ticket-statuses": () => ({ status: 200, body: rows }),
      "POST /api/v1/ticket-statuses": (_req, body) => {
        rows.push(lookup("s2", (body as { name: string }).name, { is_closed: true }));
        return { status: 201, body: rows[1] };
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/ticket-statuses");

    expect(await screen.findByText("Open")).toBeInTheDocument();
    expect(screen.getByText("Default for new tickets")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Add Status" }));
    await user.type(screen.getByLabelText(/^Name/), "Resolved");
    await user.click(screen.getByLabelText("Closed"));
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("Resolved")).toBeInTheDocument();
    const post = calls.find((c) => c.method === "POST");
    expect(post?.body).toMatchObject({ name: "Resolved", is_closed: true, is_active: true });
    expect(post?.headers.get("x-csrf-token")).toBe("csrf-1");
  });

  it("edits SLA targets of a Priority and shows a server refusal", async () => {
    mockFetch({
      ...base,
      "GET /api/v1/ticket-priorities": {
        status: 200,
        body: [lookup("p1", "High", { response_minutes: 30, resolution_minutes: 240 })],
      },
      "PUT /api/v1/ticket-priorities/p1": {
        status: 422,
        body: {
          type: "https://cooper.dev/problems/validation",
          status: 422,
          detail: "response exceeds resolution",
        },
      },
    });
    const user = userEvent.setup();
    renderApp("/administration/ticket-priorities");

    expect(await screen.findByText("Response 30 min, resolution 240 min")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Edit High" }));
    const response = screen.getByLabelText(/SLA response target/);
    await user.clear(response);
    await user.type(response, "500");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("response exceeds resolution");
  });
});

describe("Administration organisation", () => {
  it("creates a Role from grouped Capabilities", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/roles": { status: 200, body: { items: [], next_cursor: null } },
      "GET /api/v1/capabilities": {
        status: 200,
        body: ["tickets.read", "tickets.update", "analysts.manage"],
      },
      "POST /api/v1/roles": {
        status: 201,
        body: { id: "r1", name: "Dispatcher", description: "", capabilities: [] },
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/roles");

    await user.click(await screen.findByRole("button", { name: "Add Role" }));
    await user.type(screen.getByLabelText(/^Name/), "Dispatcher");
    await user.click(await screen.findByLabelText("tickets.read"));
    await user.click(screen.getByLabelText("analysts.manage"));
    await user.click(screen.getByLabelText("tickets.read")); // untick again
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(calls.find((c) => c.method === "POST")).toBeDefined());
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({
      name: "Dispatcher",
      description: "",
      capabilities: ["analysts.manage"],
    });
  });

  it("creates a Company with Sender Domains split on commas", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/companies": { status: 200, body: { items: [company], next_cursor: null } },
      "POST /api/v1/companies": { status: 201, body: { ...company, id: "c2", name: "Globex" } },
    });
    const user = userEvent.setup();
    renderApp("/administration/companies");

    expect(await screen.findByText("acme.example")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add Company" }));
    await user.type(screen.getByLabelText(/^Name/), "Globex");
    await user.type(screen.getByLabelText(/^Ticket Code/), "GLX");
    await user.type(
      screen.getByLabelText(/^Sender Domains/),
      "globex.example, mail.globex.example",
    );
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(calls.find((c) => c.method === "POST")).toBeDefined());
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({
      name: "Globex",
      ticket_code: "GLX",
      sender_domains: ["globex.example", "mail.globex.example"],
    });
  });

  it("saves the members of a Team", async () => {
    const team = {
      id: "t1",
      name: "Service Desk",
      description: "",
      is_active: true,
      analyst_ids: ["a1"],
      role_ids: [],
      company_access: { all: true, company_ids: [] },
    };
    const calls = mockFetch({
      ...base,
      "GET /api/v1/teams": { status: 200, body: { items: [team], next_cursor: null } },
      "GET /api/v1/analysts": {
        status: 200,
        body: {
          items: [
            { id: "a1", full_name: "Ada Admin" },
            { id: "a2", full_name: "Grace Hopper" },
          ],
        },
      },
      "GET /api/v1/roles": { status: 200, body: { items: [], next_cursor: null } },
      "GET /api/v1/companies": { status: 200, body: { items: [company], next_cursor: null } },
      "PUT /api/v1/teams/t1/members": { status: 200, body: team },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/teams");

    await user.click(await screen.findByRole("button", { name: "Edit Service Desk" }));
    await user.click(await screen.findByLabelText("Grace Hopper"));
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Save members" }));

    await waitFor(() =>
      expect(calls.find((c) => c.path.endsWith("/members"))?.body).toEqual({
        analyst_ids: ["a1", "a2"],
      }),
    );
  });
});
