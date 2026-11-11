import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axeViolations, mockFetch, renderApp } from "@/test/render";

afterEach(() => {
  vi.unstubAllGlobals();
});

// A month is 42 days of markup: axe needs longer than the default 5 s on a busy machine.
vi.setConfig({ testTimeout: 30_000 });

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
  "/api/v1/modules": { status: 200, body: [{ key: "calendar", is_enabled: true }] },
  "/api/v1/companies": {
    status: 200,
    body: {
      items: [{ id: "c1", name: "Acme", is_default: true, is_active: true }],
      next_cursor: null,
    },
  },
  "GET /api/v1/calendar/categories": {
    status: 200,
    body: [
      { id: "k1", name: "Meetings", colour: "#2563eb", is_active: true, system_key: null },
      { id: "k2", name: "Contracts", colour: "#7c3aed", is_active: true, system_key: "contracts" },
    ],
  },
};
const event = (over: Record<string, unknown> = {}) => ({
  id: "e1",
  company_id: "c1",
  title: "CAB meeting",
  description: "",
  category_id: "k1",
  category_name: "Meetings",
  colour: "#2563eb",
  all_day: true,
  start_date: "2026-11-12",
  end_date: "2026-11-12",
  starts_at: null,
  ends_at: null,
  location: null,
  source: null,
  editable: true,
  created_at: "2026-11-01T09:00:00Z",
  updated_at: "2026-11-01T09:00:00Z",
  ...over,
});
const generated = event({
  id: "e2",
  title: "Notice date: CON-1 Copiers",
  category_id: "k2",
  category_name: "Contracts",
  colour: "#7c3aed",
  start_date: "2026-11-20",
  end_date: "2026-11-20",
  source: { kind: "contract", id: "con-1", date: "notice_date" },
  editable: false,
});

describe("Calendar", () => {
  it("shows a month with events per day and moves between months", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/calendar/events": {
        status: 200,
        body: { items: [event(), generated], truncated: false },
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/calendar?month=2026-11");

    expect(await screen.findByRole("heading", { name: "November 2026" })).toBeInTheDocument();
    await screen.findByRole("button", { name: /CAB meeting/ });
    const day = screen.getByRole("listitem", { name: /Thursday, November 12, 2026/ });
    expect(within(day).getByRole("button", { name: /CAB meeting/ })).toHaveTextContent("Meetings");
    expect(screen.getByRole("button", { name: /Notice date/ })).toHaveTextContent("generated");
    expect(await axeViolations(container)).toEqual([]);
    const first = calls.find((c) => c.path === "/api/v1/calendar/events");
    expect(first).toBeDefined();

    await user.click(screen.getByRole("link", { name: "Next month" }));
    expect(await screen.findByRole("heading", { name: "December 2026" })).toBeInTheDocument();
  });

  it("opens generated events read-only with a link to their source", async () => {
    mockFetch({
      ...base,
      "GET /api/v1/calendar/events": {
        status: 200,
        body: { items: [generated], truncated: false },
      },
    });
    const user = userEvent.setup();
    renderApp("/calendar?month=2026-11");

    await user.click(await screen.findByRole("button", { name: /Notice date/ }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("link", { name: "Open the Contract" })).toHaveAttribute(
      "href",
      "/contracts/con-1",
    );
    expect(within(dialog).queryByRole("button", { name: "Delete event" })).toBeNull();
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("creates an all-day event and deletes a manual one", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/calendar/events": { status: 200, body: { items: [event()], truncated: false } },
      "POST /api/v1/calendar/events": { status: 201, body: event({ id: "e3", title: "Freeze" }) },
      "DELETE /api/v1/calendar/events/e1": { status: 204 },
    });
    const user = userEvent.setup();
    renderApp("/calendar?month=2026-11");

    await user.click(await screen.findByRole("button", { name: "New event" }));
    const dialog = await screen.findByRole("dialog");
    expect(await axeViolations(dialog)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Create" }));
    expect(await within(dialog).findByText("Enter a title.")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText(/^Title/), "Freeze");
    await user.selectOptions(within(dialog).getByLabelText(/^Calendar Category/), "k1");
    expect(
      within(within(dialog).getByLabelText(/^Calendar Category/)).queryByRole("option", {
        name: "Contracts",
      }),
    ).toBeNull();
    await user.click(within(dialog).getByLabelText("All day"));
    await user.clear(within(dialog).getByLabelText(/^Start date/));
    await user.type(within(dialog).getByLabelText(/^Start date/), "2026-11-28");
    await user.clear(within(dialog).getByLabelText(/^End date/));
    await user.type(within(dialog).getByLabelText(/^End date/), "2026-11-30");
    await user.click(within(dialog).getByRole("button", { name: "Create" }));
    await waitFor(() =>
      expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
        title: "Freeze",
        category_id: "k1",
        company_id: "c1",
        all_day: true,
        start_date: "2026-11-28",
        end_date: "2026-11-30",
      }),
    );

    await user.click(await screen.findByRole("button", { name: /CAB meeting/ }));
    const edit = await screen.findByRole("dialog");
    await user.click(within(edit).getByRole("button", { name: "Delete event" }));
    await user.click(within(edit).getByRole("button", { name: "Confirm delete" }));
    await waitFor(() => expect(calls.find((c) => c.method === "DELETE")).toBeDefined());
  });
});
