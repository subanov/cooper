import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";

async function ok<T>(call: Promise<{ data?: T; response: Response }>, what: string): Promise<T> {
  const { data, response } = await call;
  if (data === undefined) {
    throw new Error(`${what} request failed: ${response.status}`);
  }
  return data;
}

/** Every System Setting with its current value (FR-SYS-001). */
export const settingsQuery = queryOptions({
  queryKey: ["administration", "settings"],
  queryFn: () => ok(api.GET("/api/v1/settings"), "settings"),
});

/** Every Background Job with its schedule and latest run (FR-SYS-002). */
export const jobsQuery = queryOptions({
  queryKey: ["administration", "jobs"],
  queryFn: () => ok(api.GET("/api/v1/background-jobs"), "background jobs"),
  refetchInterval: 15_000,
});

/** Run history of one Background Job. */
export function jobRunsQuery(name: string) {
  return queryOptions({
    queryKey: ["administration", "jobs", name, "runs"],
    queryFn: () =>
      ok(
        api.GET("/api/v1/background-jobs/{name}/runs", {
          params: { path: { name }, query: { limit: 20 } },
        }),
        "job runs",
      ),
  });
}

/** Filters of the system log viewer. */
export interface LogFilter {
  level?: "warn" | "error";
  q?: string;
}

/** System log entries, newest first, page by page (FR-SYS-003). */
export function systemLogQuery(filter: LogFilter) {
  return infiniteQueryOptions({
    queryKey: ["administration", "system-log", filter],
    queryFn: ({ pageParam }) =>
      ok(
        api.GET("/api/v1/system-log", {
          params: {
            query: {
              limit: 50,
              ...(filter.level ? { level: filter.level } : {}),
              ...(filter.q ? { q: filter.q } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
        "system log",
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** Bounds a System Setting accepts, by kind. */
export interface SettingConstraints {
  min?: number;
  max?: number;
  max_chars?: number;
  choices?: string[];
}

/** Reads the loosely typed constraints of a setting. */
export function constraintsOf(setting: Schemas["SettingDto"]): SettingConstraints {
  return setting.constraints as SettingConstraints;
}
