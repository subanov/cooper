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
const modules = [
  { key: "tickets", name: "Tickets", is_enabled: true, can_disable: false },
  { key: "assets", name: "Assets", is_enabled: false, can_disable: true },
];
const base = { "/api/v1/session": { status: 200, body: session } };

describe("Administration", () => {
  it("opens System Settings first and saves a changed value", async () => {
    let value = "Cooper";
    const calls = mockFetch({
      ...base,
      "/api/v1/modules": { status: 200, body: modules },
      "GET /api/v1/settings": () => ({
        status: 200,
        body: [
          {
            key: "general.instance_name",
            description: "Shown in the header.",
            kind: "text",
            constraints: { max_chars: 100 },
            value,
            default: "Cooper",
          },
        ],
      }),
      "PUT /api/v1/settings/general.instance_name": (_req, body) => {
        value = (body as { value: string }).value;
        return {
          status: 200,
          body: {
            key: "general.instance_name",
            description: "",
            kind: "text",
            constraints: { max_chars: 100 },
            value,
            default: "Cooper",
          },
        };
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration");

    const input = await screen.findByLabelText("general.instance_name");
    expect(await axeViolations(container)).toEqual([]);
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();

    await user.clear(input);
    await user.type(input, "Acme Desk");
    await user.click(save);

    expect(await screen.findByText("Saved.")).toBeInTheDocument();
    const put = calls.find((c) => c.method === "PUT");
    expect(put?.body).toEqual({ value: "Acme Desk" });
    expect(put?.headers.get("x-csrf-token")).toBe("csrf-1");
  });

  it("shows the server's reason when a setting is rejected", async () => {
    mockFetch({
      ...base,
      "/api/v1/modules": { status: 200, body: modules },
      "GET /api/v1/settings": {
        status: 200,
        body: [
          {
            key: "security.max_attempts",
            description: "Failed sign-ins before a ban.",
            kind: "integer",
            constraints: { min: 1, max: 20 },
            value: 5,
            default: 5,
          },
        ],
      },
      "PUT /api/v1/settings/security.max_attempts": {
        status: 422,
        body: {
          type: "https://cooper.dev/problems/validation",
          status: 422,
          detail: "must be at most 20",
        },
      },
    });
    const user = userEvent.setup();
    renderApp("/administration/settings");
    const input = await screen.findByLabelText("security.max_attempts");
    await user.clear(input);
    await user.type(input, "99");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("must be at most 20");
  });

  it("turns a Module on and keeps core Modules locked", async () => {
    const calls = mockFetch({
      ...base,
      "GET /api/v1/modules": { status: 200, body: modules },
      "PUT /api/v1/modules/assets": {
        status: 200,
        body: { ...modules[1], is_enabled: true },
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/modules");

    expect(await screen.findByText("Core Module")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /^Turn (on|off) / })).toHaveLength(1);
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Turn on Assets" }));
    await waitFor(() =>
      expect(calls.find((c) => c.method === "PUT")?.body).toEqual({ is_enabled: true }),
    );
  });

  it("lists Background Jobs and runs one now", async () => {
    const calls = mockFetch({
      ...base,
      "/api/v1/modules": { status: 200, body: modules },
      "GET /api/v1/background-jobs": {
        status: 200,
        body: [
          {
            name: "sla-clock",
            schedule: { kind: "interval", interval_seconds: 60 },
            last_outcome: "failed",
            last_run_at: "2026-11-09T10:00:00Z",
            last_duration_ms: 120,
            next_run_at: null,
          },
        ],
      },
      "POST /api/v1/background-jobs/sla-clock/run": {
        status: 202,
        body: { name: "sla-clock", schedule: {} },
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/jobs");

    expect(await screen.findByRole("heading", { name: "sla-clock" })).toBeInTheDocument();
    expect(screen.getByText("Every 60 s")).toBeInTheDocument();
    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Run now" }));
    expect(await screen.findByText("Run requested for sla-clock.")).toBeInTheDocument();
    expect(calls.some((c) => c.method === "POST")).toBe(true);
  });

  it("filters the System Log by level", async () => {
    const levels: (string | null)[] = [];
    mockFetch({
      ...base,
      "/api/v1/modules": { status: 200, body: modules },
      "GET /api/v1/system-log": (request) => {
        levels.push(new URL(request.url).searchParams.get("level"));
        return {
          status: 200,
          body: {
            items: [
              {
                id: "l1",
                occurred_at: "2026-11-09T10:00:00Z",
                level: "error",
                source: "worker",
                target: "cooper_mailboxes",
                message: "IMAP login failed",
                fields: {},
              },
            ],
            next_cursor: null,
          },
        };
      },
    });
    const user = userEvent.setup();
    const { container } = renderApp("/administration/log");

    const row = (await screen.findByText("IMAP login failed")).closest("tr");
    expect(row).not.toBeNull();
    expect(within(row as HTMLElement).getByText("Error")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);

    await user.selectOptions(screen.getByLabelText("Level"), "error");
    await waitFor(() => expect(levels).toEqual([null, "error"]));
  });
});
