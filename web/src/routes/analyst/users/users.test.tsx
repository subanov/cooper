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
const lookup = (id: string, name: string) => ({
  id,
  name,
  is_active: true,
  is_closed: false,
  is_default: false,
  pauses_sla: false,
  sort_order: 0,
});
const analyst = (over: Record<string, unknown> = {}) => ({
  id: "an1",
  username: "grace",
  email: "grace@acme.example",
  full_name: "Grace Hopper",
  is_admin: false,
  is_active: true,
  has_password: true,
  totp_enabled: true,
  signature: "",
  role_ids: ["r1"],
  company_access: { all: false, company_ids: ["c1"] },
  created_at: "2026-11-01T09:00:00Z",
  updated_at: "2026-11-02T09:00:00Z",
  ...over,
});
const company = {
  id: "c1",
  name: "Acme",
  ticket_code: "ACM",
  sender_domains: [],
  is_default: true,
  is_active: true,
};
const base = {
  "/api/v1/session": { status: 200, body: session },
  "/api/v1/modules": { status: 200, body: [{ key: "tickets", is_enabled: true }] },
  "/api/v1/companies": { status: 200, body: { items: [company], next_cursor: null } },
  "/api/v1/roles": {
    status: 200,
    body: {
      items: [
        { id: "r1", name: "Service Desk", description: "Works tickets", capabilities: [] },
        { id: "r2", name: "Auditor", description: "", capabilities: [] },
      ],
      next_cursor: null,
    },
  },
};

describe("New ticket", () => {
  it("creates a ticket with only the fields filled in and opens it", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/tickets": { status: 200, body: { items: [], next_cursor: null } },
      "/api/v1/ticket-statuses": { status: 200, body: [lookup("s1", "Open")] },
      "/api/v1/ticket-priorities": { status: 200, body: [lookup("p1", "High")] },
      "/api/v1/ticket-types": { status: 200, body: [lookup("ty1", "Incident")] },
      "/api/v1/ticket-origins": { status: 200, body: [lookup("o1", "Phone")] },
      "/api/v1/ticket-categories": { status: 200, body: [] },
      "/api/v1/resolution-codes": { status: 200, body: [] },
      "/api/v1/analysts": { status: 200, body: { items: [{ id: "a1", full_name: "Ada Admin" }] } },
      "/api/v1/teams": { status: 200, body: { items: [] } },
      "/api/v1/requesters": {
        status: 200,
        body: { items: [{ id: "rq1", full_name: "Rita Requester", email: "rita@acme.example" }] },
      },
      "POST /api/v1/tickets": { status: 201, body: { id: "tk9", number: "ACM-9", subject: "x" } },
      "GET /api/v1/tickets/tk9": {
        status: 404,
        body: { type: "https://cooper.dev/problems/not-found", status: 404 },
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/tickets");

    await user.click(await screen.findByRole("link", { name: "New ticket" }));
    const form = await screen.findByRole("form", { name: "New ticket" });
    expect(form).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Create ticket" }));
    expect(await screen.findByText("Enter a subject.")).toBeInTheDocument();

    await user.type(screen.getByLabelText(/^Subject/), "Printer jam");
    await user.selectOptions(await screen.findByLabelText("Requester"), "rq1");
    await user.selectOptions(screen.getByLabelText("Priority"), "p1");
    await user.click(screen.getByRole("button", { name: "Create ticket" }));

    await waitFor(() => expect(calls.find((c) => c.method === "POST")).toBeDefined());
    const post = calls.find((c) => c.method === "POST");
    expect(post?.body).toEqual({
      subject: "Printer jam",
      company_id: "c1",
      requester_id: "rq1",
      priority_id: "p1",
    });
    expect(post?.headers.get("x-csrf-token")).toBe("csrf-1");
    await waitFor(() =>
      expect(calls.some((c) => c.method === "GET" && c.path === "/api/v1/tickets/tk9")).toBe(true),
    );
  });

  it("shows why the server refused the ticket", async () => {
    mockFetch({
      ...base,
      "/api/v1/ticket-priorities": { status: 200, body: [] },
      "/api/v1/ticket-types": { status: 200, body: [] },
      "/api/v1/ticket-origins": { status: 200, body: [] },
      "/api/v1/ticket-categories": { status: 200, body: [] },
      "/api/v1/ticket-statuses": { status: 200, body: [] },
      "/api/v1/analysts": { status: 200, body: { items: [] } },
      "/api/v1/teams": { status: 200, body: { items: [] } },
      "/api/v1/requesters": { status: 200, body: { items: [] } },
      "GET /api/v1/tickets": { status: 200, body: { items: [], next_cursor: null } },
      "POST /api/v1/tickets": {
        status: 403,
        body: {
          type: "https://cooper.dev/problems/forbidden",
          status: 403,
          detail: "no access to Company",
        },
      },
    });
    const user = userEvent.setup();
    renderApp("/tickets/new");
    await user.type(await screen.findByLabelText(/^Subject/), "Printer jam");
    await user.click(screen.getByRole("button", { name: "Create ticket" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("no access to Company");
  });
});

describe("Users", () => {
  it("lists Analysts and creates one", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/analysts": {
        status: 200,
        body: { items: [analyst({ is_admin: true })], next_cursor: null },
      },
      "POST /api/v1/analysts": {
        status: 201,
        body: analyst({ id: "an2", full_name: "Alan Turing" }),
      },
      "GET /api/v1/analysts/an2": {
        status: 200,
        body: analyst({ id: "an2", full_name: "Alan Turing" }),
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/users");

    expect(await screen.findByRole("link", { name: "Grace Hopper" })).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("link", { name: "New Analyst" }));
    // After navigation focus moves to the page heading; wait for it before typing.
    await waitFor(() => expect(screen.getByRole("heading", { level: 1 })).toHaveFocus());
    await user.type(await screen.findByLabelText(/^Full name/), "Alan Turing");
    await user.type(screen.getByLabelText(/^Username/), "alan");
    await user.type(screen.getByLabelText(/^Email/), "alan@acme.example");
    await user.type(screen.getByLabelText("Initial password"), "correct horse battery");
    await user.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(calls.find((c) => c.method === "POST")).toBeDefined());
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({
      username: "alan",
      email: "alan@acme.example",
      full_name: "Alan Turing",
      is_admin: false,
      password: "correct horse battery",
    });
    expect(await screen.findByRole("heading", { name: "Alan Turing" })).toBeInTheDocument();
  });

  it("edits an Analyst's Roles and deactivates the Analyst", async () => {
    let current = analyst();
    const calls = mockFetch({
      ...base,
      "GET /api/v1/analysts/an1": () => ({ status: 200, body: current }),
      "GET /api/v1/analysts": { status: 200, body: { items: [current], next_cursor: null } },
      "PUT /api/v1/analysts/an1/roles": (_req, body) => {
        current = analyst({
          role_ids: (body as { role_ids: string[] }).role_ids,
          updated_at: "2026-11-03T09:00:00Z",
        });
        return { status: 200, body: current };
      },
      "POST /api/v1/analysts/an1/deactivate": () => {
        current = analyst({ is_active: false, updated_at: "2026-11-04T09:00:00Z" });
        return { status: 200, body: current };
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/users/analysts/an1");

    expect(await screen.findByRole("heading", { name: /Grace Hopper/ })).toBeInTheDocument();
    expect(await screen.findByLabelText(/Auditor/)).not.toBeChecked();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByLabelText(/Auditor/));
    await user.click(screen.getByRole("button", { name: "Save Roles" }));
    await waitFor(() =>
      expect(calls.find((c) => c.path.endsWith("/roles") && c.method === "PUT")?.body).toEqual({
        role_ids: ["r1", "r2"],
      }),
    );

    await user.click(screen.getByRole("button", { name: "Deactivate" }));
    expect(await screen.findByRole("button", { name: "Reactivate" })).toBeInTheDocument();
  });

  it("creates a Requester and sends a Portal invitation", async () => {
    const requester = {
      id: "rq1",
      company_id: "c1",
      full_name: "Rita Requester",
      email: "rita@acme.example",
      phone: null,
      username: null,
      is_active: true,
      created_at: "2026-11-01T09:00:00Z",
      updated_at: "2026-11-01T09:00:00Z",
    };
    const calls = mockFetch({
      ...base,
      "GET /api/v1/requesters": { status: 200, body: { items: [], next_cursor: null } },
      "POST /api/v1/requesters": { status: 201, body: requester },
      "GET /api/v1/requesters/rq1": { status: 200, body: requester },
      "POST /api/v1/requesters/rq1/portal-invitation": { status: 202 },
    });
    const user = userEvent.setup();
    renderApp("/users/requesters/new");

    await user.type(await screen.findByLabelText(/^Full name/), "Rita Requester");
    await user.type(screen.getByLabelText(/^Email/), "rita@acme.example");
    await user.click(screen.getByRole("button", { name: "Create" }));

    await user.click(await screen.findByRole("button", { name: "Send Portal invitation" }));
    expect(await screen.findByText("Invitation sent to rita@acme.example.")).toBeInTheDocument();
    expect(calls.find((c) => c.method === "POST" && c.path === "/api/v1/requesters")?.body).toEqual(
      {
        full_name: "Rita Requester",
        email: "rita@acme.example",
        phone: null,
        username: null,
        company_id: "c1",
      },
    );
  });
});
