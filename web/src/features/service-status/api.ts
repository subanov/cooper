import { queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

export type Incident = Schemas["ServiceIncidentDto"];

export const boardQuery = queryOptions({
  queryKey: ["service-status", "board"],
  queryFn: () => call(api.GET("/api/v1/service-status")),
  refetchInterval: 60_000,
});
export const servicesQuery = queryOptions({
  queryKey: ["service-status", "services"],
  queryFn: () => call(api.GET("/api/v1/service-status/services")),
});
export const impactLevelsQuery = queryOptions({
  queryKey: ["reference", "impact-levels"],
  queryFn: () => call(api.GET("/api/v1/service-status/impact-levels")),
  staleTime: 5 * 60_000,
});
export const incidentStatusesQuery = queryOptions({
  queryKey: ["reference", "incident-statuses"],
  queryFn: () => call(api.GET("/api/v1/service-status/incident-statuses")),
  staleTime: 5 * 60_000,
});
export function incidentsQuery(open: boolean) {
  return queryOptions({
    queryKey: ["service-status", "incidents", open],
    queryFn: () =>
      call(
        api.GET("/api/v1/service-status/incidents", {
          params: { query: { limit: 100, ...(open ? { open } : {}) } },
        }),
      ),
  });
}
export const incidentQuery = (id: string) =>
  queryOptions({
    queryKey: ["service-status", "incident", id],
    queryFn: () =>
      call(api.GET("/api/v1/service-status/incidents/{id}", { params: { path: { id } } })),
  });
