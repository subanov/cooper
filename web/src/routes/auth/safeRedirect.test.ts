import { describe, expect, it } from "vitest";
import { safeRedirect } from "./safeRedirect";

describe("safeRedirect", () => {
  it("only allows paths inside the Analyst console", () => {
    expect(safeRedirect("/tickets?view=mine")).toBe("/tickets?view=mine");
    for (const bad of [
      undefined,
      "",
      "https://evil.example",
      "//evil.example",
      "/\\evil",
      "/sign-in",
      "/portal",
    ]) {
      expect(safeRedirect(bad)).toBe("/");
    }
  });
});
