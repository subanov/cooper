import {
  addDays,
  fromLocalInput,
  isDayKey,
  monthGrid,
  parseMonth,
  shiftMonth,
  toLocalInput,
} from "./dates";
import { ApiError, problemHeadline, problemOf, toProblem } from "./problem";
import { safeRedirect } from "./safeRedirect";

describe("safeRedirect", () => {
  it("keeps paths inside the Analyst console", () => {
    expect(safeRedirect("/tickets?view=mine")).toBe("/tickets?view=mine");
  });
  it.each(["https://evil.example", "//evil.example", "/\\evil", "/portal/tickets", "/sign-in"])(
    "refuses %s for Analysts",
    (target) => expect(safeRedirect(target)).toBe("/"),
  );
  it("keeps Portal paths for the Portal and refuses its public screens", () => {
    expect(safeRedirect("/portal/tickets/1", "portal")).toBe("/portal/tickets/1");
    expect(safeRedirect("/tickets", "portal")).toBe("/portal");
    expect(safeRedirect("/portal/sign-in", "portal")).toBe("/portal");
  });
});

describe("dates", () => {
  it("adds days across month ends without time-zone drift", () => {
    expect(addDays("2026-03-31", 1)).toBe("2026-04-01");
    expect(addDays("2026-03-29", -30)).toBe("2026-02-27");
  });
  it("reads months from the URL and falls back to the current month", () => {
    expect(parseMonth("2026-02")).toBe("2026-02");
    expect(parseMonth("2026-13", new Date(2026, 4, 3))).toBe("2026-05");
    expect(shiftMonth("2026-01", -1)).toBe("2025-12");
  });
  it("lays out a month in Monday-first weeks", () => {
    const weeks = monthGrid("2026-02");
    expect(weeks[0]?.[0]).toBe("2026-01-26");
    expect(weeks.every((w) => w.length === 7)).toBe(true);
    expect(weeks.flat()).toContain("2026-02-28");
  });
  it("validates day keys", () => {
    expect(isDayKey("2026-02-28")).toBe(true);
    expect(isDayKey("2026-2-28")).toBe(false);
  });
  it("round-trips datetime-local values", () => {
    const iso = fromLocalInput("2026-03-01T09:30");
    expect(iso).not.toBeNull();
    expect(toLocalInput(iso)).toBe("2026-03-01T09:30");
    expect(fromLocalInput("")).toBeNull();
  });
});

describe("problems", () => {
  it("reads the kind, detail and field of a problem", () => {
    const response = new Response(null, { status: 422, headers: { "retry-after": "30" } });
    const p = toProblem(
      { type: "https://cooper.dev/problems/validation", detail: "too short", field: "subject" },
      response,
    );
    expect(p).toMatchObject({
      kind: "validation",
      status: 422,
      detail: "too short",
      field: "subject",
      retryAfter: 30,
    });
    expect(problemHeadline(p)).toBe("problem.validation");
  });
  it("treats a failed request without a response as a network problem", () => {
    const p = problemOf(new ApiError(toProblem({}, undefined)));
    expect(p.kind).toBe("network");
    expect(problemHeadline(p)).toBe("problem.network");
  });
  it("maps statuses it does not know by kind", () => {
    expect(problemHeadline(toProblem({}, new Response(null, { status: 403 })))).toBe(
      "problem.forbidden",
    );
    expect(problemHeadline(toProblem({}, new Response(null, { status: 500 })))).toBe(
      "problem.server",
    );
  });
});
