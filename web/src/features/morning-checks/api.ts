import { queryOptions } from "@tanstack/react-query";
import { ok } from "@/features/okQuery";
import { result } from "@/features/result";
import { api, type Schemas } from "@/lib/api";

/** One day of Morning Checks with the recorded results (`date`: `YYYY-MM-DD`, default today). */
export const checkDayQuery = (date: string | undefined) =>
  queryOptions({
    queryKey: ["morning-checks", "day", date ?? "today"],
    queryFn: () =>
      ok(
        api.GET("/api/v1/morning-checks/results", {
          params: { query: date ? { date } : {} },
        }),
        "morning checks",
      ),
  });

/** Result statuses (OK, Warning, Failed, …). */
export const checkStatusesQuery = queryOptions({
  queryKey: ["morning-checks", "statuses"],
  queryFn: () => ok(api.GET("/api/v1/morning-checks/statuses"), "check statuses"),
  staleTime: 5 * 60_000,
});

/** All Checks, including inactive ones. */
export const checksQuery = queryOptions({
  queryKey: ["morning-checks", "checks"],
  queryFn: () => ok(api.GET("/api/v1/morning-checks/checks"), "checks"),
});

/** Check Groups. */
export const checkGroupsQuery = queryOptions({
  queryKey: ["morning-checks", "groups"],
  queryFn: () => ok(api.GET("/api/v1/morning-checks/groups"), "check groups"),
});

/** Records (or changes) the result of a Check for a day. */
export const recordResult = (body: Schemas["RecordCheckResultRequest"]) =>
  result(api.POST("/api/v1/morning-checks/results", { body }));

/** Creates a Check. */
export const createCheck = (body: Schemas["CheckRequest"]) =>
  result(api.POST("/api/v1/morning-checks/checks", { body }));

/** Changes a Check. */
export const updateCheck = (id: string, body: Schemas["CheckRequest"]) =>
  result(api.PUT("/api/v1/morning-checks/checks/{id}", { params: { path: { id } }, body }));
