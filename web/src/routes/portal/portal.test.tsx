import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axeViolations, mockFetch, renderApp } from "@/test/render";

afterEach(() => {
  vi.unstubAllGlobals();
});

const problem = (slug: string, status = 401) => ({
  type: `https://cooper.dev/problems/${slug}`,
  status,
});

const acme = { id: "r1", full_name: "Rita Requester", company_id: "c1", company_name: "Acme" };
const globex = { id: "r2", full_name: "Rita Requester", company_id: "c2", company_name: "Globex" };

function portalSession(requester = acme, requesters = [acme]) {
  return {
    email: "rita@example.com",
    requester,
    requesters,
    csrf_token: "portal-csrf",
    idle_expires_at: "2026-11-06T12:00:00Z",
    expires_at: "2026-11-07T12:00:00Z",
  };
}

const options = (selfRegistration: boolean) => ({
  status: 200,
  body: { self_registration: selfRegistration },
});

describe("Portal sign-in (FR-PRT-001)", () => {
  it("sends signed-out Requesters to sign-in and back where they wanted to go", async () => {
    let signedIn = false;
    const calls = mockFetch({
      "GET /api/v1/portal/session": () =>
        signedIn
          ? { status: 200, body: portalSession() }
          : { status: 401, body: problem("unauthenticated") },
      "POST /api/v1/portal/session": (_req, body) => {
        if ((body as { password: string }).password !== "correct horse battery") {
          return { status: 401, body: problem("invalid-credentials") };
        }
        signedIn = true;
        return { status: 200, body: portalSession() };
      },
      "/api/v1/portal/sign-in-options": options(false),
    });
    const user = userEvent.setup();
    const { container } = renderApp("/portal/tickets");

    expect(await screen.findByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.getByText("Self-Service Portal")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    expect(screen.queryByRole("link", { name: /Register/ })).toBeNull();

    await user.type(screen.getByLabelText("Email address"), "rita@example.com");
    await user.type(screen.getByLabelText("Password"), "wrong");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The email address or password is wrong.",
    );

    await user.clear(screen.getByLabelText("Password"));
    await user.type(screen.getByLabelText("Password"), "correct horse battery");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("heading", { name: "My tickets" })).toBeInTheDocument();
    expect(calls.filter((c) => c.method === "POST").at(-1)?.body).toEqual({
      email: "rita@example.com",
      password: "correct horse battery",
    });
  });

  it("offers Self-Registration only when it is on", async () => {
    mockFetch({
      "/api/v1/portal/session": { status: 401 },
      "/api/v1/portal/sign-in-options": options(true),
    });
    renderApp("/portal/sign-in");
    expect(
      await screen.findByRole("link", { name: "New here? Register with your work email." }),
    ).toHaveAttribute("href", "/portal/register");
  });

  it("asks for a set-password link without saying whether the address is known", async () => {
    const calls = mockFetch({ "POST /api/v1/portal/password-reset": { status: 202 } });
    const user = userEvent.setup();
    renderApp("/portal/reset-password");
    await user.type(await screen.findByLabelText("Email address"), "who@example.com");
    await user.click(screen.getByRole("button", { name: "Send the link" }));
    expect(await screen.findByRole("status")).toHaveTextContent("If the address is known");
    expect(calls.at(-1)?.body).toEqual({ email: "who@example.com" });
  });

  it("sets the password from the emailed link and explains an expired link", async () => {
    let used = false;
    const calls = mockFetch({
      "POST /api/v1/portal/password": () => {
        if (used) {
          return { status: 401, body: problem("invalid-token") };
        }
        used = true;
        return { status: 204 };
      },
    });
    const user = userEvent.setup();
    const { container, unmount } = renderApp("/portal/set-password?token=tok-1");
    await user.type(await screen.findByLabelText("New password"), "a long new password");
    await user.type(screen.getByLabelText("Repeat the new password"), "a long new password");
    expect(await axeViolations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Save the password" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Your password is set");
    expect(calls.at(-1)?.body).toEqual({ token: "tok-1", password: "a long new password" });
    unmount();

    renderApp("/portal/set-password?token=tok-1");
    await user.type(await screen.findByLabelText("New password"), "a long new password");
    await user.type(screen.getByLabelText("Repeat the new password"), "a long new password");
    await user.click(screen.getByRole("button", { name: "Save the password" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("This link is invalid");
  });

  it("registers with a work address and explains a foreign domain", async () => {
    const calls = mockFetch({
      "POST /api/v1/portal/registration": (_req, body) =>
        (body as { email: string }).email.endsWith("@acme.example")
          ? { status: 202 }
          : { status: 422, body: problem("invalid", 422) },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/portal/register");
    await user.type(await screen.findByLabelText("Your name"), "Nia New");
    await user.type(screen.getByLabelText("Work email address"), "nia@gmail.com");
    await user.click(screen.getByRole("button", { name: "Send the confirmation link" }));
    await waitFor(() =>
      expect(screen.getByLabelText("Work email address")).toHaveAccessibleDescription(
        "This address does not belong to a Company we support. Use your work email address.",
      ),
    );
    expect(await axeViolations(container)).toEqual([]);

    await user.clear(screen.getByLabelText("Work email address"));
    await user.type(screen.getByLabelText("Work email address"), "nia@acme.example");
    await user.click(screen.getByRole("button", { name: "Send the confirmation link" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Check your email");
    expect(calls.at(-1)?.body).toEqual({ full_name: "Nia New", email: "nia@acme.example" });
  });
});

describe("Portal shell", () => {
  it("shows the Requester's Company, the Portal navigation and passes axe-core", async () => {
    mockFetch({ "/api/v1/portal/session": { status: 200, body: portalSession() } });
    const { container } = renderApp("/portal");
    expect(
      await screen.findByRole("heading", { name: "Welcome, Rita Requester" }),
    ).toBeInTheDocument();
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(
      within(nav)
        .getAllByRole("link")
        .map((l) => l.textContent),
    ).toEqual(["Home", "My tickets"]);
    expect(screen.getByText("Acme", { selector: "span" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Company:/ })).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("switches the Company the session acts for", async () => {
    let current = acme;
    const calls = mockFetch({
      "GET /api/v1/portal/session": () => ({
        status: 200,
        body: portalSession(current, [acme, globex]),
      }),
      "PUT /api/v1/portal/session/requester": (_req, body) => {
        current = (body as { requester_id: string }).requester_id === "r2" ? globex : acme;
        return { status: 200, body: portalSession(current, [acme, globex]) };
      },
    });
    const user = userEvent.setup();
    renderApp("/portal");
    await user.click(await screen.findByRole("button", { name: "Company: Acme" }));
    await user.click(await screen.findByRole("menuitemradio", { name: "Globex" }));
    expect(await screen.findByRole("button", { name: "Company: Globex" })).toBeInTheDocument();
    expect(
      await screen.findByText("Raise and follow your tickets with Globex."),
    ).toBeInTheDocument();
    const put = calls.find((c) => c.method === "PUT");
    expect(put?.body).toEqual({ requester_id: "r2" });
    expect(put?.headers.get("X-CSRF-Token")).toBe("portal-csrf");
  });

  it("signs out of the Portal only", async () => {
    let signedIn = true;
    const calls = mockFetch({
      "GET /api/v1/portal/session": () =>
        signedIn ? { status: 200, body: portalSession() } : { status: 401 },
      "DELETE /api/v1/portal/session": () => {
        signedIn = false;
        return { status: 204 };
      },
      "/api/v1/portal/sign-in-options": options(false),
    });
    const user = userEvent.setup();
    renderApp("/portal");
    await user.click(
      await screen.findByRole("button", { name: "Account menu for Rita Requester" }),
    );
    await user.click(await screen.findByRole("menuitem", { name: "Sign out" }));
    expect(await screen.findByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(calls.some((c) => c.path === "/api/v1/session")).toBe(false);
    expect(calls.find((c) => c.method === "DELETE")?.headers.get("X-CSRF-Token")).toBe(
      "portal-csrf",
    );
  });
});
