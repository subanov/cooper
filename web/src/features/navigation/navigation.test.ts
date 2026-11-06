import { describe, expect, it } from "vitest";
import type { Schemas } from "@/lib/api";
import { visibleNavItems } from "./navigation";

function session(capabilities: string[], isAdmin = false): Schemas["SessionDto"] {
  return { capabilities, is_admin: isAdmin } as unknown as Schemas["SessionDto"];
}

function modules(enabled: Record<string, boolean>): Schemas["ModuleDto"][] {
  return Object.entries(enabled).map(
    ([key, is_enabled]) => ({ key, is_enabled }) as unknown as Schemas["ModuleDto"],
  );
}

describe("navigation", () => {
  it("follows Capabilities and Module switches", () => {
    const paths = (s: Schemas["SessionDto"], m?: Schemas["ModuleDto"][]) =>
      visibleNavItems(s, m).map((i) => i.to);
    expect(paths(session([]), modules({ tickets: true }))).toEqual(["/"]);
    expect(paths(session(["tickets.read"]), modules({ tickets: true }))).toEqual(["/", "/tickets"]);
    expect(paths(session(["tickets.read"]), modules({ tickets: false }))).toEqual(["/"]);
    expect(paths(session([], true), modules({ tickets: true }))).toEqual([
      "/",
      "/tickets",
      "/administration",
    ]);
  });
});
