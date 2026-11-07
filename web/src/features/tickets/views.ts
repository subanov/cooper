import type { MessageId } from "@/i18n/messages";

/** Built-in ticket views (FR-TKT-040), in the order the list shows them. */
export const VIEWS = ["mine", "queue", "unassigned", "open", "snoozed", "recently_closed"] as const;
export type View = (typeof VIEWS)[number];

export const VIEW_LABELS: Record<View, MessageId> = {
  mine: "views.mine",
  queue: "views.queue",
  unassigned: "views.unassigned",
  open: "views.open",
  snoozed: "views.snoozed",
  recently_closed: "views.recentlyClosed",
};

export const SORTS = ["updated_at", "created_at", "number"] as const;
export type Sort = (typeof SORTS)[number];

/** The ticket list's state, held in the URL so it survives reloads and can be shared. */
export interface TicketSearch {
  view?: View | undefined;
  q?: string | undefined;
  sort?: Sort | undefined;
  order?: "asc" | "desc" | undefined;
}

function oneOf<T extends string>(values: readonly T[], value: unknown): T | undefined {
  return typeof value === "string" && (values as readonly string[]).includes(value)
    ? (value as T)
    : undefined;
}

/** Reads the list state from the URL, dropping anything unknown. */
export function parseTicketSearch(search: Record<string, unknown>): TicketSearch {
  const result: TicketSearch = {};
  const view = oneOf(VIEWS, search.view);
  if (view) result.view = view;
  if (typeof search.q === "string" && search.q.trim()) result.q = search.q;
  const sort = oneOf(SORTS, search.sort);
  if (sort) result.sort = sort;
  const order = oneOf(["asc", "desc"] as const, search.order);
  if (order) result.order = order;
  return result;
}

/** The view shown when the URL names none. */
export const DEFAULT_VIEW: View = "mine";
