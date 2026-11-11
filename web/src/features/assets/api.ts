import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { ok } from "@/features/okQuery";
import { result } from "@/features/result";
import { api, type Schemas } from "@/lib/api";

/** Filters of the Asset list. Retired Assets are left out unless asked for (FR-AST-005). */
export interface AssetFilter {
  q?: string;
  assetTypeId?: string;
  assetStatusId?: string;
  locationId?: string;
  includeRetired?: boolean;
}

/** Assets, newest first, page by page. */
export function assetListQuery(filter: AssetFilter) {
  return infiniteQueryOptions({
    queryKey: ["assets", "list", filter],
    queryFn: ({ pageParam }) =>
      ok(
        api.GET("/api/v1/assets", {
          params: {
            query: {
              limit: 50,
              ...(filter.q ? { q: filter.q } : {}),
              ...(filter.assetTypeId ? { asset_type_id: filter.assetTypeId } : {}),
              ...(filter.assetStatusId ? { asset_status_id: filter.assetStatusId } : {}),
              ...(filter.locationId ? { location_id: filter.locationId } : {}),
              ...(filter.includeRetired ? { include_retired: true } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
        "assets",
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** Active and retired counts. */
export const assetSummaryQuery = queryOptions({
  queryKey: ["assets", "summary"],
  queryFn: () => ok(api.GET("/api/v1/assets/summary"), "asset summary"),
});

/** One Asset. */
export const assetQuery = (id: string) =>
  queryOptions({
    queryKey: ["assets", "detail", id],
    queryFn: () => ok(api.GET("/api/v1/assets/{id}", { params: { path: { id } } }), "asset"),
  });

/** The Check Out log of an Asset. */
export const assetCheckoutsQuery = (id: string) =>
  queryOptions({
    queryKey: ["assets", "checkouts", id],
    queryFn: () =>
      ok(api.GET("/api/v1/assets/{id}/checkouts", { params: { path: { id } } }), "checkouts"),
  });

/** Linked tickets and Contracts of an Asset. */
export const assetLinksQuery = (id: string) =>
  queryOptions({
    queryKey: ["assets", "links", id],
    queryFn: () =>
      ok(api.GET("/api/v1/assets/{id}/links", { params: { path: { id } } }), "asset links"),
  });

/** The history of an Asset. */
export const assetHistoryQuery = (id: string) =>
  queryOptions({
    queryKey: ["assets", "history", id],
    queryFn: () =>
      ok(api.GET("/api/v1/assets/{id}/history", { params: { path: { id } } }), "asset history"),
  });

/** Kinds of Asset reference data (FR-AST-002). */
export type AssetLookupKind = "asset-types" | "asset-statuses" | "asset-locations";

// The three kinds share one request and response shape; the path is typed once.
type LookupPath = "/api/v1/asset-types";
const path = (kind: AssetLookupKind) => `/api/v1/${kind}` as LookupPath;
const itemPath = (kind: AssetLookupKind) => `/api/v1/${kind}/{id}` as "/api/v1/asset-types/{id}";

/** Every row of one kind of Asset reference data, including inactive rows. */
export function assetLookupQuery(kind: AssetLookupKind) {
  return queryOptions({
    queryKey: ["lookups", kind],
    queryFn: () => ok(api.GET(path(kind)), kind),
    staleTime: 5 * 60_000,
  });
}

/** Creates an Asset Type, Asset Status or Location. */
export const createAssetLookup = (kind: AssetLookupKind, body: Schemas["AssetLookupRequest"]) =>
  result(api.POST(path(kind), { body }));

/** Changes an Asset Type, Asset Status or Location. */
export const updateAssetLookup = (
  kind: AssetLookupKind,
  id: string,
  body: Schemas["AssetLookupRequest"],
) => result(api.PUT(itemPath(kind), { params: { path: { id } }, body }));

/** Registers an Asset. */
export const createAsset = (body: Schemas["AssetRequest"]) =>
  result(api.POST("/api/v1/assets", { body }));

/** Changes an Asset. */
export const updateAsset = (id: string, body: Schemas["AssetRequest"]) =>
  result(api.PUT("/api/v1/assets/{id}", { params: { path: { id } }, body }));

/** Checks an Asset out to a Holder. */
export const checkOutAsset = (id: string, body: Schemas["AssetCheckOutRequest"]) =>
  result(api.POST("/api/v1/assets/{id}/check-out", { params: { path: { id } }, body }));

/** Checks an Asset back in. */
export const checkInAsset = (id: string, note: string) =>
  result(api.POST("/api/v1/assets/{id}/check-in", { params: { path: { id } }, body: { note } }));

/** Retires an Asset. */
export const retireAsset = (id: string, body: Schemas["AssetRetireRequest"]) =>
  result(api.POST("/api/v1/assets/{id}/retire", { params: { path: { id } }, body }));

/** Links a ticket or Contract. */
export const linkAsset = (id: string, body: Schemas["AssetLinkRequest"]) =>
  result(api.POST("/api/v1/assets/{id}/links", { params: { path: { id } }, body }));

/** Removes a link. */
export const unlinkAsset = (id: string, kind: string, targetId: string) =>
  result(
    api.DELETE("/api/v1/assets/{id}/links/{kind}/{target_id}", {
      params: { path: { id, kind, target_id: targetId } },
    }),
  );

/** The ticket with this Ticket Number, found through ticket search. */
export async function findTicketByNumber(
  number: string,
): Promise<Schemas["TicketDto"] | undefined> {
  const { data } = await api.GET("/api/v1/tickets", {
    params: { query: { q: number, limit: 20 } },
  });
  const wanted = number.trim().toUpperCase();
  return (data?.items ?? []).find((t) => t.number.toUpperCase() === wanted);
}

/** Location names with their parents, `London › Floor 2`, for pickers and tables. */
export function locationPaths(
  rows: readonly Schemas["AssetLookupDto"][] | undefined,
): Map<string, string> {
  const byId = new Map((rows ?? []).map((r) => [r.id, r]));
  const out = new Map<string, string>();
  for (const row of rows ?? []) {
    const names: string[] = [];
    let current: Schemas["AssetLookupDto"] | undefined = row;
    while (current && names.length < 10) {
      names.unshift(current.name);
      current = current.parent_id ? byId.get(current.parent_id) : undefined;
    }
    out.set(row.id, names.join(" › "));
  }
  return out;
}
