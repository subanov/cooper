import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { analystApi, lookup, MODULES, ticket } from "@/test/fixtures";
import { axeViolations, mockApi, renderApp } from "@/test/render";

const SIGNED_OUT = { "GET /api/v1/session": { status: 401, body: { type: "x/unauthenticated" } } };

describe("signing in", () => {
  it("sends signed-out Analysts to sign-in and back where they were going", async () => {
    mockApi({
      ...SIGNED_OUT,
      "/api/v1/sso/providers": { status: 200, body: [] },
      "/api/v1/setup": { status: 200, body: { required: false } },
    });
    const { router, container } = renderApp("/tickets?view=open");
    expect(await screen.findByRole("heading", { name: "Sign in to Cooper" })).toBeInTheDocument();
    expect(router.state.location.search).toEqual({ redirect: "/tickets?view=open" });
    expect(await axeViolations(container)).toEqual([]);
  });

  it("asks for the two-factor code when the account needs one", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...SIGNED_OUT,
      "/api/v1/sso/providers": { status: 200, body: [] },
      "/api/v1/setup": { status: 200, body: { required: false } },
      "POST /api/v1/session": {
        status: 401,
        body: { type: "https://cooper.dev/problems/totp-required", detail: "code" },
      },
    });
    renderApp("/sign-in");
    await user.type(await screen.findByLabelText(/Username or email/), "ada");
    await user.type(screen.getByLabelText(/^Password/), "correct horse battery");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByLabelText(/Two-factor code/)).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Enter the code from your authenticator app",
    );
    expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
      login: "ada",
      totp_code: null,
    });
  });
});

describe("the Analyst console", () => {
  it("shows only the navigation the Analyst's Capabilities allow", async () => {
    mockApi({
      ...analystApi(["tickets.read"]),
      "/api/v1/tickets": { status: 200, body: { items: [] } },
    });
    const { container } = renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    expect(within(nav).getByRole("link", { name: "Tickets" })).toBeInTheDocument();
    expect(within(nav).queryByRole("link", { name: "Administration" })).toBeNull();
    expect(within(nav).queryByRole("link", { name: "Assets" })).toBeNull();
    expect(screen.queryByRole("link", { name: /New ticket/ })).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("hides the entries of Modules that are off", async () => {
    mockApi({
      ...analystApi(["tickets.read", "tasks.read"]),
      "GET /api/v1/modules": {
        status: 200,
        body: MODULES.map((m) => (m.key === "tasks" ? { ...m, is_enabled: false } : m)),
      },
      "/api/v1/tickets": { status: 200, body: { items: [] } },
    });
    renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    await waitFor(() => expect(within(nav).queryByRole("link", { name: "Tasks" })).toBeNull());
  });

  it("lists a view's tickets with their SLA and opens one beside the list", async () => {
    const user = userEvent.setup();
    const tk = ticket({ requester_id: "00000000-0000-7000-8000-0000000000r1" });
    const calls = mockApi({
      ...analystApi(["tickets.read", "tickets.update"]),
      "/api/v1/tickets": { status: 200, body: { items: [tk] } },
      [`/api/v1/tickets/${tk.id}`]: { status: 200, body: tk },
      [`/api/v1/tickets/${tk.id}/timeline`]: { status: 200, body: [] },
      [`/api/v1/tickets/${tk.id}/presence`]: { status: 200, body: [] },
      [`/api/v1/tickets/${tk.id}/attachments`]: { status: 200, body: [] },
      [`/api/v1/tickets/${tk.id}/related`]: {
        status: 200,
        body: { articles: [], changes: [], check_results: [], problems: [], tasks: [] },
      },
      "/api/v1/reply-templates": { status: 200, body: [] },
      [`POST /api/v1/tickets/${tk.id}/notes`]: {
        status: 201,
        body: {
          id: "n1",
          body: "Ordered a roller.",
          is_public: false,
          created_at: "2026-03-01T10:00:00Z",
        },
      },
    });
    const { container } = renderApp("/tickets?view=open");
    const list = await screen.findByRole("list", { name: "Tickets in All open" });
    expect(within(list).getByText("1h left")).toBeInTheDocument();
    await user.click(within(list).getByRole("link", { name: tk.subject }));
    expect(await screen.findByRole("heading", { name: tk.subject, level: 2 })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("radio", { name: "Note" }));
    await user.type(screen.getByLabelText("Note", { selector: "textarea" }), "Ordered a roller.");
    await user.click(screen.getByRole("button", { name: "Add Note" }));
    await waitFor(() =>
      expect(calls.some((c) => c.method === "POST" && c.path.endsWith("/notes"))).toBe(true),
    );
    const note = calls.find((c) => c.method === "POST" && c.path.endsWith("/notes"));
    expect(note?.body).toEqual({ body: "Ordered a roller.", is_public: false });
    expect(note?.headers.get("X-CSRF-Token")).toBe("csrf-analyst");
    expect(await screen.findByText("Note added.")).toBeInTheDocument();
  });

  it("explains an empty view and how to fill it", async () => {
    mockApi({
      ...analystApi(["tickets.read"]),
      "/api/v1/tickets": { status: 200, body: { items: [] } },
    });
    renderApp("/tickets?view=mine");
    expect(await screen.findByText(/Pick up a ticket from Unassigned/)).toBeInTheDocument();
  });

  it("asks for a subject before creating a ticket and keeps what was typed", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...analystApi(["tickets.read", "tickets.create"]),
      "/api/v1/tickets": { status: 200, body: { items: [] } },
      "/api/v1/ticket-fields": { status: 200, body: [] },
    });
    const { container } = renderApp("/tickets/new?view=open");
    const description = await screen.findByLabelText("Description");
    await user.type(description, "It beeps.");
    await user.click(screen.getByRole("button", { name: "Create ticket" }));
    expect(await screen.findByText("Enter a subject.")).toBeInTheDocument();
    expect(screen.getByLabelText(/Subject/)).toHaveFocus();
    expect(description).toHaveValue("It beeps.");
    expect(calls.some((c) => c.method === "POST")).toBe(false);
    expect(await axeViolations(container)).toEqual([]);
  });

  it("closes a ticket only with a Resolution Code", async () => {
    const user = userEvent.setup();
    const tk = ticket();
    mockApi({
      ...analystApi(["tickets.read", "tickets.update"]),
      "/api/v1/tickets": { status: 200, body: { items: [tk] } },
      [`/api/v1/tickets/${tk.id}`]: { status: 200, body: tk },
      [`/api/v1/tickets/${tk.id}/timeline`]: { status: 200, body: [] },
      [`/api/v1/tickets/${tk.id}/presence`]: { status: 200, body: [] },
      [`/api/v1/tickets/${tk.id}/attachments`]: { status: 200, body: [] },
      "/api/v1/reply-templates": { status: 200, body: [] },
    });
    renderApp(`/tickets/${tk.id}?view=open`);
    await user.click(await screen.findByRole("button", { name: "Change status" }));
    const dialog = await screen.findByRole("dialog");
    await user.selectOptions(within(dialog).getByLabelText(/^Status/), "s-closed");
    const submit = within(dialog).getByRole("button", { name: "Change status" });
    expect(submit).toBeDisabled();
    await user.selectOptions(within(dialog).getByLabelText(/Resolution Code/), "r-fixed");
    expect(submit).toBeEnabled();
  });

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
    renderApp("/administration/ticket-statuses");
    expect(await screen.findByRole("table", { name: "Ticket Statuses" })).toBeInTheDocument();
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

  it("shows a friendly page for unknown addresses", async () => {
    mockApi(analystApi([]));
    renderApp("/no-such-page");
    expect(await screen.findByRole("heading", { name: "Page not found" })).toBeInTheDocument();
  });
});

describe("the Self-Service Portal", () => {
  const PORTAL_SESSION = {
    csrf_token: "csrf-portal",
    email: "rita@example.com",
    expires_at: "2099-01-01T00:00:00Z",
    idle_expires_at: "2099-01-01T00:00:00Z",
    requester: { id: "r1", full_name: "Rita Requester", company_id: "c1", company_name: "Acme" },
    requesters: [{ id: "r1", full_name: "Rita Requester", company_id: "c1", company_name: "Acme" }],
  };

  it("sends signed-out Requesters to the Portal sign-in", async () => {
    mockApi({
      "GET /api/v1/portal/session": { status: 401, body: { type: "x/unauthenticated" } },
      "/api/v1/portal/sign-in-options": { status: 200, body: { self_registration: true } },
      "/api/v1/sso/providers": { status: 200, body: [] },
    });
    const { container } = renderApp("/portal/tickets");
    expect(
      await screen.findByRole("heading", { name: "Sign in to the Help Centre" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("link", { name: /Register with your work email/ }),
    ).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("raises a request and confirms it with the Ticket Number", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      "GET /api/v1/portal/session": { status: 200, body: PORTAL_SESSION },
      "/api/v1/portal/notifications/unread-count": { status: 200, body: { unread: 0 } },
      "/api/v1/portal/catalogue": {
        status: 200,
        body: [{ id: "cat1", name: "Hardware", depth: 0 }],
      },
      "/api/v1/portal/ticket-fields": { status: 200, body: [] },
      "/api/v1/portal/knowledge": { status: 200, body: [] },
      "POST /api/v1/portal/tickets": {
        status: 201,
        body: {
          id: "t9",
          number: "ACM-REQ-0009",
          subject: "Laptop will not start",
          description: "",
          status: "New",
          is_open: true,
          created_at: "2026-03-01T09:00:00Z",
          updated_at: "2026-03-01T09:00:00Z",
        },
      },
    });
    const { container } = renderApp("/portal/tickets/new?category=cat1");
    const summary = await screen.findByLabelText(/Summary/);
    await user.type(summary, "Laptop will not start");
    await user.click(screen.getByRole("button", { name: "Send request" }));
    expect(
      await screen.findByRole("heading", { name: "Request ACM-REQ-0009 received" }),
    ).toBeInTheDocument();
    const post = calls.find((c) => c.method === "POST");
    expect(post?.body).toMatchObject({ subject: "Laptop will not start", category_id: "cat1" });
    expect(post?.headers.get("X-CSRF-Token")).toBe("csrf-portal");
    expect(await axeViolations(container)).toEqual([]);
  });
});
