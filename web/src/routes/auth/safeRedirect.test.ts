import { describe, expect, it } from "vitest";
import { safeRedirect } from "./safeRedirect";

const OFFSITE = [undefined, "", "https://evil.example", "//evil.example", "/\\evil"];

describe("safeRedirect", () => {
  it("only allows paths inside the Analyst console", () => {
    expect(safeRedirect("/tickets?view=mine")).toBe("/tickets?view=mine");
    for (const bad of [...OFFSITE, "/sign-in", "/portal", "/portal/tickets"]) {
      expect(safeRedirect(bad)).toBe("/");
    }
  });

  it("only allows signed-in Portal paths after a Portal sign-in", () => {
    expect(safeRedirect("/portal/tickets?status=open", "portal")).toBe(
      "/portal/tickets?status=open",
    );
    for (const bad of [
      ...OFFSITE,
      "/tickets",
      "/portals",
      "/portal/sign-in",
      "/portal/set-password?token=x",
    ]) {
      expect(safeRedirect(bad, "portal")).toBe("/portal");
    }
  });
});
