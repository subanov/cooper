import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { ok } from "@/features/okQuery";
import { result } from "@/features/result";
import { api, type Schemas } from "@/lib/api";

/** The Task board: one column per Task Status, open Tasks only unless `open` is false. */
export function taskBoardQuery(filter: { q?: string; mine?: string; open?: boolean }) {
  return queryOptions({
    queryKey: ["tasks", "board", filter],
    queryFn: () =>
      ok(
        api.GET("/api/v1/tasks/board", {
          params: {
            query: {
              open: filter.open ?? true,
              ...(filter.q ? { q: filter.q } : {}),
              ...(filter.mine ? { assignee: filter.mine } : {}),
            },
          },
        }),
        "task board",
      ),
  });
}

/** Tasks as a list, page by page. */
export function taskListQuery(filter: { q?: string; open?: boolean }) {
  return infiniteQueryOptions({
    queryKey: ["tasks", "list", filter],
    queryFn: ({ pageParam }) =>
      ok(
        api.GET("/api/v1/tasks", {
          params: {
            query: {
              limit: 50,
              open: filter.open ?? true,
              ...(filter.q ? { q: filter.q } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
        "tasks",
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One Task. */
export const taskQuery = (id: string) =>
  queryOptions({
    queryKey: ["tasks", "detail", id],
    queryFn: () => ok(api.GET("/api/v1/tasks/{id}", { params: { path: { id } } }), "task"),
  });

/** Comments of a Task. */
export const taskCommentsQuery = (id: string) =>
  queryOptions({
    queryKey: ["tasks", "comments", id],
    queryFn: () =>
      ok(api.GET("/api/v1/tasks/{id}/comments", { params: { path: { id } } }), "task comments"),
  });

/** Task Statuses. */
export const taskStatusesQuery = queryOptions({
  queryKey: ["lookups", "task-statuses"],
  queryFn: () => ok(api.GET("/api/v1/task-statuses"), "task statuses"),
  staleTime: 5 * 60_000,
});

/** Task Priorities. */
export const taskPrioritiesQuery = queryOptions({
  queryKey: ["lookups", "task-priorities"],
  queryFn: () => ok(api.GET("/api/v1/task-priorities"), "task priorities"),
  staleTime: 5 * 60_000,
});

/** Creates a Task. */
export const createTask = (body: Schemas["TaskRequest"]) =>
  result(api.POST("/api/v1/tasks", { body }));

/** Changes a Task. */
export const updateTask = (id: string, body: Schemas["TaskRequest"]) =>
  result(api.PUT("/api/v1/tasks/{id}", { params: { path: { id } }, body }));

/** Moves a Task to another Status (closing needs a closed Status). */
export const moveTask = (id: string, status_id: string) =>
  result(api.POST("/api/v1/tasks/{id}/status", { params: { path: { id } }, body: { status_id } }));

/** Adds a comment to a Task. */
export const addTaskComment = (id: string, body: string) =>
  result(api.POST("/api/v1/tasks/{id}/comments", { params: { path: { id } }, body: { body } }));
