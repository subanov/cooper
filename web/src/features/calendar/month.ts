import type { Schemas } from "@/lib/api";

/** A day as `YYYY-MM-DD`. */
export type DayKey = string;

/** A month as `YYYY-MM`. */
export type MonthKey = string;

const pad = (n: number, width = 2) => String(n).padStart(width, "0");

/** The day of a `Date` in UTC arithmetic (date-only maths never meets DST). */
function utcDay(key: DayKey): Date {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(Date.UTC(y ?? 1970, (m ?? 1) - 1, d ?? 1));
}

function keyOfUtc(date: Date): DayKey {
  return `${pad(date.getUTCFullYear(), 4)}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())}`;
}

/** A `YYYY-MM-DD` day as a local `Date` (never parsed as UTC midnight). */
export function dayDate(key: DayKey): Date {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
}

/** The browser-local day of an instant. */
export function localDay(instant: Date): DayKey {
  return `${pad(instant.getFullYear(), 4)}-${pad(instant.getMonth() + 1)}-${pad(instant.getDate())}`;
}

/** The day after (or `n` days after) `key`. */
export function addDays(key: DayKey, n: number): DayKey {
  const d = utcDay(key);
  d.setUTCDate(d.getUTCDate() + n);
  return keyOfUtc(d);
}

/** `value` when it is a `YYYY-MM` month, else the month of `today`. */
export function parseMonth(value: unknown, today: Date = new Date()): MonthKey {
  if (typeof value === "string" && /^\d{4}-(0[1-9]|1[0-2])$/.test(value)) {
    return value;
  }
  return localDay(today).slice(0, 7);
}

/** The month `n` months after `month`. */
export function shiftMonth(month: MonthKey, n: number): MonthKey {
  const [y, m] = month.split("-").map(Number);
  const d = new Date(Date.UTC(y ?? 1970, (m ?? 1) - 1 + n, 1));
  return keyOfUtc(d).slice(0, 7);
}

/** The weeks shown for a month, Monday first, each a list of seven days. */
export function monthGrid(month: MonthKey): DayKey[][] {
  const first = `${month}-01`;
  const weekday = (utcDay(first).getUTCDay() + 6) % 7; // Monday = 0
  let day = addDays(first, -weekday);
  const weeks: DayKey[][] = [];
  do {
    const week: DayKey[] = [];
    for (let i = 0; i < 7; i++) {
      week.push(day);
      day = addDays(day, 1);
    }
    weeks.push(week);
  } while (day.slice(0, 7) === month);
  return weeks;
}

/** The days an event covers, as the browser sees them (timed events in local time). */
export function eventDays(event: Schemas["CalendarEventDto"]): { first: DayKey; last: DayKey } {
  if (event.all_day && event.start_date && event.end_date) {
    return { first: event.start_date, last: event.end_date };
  }
  const start = new Date(event.starts_at ?? 0);
  const end = new Date(event.ends_at ?? event.starts_at ?? 0);
  const first = localDay(start);
  let last = localDay(end);
  // An event ending at midnight does not occupy the day it ends on.
  if (last > first && end.getHours() === 0 && end.getMinutes() === 0) {
    last = addDays(last, -1);
  }
  return { first, last };
}

/** Events per day for the given days, in the order the API returned them. */
export function eventsByDay(
  events: readonly Schemas["CalendarEventDto"][],
  days: readonly DayKey[],
): Map<DayKey, Schemas["CalendarEventDto"][]> {
  const byDay = new Map<DayKey, Schemas["CalendarEventDto"][]>(days.map((d) => [d, []]));
  for (const event of events) {
    const { first, last } = eventDays(event);
    for (const day of days) {
      if (day >= first && day <= last) {
        byDay.get(day)?.push(event);
      }
    }
  }
  return byDay;
}
