import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axeViolations, mockFetch, renderApp } from "@/test/render";

// The forms have many fields; typing them all takes a while under a full parallel run.
vi.setConfig({ testTimeout: 20_000 });

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
const company = {
  id: "c1",
  name: "Acme",
  ticket_code: "ACM",
  sender_domains: ["acme.example"],
  is_default: true,
  is_active: true,
};
const base = {
  "/api/v1/session": { status: 200, body: session },
  "/api/v1/modules": { status: 200, body: [{ key: "tickets", is_enabled: true }] },
  "/api/v1/companies": { status: 200, body: { items: [company], next_cursor: null } },
  "/api/v1/departments": { status: 200, body: { items: [], next_cursor: null } },
  "/api/v1/teams": { status: 200, body: { items: [], next_cursor: null } },
  "/api/v1/ticket-origins": { status: 200, body: [] },
};

const mailbox = (over: Record<string, unknown> = {}) => ({
  id: "m1",
  name: "Service desk",
  address: "help@acme.example",
  provider: "imap",
  auth_mode: null,
  imap_host: "imap.acme.example",
  imap_port: 993,
  imap_security: "tls",
  imap_username: "help",
  folder: "INBOX",
  has_secret: true,
  company_id: "c1",
  department_id: null,
  origin_id: null,
  team_id: null,
  post_import_action: "mark_read",
  move_to_folder: null,
  unknown_sender_action: "create_requester",
  rejected_action: "delete",
  whitelist: [],
  is_active: true,
  health: {
    consecutive_failures: 3,
    last_error: "authentication failed",
    last_error_at: "2026-11-16T08:00:00Z",
    last_polled_at: "2026-11-16T08:00:00Z",
    last_success_at: "2026-11-15T08:00:00Z",
  },
  ...over,
});

describe("Mailboxes (FR-EML-001)", () => {
  it("adds an IMAP Mailbox with a Whitelist and sends the password once", async () => {
    const rows: ReturnType<typeof mailbox>[] = [];
    const calls = mockFetch({
      ...base,
      "GET /api/v1/mailboxes": () => ({ status: 200, body: rows }),
      "POST /api/v1/mailboxes": (_req, body) => {
        rows.push(mailbox({ ...(body as Record<string, unknown>), id: "m2", health: null }));
        return { status: 201, body: rows[0] };
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/mailboxes");

    expect(await screen.findByText("Nothing here yet.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add Mailbox" }));
    await user.type(screen.getByLabelText("Name"), "Service desk");
    await user.type(screen.getByLabelText("Email address"), "help@acme.example");
    await user.type(screen.getByLabelText("IMAP host"), "imap.acme.example");
    await user.type(screen.getByLabelText("IMAP username"), "help");
    await user.type(screen.getByLabelText("Password"), "s3cret");
    await waitFor(() =>
      expect(within(screen.getByLabelText("Company")).getByText("Acme")).toBeInTheDocument(),
    );
    await user.selectOptions(screen.getByLabelText("Company"), "c1");
    await user.selectOptions(screen.getByLabelText("After import"), "move");
    await user.type(screen.getByLabelText("Move to folder"), "Imported");
    await user.click(screen.getByRole("button", { name: "Add an entry" }));
    await user.type(screen.getByLabelText("Address or domain 1"), "spam.example");
    await user.selectOptions(screen.getByLabelText("Rule 1"), "block");
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("help@acme.example")).toBeInTheDocument();
    const post = calls.find((c) => c.method === "POST");
    expect(post?.body).toMatchObject({
      name: "Service desk",
      provider: "imap",
      imap_host: "imap.acme.example",
      imap_port: 993,
      imap_security: "tls",
      password: "s3cret",
      credentials: null,
      company_id: "c1",
      post_import_action: "move",
      move_to_folder: "Imported",
      whitelist: [{ pattern: "spam.example", allow: false }],
    });
    expect(post?.headers.get("x-csrf-token")).toBe("csrf-1");
  });

  it("shows health and Rejected Emails, and keeps the stored secret on save", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/mailboxes": { status: 200, body: [mailbox()] },
      "GET /api/v1/mailboxes/m1/rejected-emails": {
        status: 200,
        body: [
          {
            from: "x@spam.example",
            subject: "Win now",
            reason: "blocked by the Whitelist",
            received_at: "2026-11-16T07:00:00Z",
          },
        ],
      },
      "PUT /api/v1/mailboxes/m1": (_req, body) => ({
        status: 200,
        body: mailbox(body as Record<string, unknown>),
      }),
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/mailboxes");

    expect(await screen.findByText("Failing (3 errors in a row)")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Edit Service desk" }));
    expect(await screen.findByText("Win now")).toBeInTheDocument();
    expect(screen.getByText(/authentication failed/)).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toHaveValue("");
    expect(screen.getByLabelText("Password")).toHaveAccessibleDescription(/A secret is stored/);
    expect(screen.queryByRole("button", { name: "Connect the Mailbox" })).toBeNull();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Saved.");
    const put = calls.find((c) => c.method === "PUT");
    expect(put?.body).toMatchObject({ password: null, credentials: null, name: "Service desk" });
  });

  it("checks the connection and shows the error (FR-EML-013)", async () => {
    let answer: { ok: boolean; error: string | null } = {
      ok: false,
      error: "IMAP login failed: authentication failed",
    };
    const calls = mockFetch({
      ...base,
      "GET /api/v1/mailboxes": { status: 200, body: [mailbox()] },
      "GET /api/v1/mailboxes/m1/rejected-emails": { status: 200, body: [] },
      "POST /api/v1/mailboxes/m1/connection-check": () => ({ status: 200, body: answer }),
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/mailboxes");
    await user.click(await screen.findByRole("button", { name: "Edit Service desk" }));

    await user.click(screen.getByRole("button", { name: "Check connection" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The check failed: IMAP login failed: authentication failed",
    );
    expect(await axeViolations(container)).toEqual([]);
    const check = calls.find((c) => c.path.endsWith("/connection-check"));
    expect(check?.headers.get("x-csrf-token")).toBe("csrf-1");

    answer = { ok: true, error: null };
    await user.click(screen.getByRole("button", { name: "Check connection" }));
    expect(await screen.findByText(/Connected: Cooper signed in/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows a refused check as an error", async () => {
    mockFetch({
      ...base,
      "GET /api/v1/mailboxes": { status: 200, body: [mailbox()] },
      "GET /api/v1/mailboxes/m1/rejected-emails": { status: 200, body: [] },
      "POST /api/v1/mailboxes/m1/connection-check": {
        status: 403,
        body: { title: "Forbidden", status: 403, detail: "needs tickets.configure" },
      },
    });
    const user = userEvent.setup();
    renderApp("/administration/mailboxes");
    await user.click(await screen.findByRole("button", { name: "Edit Service desk" }));
    await user.click(screen.getByRole("button", { name: "Check connection" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/The check failed:/);
  });

  it("starts Mailbox Consent for a delegated Microsoft Mailbox", async () => {
    const assign = vi.fn();
    vi.stubGlobal("location", { ...window.location, assign });
    mockFetch({
      ...base,
      "GET /api/v1/mailboxes": {
        status: 200,
        body: [mailbox({ provider: "microsoft", auth_mode: "delegated", imap_host: null })],
      },
      "GET /api/v1/mailboxes/m1/rejected-emails": { status: 200, body: [] },
      "POST /api/v1/mailboxes/m1/consent": {
        status: 200,
        body: { authorization_url: "https://login.example/authorize?state=x" },
      },
    });
    const user = userEvent.setup();
    renderApp("/administration/mailboxes");
    await user.click(await screen.findByRole("button", { name: "Edit Service desk" }));
    expect(screen.getByLabelText("Tenant ID or domain")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Connect the Mailbox" }));
    await waitFor(() =>
      expect(assign).toHaveBeenCalledWith("https://login.example/authorize?state=x"),
    );
  });

  it("confirms a finished Mailbox Consent", async () => {
    mockFetch({ ...base, "GET /api/v1/mailboxes": { status: 200, body: [mailbox()] } });
    renderApp("/administration/mailboxes?mailbox_consent=connected&mailbox=m1");
    expect(await screen.findByRole("status")).toHaveTextContent("The Mailbox is connected");
  });
});
