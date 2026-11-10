import { queryOptions } from "@tanstack/react-query";
import { ok } from "@/features/okQuery";
import { result } from "@/features/result";
import { api, type Schemas } from "@/lib/api";

/** The current impact level of every service. */
export const statusOverviewQuery = queryOptions({
  queryKey: ["service-status", "overview"],
  queryFn: () => ok(api.GET("/api/v1/service-status"), "service status"),
  refetchInterval: 60_000,
});

/** Impact levels, worst last. */
export const impactLevelsQuery = queryOptions({
  queryKey: ["service-status", "impact-levels"],
  queryFn: () => ok(api.GET("/api/v1/service-status/impact-levels"), "impact levels"),
  staleTime: 5 * 60_000,
});

/** Incident statuses (Investigating, Resolved, …). */
export const incidentStatusesQuery = queryOptions({
  queryKey: ["service-status", "incident-statuses"],
  queryFn: () => ok(api.GET("/api/v1/service-status/incident-statuses"), "incident statuses"),
  staleTime: 5 * 60_000,
});

/** Services shown on the status page. */
export const servicesQuery = queryOptions({
  queryKey: ["service-status", "services"],
  queryFn: () => ok(api.GET("/api/v1/service-status/services"), "services"),
});

/** Incidents, newest first. */
export const incidentsQuery = (open: boolean) =>
  queryOptions({
    queryKey: ["service-status", "incidents", open],
    queryFn: () =>
      ok(
        api.GET("/api/v1/service-status/incidents", { params: { query: { open, limit: 50 } } }),
        "incidents",
      ),
  });

/** One Incident with its updates. */
export const incidentQuery = (id: string) =>
  queryOptions({
    queryKey: ["service-status", "incident", id],
    queryFn: () =>
      ok(
        api.GET("/api/v1/service-status/incidents/{id}", { params: { path: { id } } }),
        "incident",
      ),
  });

/** Opens an Incident. */
export const openIncident = (body: Schemas["OpenServiceIncidentRequest"]) =>
  result(api.POST("/api/v1/service-status/incidents", { body }));

/** Posts an update to an Incident (status, impact and a message). */
export const postIncidentUpdate = (id: string, body: Schemas["PostIncidentUpdateRequest"]) =>
  result(
    api.POST("/api/v1/service-status/incidents/{id}/updates", { params: { path: { id } }, body }),
  );

/** Creates a service. */
export const createService = (body: Schemas["StatusServiceRequest"]) =>
  result(api.POST("/api/v1/service-status/services", { body }));

/** Changes a service. */
export const updateService = (id: string, body: Schemas["StatusServiceRequest"]) =>
  result(api.PUT("/api/v1/service-status/services/{id}", { params: { path: { id } }, body }));
