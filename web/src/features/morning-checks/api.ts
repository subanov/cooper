import { queryOptions } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { call } from "@/lib/request";

export const checkStatusesQuery = queryOptions({
  queryKey: ["reference", "check-statuses"],
  queryFn: () => call(api.GET("/api/v1/morning-checks/statuses")),
  staleTime: 5 * 60_000,
});
export const checkGroupsQuery = queryOptions({
  queryKey: ["morning-checks", "groups"],
  queryFn: () => call(api.GET("/api/v1/morning-checks/groups")),
});
export const checksQuery = queryOptions({
  queryKey: ["morning-checks", "checks"],
  queryFn: () => call(api.GET("/api/v1/morning-checks/checks")),
});
export function roundQuery(date: string | undefined, mine: boolean) {
  return queryOptions({
    queryKey: ["morning-checks", "round", date ?? "today", mine],
    queryFn: () =>
      call(
        api.GET("/api/v1/morning-checks/results", {
          params: { query: { ...(date ? { date } : {}), ...(mine ? { mine } : {}) } },
        }),
      ),
  });
}
