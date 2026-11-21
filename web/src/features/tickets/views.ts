import type { MessageId } from "@/i18n/messages";
import { compact, oneOf, text } from "@/routes/search";

/** Built-in ticket views (FR-TKT-040), in list order. `deleted` is for Administrators. */
export const VIEWS = [
  "mine",
  "queue",
  "unassigned",
  "open",
  "snoozed",
  "recently_closed",
  "deleted",
] as const;
export type View = (typeof VIEWS)[number];

export const VIEW_LABELS: Record<View, MessageId> = {
  mine: "views.mine",
  queue: "views.queue",
  unassigned: "views.unassigned",
  open: "views.open",
  snoozed: "views.snoozed",
  recently_closed: "views.recentlyClosed",
  deleted: "views.deleted",
};

export const SORTS = ["updated_at", "created_at", "number"] as const;
export type Sort = (typeof SORTS)[number];

/** The ticket list's state, held in the URL so reloads, Back and sharing keep it. */
export interface TicketSearch {
  view?: View;
  q?: string;
  sort?: Sort;
  order?: "asc" | "desc";
}

export const DEFAULT_VIEW: View = "mine";

/** Reads the list state from the URL, dropping anything unknown. */
export function parseTicketSearch(search: Record<string, unknown>): TicketSearch {
  return compact({
    view: oneOf(search, "view", VIEWS),
    q: text(search, "q"),
    sort: oneOf(search, "sort", SORTS),
    order: oneOf(search, "order", ["asc", "desc"] as const),
  });
}
