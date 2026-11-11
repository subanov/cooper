import { screen, waitFor, within } from "@testing-library/react";
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
  "/api/v1/modules": { status: 200, body: [{ key: "assets", is_enabled: true }] },
  "/api/v1/companies": {
    status: 200,
    body: {
      items: [{ id: "c1", name: "Acme", is_default: true, is_active: true }],
      next_cursor: null,
    },
  },
  "/api/v1/cost-centres": {
    status: 200,
    body: {
      items: [{ id: "cc1", company_id: "c1", code: "IT", name: "IT", is_active: true }],
      next_cursor: null,
    },
  },
  "/api/v1/analysts": {
    status: 200,
    body: { items: [{ id: "a1", full_name: "Ada Admin" }], next_cursor: null },
  },
};
const row = (id: string, name: string, over: Record<string, unknown> = {}) => ({
  id,
  name,
  parent_id: null,
  is_retired: false,
  colour: null,
  sort_order: 0,
  is_active: true,
  ...over,
});
const lookups = {
  "GET /api/v1/asset-types": { status: 200, body: [row("t1", "Laptop")] },
  "GET /api/v1/asset-statuses": {
    status: 200,
    body: [row("s1", "In use"), row("s2", "Disposed", { is_retired: true })],
  },
  "GET /api/v1/asset-locations": {
    status: 200,
    body: [row("l1", "London"), row("l2", "Room 101", { parent_id: "l1" })],
  },
};
const asset = (over: Record<string, unknown> = {}) => ({
  id: "as1",
  company_id: "c1",
  hostname: "lt-0001",
  service_tag: "SN123",
  asset_tag: "A-0001",
  asset_type_id: "t1",
  asset_status_id: "s1",
  location_id: "l2",
  supplier_id: null,
  cost_centre_id: null,
  purchase_date: "2026-01-15",
  purchase_cost: "1299.00",
  purchase_currency: "GBP",
  warranty_expires_on: "2029-01-15",
  lease_starts_on: null,
  lease_expires_on: null,
  checkout: null,
  is_active: true,
  retired_on: null,
  retirement_reason: null,
  created_at: "2026-11-01T09:00:00Z",
  updated_at: "2026-11-01T09:00:00Z",
  ...over,
});
const detail = (current: () => Record<string, unknown>) => ({
  "GET /api/v1/assets/as1": () => ({ status: 200, body: current() }),
  "GET /api/v1/assets/as1/checkouts": { status: 200, body: [] },
  "GET /api/v1/assets/as1/links": { status: 200, body: [] },
  "GET /api/v1/assets/as1/history": {
    status: 200,
    body: [
      {
        id: "h1",
        at: "2026-11-01T09:00:00Z",
        actor: { kind: "analyst", id: "a1" },
        action: "created",
        field: "asset_tag",
        old_value: null,
        new_value: "A-0001",
      },
    ],
  },
});

describe("Assets", () => {
  it("lists active Assets with filters and asks for retired ones on request", async () => {
    const calls = mockFetch({
      ...base,
      ...lookups,
      "GET /api/v1/assets/summary": {
        status: 200,
        body: { active: 1, retired: 2, checked_out: 0, by_type: [], by_status: [] },
      },
      "GET /api/v1/assets": { status: 200, body: { items: [asset()], next_cursor: null } },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/assets");

    expect(
      await screen.findByRole("link", { name: "A-0001 · lt-0001" }, { timeout: 5000 }),
    ).toBeInTheDocument();
    expect(await screen.findByText(/1 Asset in the estate/)).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "London › Room 101" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByLabelText("Include retired"));
    await user.selectOptions(screen.getByLabelText("Location"), "l1");
    await waitFor(() =>
      expect(calls.some((c) => c.path === "/api/v1/assets" && c.method === "GET")).toBe(true),
    );
  });

  it("registers an Asset in the default Company", async () => {
    const calls = mockFetch({
      ...base,
      ...lookups,
      "POST /api/v1/assets": { status: 201, body: asset() },
      ...detail(() => asset()),
    });
    const user = userEvent.setup();
    const { container } = renderApp("/assets/new");

    await user.type(await screen.findByLabelText("Asset Tag"), "A-0001");
    await user.type(screen.getByLabelText("Hostname"), "lt-0001");
    await user.selectOptions(screen.getByLabelText("Asset Type"), "t1");
    await user.type(screen.getByLabelText(/^Purchase cost/), "1299");
    await user.type(screen.getByLabelText(/^Currency/), "gbp");
    expect(await axeViolations(container)).toEqual([]);
    expect(
      within(screen.getByLabelText("Asset Status")).queryByRole("option", { name: "Disposed" }),
    ).toBeNull();
    await user.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(calls.find((c) => c.method === "POST")).toBeDefined());
    expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
      company_id: "c1",
      asset_tag: "A-0001",
      hostname: "lt-0001",
      asset_type_id: "t1",
      purchase_cost: "1299",
      purchase_currency: "GBP",
    });
    expect(await screen.findByRole("heading", { name: "A-0001 · lt-0001" })).toBeInTheDocument();
  });

  it("checks an Asset out to an Analyst and back in", async () => {
    let current = asset();
    const calls = mockFetch({
      ...base,
      ...lookups,
      ...detail(() => current),
      "POST /api/v1/assets/as1/check-out": (_request, body) => {
        current = asset({
          checkout: {
            id: "co1",
            holder: { kind: "analyst", id: (body as { holder_id: string }).holder_id },
            checked_out_at: "2026-11-11T09:00:00Z",
            due_back_on: null,
          },
          updated_at: "2026-11-11T09:00:00Z",
        });
        return { status: 200, body: current };
      },
      "POST /api/v1/assets/as1/check-in": () => {
        current = asset({ updated_at: "2026-11-11T10:00:00Z" });
        return { status: 200, body: current };
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/assets/as1");

    expect(await screen.findByRole("heading", { name: "A-0001 · lt-0001" })).toBeInTheDocument();
    expect(await screen.findByRole("cell", { name: "asset_tag" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.selectOptions(screen.getByLabelText("Holder is a"), "analyst");
    await user.selectOptions(screen.getByLabelText("Holder"), "a1");
    await user.click(screen.getByRole("button", { name: "Check Out" }));
    await waitFor(() =>
      expect(calls.find((c) => c.path.endsWith("/check-out"))?.body).toMatchObject({
        holder_kind: "analyst",
        holder_id: "a1",
      }),
    );
    expect(await screen.findByText(/Checked out to Ada Admin/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Check In" }));
    await waitFor(() => expect(calls.some((c) => c.path.endsWith("/check-in"))).toBe(true));
  });

  it("retires an Asset with a reason and a retired status", async () => {
    const calls = mockFetch({
      ...base,
      ...lookups,
      ...detail(() => asset()),
      "POST /api/v1/assets/as1/retire": {
        status: 200,
        body: asset({ is_active: false, retired_on: "2026-11-11", retirement_reason: "Broken" }),
      },
    });
    const user = userEvent.setup();
    renderApp("/assets/as1");

    await screen.findByRole("heading", { name: "A-0001 · lt-0001" });
    await user.click(screen.getByRole("button", { name: "Retire" }));
    expect(await screen.findByText("Enter a reason.")).toBeInTheDocument();
    await user.type(screen.getByLabelText(/^Reason/), "Broken");
    await user.selectOptions(screen.getByLabelText("Retired status"), "s2");
    await user.click(screen.getByRole("button", { name: "Retire" }));
    await waitFor(() =>
      expect(calls.find((c) => c.path.endsWith("/retire"))?.body).toEqual({
        reason: "Broken",
        retired_on: null,
        asset_status_id: "s2",
      }),
    );
  });
});

describe("Asset administration", () => {
  it("adds a Location inside another", async () => {
    const calls = mockFetch({
      ...base,
      ...lookups,
      "POST /api/v1/asset-locations": { status: 201, body: row("l3", "Floor 2") },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/asset-locations");

    expect(await screen.findByText("London › Room 101")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Add Location" }));
    await user.type(screen.getByLabelText(/^Name/), "Floor 2");
    await user.selectOptions(screen.getByLabelText("Inside"), "l1");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
        name: "Floor 2",
        parent_id: "l1",
      }),
    );
  });

  it("marks an Asset Status as retired", async () => {
    const calls = mockFetch({
      ...base,
      ...lookups,
      "POST /api/v1/asset-statuses": { status: 201, body: row("s3", "Sold", { is_retired: true }) },
    });
    const user = userEvent.setup();
    renderApp("/administration/asset-statuses");

    expect(await screen.findByText("Disposed")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add Asset Status" }));
    await user.type(screen.getByLabelText(/^Name/), "Sold");
    await user.click(screen.getByLabelText("Retired"));
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
        name: "Sold",
        is_retired: true,
      }),
    );
  });
});
