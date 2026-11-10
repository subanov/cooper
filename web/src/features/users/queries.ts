import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";

async function ok<T>(call: Promise<{ data?: T; response: Response }>, what: string): Promise<T> {
  const { data, response } = await call;
  if (data === undefined) {
    throw new Error(`${what} request failed: ${response.status}`);
  }
  return data;
}

/** Filters of the Analyst and Requester lists. */
export interface UserFilter {
  q?: string;
  includeInactive?: boolean;
}

function listQuery(filter: UserFilter, cursor: string | undefined) {
  return {
    limit: 50,
    include_inactive: filter.includeInactive ?? false,
    ...(filter.q ? { q: filter.q } : {}),
    ...(cursor ? { cursor } : {}),
  };
}

/** Analysts, page by page. */
export function analystListQuery(filter: UserFilter) {
  return infiniteQueryOptions({
    queryKey: ["users", "analysts", "list", filter],
    queryFn: ({ pageParam }) =>
      ok(
        api.GET("/api/v1/analysts", { params: { query: listQuery(filter, pageParam) } }),
        "analysts",
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One Analyst. */
export function analystQuery(id: string) {
  return queryOptions({
    queryKey: ["users", "analysts", "detail", id],
    queryFn: () => ok(api.GET("/api/v1/analysts/{id}", { params: { path: { id } } }), "analyst"),
  });
}

/** Requesters, page by page. */
export function requesterListQuery(filter: UserFilter) {
  return infiniteQueryOptions({
    queryKey: ["users", "requesters", "list", filter],
    queryFn: ({ pageParam }) =>
      ok(
        api.GET("/api/v1/requesters", { params: { query: listQuery(filter, pageParam) } }),
        "requesters",
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One Requester. */
export function requesterQuery(id: string) {
  return queryOptions({
    queryKey: ["users", "requesters", "detail", id],
    queryFn: () =>
      ok(api.GET("/api/v1/requesters/{id}", { params: { path: { id } } }), "requester"),
  });
}

/** Companies the Analyst may see (reference for pickers). */
export const companiesQuery = queryOptions({
  queryKey: ["lookups", "companies"],
  queryFn: async (): Promise<Schemas["CompanyDto"][]> =>
    (await ok(api.GET("/api/v1/companies", { params: { query: { limit: 200 } } }), "companies"))
      .items,
  staleTime: 5 * 60_000,
});

/** Roles an Analyst or Team can hold. */
export const rolesQuery = queryOptions({
  queryKey: ["lookups", "roles"],
  queryFn: async (): Promise<Schemas["RoleDto"][]> =>
    (await ok(api.GET("/api/v1/roles", { params: { query: { limit: 200 } } }), "roles")).items,
  staleTime: 5 * 60_000,
});

/** The default Company (or the only one the Analyst sees), to preselect in forms. */
export function defaultCompanyId(companies: readonly Schemas["CompanyDto"][] | undefined): string {
  const active = (companies ?? []).filter((c) => c.is_active);
  return (active.find((c) => c.is_default) ?? active[0])?.id ?? "";
}
