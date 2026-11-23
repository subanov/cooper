import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { analystApi, lookup, MODULES, ticket } from "@/test/fixtures";
import { axeViolations, mockApi, renderApp } from "@/test/render";

const SIGNED_OUT = { "GET /api/v1/session": { status: 401, body: { type: "x/unauthenticated" } } };

/** The answers the ticket screen needs for `tk`. */
function ticketApi(tk: ReturnType<typeof ticket>) {
  return {
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
  };
}

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
    expect(screen.getByText(/Enter the code from your authenticator app/)).toBeInTheDocument();
    expect(calls.find((c) => c.method === "POST")?.body).toMatchObject({
      login: "ada",
      totp_code: null,
    });
  });

  it("offers first-run setup when Cooper is not set up", async () => {
    mockApi({
      ...SIGNED_OUT,
      "/api/v1/sso/providers": { status: 200, body: [{ id: "p1", display_name: "Contoso" }] },
      "/api/v1/setup": { status: 200, body: { required: true } },
    });
    renderApp("/sign-in");
    expect(await screen.findByRole("link", { name: "Set up Cooper" })).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "Sign in with Contoso" })).toHaveAttribute(
      "href",
      "/api/v1/sso/p1/start?audience=analysts&return_to=%2F",
    );
  });

  it("checks the new password before asking for a reset", async () => {
    const user = userEvent.setup();
    const calls = mockApi({});
    renderApp("/reset-password?token=abc");
    await user.type(await screen.findByLabelText(/^New password/), "short");
    await user.click(screen.getByRole("button", { name: "Save the password" }));
    expect(await screen.findByText("Use at least 12 characters.")).toBeInTheDocument();
    expect(screen.getByLabelText(/^New password/)).toHaveFocus();
    expect(calls.some((c) => c.method === "POST")).toBe(false);
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

  it("collapses the navigation to icons and remembers it", async () => {
    const user = userEvent.setup();
    mockApi({
      ...analystApi(["tickets.read"]),
      "/api/v1/tickets": { status: 200, body: { items: [] } },
    });
    renderApp("/");
    await user.click(await screen.findByRole("button", { name: "Collapse navigation" }));
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(within(nav).getByRole("link", { name: "Tickets" })).toHaveAttribute(
      "data-tip",
      "Tickets",
    );
    expect(window.localStorage.getItem("cooper.preferences")).toContain('"nav":"collapsed"');
  });

  it("opens a ticket by number from the command palette", async () => {
    const user = userEvent.setup();
    const tk = ticket();
    mockApi({
      ...analystApi(["tickets.read"]),
      ...ticketApi(tk),
      [`/api/v1/tickets/by-number/${tk.number}`]: { status: 200, body: tk },
    });
    const { router } = renderApp("/");
    await screen.findByRole("navigation", { name: "Main" });
    await user.keyboard("{Control>}k{/Control}");
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    await user.type(within(palette).getByRole("combobox"), tk.number.toLowerCase());
    await user.click(await within(palette).findByText(`Open ticket ${tk.number}`));
    await waitFor(() => expect(router.state.location.pathname).toBe(`/tickets/${tk.id}`));
  });

  it("lists every keyboard shortcut", async () => {
    const user = userEvent.setup();
    mockApi({
      ...analystApi(["tickets.read"]),
      "/api/v1/tickets": { status: 200, body: { items: [] } },
    });
    renderApp("/");
    await screen.findByRole("navigation", { name: "Main" });
    await user.keyboard("?");
    const dialog = await screen.findByRole("dialog", { name: "Keyboard shortcuts" });
    expect(within(dialog).getByText("Go to Tickets")).toBeInTheDocument();
  });

  it("shows a friendly page for unknown addresses", async () => {
    mockApi(analystApi([]));
    renderApp("/no-such-page");
    expect(await screen.findByRole("heading", { name: "Page not found" })).toBeInTheDocument();
  });
});

describe("tickets", () => {
  it("lists a view's tickets with their SLA and opens one beside the list", async () => {
    const user = userEvent.setup();
    const tk = ticket({ requester_id: "00000000-0000-7000-8000-0000000000r1" });
    const calls = mockApi({
      ...analystApi(["tickets.read", "tickets.update"]),
      ...ticketApi(tk),
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
    expect(within(list).getByRole("link", { name: tk.subject })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("radio", { name: "Note" }));
    expect(screen.getByText("Only Analysts see this")).toBeInTheDocument();
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

  it("keeps view, search and sort in the address", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...analystApi(["tickets.read"]),
      "/api/v1/tickets": { status: 200, body: { items: [] } },
    });
    const { router } = renderApp("/tickets?view=open");
    const search = await screen.findByRole("searchbox", { name: "Search tickets" });
    await user.type(search, "printer{Enter}");
    await waitFor(() =>
      expect(router.state.location.search).toMatchObject({ view: "open", q: "printer" }),
    );
    await waitFor(() =>
      expect(
        calls.some((c) => c.path === "/api/v1/tickets" && c.search.includes("q=printer")),
      ).toBe(true),
    );
  });

  it("assigns a ticket to me from the list and offers Undo", async () => {
    const user = userEvent.setup();
    const tk = ticket();
    const calls = mockApi({
      ...analystApi(["tickets.read", "tickets.assign"]),
      ...ticketApi(tk),
      [`PUT /api/v1/tickets/${tk.id}/assignment`]: {
        status: 200,
        body: { ...tk, analyst_id: "00000000-0000-7000-8000-0000000000a1" },
      },
    });
    renderApp("/tickets?view=open");
    await user.click(await screen.findByRole("button", { name: `Quick actions for ${tk.number}` }));
    await user.click(await screen.findByRole("menuitem", { name: "Assign to me" }));
    await waitFor(() =>
      expect(calls.some((c) => c.method === "PUT" && c.path.endsWith("/assignment"))).toBe(true),
    );
    expect(calls.find((c) => c.method === "PUT")?.body).toEqual({
      analyst_id: "00000000-0000-7000-8000-0000000000a1",
      team_id: null,
    });
    expect(await screen.findByText("Assigned to you.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
  });

  it("changes the Priority in place in the Properties panel", async () => {
    const user = userEvent.setup();
    const tk = ticket();
    const calls = mockApi({
      ...analystApi(["tickets.read", "tickets.update"]),
      ...ticketApi(tk),
      "GET /api/v1/ticket-priorities": {
        status: 200,
        body: [lookup("p-normal", "Normal", { is_default: true }), lookup("p-high", "High")],
      },
      [`PUT /api/v1/tickets/${tk.id}`]: { status: 200, body: { ...tk, priority_id: "p-high" } },
    });
    renderApp(`/tickets/${tk.id}?view=open`);
    const details = await screen.findByRole("complementary", { name: "Details" });
    const priority = await within(details).findByRole("combobox", { name: "Priority" });
    await waitFor(() => expect(within(priority).getAllByRole("option")).toHaveLength(2));
    await user.selectOptions(priority, "p-high");
    await waitFor(() =>
      expect(calls.some((c) => c.method === "PUT" && c.path === `/api/v1/tickets/${tk.id}`)).toBe(
        true,
      ),
    );
    expect(
      calls.find((c) => c.method === "PUT" && c.path === `/api/v1/tickets/${tk.id}`)?.body,
    ).toMatchObject({
      priority_id: "p-high",
      subject: tk.subject,
    });
    expect(await screen.findByText("Priority changed to High.")).toBeInTheDocument();
  });

  it("closes a ticket only with a Resolution Code", async () => {
    const user = userEvent.setup();
    const tk = ticket();
    mockApi({ ...analystApi(["tickets.read", "tickets.update"]), ...ticketApi(tk) });
    renderApp(`/tickets/${tk.id}?view=open`);
    await user.click(await screen.findByRole("button", { name: "Change status" }));
    const dialog = await screen.findByRole("dialog");
    await user.selectOptions(within(dialog).getByLabelText(/^Status/), "s-closed");
    const submit = within(dialog).getByRole("button", { name: "Change status" });
    expect(submit).toBeDisabled();
    await user.selectOptions(within(dialog).getByLabelText(/Resolution Code/), "r-fixed");
    expect(submit).toBeEnabled();
  });

  it("suggests a reply with AI, using typed text as guidance, and offers Undo", async () => {
    const user = userEvent.setup();
    const tk = ticket({ requester_id: "req-1", can_suggest_reply: true });
    const calls = mockApi({
      ...analystApi(["tickets.read", "tickets.update"]),
      ...ticketApi(tk),
      [`POST /api/v1/tickets/${tk.id}/suggested-reply`]: {
        status: 200,
        body: { text: "Hello, please restart the printer.", model: "gemini-test" },
      },
    });
    const { container } = renderApp(`/tickets/${tk.id}?view=open`);
    const box = await screen.findByRole("textbox", { name: /^Reply to/ });
    expect(screen.getByRole("button", { name: "Suggest reply" })).toBeInTheDocument();
    await user.type(box, "ask them to restart");
    await user.click(screen.getByRole("button", { name: "Improve with AI" }));
    await waitFor(() => expect(box).toHaveValue("Hello, please restart the printer."));
    expect(calls.find((c) => c.path.endsWith("/suggested-reply"))?.body).toEqual({
      guidance: "ask them to restart",
    });
    expect(
      await screen.findByText("Suggested reply inserted. Review it before sending."),
    ).toBeInTheDocument();
    expect(calls.some((c) => c.path.endsWith("/replies"))).toBe(false);
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(box).toHaveValue("ask them to restart");
  });

  it("keeps the typed text when no reply can be suggested", async () => {
    const user = userEvent.setup();
    const tk = ticket({ requester_id: "req-1", can_suggest_reply: true });
    mockApi({
      ...analystApi(["tickets.read", "tickets.update"]),
      ...ticketApi(tk),
      [`POST /api/v1/tickets/${tk.id}/suggested-reply`]: {
        status: 503,
        body: { type: "https://cooper.dev/problems/ai-unavailable", status: 503 },
      },
    });
    renderApp(`/tickets/${tk.id}?view=open`);
    const box = await screen.findByRole("textbox", { name: /^Reply to/ });
    await user.type(box, "my draft");
    await user.click(screen.getByRole("button", { name: "Improve with AI" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Improve with AI" })).toBeEnabled(),
    );
    expect(box).toHaveValue("my draft");
  });

  it("offers no Suggest reply when the API does not", async () => {
    const tk = ticket({ requester_id: "req-1", can_suggest_reply: false });
    mockApi({ ...analystApi(["tickets.read", "tickets.update"]), ...ticketApi(tk) });
    renderApp(`/tickets/${tk.id}?view=open`);
    await screen.findByRole("textbox", { name: /^Reply to/ });
    expect(screen.queryByRole("button", { name: "Suggest reply" })).not.toBeInTheDocument();
  });

  it("asks for a Resolution Code when closing from the Properties panel", async () => {
    const user = userEvent.setup();
    const tk = ticket();
    const calls = mockApi({ ...analystApi(["tickets.read", "tickets.update"]), ...ticketApi(tk) });
    renderApp(`/tickets/${tk.id}?view=open`);
    const details = await screen.findByRole("complementary", { name: "Details" });
    const status = await within(details).findByRole("combobox", { name: "Status" });
    await waitFor(() => expect(within(status).getAllByRole("option")).toHaveLength(2));
    await user.selectOptions(status, "s-closed");
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByLabelText(/Resolution Code/)).toBeInTheDocument();
    expect(calls.some((c) => c.method === "POST" && c.path.endsWith("/status"))).toBe(false);
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

  it("applies one action to the selected tickets", async () => {
    const user = userEvent.setup();
    const a = ticket();
    const b = ticket({
      id: "00000000-0000-7000-8000-000000000072",
      number: "ACM-INC-0043",
      subject: "Scanner jams",
    });
    const calls = mockApi({
      ...analystApi(["tickets.read", "tickets.assign"]),
      "/api/v1/tickets": { status: 200, body: { items: [a, b] } },
      "POST /api/v1/tickets/bulk": {
        status: 200,
        body: [
          { ticket_id: a.id, ok: true },
          { ticket_id: b.id, ok: true },
        ],
      },
    });
    renderApp("/tickets?view=open");
    await user.click(await screen.findByRole("checkbox", { name: "Select every ticket shown" }));
    expect(screen.getByText("2 tickets selected")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Assign to me" }));
    await waitFor(() => expect(calls.some((c) => c.path === "/api/v1/tickets/bulk")).toBe(true));
    expect(calls.find((c) => c.path === "/api/v1/tickets/bulk")?.body).toMatchObject({
      action: "assign",
      ticket_ids: [a.id, b.id],
    });
    expect(await screen.findByText("2 tickets changed.")).toBeInTheDocument();
  });

  it("moves through the list with j and k", async () => {
    const user = userEvent.setup();
    const a = ticket();
    const b = ticket({
      id: "00000000-0000-7000-8000-000000000072",
      number: "ACM-INC-0043",
      subject: "Scanner jams",
    });
    mockApi({
      ...analystApi(["tickets.read"]),
      ...ticketApi(a),
      "/api/v1/tickets": { status: 200, body: { items: [a, b] } },
    });
    const { router } = renderApp("/tickets?view=open");
    await screen.findByRole("list", { name: "Tickets in All open" });
    await user.keyboard("j");
    await waitFor(() => expect(router.state.location.pathname).toBe(`/tickets/${a.id}`));
    await user.keyboard("j");
    await waitFor(() => expect(router.state.location.pathname).toBe(`/tickets/${b.id}`));
    expect(router.state.location.search).toMatchObject({ view: "open" });
  });
});
