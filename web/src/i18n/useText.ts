import { useIntl } from "react-intl";
import type { MessageId } from "./messages";

export type Text = (id: MessageId, values?: Record<string, string | number>) => string;

/** Formats a catalogue message by id (ids are type-checked). */
export function useText(): Text {
  const intl = useIntl();
  return (id, values) => intl.formatMessage({ id }, values);
}

/** Date, time and number formatting in the user's locale and time zone. */
export function useFormat() {
  const intl = useIntl();
  return {
    /** "12 Mar 2026, 14:05" */
    dateTime: (at: string | Date) =>
      intl.formatDate(at, { dateStyle: "medium", timeStyle: "short" }),
    /** "12 Mar 2026" for an instant. */
    date: (at: string | Date) => intl.formatDate(at, { dateStyle: "medium" }),
    /** "12 Mar 2026" for a calendar day (`YYYY-MM-DD`), without time-zone shifts. */
    day: (key: string) => {
      const [y, m, d] = key.split("-").map(Number);
      return intl.formatDate(new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1), { dateStyle: "medium" });
    },
    /** "14:05" */
    time: (at: string | Date) => intl.formatDate(at, { timeStyle: "short" }),
    /** "3 hours ago", "in 2 days" */
    relative: (at: string | Date, now: number = Date.now()) => {
      const seconds = Math.round((new Date(at).getTime() - now) / 1000);
      const abs = Math.abs(seconds);
      if (abs < 60) return intl.formatRelativeTime(seconds, "second", { numeric: "auto" });
      if (abs < 3600) return intl.formatRelativeTime(Math.round(seconds / 60), "minute");
      if (abs < 86_400) return intl.formatRelativeTime(Math.round(seconds / 3600), "hour");
      return intl.formatRelativeTime(Math.round(seconds / 86_400), "day", { numeric: "auto" });
    },
    number: (n: number) => intl.formatNumber(n),
  };
}

/** `2d 3h`, `3h 20m` or `45m`: a compact duration for SLA time left. */
export function compactDuration(seconds: number): string {
  const minutes = Math.max(0, Math.round(Math.abs(seconds) / 60));
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  const mins = minutes % 60;
  if (days > 0) return hours ? `${days}d ${hours}h` : `${days}d`;
  if (hours > 0) return mins ? `${hours}h ${mins}m` : `${hours}h`;
  return `${mins}m`;
}
