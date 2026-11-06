import { describe, expect, it } from "vitest";
import { setPreferences } from "./preferences";

describe("preferences", () => {
  it("puts theme and density on <html> and remembers them", () => {
    setPreferences({ theme: "dark", density: "compact" });
    const root = document.documentElement;
    expect(root.classList.contains("dark")).toBe(true);
    expect(root.classList.contains("light")).toBe(false);
    expect(root.dataset.density).toBe("compact");
    expect(JSON.parse(window.localStorage.getItem("cooper.preferences") ?? "{}")).toEqual({
      theme: "dark",
      density: "compact",
    });
    setPreferences({ theme: "system", density: "comfortable" });
    expect(root.classList.contains("dark")).toBe(false);
    expect(root.dataset.density).toBe("comfortable");
  });
});
