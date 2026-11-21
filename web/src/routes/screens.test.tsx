import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  analystApi,
  article,
  asset,
  COMPANY_ID,
  contract,
  lookup,
  TASK_API,
  task,
  ticket,
} from "@/test/fixtures";
import { axeViolations, mockApi, renderApp } from "@/test/render";

const AT = "2026-03-01T09:00:00Z";

describe("home", () => {
  it("shows the Watchtower and the Analyst's own tickets", async () => {
    mockApi({
      ...analystApi(["tickets.read", "reporting.read"]),
      "/api/v1/tickets": { status: 200, body: { items: [ticket()] } },
      "/api/v1/watchtower": {
        status: 200,
        body: {
          scope: "all",
          generated_at: AT,
          today: "2026-03-01",
          tickets: {
            total_open: 7,
            unassigned: 2,
            high_priority: 1,
            by_status: [{ id: "s-open", name: "Open", colour: "#2563eb", count: 7 }],
          },
        },
      },
    });
    const { container } = renderApp("/");
    expect(await screen.findByRole("heading", { name: /Ada/, level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "Unassigned" })).toBeInTheDocument();
    expect(
      await screen.findByRole("link", { name: "Printer on floor 2 is offline" }),
    ).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("tasks", () => {
  it("shows the Board and moves a Task with Undo", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...analystApi(["tasks.read", "tasks.update", "tasks.create"]),
      ...TASK_API,
      [`POST /api/v1/tasks/${task().id}/status`]: {
        status: 200,
        body: task({ status_id: "ts-done" }),
      },
    });
    const { container } = renderApp("/tasks");
    const board = await screen.findByRole("region", { name: "Task Board" });
    expect(within(board).getByRole("region", { name: "To do, 1 Task" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    await user.click(
      within(board).getByRole("button", { name: `Move ${task().title} to another status` }),
    );
    await user.click(await screen.findByRole("menuitem", { name: "Done" }));
    await waitFor(() => expect(calls.some((c) => c.path.endsWith("/status"))).toBe(true));
    expect(await screen.findByText("Moved to Done.")).toBeInTheDocument();
  });

  it("opens a Task beside a list of the same Tasks", async () => {
    const user = userEvent.setup();
    const k = task();
    mockApi({
      ...analystApi(["tasks.read", "tasks.update"]),
      ...TASK_API,
      [`/api/v1/tasks/${k.id}`]: { status: 200, body: k },
      [`/api/v1/tasks/${k.id}/comments`]: { status: 200, body: [] },
      [`/api/v1/tasks/${k.id}/links`]: { status: 200, body: [] },
      [`/api/v1/tasks/${k.id}/history`]: { status: 200, body: [] },
      "GET /api/v1/tasks": { status: 200, body: { items: [] } },
    });
    const { container } = renderApp("/tasks");
    await user.click(await screen.findByRole("link", { name: k.title }));
    expect(await screen.findByRole("heading", { name: k.title, level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "To do" })).toBeInTheDocument();
    expect(await screen.findByRole("textbox", { name: /Title/ })).toHaveValue(k.title);
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("assets", () => {
  it("lists Assets with their custody and opens one", async () => {
    const user = userEvent.setup();
    const a = asset();
    mockApi({
      ...analystApi(["assets.read", "assets.update"]),
      "/api/v1/assets": { status: 200, body: { items: [a] } },
      "/api/v1/assets/summary": {
        status: 200,
        body: { active: 1, retired: 0, checked_out: 0, by_type: [], by_status: [] },
      },
      "/api/v1/asset-types": { status: 200, body: [] },
      "/api/v1/asset-statuses": { status: 200, body: [] },
      "/api/v1/asset-locations": { status: 200, body: [] },
      "/api/v1/cost-centres": { status: 200, body: { items: [] } },
      [`/api/v1/assets/${a.id}`]: { status: 200, body: a },
      [`/api/v1/assets/${a.id}/links`]: { status: 200, body: [] },
      [`/api/v1/assets/${a.id}/checkouts`]: { status: 200, body: [] },
      [`/api/v1/assets/${a.id}/history`]: { status: 200, body: [] },
    });
    const { container } = renderApp("/assets");
    expect(
      await screen.findByText("1 Asset in service · 0 checked out · 0 retired"),
    ).toBeInTheDocument();
    await user.click(await screen.findByRole("link", { name: "LT-0042" }));
    expect(await screen.findByRole("heading", { name: "LT-0042", level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check Out" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("asks for an identifier before registering an Asset", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...analystApi(["assets.read", "assets.create"]),
      "/api/v1/assets": { status: 200, body: { items: [] } },
      "/api/v1/assets/summary": {
        status: 200,
        body: { active: 0, retired: 0, checked_out: 0, by_type: [], by_status: [] },
      },
      "/api/v1/asset-types": { status: 200, body: [] },
      "/api/v1/asset-statuses": { status: 200, body: [] },
      "/api/v1/asset-locations": { status: 200, body: [] },
      "/api/v1/cost-centres": { status: 200, body: { items: [] } },
    });
    renderApp("/assets/new");
    await user.click(await screen.findByRole("button", { name: "Create" }));
    expect(
      await screen.findByText("Enter an Asset Tag, hostname or Service Tag."),
    ).toBeInTheDocument();
    expect(calls.some((c) => c.method === "POST")).toBe(false);
  });
});

describe("contracts", () => {
  it("lists Contracts with their Notice Date state", async () => {
    mockApi({
      ...analystApi(["contracts.read"]),
      "/api/v1/contracts": { status: 200, body: { items: [contract()] } },
      "/api/v1/contract-statuses": { status: 200, body: [] },
      "/api/v1/suppliers": { status: 200, body: { items: [] } },
    });
    const { container } = renderApp("/contracts");
    expect(await screen.findByRole("link", { name: "Printer lease" })).toBeInTheDocument();
    expect(screen.getByText("Notice Date soon")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Contracts sections" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("knowledge", () => {
  it("reads an Article beside the list and publishes with Undo", async () => {
    const user = userEvent.setup();
    const a = article({ is_published: false });
    const calls = mockApi({
      ...analystApi(["knowledge.read", "knowledge.write"]),
      "/api/v1/knowledge/articles": { status: 200, body: { items: [a] } },
      "/api/v1/knowledge/folders": { status: 200, body: [] },
      [`/api/v1/knowledge/articles/${a.id}`]: { status: 200, body: a },
      [`/api/v1/knowledge/articles/${a.id}/versions`]: { status: 200, body: [] },
      [`PUT /api/v1/knowledge/articles/${a.id}/published`]: {
        status: 200,
        body: { ...a, is_published: true },
      },
    });
    const { container } = renderApp("/knowledge");
    await user.click(await screen.findByRole("link", { name: a.title }));
    expect(await screen.findByRole("heading", { name: a.title, level: 2 })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Publish" }));
    await waitFor(() => expect(calls.some((c) => c.path.endsWith("/published"))).toBe(true));
    expect(await screen.findByText("Article published.")).toBeInTheDocument();
  });
});

describe("calendar", () => {
  it("shows the month as a table and switches to the list in the address", async () => {
    const user = userEvent.setup();
    mockApi({
      ...analystApi(["calendar.read"]),
      "/api/v1/calendar/categories": {
        status: 200,
        body: [{ id: "cc1", name: "Maintenance", colour: "#2563eb", is_active: true }],
      },
      "/api/v1/calendar/events": {
        status: 200,
        body: {
          truncated: false,
          items: [
            {
              id: "ev1",
              company_id: COMPANY_ID,
              title: "Patch night",
              description: "",
              category_id: "cc1",
              category_name: "Maintenance",
              colour: "#2563eb",
              all_day: true,
              start_date: "2026-03-10",
              end_date: "2026-03-10",
              editable: true,
              created_at: AT,
              updated_at: AT,
            },
          ],
        },
      },
    });
    const { container, router } = renderApp("/calendar?month=2026-03&view=month");
    expect(await screen.findByRole("table")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /Patch night/ })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("radio", { name: "List" }));
    await waitFor(() => expect(router.state.location.search).toMatchObject({ view: "list" }));
  });
});

describe("service status", () => {
  it("shows every Service with its Current Status", async () => {
    mockApi({
      ...analystApi(["service-status.read"]),
      "/api/v1/service-status": {
        status: 200,
        body: [
          {
            service: {
              id: "sv1",
              name: "Email",
              description: "",
              sort_order: 0,
              is_active: true,
              created_at: AT,
              updated_at: AT,
            },
            status: {
              id: "il1",
              name: "Operational",
              colour: "#16a34a",
              severity_order: 0,
              is_default: true,
            },
          },
        ],
      },
      "/api/v1/service-status/incidents": { status: 200, body: { items: [] } },
    });
    const { container } = renderApp("/service-status");
    expect(await screen.findByText("Every Service is running normally.")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Service Status sections" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("morning checks", () => {
  it("records a Check in the Round and asks for notes when the status needs them", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...analystApi(["morning-checks.read", "morning-checks.record"]),
      "/api/v1/morning-checks/statuses": {
        status: 200,
        body: [
          {
            id: "g",
            name: "Green",
            colour: "#28a745",
            requires_notes: false,
            sort_order: 0,
            is_active: true,
          },
          {
            id: "r",
            name: "Red",
            colour: "#dc3545",
            requires_notes: true,
            sort_order: 2,
            is_active: true,
          },
        ],
      },
      "/api/v1/morning-checks/results": {
        status: 200,
        body: {
          check_date: "2026-03-01",
          today: "2026-03-01",
          items: [
            {
              check: {
                id: "ck1",
                company_id: COMPANY_ID,
                name: "Backups completed",
                description: "",
                routing: {},
                sort_order: 0,
                is_active: true,
                in_round: true,
                created_at: AT,
                updated_at: AT,
              },
            },
          ],
        },
      },
      "POST /api/v1/morning-checks/results": { status: 201, body: {} },
    });
    const { container } = renderApp("/morning-checks/today");
    expect(await screen.findByText("0 of 1 recorded")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Round progress" })).toHaveAttribute(
      "aria-valuenow",
      "0",
    );
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("radio", { name: "Red" }));
    await user.click(screen.getByRole("button", { name: "Record" }));
    expect(await screen.findByText("This status needs notes.")).toBeInTheDocument();
    expect(calls.some((c) => c.method === "POST")).toBe(false);
    await user.type(
      screen.getByRole("textbox", { name: "Notes for Backups completed" }),
      "Job 3 failed",
    );
    await user.click(screen.getByRole("button", { name: "Record" }));
    await waitFor(() => expect(calls.some((c) => c.method === "POST")).toBe(true));
    expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
      check_id: "ck1",
      status_id: "r",
      notes: "Job 3 failed",
    });
  });
});

describe("users", () => {
  it("lists Analysts and opens one with Roles and Company access", async () => {
    const user = userEvent.setup();
    mockApi({
      ...analystApi(["analysts.manage"]),
      "/api/v1/roles": { status: 200, body: { items: [] } },
      "/api/v1/analysts/00000000-0000-7000-8000-0000000000a1": {
        status: 200,
        body: analystApi([])["GET /api/v1/analysts"].body.items[0],
      },
    });
    const { container } = renderApp("/users");
    await user.click(await screen.findByRole("link", { name: "Ada Lovelace" }));
    expect(
      await screen.findByRole("heading", { name: "Ada Lovelace", level: 2 }),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Roles" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Company access" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("administration", () => {
  it("shows Administration pages only to those who may use them", async () => {
    mockApi({
      ...analystApi(["system.manage"]),
      "/api/v1/settings": {
        status: 200,
        body: [
          {
            key: "general.instance_name",
            kind: "text",
            value: "Cooper",
            default: "Cooper",
            description: "Name.",
            constraints: {},
          },
        ],
      },
    });
    const { container } = renderApp("/administration");
    expect(await screen.findByRole("heading", { name: "System Settings" })).toBeInTheDocument();
    const sections = screen.getByRole("navigation", { name: "Administration sections" });
    expect(within(sections).queryByRole("link", { name: "Ticket Statuses" })).toBeNull();
    expect(await screen.findByLabelText("general.instance_name")).toHaveValue("Cooper");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("edits reference data in one consistent list", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...analystApi(["tickets.configure"]),
      "POST /api/v1/ticket-statuses": { status: 201, body: lookup("s-new", "Waiting") },
    });
    const { container } = renderApp("/administration/ticket-statuses");
    expect(await screen.findByRole("table", { name: "Ticket Statuses" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Add" }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText(/^Name/), "Waiting");
    await user.click(within(dialog).getByLabelText("Pauses the SLA"));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.some((c) => c.method === "POST")).toBe(true));
    expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
      name: "Waiting",
      pauses_sla: true,
      is_active: true,
    });
  });

  it("turns a Module off with a switch and offers Undo", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...analystApi(["system.manage"]),
      "PUT /api/v1/modules/tasks": { status: 200, body: {} },
    });
    renderApp("/administration/modules");
    await user.click(await screen.findByRole("switch", { name: "tasks Module" }));
    await waitFor(() => expect(calls.some((c) => c.method === "PUT")).toBe(true));
    expect(calls.find((c) => c.method === "PUT")?.body).toEqual({ is_enabled: false });
    expect(await screen.findByText("tasks is off.")).toBeInTheDocument();
  });
});
