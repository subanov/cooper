import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

export type Task = Schemas["TaskDto"];

const LOOKUP = { staleTime: 5 * 60_000 } as const;

export const taskStatusesQuery = queryOptions({
  queryKey: ["reference", "task-statuses"],
  queryFn: () => call(api.GET("/api/v1/task-statuses")),
  ...LOOKUP,
});

export const taskPrioritiesQuery = queryOptions({
  queryKey: ["reference", "task-priorities"],
  queryFn: () => call(api.GET("/api/v1/task-priorities")),
  ...LOOKUP,
});

export interface TaskSearch {
  q?: string;
  mine?: true;
  closed?: true;
}

/** The Board (FR-TSK-005): a column per active status, top-level Tasks in order. */
export function boardQuery(search: TaskSearch) {
  return queryOptions({
    queryKey: ["tasks", "board", search],
    queryFn: () =>
      call(
        api.GET("/api/v1/tasks/board", {
          params: {
            query: {
              ...(search.q ? { q: search.q } : {}),
              ...(search.mine ? { assignee: "me" } : {}),
              ...(search.closed ? {} : { open: true }),
            },
          },
        }),
      ),
  });
}

/** Tasks linked to a ticket. */
export function ticketTasksQuery(ticketId: string) {
  return queryOptions({
    queryKey: ["tasks", "by-ticket", ticketId],
    queryFn: async () =>
      (
        await call(
          api.GET("/api/v1/tasks", { params: { query: { ticket_id: ticketId, limit: 100 } } }),
        )
      ).items,
  });
}

export function taskQuery(id: string) {
  return queryOptions({
    queryKey: ["tasks", "detail", id],
    queryFn: () => call(api.GET("/api/v1/tasks/{id}", { params: { path: { id } } })),
  });
}

export function taskCommentsQuery(id: string) {
  return queryOptions({
    queryKey: ["tasks", "detail", id, "comments"],
    queryFn: () => call(api.GET("/api/v1/tasks/{id}/comments", { params: { path: { id } } })),
  });
}

export function taskLinksQuery(id: string) {
  return queryOptions({
    queryKey: ["tasks", "detail", id, "links"],
    queryFn: () => call(api.GET("/api/v1/tasks/{id}/links", { params: { path: { id } } })),
  });
}

export function taskHistoryQuery(id: string) {
  return queryOptions({
    queryKey: ["tasks", "detail", id, "history"],
    queryFn: () => call(api.GET("/api/v1/tasks/{id}/history", { params: { path: { id } } })),
  });
}

/** Subtasks of a Task. */
export function subtasksQuery(id: string) {
  return infiniteQueryOptions({
    queryKey: ["tasks", "detail", id, "subtasks"],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/tasks", {
          params: {
            query: { parent_task_id: id, limit: 50, ...(pageParam ? { cursor: pageParam } : {}) },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

const path = (id: string) => ({ params: { path: { id } } });

export const taskActions = {
  create: (body: Schemas["TaskRequest"]) => call(api.POST("/api/v1/tasks", { body })),
  update: (id: string, body: Schemas["TaskRequest"]) =>
    call(api.PUT("/api/v1/tasks/{id}", { ...path(id), body })),
  move: (id: string, body: Schemas["TaskMoveRequest"]) =>
    call(api.POST("/api/v1/tasks/{id}/status", { ...path(id), body })),
  comment: (id: string, body: string) =>
    call(api.POST("/api/v1/tasks/{id}/comments", { ...path(id), body: { body } })),
  link: (id: string, body: Schemas["TaskLinkRequest"]) =>
    call(api.POST("/api/v1/tasks/{id}/links", { ...path(id), body })),
  unlink: (id: string, kind: string, targetId: string) =>
    call(
      api.DELETE("/api/v1/tasks/{id}/links/{kind}/{target_id}", {
        params: { path: { id, kind, target_id: targetId } },
      }),
    ),
};
