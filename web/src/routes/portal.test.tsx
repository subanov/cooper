import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { article } from "@/test/fixtures";
import { axeViolations, mockApi, renderApp } from "@/test/render";

const PORTAL_SESSION = {
  csrf_token: "csrf-portal",
  email: "rita@example.com",
  expires_at: "2099-01-01T00:00:00Z",
  idle_expires_at: "2099-01-01T00:00:00Z",
  requester: { id: "r1", full_name: "Rita Requester", company_id: "c1", company_name: "Acme" },
  requesters: [
    { id: "r1", full_name: "Rita Requester", company_id: "c1", company_name: "Acme" },
    { id: "r2", full_name: "Rita Requester", company_id: "c2", company_name: "Globex" },
  ],
};

const SIGNED_IN = {
  "GET /api/v1/portal/session": { status: 200, body: PORTAL_SESSION },
  "/api/v1/portal/notifications/unread-count": { status: 200, body: { unread: 2 } },
};

const TICKET = {
  id: "t9",
  number: "ACM-REQ-0009",
  subject: "Laptop will not start",
  description: "Black screen.",
  status: "Open",
  is_open: true,
  created_at: "2026-03-01T09:00:00Z",
  updated_at: "2026-03-01T09:00:00Z",
};

describe("the Self-Service Portal", () => {
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

  it("says plainly when the email or password is wrong", async () => {
    const user = userEvent.setup();
    mockApi({
      "GET /api/v1/portal/session": { status: 401, body: { type: "x/unauthenticated" } },
      "/api/v1/portal/sign-in-options": { status: 200, body: { self_registration: false } },
      "/api/v1/sso/providers": { status: 200, body: [] },
      "POST /api/v1/portal/session": { status: 401, body: { type: "x/invalid-credentials" } },
    });
    renderApp("/portal/sign-in");
    await user.type(await screen.findByLabelText(/Email address/), "rita@example.com");
    await user.type(screen.getByLabelText(/^Password/), "wrong");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByText("The email address or password is wrong.")).toBeInTheDocument();
  });

  it("starts with search, the catalogue, open tickets and Service Status", async () => {
    mockApi({
      ...SIGNED_IN,
      "/api/v1/portal/catalogue": {
        status: 200,
        body: [{ id: "cat1", name: "Hardware", depth: 0 }],
      },
      "/api/v1/portal/tickets": { status: 200, body: { items: [TICKET] } },
      "/api/v1/portal/service-status": {
        status: 200,
        body: { services: [], incidents: [], planned_maintenance: [] },
      },
    });
    const { container } = renderApp("/portal");
    expect(
      await screen.findByRole("heading", { name: "Hello, Rita. How can we help?" }),
    ).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "Hardware" })).toHaveAttribute(
      "href",
      "/portal/tickets/new?category=cat1",
    );
    expect(await screen.findByRole("link", { name: TICKET.subject })).toBeInTheDocument();
    expect(await screen.findByText("All services are running normally.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Notifications, 2 unread" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("raises a request and confirms it with the Ticket Number", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...SIGNED_IN,
      "/api/v1/portal/catalogue": {
        status: 200,
        body: [{ id: "cat1", name: "Hardware", depth: 0 }],
      },
      "/api/v1/portal/ticket-fields": { status: 200, body: [] },
      "/api/v1/portal/knowledge": { status: 200, body: [] },
      "POST /api/v1/portal/tickets": { status: 201, body: TICKET },
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

  it("follows a ticket's conversation and sends a reply", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...SIGNED_IN,
      "/api/v1/portal/tickets/t9": { status: 200, body: TICKET },
      "/api/v1/portal/tickets/t9/timeline": {
        status: 200,
        body: [
          {
            at: "2026-03-01T10:00:00Z",
            kind: "email",
            body: "Is it plugged in?",
            from_requester: false,
          },
        ],
      },
      "/api/v1/portal/tickets/t9/attachments": { status: 200, body: [] },
      "POST /api/v1/portal/tickets/t9/replies": { status: 201, body: {} },
    });
    const { container } = renderApp("/portal/tickets/t9");
    expect(await screen.findByRole("heading", { name: TICKET.subject })).toBeInTheDocument();
    expect(await screen.findByText("Is it plugged in?")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    await user.type(screen.getByRole("textbox", { name: "Add a reply" }), "Yes.");
    await user.click(screen.getByRole("button", { name: "Send reply" }));
    await waitFor(() => expect(calls.some((c) => c.method === "POST")).toBe(true));
    expect(await screen.findByText("Reply sent.")).toBeInTheDocument();
  });

  it("lets a Requester of several Companies switch between them", async () => {
    const user = userEvent.setup();
    const calls = mockApi({
      ...SIGNED_IN,
      "/api/v1/portal/knowledge": { status: 200, body: [article()] },
      "PUT /api/v1/portal/session/requester": {
        status: 200,
        body: { ...PORTAL_SESSION, requester: PORTAL_SESSION.requesters[1] },
      },
    });
    renderApp("/portal/knowledge");
    expect(await screen.findByRole("link", { name: article().title })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Rita Requester, Acme" }));
    const menu = await screen.findByRole("menu");
    await user.click(within(menu).getByRole("menuitemradio", { name: "Globex" }));
    await waitFor(() => expect(calls.some((c) => c.method === "PUT")).toBe(true));
    expect(await screen.findByText("You now act for Globex.")).toBeInTheDocument();
  });
});
