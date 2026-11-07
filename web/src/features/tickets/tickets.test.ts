import { describe, expect, it } from "vitest";
import { currentTarget, formatDuration, slaState } from "./sla";
import { describeChange } from "./timeline";
import { parseTicketSearch } from "./views";

describe("ticket list URL state", () => {
  it("keeps known values and drops the rest", () => {
    expect(
      parseTicketSearch({ view: "queue", q: " printer ", sort: "number", order: "asc" }),
    ).toEqual({ view: "queue", q: " printer ", sort: "number", order: "asc" });
    expect(parseTicketSearch({ view: "everything", q: "  ", sort: "subject", order: 1 })).toEqual(
      {},
    );
  });
});

describe("SLA display (FR-SLA-007)", () => {
  it("formats remaining time compactly", () => {
    expect(formatDuration(45 * 60)).toBe("45m");
    expect(formatDuration(3 * 3600 + 20 * 60)).toBe("3h 20m");
    expect(formatDuration(2 * 86400 + 3 * 3600)).toBe("2d 3h");
    expect(formatDuration(-90 * 60)).toBe("1h 30m");
  });

  it("shows the response target until it is met, then resolution", () => {
    const target = (state: string) => ({ state, due_at: null, remaining_seconds: null });
    const sla = (response: string) => ({
      paused_at: null,
      responded_at: null,
      response: target(response),
      resolution: target("on_track"),
    });
    expect(currentTarget(sla("warning")).state).toBe("warning");
    expect(currentTarget(sla("met")).state).toBe("on_track");
    expect(slaState(target("breached"))).toEqual({ label: "sla.breached", tone: "breached" });
    expect(slaState(target("something-new")).label).toBe("sla.none");
  });
});

describe("timeline changes", () => {
  const lookups = {
    analyst: new Map([["a1", "Ada"]]),
    status: new Map([["s2", "Closed"]]),
    priority: new Map(),
    team: new Map(),
    resolution: new Map(),
  };

  it("names IDs, hides bookkeeping fields and long text values", () => {
    expect(
      describeChange({ field: "status_id", old_value: "s1", new_value: "s2" }, lookups),
    ).toEqual({ field: "status_id", label: "field.status", value: "Closed" });
    expect(
      describeChange({ field: "assigned_analyst_id", old_value: "a1", new_value: null }, lookups),
    ).toEqual({ field: "assigned_analyst_id", label: "field.assignee", value: "" });
    expect(
      describeChange({ field: "description", old_value: "a", new_value: "b" }, lookups),
    ).toEqual({ field: "description", label: "field.description" });
    expect(
      describeChange({ field: "resolution_due_at", old_value: null, new_value: "x" }, lookups),
    ).toBeNull();
  });
});
