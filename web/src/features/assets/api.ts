import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

export type Asset = Schemas["AssetDto"];
export type AssetLookupKind = "asset-types" | "asset-statuses" | "asset-locations";

export interface AssetSearch {
  q?: string;
  type?: string;
  status?: string;
  location?: string;
  retired?: true;
}

export function assetLookupQuery(kind: AssetLookupKind) {
  return queryOptions({
    queryKey: ["reference", kind],
    queryFn: () => {
      switch (kind) {
        case "asset-types":
          return call(api.GET("/api/v1/asset-types"));
        case "asset-statuses":
          return call(api.GET("/api/v1/asset-statuses"));
        case "asset-locations":
          return call(api.GET("/api/v1/asset-locations"));
      }
    },
    staleTime: 5 * 60_000,
  });
}

export function assetsQuery(search: AssetSearch) {
  return infiniteQueryOptions({
    queryKey: ["assets", "list", search],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/assets", {
          params: {
            query: {
              limit: 50,
              ...(search.q ? { q: search.q } : {}),
              ...(search.type ? { asset_type_id: search.type } : {}),
              ...(search.status ? { asset_status_id: search.status } : {}),
              ...(search.location ? { location_id: search.location } : {}),
              ...(search.retired ? { include_retired: true } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

export const assetSummaryQuery = queryOptions({
  queryKey: ["assets", "summary"],
  queryFn: () => call(api.GET("/api/v1/assets/summary")),
});

export const assetQuery = (id: string) =>
  queryOptions({
    queryKey: ["assets", "detail", id],
    queryFn: () => call(api.GET("/api/v1/assets/{id}", { params: { path: { id } } })),
  });
export const checkoutsQuery = (id: string) =>
  queryOptions({
    queryKey: ["assets", "detail", id, "checkouts"],
    queryFn: () => call(api.GET("/api/v1/assets/{id}/checkouts", { params: { path: { id } } })),
  });
export const assetHistoryQuery = (id: string) =>
  queryOptions({
    queryKey: ["assets", "detail", id, "history"],
    queryFn: () => call(api.GET("/api/v1/assets/{id}/history", { params: { path: { id } } })),
  });
export const assetLinksQuery = (id: string) =>
  queryOptions({
    queryKey: ["assets", "detail", id, "links"],
    queryFn: () => call(api.GET("/api/v1/assets/{id}/links", { params: { path: { id } } })),
  });
const path = (id: string) => ({ params: { path: { id } } });
export const assetActions = {
  create: (body: Schemas["AssetRequest"]) => call(api.POST("/api/v1/assets", { body })),
  update: (id: string, body: Schemas["AssetRequest"]) =>
    call(api.PUT("/api/v1/assets/{id}", { ...path(id), body })),
  checkOut: (id: string, body: Schemas["AssetCheckOutRequest"]) =>
    call(api.POST("/api/v1/assets/{id}/check-out", { ...path(id), body })),
  checkIn: (id: string, body: Schemas["AssetCheckInRequest"]) =>
    call(api.POST("/api/v1/assets/{id}/check-in", { ...path(id), body })),
  retire: (id: string, body: Schemas["AssetRetireRequest"]) =>
    call(api.POST("/api/v1/assets/{id}/retire", { ...path(id), body })),
  link: (id: string, body: Schemas["AssetLinkRequest"]) =>
    call(api.POST("/api/v1/assets/{id}/links", { ...path(id), body })),
  unlink: (id: string, kind: string, targetId: string) =>
    call(
      api.DELETE("/api/v1/assets/{id}/links/{kind}/{target_id}", {
        params: { path: { id, kind, target_id: targetId } },
      }),
    ),
};

/** The name an Asset is known by: tag, else hostname, else Service Tag. */
export function assetName(
  a: Pick<Asset, "asset_tag" | "hostname" | "service_tag">,
): string | undefined {
  return a.asset_tag || a.hostname || a.service_tag || undefined;
}

/** Location options with their path ("HQ › Floor 2 › Room 4"). */
export function locationOptions(
  rows: readonly Schemas["AssetLookupDto"][] | undefined,
  current?: string | null,
) {
  const byId = new Map((rows ?? []).map((r) => [r.id, r]));
  const pathOf = (r: Schemas["AssetLookupDto"], depth = 0): string => {
    const parent = r.parent_id && depth < 8 ? byId.get(r.parent_id) : undefined;
    return parent ? `${pathOf(parent, depth + 1)} › ${r.name}` : r.name;
  };
  return (rows ?? [])
    .filter((r) => r.is_active || r.id === current)
    .map((r) => ({ value: r.id, label: pathOf(r) }))
    .sort((a, b) => a.label.localeCompare(b.label));
}
