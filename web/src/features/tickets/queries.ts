import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";
import { DEFAULT_VIEW, type TicketSearch } from "./views";

async function ok<T>(call: Promise<{ data?: T; response: Response }>, what: string): Promise<T> {
  const { data, response } = await call;
  if (data === undefined) {
    throw new Error(`${what} request failed: ${response.status}`);
  }
  return data;
}

/**
 * Live Updates (ADR-0022) refresh ticket screens when a ticket changes, including SLA
 * Warnings and Breaches. This slow refresh only catches what a push cannot: a ticket the
 * Analyst may no longer see, and the SLA time remaining as it counts down.
 */
const FALLBACK_REFETCH_MS = 5 * 60_000;

/** One built-in view of the ticket list, page by page (cursor pagination). */
export function ticketListQuery(search: TicketSearch) {
  return infiniteQueryOptions({
    queryKey: ["tickets", "list", search],
    queryFn: ({ pageParam }) =>
      ok(
        api.GET("/api/v1/tickets", {
          params: {
            query: {
              view: search.view ?? DEFAULT_VIEW,
              sort: search.sort ?? "updated_at",
              order: search.order ?? "desc",
              limit: 50,
              ...(search.q ? { q: search.q } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
        "ticket list",
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
    refetchInterval: FALLBACK_REFETCH_MS,
  });
}

/** One ticket. */
export function ticketQuery(id: string) {
  return queryOptions({
    queryKey: ["tickets", "detail", id],
    queryFn: () => ok(api.GET("/api/v1/tickets/{id}", { params: { path: { id } } }), "ticket"),
    refetchInterval: FALLBACK_REFETCH_MS,
  });
}

/** The ticket's timeline: emails, Notes, Time Entries and changes (FR-TKT-024). */
export function timelineQuery(id: string) {
  return queryOptions({
    queryKey: ["tickets", "timeline", id],
    queryFn: () =>
      ok(api.GET("/api/v1/tickets/{id}/timeline", { params: { path: { id } } }), "timeline"),
  });
}

const LOOKUP = { staleTime: 5 * 60_000 } as const;

/** Ticket Statuses (reference data). */
export const statusesQuery = queryOptions({
  queryKey: ["lookups", "statuses"],
  queryFn: () => ok(api.GET("/api/v1/ticket-statuses"), "statuses"),
  ...LOOKUP,
});

/** Priorities (reference data). */
export const prioritiesQuery = queryOptions({
  queryKey: ["lookups", "priorities"],
  queryFn: () => ok(api.GET("/api/v1/ticket-priorities"), "priorities"),
  ...LOOKUP,
});

/** Resolution Codes (reference data). */
export const resolutionCodesQuery = queryOptions({
  queryKey: ["lookups", "resolution-codes"],
  queryFn: () => ok(api.GET("/api/v1/resolution-codes"), "resolution codes"),
  ...LOOKUP,
});

/** Active Analysts, for names and assignment. */
export const analystsQuery = queryOptions({
  queryKey: ["lookups", "analysts"],
  queryFn: async () =>
    (await ok(api.GET("/api/v1/analysts", { params: { query: { limit: 200 } } }), "analysts"))
      .items,
  ...LOOKUP,
});

/** Active Teams (Queues), for names. */
export const teamsQuery = queryOptions({
  queryKey: ["lookups", "teams"],
  queryFn: async () =>
    (await ok(api.GET("/api/v1/teams", { params: { query: { limit: 200 } } }), "teams")).items,
  ...LOOKUP,
});

/**
 * Names of the Requesters on screen, looked up by ID in one request. Without
 * `requesters.read` the request is refused and names stay hidden.
 */
export function requesterNamesQuery(ids: readonly string[]) {
  const sorted = [...new Set(ids)].sort();
  return queryOptions({
    queryKey: ["lookups", "requesters", sorted],
    queryFn: async (): Promise<Record<string, Schemas["RequesterDto"]>> => {
      if (sorted.length === 0) {
        return {};
      }
      const { data } = await api.GET("/api/v1/requesters", {
        params: { query: { id: sorted.join(","), include_inactive: true, limit: 200 } },
      });
      return Object.fromEntries((data?.items ?? []).map((r) => [r.id, r]));
    },
    ...LOOKUP,
  });
}

/** `id → name` for a list of lookup rows. */
export function namesById<T extends { id: string }>(
  rows: readonly T[] | undefined,
  name: (row: T) => string,
): Map<string, string> {
  return new Map((rows ?? []).map((r) => [r.id, name(r)]));
}
