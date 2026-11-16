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
  "/api/v1/modules": { status: 200, body: [{ key: "tasks", is_enabled: true }] },
  "/api/v1/companies": {
    status: 200,
    body: {
      items: [{ id: "c1", name: "Acme", is_default: true, is_active: true }],
      next_cursor: null,
    },
  },
};
const taskLookup = (id: string, name: string, over: Record<string, unknown> = {}) => ({
  id,
  name,
  is_active: true,
  is_closed: false,
  is_done: false,
  is_default: false,
  sort_order: 0,
  ...over,
});
const task = (over: Record<string, unknown> = {}) => ({
  id: "k1",
  company_id: "c1",
  title: "Patch the print server",
  description: "",
  status_id: "ts1",
  is_closed: false,
  priority_id: "tp1",
  tags: ["infra"],
  board_position: 0,
  created_at: "2026-11-01T09:00:00Z",
  updated_at: "2026-11-01T09:00:00Z",
  due_date: null,
  ...over,
});
const taskLookups = {
  "/api/v1/task-statuses": {
    status: 200,
    body: [taskLookup("ts1", "To do"), taskLookup("ts2", "Done", { is_closed: true })],
  },
  "/api/v1/task-priorities": { status: 200, body: [taskLookup("tp1", "Normal")] },
};

describe("Tasks", () => {
  it("shows the board and moves a Task with the Move to list", async () => {
    const calls = mockFetch({
      ...base,
      ...taskLookups,
      "GET /api/v1/tasks/board": {
        status: 200,
        body: [
          { status: taskLookup("ts1", "To do"), tasks: [task()], has_more: false },
          { status: taskLookup("ts2", "Done", { is_closed: true }), tasks: [], has_more: false },
        ],
      },
      "POST /api/v1/tasks/k1/status": { status: 200, body: task({ status_id: "ts2" }) },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/tasks");

    expect(await screen.findByRole("link", { name: "Patch the print server" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /To do/ })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.selectOptions(screen.getByLabelText("Move to: Patch the print server"), "ts2");
    await waitFor(() =>
      expect(calls.find((c) => c.path.endsWith("/status"))?.body).toEqual({ status_id: "ts2" }),
    );
  });

  it("creates a Task in the default Company", async () => {
    const calls = mockFetch({
      ...base,
      ...taskLookups,
      "/api/v1/analysts": { status: 200, body: { items: [{ id: "a1", full_name: "Ada Admin" }] } },
      "POST /api/v1/tasks": { status: 201, body: task({ id: "k2", title: "Order toner" }) },
      "GET /api/v1/tasks/k2": { status: 200, body: task({ id: "k2", title: "Order toner" }) },
      "GET /api/v1/tasks/k2/comments": { status: 200, body: [] },
    });
    const user = userEvent.setup();
    renderApp("/tasks/new");

    await user.type(await screen.findByLabelText(/^Title/), "Order toner");
    await user.type(screen.getByLabelText("Tags"), "supplies, office");
    await user.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(calls.find((c) => c.method === "POST")).toBeDefined());
    expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
      title: "Order toner",
      company_id: "c1",
      tags: ["supplies", "office"],
    });
    expect(await screen.findByRole("heading", { name: "Order toner" })).toBeInTheDocument();
  });

  it("links a ticket by Ticket Number and removes the link (FR-TSK-004)", async () => {
    let links: Record<string, unknown>[] = [];
    const calls = mockFetch({
      ...base,
      ...taskLookups,
      "/api/v1/analysts": { status: 200, body: { items: [] } },
      "GET /api/v1/tasks/k1": { status: 200, body: task() },
      "GET /api/v1/tasks/k1/comments": { status: 200, body: [] },
      "GET /api/v1/tasks/k1/links": () => ({ status: 200, body: links }),
      "GET /api/v1/tickets": {
        status: 200,
        body: { items: [{ id: "tk5", number: "UP-000005" }], next_cursor: null },
      },
      "POST /api/v1/tasks/k1/links": (_req, body) => {
        const link = {
          ...(body as object),
          number: "UP-000005",
          linked_at: "2026-11-16T12:00:00Z",
        };
        links = [link];
        return { status: 201, body: link };
      },
      "DELETE /api/v1/tasks/k1/links/ticket/tk5": () => {
        links = [];
        return { status: 204 };
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/tasks/k1");

    expect(await screen.findByText("Not linked to tickets or Changes.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Ticket Number"), "up-000005");
    await user.click(screen.getByRole("button", { name: "Link" }));

    expect(await screen.findByRole("link", { name: "UP-000005" })).toHaveAttribute(
      "href",
      "/tickets/tk5",
    );
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({
      kind: "ticket",
      target_id: "tk5",
    });
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Remove link to UP-000005" }));
    expect(await screen.findByText("Not linked to tickets or Changes.")).toBeInTheDocument();
  });

  it("says when no visible ticket has that Ticket Number", async () => {
    const calls = mockFetch({
      ...base,
      ...taskLookups,
      "/api/v1/analysts": { status: 200, body: { items: [] } },
      "GET /api/v1/tasks/k1": { status: 200, body: task() },
      "GET /api/v1/tasks/k1/comments": { status: 200, body: [] },
      "GET /api/v1/tasks/k1/links": { status: 200, body: [] },
      "GET /api/v1/tickets": { status: 200, body: { items: [], next_cursor: null } },
    });
    const user = userEvent.setup();
    renderApp("/tasks/k1");

    await user.type(await screen.findByLabelText("Ticket Number"), "UP-999999");
    await user.click(screen.getByRole("button", { name: "Link" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No ticket UP-999999 that you can see.",
    );
    expect(calls.some((c) => c.method === "POST")).toBe(false);
  });
});

describe("Knowledge", () => {
  const article = (over: Record<string, unknown> = {}) => ({
    id: "ar1",
    title: "Reset a password",
    body: "<p>Use the reset link.</p>",
    body_text: "Use the reset link.",
    tags: [],
    audience: "internal",
    is_published: false,
    is_archived: false,
    version: 1,
    view_count: 0,
    access: {},
    created_at: "2026-11-01T09:00:00Z",
    updated_at: "2026-11-01T09:00:00Z",
    ...over,
  });

  it("lists Articles and publishes a draft", async () => {
    let current = article();
    const calls = mockFetch({
      ...base,
      "GET /api/v1/knowledge/folders": { status: 200, body: [] },
      "GET /api/v1/knowledge/articles": () => ({
        status: 200,
        body: { items: [current], next_cursor: null },
      }),
      "GET /api/v1/knowledge/articles/ar1": () => ({ status: 200, body: current }),
      "PUT /api/v1/knowledge/articles/ar1/published": () => {
        current = article({ is_published: true, updated_at: "2026-11-02T09:00:00Z" });
        return { status: 200, body: current };
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/knowledge");

    await user.click(await screen.findByRole("link", { name: "Reset a password" }));
    expect(await screen.findByText("Use the reset link.")).toBeInTheDocument();
    expect(screen.getByText("Draft")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Publish" }));
    expect(await screen.findByText("Published.")).toBeInTheDocument();
    expect(calls.find((c) => c.path.endsWith("/published"))?.body).toEqual({ is_published: true });
  });
});

describe("Service Status", () => {
  const level = (id: string, name: string, over: Record<string, unknown> = {}) => ({
    id,
    name,
    severity_order: 0,
    counts_as_downtime: false,
    is_default: false,
    is_active: true,
    ...over,
  });
  const service = { id: "sv1", name: "Email", description: "", sort_order: 0, is_active: true };

  it("shows each service's impact level", async () => {
    mockFetch({
      ...base,
      "GET /api/v1/service-status": {
        status: 200,
        body: [{ service, status: level("l2", "Major outage", { counts_as_downtime: true }) }],
      },
    });
    const { container } = renderApp("/service-status");
    expect(await screen.findByText("Major outage")).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: /Email/ })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("opens an Incident for the affected services", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/service-status/services": { status: 200, body: [service] },
      "GET /api/v1/service-status/impact-levels": {
        status: 200,
        body: [
          level("l1", "Operational", { is_default: true }),
          level("l2", "Major outage", { counts_as_downtime: true }),
        ],
      },
      "GET /api/v1/service-status/incident-statuses": {
        status: 200,
        body: [
          {
            id: "is1",
            name: "Investigating",
            is_resolved: false,
            is_default: true,
            sort_order: 0,
            is_active: true,
          },
        ],
      },
      "POST /api/v1/service-status/incidents": {
        status: 201,
        body: {
          incident: {
            id: "i1",
            title: "Mail down",
            status_id: "is1",
            is_resolved: false,
            services: [],
            started_at: "2026-11-09T10:00:00Z",
          },
          updates: [],
        },
      },
      "GET /api/v1/service-status/incidents/i1": {
        status: 200,
        body: {
          incident: {
            id: "i1",
            title: "Mail down",
            status_id: "is1",
            is_resolved: false,
            services: [],
            started_at: "2026-11-09T10:00:00Z",
          },
          updates: [],
        },
      },
    });
    const user = userEvent.setup();
    renderApp("/service-status/incidents/new");

    await user.type(await screen.findByLabelText(/^Title/), "Mail down");
    await user.selectOptions(await screen.findByLabelText("Email"), "l2");
    await user.click(screen.getByRole("button", { name: "Open an Incident" }));

    await waitFor(() => expect(calls.find((c) => c.method === "POST")).toBeDefined());
    expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
      title: "Mail down",
      services: [{ service_id: "sv1", impact_level_id: "l2" }],
    });
    expect(await screen.findByRole("heading", { name: "Mail down" })).toBeInTheDocument();
  });
});

describe("Morning Checks", () => {
  it("records the result of a Check", async () => {
    const check = {
      id: "ck1",
      company_id: "c1",
      name: "Backups ran",
      description: "",
      check_group_name: null,
      is_active: true,
      sort_order: 0,
    };
    const calls = mockFetch({
      ...base,
      "GET /api/v1/morning-checks/results": {
        status: 200,
        body: { check_date: "2026-11-09", today: "2026-11-09", items: [{ check, result: null }] },
      },
      "GET /api/v1/morning-checks/statuses": {
        status: 200,
        body: [
          {
            id: "st1",
            name: "OK",
            colour: "green",
            requires_notes: false,
            sort_order: 0,
            is_active: true,
          },
          {
            id: "st2",
            name: "Failed",
            colour: "red",
            requires_notes: true,
            sort_order: 1,
            is_active: true,
          },
        ],
      },
      "POST /api/v1/morning-checks/results": { status: 201, body: { id: "r1" } },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/morning-checks");

    expect(await screen.findByText("Backups ran")).toBeInTheDocument();
    expect(screen.getByText("Not recorded")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.selectOptions(screen.getByLabelText("Status: Backups ran"), "st2");
    await user.type(screen.getByLabelText("Notes: Backups ran"), "tape drive offline");
    await user.click(screen.getByRole("button", { name: "Record" }));

    expect(await screen.findByText("Recorded.")).toBeInTheDocument();
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({
      check_id: "ck1",
      status_id: "st2",
      notes: "tape drive offline",
      check_date: "2026-11-09",
    });
  });
});
