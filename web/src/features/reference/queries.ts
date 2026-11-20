import { queryOptions } from "@tanstack/react-query";
import type { PickerOption } from "@/components/Combobox";
import type { Option } from "@/components/Field";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

const LOOKUP = { staleTime: 5 * 60_000 } as const;

/** Reads every page of a cursor-paged list (reference lists are short). */
async function allPages<T>(
  page: (cursor: string | undefined) => Promise<{ items: T[]; next_cursor?: string | null }>,
): Promise<T[]> {
  const items: T[] = [];
  let cursor: string | undefined;
  for (let i = 0; i < 20; i++) {
    const result = await page(cursor);
    items.push(...result.items);
    if (!result.next_cursor) break;
    cursor = result.next_cursor;
  }
  return items;
}

export type Lookup = Schemas["LookupDto"];
export type TicketLookupKind =
  | "ticket-statuses"
  | "ticket-priorities"
  | "ticket-types"
  | "ticket-origins"
  | "resolution-codes";

/** Ticket reference data of one kind (statuses, Priorities, Ticket Types, Origins, codes). */
export function ticketLookupQuery(kind: TicketLookupKind) {
  return queryOptions({
    queryKey: ["reference", kind],
    queryFn: (): Promise<Lookup[]> => {
      switch (kind) {
        case "ticket-statuses":
          return call(api.GET("/api/v1/ticket-statuses"));
        case "ticket-priorities":
          return call(api.GET("/api/v1/ticket-priorities"));
        case "ticket-types":
          return call(api.GET("/api/v1/ticket-types"));
        case "ticket-origins":
          return call(api.GET("/api/v1/ticket-origins"));
        case "resolution-codes":
          return call(api.GET("/api/v1/resolution-codes"));
      }
    },
    ...LOOKUP,
  });
}

/** The Category tree, in tree order (`depth` gives the indent). */
export const categoriesQuery = queryOptions({
  queryKey: ["reference", "ticket-categories"],
  queryFn: () => call(api.GET("/api/v1/ticket-categories")),
  ...LOOKUP,
});

/** Companies the Analyst can see, inactive ones included (for names). */
export const companiesQuery = queryOptions({
  queryKey: ["reference", "companies"],
  queryFn: () =>
    allPages((cursor) =>
      call(
        api.GET("/api/v1/companies", {
          params: { query: { include_inactive: true, limit: 200, ...(cursor ? { cursor } : {}) } },
        }),
      ),
    ),
  ...LOOKUP,
});

/** Teams (Queues), inactive ones included. */
export const teamsQuery = queryOptions({
  queryKey: ["reference", "teams"],
  queryFn: () =>
    allPages((cursor) =>
      call(
        api.GET("/api/v1/teams", {
          params: { query: { include_inactive: true, limit: 200, ...(cursor ? { cursor } : {}) } },
        }),
      ),
    ),
  ...LOOKUP,
});

/** Analysts, deactivated ones included (for names in history). */
export const analystsQuery = queryOptions({
  queryKey: ["reference", "analysts"],
  queryFn: () =>
    allPages((cursor) =>
      call(
        api.GET("/api/v1/analysts", {
          params: { query: { include_inactive: true, limit: 200, ...(cursor ? { cursor } : {}) } },
        }),
      ),
    ),
  ...LOOKUP,
});

/** Departments, inactive ones included. */
export const departmentsQuery = queryOptions({
  queryKey: ["reference", "departments"],
  queryFn: () =>
    allPages((cursor) =>
      call(
        api.GET("/api/v1/departments", {
          params: { query: { include_inactive: true, limit: 200, ...(cursor ? { cursor } : {}) } },
        }),
      ),
    ),
  ...LOOKUP,
});

/** Cost Centres, inactive ones included. */
export const costCentresQuery = queryOptions({
  queryKey: ["reference", "cost-centres"],
  queryFn: () =>
    allPages((cursor) =>
      call(
        api.GET("/api/v1/cost-centres", {
          params: { query: { include_inactive: true, limit: 200, ...(cursor ? { cursor } : {}) } },
        }),
      ),
    ),
  ...LOOKUP,
});

/** Roles. */
export const rolesQuery = queryOptions({
  queryKey: ["reference", "roles"],
  queryFn: () =>
    allPages((cursor) =>
      call(
        api.GET("/api/v1/roles", {
          params: { query: { limit: 200, ...(cursor ? { cursor } : {}) } },
        }),
      ),
    ),
  ...LOOKUP,
});

/**
 * Requesters by ID, fetched together. Without `requesters.read` the request is refused
 * and names stay hidden; screens then show "Requester" without a name.
 */
export function requestersByIdQuery(ids: readonly (string | null | undefined)[]) {
  const sorted = [...new Set(ids.filter((id): id is string => Boolean(id)))].sort();
  return queryOptions({
    queryKey: ["reference", "requesters", sorted],
    queryFn: async (): Promise<Map<string, Schemas["RequesterDto"]>> => {
      if (sorted.length === 0) return new Map();
      const { data } = await api.GET("/api/v1/requesters", {
        params: { query: { id: sorted.join(","), include_inactive: true, limit: 200 } },
      });
      return new Map((data?.items ?? []).map((r) => [r.id, r]));
    },
    ...LOOKUP,
  });
}

/** `id → name` for a list of rows. */
export function namesById<T extends { id: string }>(
  rows: readonly T[] | undefined,
  name: (row: T) => string = (row) => (row as unknown as { name: string }).name,
): Map<string, string> {
  return new Map((rows ?? []).map((r) => [r.id, name(r)]));
}

/** Select options from rows: active ones, plus the current value even if inactive. */
export function toOptions<T extends { id: string; is_active?: boolean }>(
  rows: readonly T[] | undefined,
  name: (row: T) => string,
  current?: string | null,
): Option[] {
  return (rows ?? [])
    .filter((r) => r.is_active !== false || r.id === current)
    .map((r) => ({ value: r.id, label: name(r) }));
}

/** Category options with their tree path ("Hardware › Printer"). */
export function categoryOptions(
  rows: readonly Schemas["CategoryDto"][] | undefined,
  current?: string | null,
): Option[] {
  const byId = new Map((rows ?? []).map((c) => [c.id, c]));
  const path = (c: Schemas["CategoryDto"]): string => {
    const parent = c.parent_id ? byId.get(c.parent_id) : undefined;
    return parent ? `${path(parent)} › ${c.name}` : c.name;
  };
  return (rows ?? [])
    .filter((c) => c.is_active || c.id === current)
    .map((c) => ({ value: c.id, label: path(c) }));
}

/** A Requester as a picker option. */
export function requesterOption(r: Schemas["RequesterDto"]): PickerOption {
  return { id: r.id, label: r.full_name, description: r.email };
}

/** Active Analysts as picker options. */
export function analystOptions(rows: readonly Schemas["AnalystDto"][] | undefined): PickerOption[] {
  return (rows ?? [])
    .filter((a) => a.is_active)
    .map((a) => ({ id: a.id, label: a.full_name, description: a.email }));
}
