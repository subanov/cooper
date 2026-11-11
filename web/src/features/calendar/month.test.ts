import { describe, expect, it } from "vitest";
import type { Schemas } from "@/lib/api";
import { addDays, eventsByDay, monthGrid, parseMonth, shiftMonth } from "./month";

const event = (over: Partial<Schemas["CalendarEventDto"]>): Schemas["CalendarEventDto"] => ({
  id: "e1",
  company_id: "c1",
  title: "Event",
  description: "",
  category_id: "k1",
  category_name: "Meetings",
  colour: "#2563eb",
  all_day: true,
  editable: true,
  created_at: "2026-11-01T00:00:00Z",
  updated_at: "2026-11-01T00:00:00Z",
  ...over,
});

describe("calendar months", () => {
  it("parses months and falls back to the current one", () => {
    expect(parseMonth("2026-11")).toBe("2026-11");
    expect(parseMonth("2026-13", new Date(2027, 0, 5))).toBe("2027-01");
    expect(parseMonth(undefined, new Date(2027, 0, 5))).toBe("2027-01");
  });

  it("moves across years", () => {
    expect(shiftMonth("2026-12", 1)).toBe("2027-01");
    expect(shiftMonth("2026-01", -1)).toBe("2025-12");
    expect(addDays("2024-02-28", 1)).toBe("2024-02-29");
  });

  it("lays out whole weeks starting on Monday", () => {
    const weeks = monthGrid("2026-11");
    expect(weeks[0]?.[0]).toBe("2026-10-26");
    expect(weeks.at(-1)?.[6]).toBe("2026-12-06");
    expect(weeks.every((w) => w.length === 7)).toBe(true);
    expect(monthGrid("2021-02")).toHaveLength(4);
  });

  it("places multi-day events on every day they cover", () => {
    const days = monthGrid("2026-11").flat();
    const byDay = eventsByDay([event({ start_date: "2026-11-02", end_date: "2026-11-04" })], days);
    expect(byDay.get("2026-11-01")).toHaveLength(0);
    expect(byDay.get("2026-11-02")).toHaveLength(1);
    expect(byDay.get("2026-11-04")).toHaveLength(1);
    expect(byDay.get("2026-11-05")).toHaveLength(0);
  });
});
