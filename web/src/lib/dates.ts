/** A calendar day as `YYYY-MM-DD`. */
export type DayKey = string;
/** A month as `YYYY-MM`. */
export type MonthKey = string;

const pad = (n: number, width = 2) => String(n).padStart(width, "0");

function utcDay(key: DayKey): Date {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(Date.UTC(y ?? 1970, (m ?? 1) - 1, d ?? 1));
}

function utcKey(date: Date): DayKey {
  return `${pad(date.getUTCFullYear(), 4)}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())}`;
}

/** The browser-local day of an instant. */
export function localDay(instant: Date = new Date()): DayKey {
  return `${pad(instant.getFullYear(), 4)}-${pad(instant.getMonth() + 1)}-${pad(instant.getDate())}`;
}

/** A `YYYY-MM-DD` day as a local midnight `Date` (never parsed as UTC). */
export function dayDate(key: DayKey): Date {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
}

/** `n` days after `key`. Date-only arithmetic in UTC never meets daylight saving. */
export function addDays(key: DayKey, n: number): DayKey {
  const d = utcDay(key);
  d.setUTCDate(d.getUTCDate() + n);
  return utcKey(d);
}

/** Whether a string is a `YYYY-MM-DD` day. */
export function isDayKey(value: unknown): value is DayKey {
  return typeof value === "string" && /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/.test(value);
}

/** `value` when it is a `YYYY-MM` month, else the month of `today`. */
export function parseMonth(value: unknown, today: Date = new Date()): MonthKey {
  return typeof value === "string" && /^\d{4}-(0[1-9]|1[0-2])$/.test(value)
    ? value
    : localDay(today).slice(0, 7);
}

/** The month `n` months after `month`. */
export function shiftMonth(month: MonthKey, n: number): MonthKey {
  const [y, m] = month.split("-").map(Number);
  return utcKey(new Date(Date.UTC(y ?? 1970, (m ?? 1) - 1 + n, 1))).slice(0, 7);
}

/** The weeks of a month view, Monday first, seven days each. */
export function monthGrid(month: MonthKey): DayKey[][] {
  const first = `${month}-01`;
  let day = addDays(first, -((utcDay(first).getUTCDay() + 6) % 7));
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

/** A `datetime-local` input value for an instant, in the browser's time zone. */
export function toLocalInput(instant: string | Date | null | undefined): string {
  if (!instant) return "";
  const d = typeof instant === "string" ? new Date(instant) : instant;
  if (Number.isNaN(d.getTime())) return "";
  return `${localDay(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** An RFC 3339 instant from a `datetime-local` input value (browser time zone). */
export function fromLocalInput(value: string): string | null {
  if (!value) return null;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}
