import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axeViolations, mockFetch, renderApp } from "@/test/render";

afterEach(() => {
  vi.unstubAllGlobals();
});

const problem = (slug: string) => ({ type: `https://cooper.dev/problems/${slug}`, status: 401 });

function session(overrides: Record<string, unknown> = {}) {
  return {
    analyst: { full_name: "Ada Admin" },
    csrf_token: "csrf-1",
    is_admin: false,
    capabilities: ["tickets.read"],
    all_companies: false,
    company_ids: ["c1"],
    totp_enrollment_required: false,
    ...overrides,
  };
}

const modules = { status: 200, body: [{ key: "tickets", is_enabled: true }] };

describe("Analyst sign-in", () => {
  it("sends signed-out Analysts to sign-in and back after a two-factor sign-in", async () => {
    let signedIn = false;
    const calls = mockFetch({
      "GET /api/v1/session": () =>
        signedIn
          ? { status: 200, body: session() }
          : { status: 401, body: problem("unauthenticated") },
      "POST /api/v1/session": (_req, body) => {
        const b = body as { password: string; totp_code: string | null };
        if (b.password !== "correct horse battery") {
          return { status: 401, body: problem("invalid-credentials") };
        }
        if (!b.totp_code) {
          return { status: 401, body: problem("totp-required") };
        }
        signedIn = true;
        return { status: 200, body: session() };
      },
      "/api/v1/modules": modules,
    });
    const user = userEvent.setup();
    const { container } = renderApp("/tickets");

    expect(await screen.findByRole("heading", { name: "Sign in to Cooper" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.type(screen.getByLabelText("Username or email"), "admin");
    await user.type(screen.getByLabelText("Password"), "wrong");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The username or password is wrong");

    await user.clear(screen.getByLabelText("Password"));
    await user.type(screen.getByLabelText("Password"), "correct horse battery");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    const code = await screen.findByLabelText("Two-factor code");
    expect(code).toHaveFocus();
    await user.type(code, "123456");
    await user.click(screen.getByLabelText("Remember this browser for 30 days"));
    await user.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("heading", { name: "Tickets" })).toBeInTheDocument();
    const last = calls.filter((c) => c.method === "POST").at(-1);
    expect(last?.body).toMatchObject({ totp_code: "123456", remember_device: true });
  });

  it("explains an IP ban with the waiting time", async () => {
    mockFetch({
      "GET /api/v1/session": { status: 401, body: problem("unauthenticated") },
      "POST /api/v1/session": {
        status: 429,
        body: problem("too-many-sign-in-attempts"),
        headers: { "retry-after": "600" },
      },
    });
    const user = userEvent.setup();
    renderApp("/sign-in");
    await user.type(await screen.findByLabelText("Username or email"), "admin");
    await user.type(screen.getByLabelText("Password"), "x");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Try again in 10 minutes");
  });

  it("sends Analysts who must set up two-factor sign-in to enrollment first", async () => {
    mockFetch({
      "GET /api/v1/session": { status: 200, body: session({ totp_enrollment_required: true }) },
      "POST /api/v1/session/totp/enrollment": {
        status: 200,
        body: { secret: "JBSWY3DPEHPK3PXP", otpauth_uri: "otpauth://totp/Cooper:admin?secret=x" },
      },
    });
    renderApp("/");
    expect(
      await screen.findByRole("heading", { name: "Set up two-factor sign-in" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("JBSW Y3DP EHPK 3PXP")).toBeInTheDocument();
  });
});

describe("password reset", () => {
  it("checks the repeated password before sending it", async () => {
    const calls = mockFetch({ "POST /api/v1/password-reset/confirm": { status: 204 } });
    const user = userEvent.setup();
    renderApp("/reset-password?token=abc");
    await user.type(await screen.findByLabelText("New password"), "a long new password");
    await user.type(screen.getByLabelText("Repeat the new password"), "something else!!");
    await user.click(screen.getByRole("button", { name: "Save the password" }));
    expect(screen.getByLabelText("Repeat the new password")).toHaveAccessibleDescription(
      "The passwords do not match.",
    );
    await user.clear(screen.getByLabelText("Repeat the new password"));
    await user.type(screen.getByLabelText("Repeat the new password"), "a long new password");
    await user.click(screen.getByRole("button", { name: "Save the password" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Your password is changed");
    expect(calls.at(-1)?.body).toEqual({ token: "abc", password: "a long new password" });
  });
});

describe("Analyst shell", () => {
  it("shows navigation for enabled Modules and held Capabilities, and passes axe-core", async () => {
    mockFetch({ "/api/v1/session": { status: 200, body: session() }, "/api/v1/modules": modules });
    const { container } = renderApp("/");
    expect(await screen.findByRole("heading", { name: "Welcome, Ada Admin" })).toBeInTheDocument();
    const nav = screen.getByRole("navigation", { name: "Main" });
    await waitFor(() =>
      expect(
        within(nav)
          .getAllByRole("link")
          .map((l) => l.textContent),
      ).toEqual(["Home", "Tickets"]),
    );
    expect(within(nav).getByRole("link", { name: "Home" })).toHaveAttribute("aria-current", "page");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("hides a turned-off Module", async () => {
    mockFetch({
      "/api/v1/session": { status: 200, body: session() },
      "/api/v1/modules": { status: 200, body: [{ key: "tickets", is_enabled: false }] },
    });
    renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    await waitFor(() => expect(within(nav).queryByRole("link", { name: "Tickets" })).toBeNull());
  });

  it("opens the command palette with Ctrl+K and jumps with g-shortcuts", async () => {
    mockFetch({ "/api/v1/session": { status: 200, body: session() }, "/api/v1/modules": modules });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: "Welcome, Ada Admin" });
    await waitFor(() => expect(screen.getAllByRole("link", { name: "Tickets" }).length).toBe(1));

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog");
    await user.type(within(palette).getByRole("combobox"), "tick");
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("heading", { name: "Tickets" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.keyDown(window, { key: "g" });
    fireEvent.keyDown(window, { key: "h" });
    expect(await screen.findByRole("heading", { name: "Welcome, Ada Admin" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "?" });
    expect(await screen.findByRole("dialog", { name: "Keyboard shortcuts" })).toBeInTheDocument();
  });

  it("signs out with the CSRF token and returns to sign-in", async () => {
    let signedIn = true;
    const calls = mockFetch({
      "GET /api/v1/session": () =>
        signedIn
          ? { status: 200, body: session() }
          : { status: 401, body: problem("unauthenticated") },
      "DELETE /api/v1/session": () => {
        signedIn = false;
        return { status: 204 };
      },
      "/api/v1/modules": modules,
    });
    const user = userEvent.setup();
    renderApp("/");
    await user.click(await screen.findByRole("button", { name: "Account menu for Ada Admin" }));
    await user.click(await screen.findByRole("menuitem", { name: "Sign out" }));
    expect(await screen.findByRole("heading", { name: "Sign in to Cooper" })).toBeInTheDocument();
    const del = calls.find((c) => c.method === "DELETE");
    expect(del?.headers.get("X-CSRF-Token")).toBe("csrf-1");
  });
});

describe("Portal and unknown routes", () => {
  it("tells signed-out Requesters to sign in to the Portal", async () => {
    mockFetch({ "/api/v1/portal/session": { status: 401 } });
    const { container } = renderApp("/portal");
    expect(
      await screen.findByText("Sign in to raise and follow your tickets."),
    ).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows a not-found page for unknown routes", async () => {
    mockFetch({});
    renderApp("/no/such/page");
    expect(await screen.findByRole("heading", { name: "Page not found" })).toBeInTheDocument();
  });
});
