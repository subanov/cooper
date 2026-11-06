import { screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axeViolations, mockFetch, renderApp } from "@/test/render";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("route trees", () => {
  it("shows the Analyst console with the signed-in Analyst", async () => {
    mockFetch({
      "/api/v1/session": {
        status: 200,
        body: { analyst: { full_name: "Ada Admin" }, csrf_token: "t" },
      },
    });
    const { container } = renderApp("/");
    expect(await screen.findByText("Signed in as Ada Admin")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("tells signed-out Requesters to sign in to the Portal", async () => {
    mockFetch({ "/api/v1/portal/session": { status: 401 } });
    const { container } = renderApp("/portal");
    expect(
      await screen.findByText("Sign in to raise and follow your tickets."),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Skip to content" })).toHaveAttribute("href", "#main");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows a not-found page for unknown routes", async () => {
    mockFetch({});
    renderApp("/no/such/page");
    expect(await screen.findByRole("heading", { name: "Page not found" })).toBeInTheDocument();
  });
});
