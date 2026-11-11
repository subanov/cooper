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
const lookup = (id: string, name: string, over: Record<string, unknown> = {}) => ({
  id,
  name,
  is_closed: false,
  is_active: true,
  sort_order: 0,
  ...over,
});
const base = {
  "/api/v1/session": { status: 200, body: session },
  "/api/v1/modules": { status: 200, body: [{ key: "contracts", is_enabled: true }] },
  "/api/v1/companies": {
    status: 200,
    body: {
      items: [{ id: "c1", name: "Acme", is_default: true, is_active: true }],
      next_cursor: null,
    },
  },
  "/api/v1/analysts": { status: 200, body: { items: [{ id: "a1", full_name: "Ada Admin" }] } },
  "GET /api/v1/contract-statuses": {
    status: 200,
    body: [lookup("cs1", "Live"), lookup("cs2", "Terminated", { is_closed: true })],
  },
  "GET /api/v1/payment-schedules": { status: 200, body: [lookup("ps1", "Monthly")] },
  "GET /api/v1/supplier-types": { status: 200, body: [lookup("st1", "Software")] },
  "GET /api/v1/supplier-statuses": { status: 200, body: [lookup("ss1", "Approved")] },
};
const supplier = (over: Record<string, unknown> = {}) => ({
  id: "s1",
  company_id: "c1",
  legal_name: "Initech Holdings Ltd",
  trading_name: "Initech",
  registration_number: null,
  vat_number: null,
  supplier_type_id: "st1",
  supplier_status_id: "ss1",
  notes: "",
  is_active: true,
  created_at: "2026-11-01T09:00:00Z",
  updated_at: "2026-11-01T09:00:00Z",
  ...over,
});
const contract = (over: Record<string, unknown> = {}) => ({
  id: "k1",
  company_id: "c1",
  contract_number: "CON-001",
  title: "Licences",
  description: "",
  party: "supplier",
  supplier_id: "s1",
  requester_id: null,
  owner_id: "a1",
  status_id: "cs1",
  is_active: true,
  start_date: "2026-07-01",
  end_date: "2027-06-30",
  notice_period: { amount: 3, unit: "months" },
  notice_date: "2027-03-31",
  notice_status: "upcoming",
  value: "12000.50",
  currency: "GBP",
  payment_schedule_id: "ps1",
  cost_centre_id: null,
  dms_link: null,
  personal_data_transferred: false,
  dpia_required: false,
  dpia_completed_on: null,
  dpia_link: null,
  created_at: "2026-11-01T09:00:00Z",
  updated_at: "2026-11-01T09:00:00Z",
  ...over,
});
const suppliers = {
  "GET /api/v1/suppliers": { status: 200, body: { items: [supplier()], next_cursor: null } },
};

describe("Contracts", () => {
  it("lists Contracts with their Notice Date and highlights upcoming ones", async () => {
    const calls = mockFetch({
      ...base,
      ...suppliers,
      "GET /api/v1/contracts": {
        status: 200,
        body: {
          items: [
            contract(),
            contract({
              id: "k2",
              contract_number: "CON-002",
              title: "Old lease",
              status_id: "cs2",
              is_active: false,
              notice_status: "none",
            }),
          ],
          next_cursor: null,
        },
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/contracts");

    expect(
      await screen.findByRole("link", { name: "CON-001" }, { timeout: 5000 }),
    ).toBeInTheDocument();
    expect(screen.getByText("Notice Date soon")).toBeInTheDocument();
    expect(screen.getByText("Closed")).toBeInTheDocument();
    expect(await screen.findAllByText("Initech")).not.toHaveLength(0);
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByLabelText("Upcoming Notice Dates only"));
    await waitFor(() =>
      expect(calls.some((c) => c.path === "/api/v1/contracts" && c.method === "GET")).toBe(true),
    );
  }, 20_000);

  it("creates a Contract with a notice period, value and data protection fields", async () => {
    const calls = mockFetch({
      ...base,
      ...suppliers,
      "POST /api/v1/contracts": { status: 201, body: contract({ id: "k3" }) },
      "GET /api/v1/contracts/k3": { status: 200, body: contract({ id: "k3" }) },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/contracts/new");

    await user.type(
      await screen.findByLabelText(/^Contract number/, {}, { timeout: 5000 }),
      "CON-001",
    );
    await user.type(screen.getByLabelText(/^Title/), "Licences");
    await screen.findByRole("option", { name: "Initech" });
    await user.selectOptions(screen.getByLabelText("Supplier"), "s1");
    await user.type(screen.getByLabelText("End date"), "2027-06-30");
    await user.type(screen.getByLabelText("Notice period"), "3");
    await user.type(screen.getByLabelText("Value"), "12000.5");
    await user.type(screen.getByLabelText("Currency"), "GBP");
    await user.click(screen.getByLabelText("Personal data is transferred"));
    await user.click(screen.getByLabelText("DPIA required"));
    await user.type(screen.getByLabelText("DPIA link"), "https://dms.example/dpia/1");
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(calls.find((c) => c.method === "POST")).toBeDefined());
    expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
      company_id: "c1",
      contract_number: "CON-001",
      title: "Licences",
      supplier_id: "s1",
      end_date: "2027-06-30",
      notice_period: { amount: 3, unit: "months" },
      value: "12000.5",
      currency: "GBP",
      personal_data_transferred: true,
      dpia_required: true,
      dpia_link: "https://dms.example/dpia/1",
    });
    expect(await screen.findByText("Notice Date soon")).toBeInTheDocument();
  }, 20_000);

  it("shows a Supplier with its Contacts and adds one", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/suppliers/s1": { status: 200, body: supplier() },
      "GET /api/v1/suppliers/s1/contacts": {
        status: 200,
        body: [
          {
            id: "p1",
            company_id: "c1",
            supplier_id: "s1",
            full_name: "Pat Jones",
            job_title: "Account Manager",
            email: "pat@initech.example",
            phone: null,
            mobile: null,
            notes: "",
            is_active: true,
            created_at: "2026-11-01T09:00:00Z",
            updated_at: "2026-11-01T09:00:00Z",
          },
        ],
      },
      "POST /api/v1/suppliers/s1/contacts": { status: 201, body: {} },
      "GET /api/v1/contracts": { status: 200, body: { items: [contract()], next_cursor: null } },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/suppliers/s1");

    expect(await screen.findByText("Pat Jones", {}, { timeout: 5000 })).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "CON-001" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Add Contact" }));
    await user.type(screen.getByLabelText(/^Full name/), "Sam Lee");
    await user.type(screen.getByLabelText("Email"), "sam@initech.example");
    await user.click(screen.getAllByRole("button", { name: "Save" }).at(-1) as HTMLElement);
    await waitFor(() =>
      expect(
        calls.find((c) => c.path === "/api/v1/suppliers/s1/contacts" && c.method === "POST")?.body,
      ).toMatchObject({
        full_name: "Sam Lee",
        email: "sam@initech.example",
      }),
    );
  }, 20_000);

  it("lets Administrators add Contract Statuses that close Contracts", async () => {
    const calls = mockFetch({
      ...base,
      "POST /api/v1/contract-statuses": { status: 201, body: lookup("cs3", "Expired") },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/contract-statuses");

    expect(await screen.findByText("Terminated", {}, { timeout: 5000 })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Add Contract Status" }));
    await user.type(screen.getByLabelText(/^Name/), "Expired");
    await user.click(screen.getByLabelText("Closed: no reminders or Calendar Events"));
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
        name: "Expired",
        is_closed: true,
      }),
    );
  }, 20_000);
});
