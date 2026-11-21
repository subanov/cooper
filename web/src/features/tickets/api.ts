import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas, upload } from "@/lib/api";
import { call, uploaded } from "@/lib/request";
import { DEFAULT_VIEW, type TicketSearch } from "./views";

export type Ticket = Schemas["TicketDto"];

/**
 * Live Updates refresh ticket screens when a ticket changes (ADR-0022). This slow
 * refresh only catches what a push cannot, such as SLA time counting down.
 */
const FALLBACK_REFETCH_MS = 5 * 60_000;

/** One view of the ticket list, page by page. */
export function ticketListQuery(search: TicketSearch) {
  return infiniteQueryOptions({
    queryKey: ["tickets", "list", search],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/tickets", {
          params: {
            query: {
              view: search.view ?? DEFAULT_VIEW,
              sort: search.sort ?? "updated_at",
              order: search.order ?? "desc",
              limit: 50,
              ...(search.q ? { q: search.q } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
    refetchInterval: FALLBACK_REFETCH_MS,
  });
}

export function ticketQuery(id: string) {
  return queryOptions({
    queryKey: ["tickets", "detail", id],
    queryFn: () => call(api.GET("/api/v1/tickets/{id}", { params: { path: { id } } })),
    refetchInterval: FALLBACK_REFETCH_MS,
  });
}

/** The timeline: emails, Notes, Time Entries and changes (FR-TKT-024). */
export function timelineQuery(id: string) {
  return queryOptions({
    queryKey: ["tickets", "detail", id, "timeline"],
    queryFn: () => call(api.GET("/api/v1/tickets/{id}/timeline", { params: { path: { id } } })),
  });
}

export function attachmentsQuery(id: string) {
  return queryOptions({
    queryKey: ["tickets", "detail", id, "attachments"],
    queryFn: () => call(api.GET("/api/v1/tickets/{id}/attachments", { params: { path: { id } } })),
  });
}

export function relatedQuery(id: string) {
  return queryOptions({
    queryKey: ["tickets", "detail", id, "related"],
    queryFn: () => call(api.GET("/api/v1/tickets/{id}/related", { params: { path: { id } } })),
  });
}

export const replyTemplatesQuery = queryOptions({
  queryKey: ["reference", "reply-templates"],
  queryFn: () => call(api.GET("/api/v1/reply-templates")),
  staleTime: 5 * 60_000,
});

/** Active Ticket Fields (needs `tickets.read`). */
export const ticketFieldsQuery = queryOptions({
  queryKey: ["reference", "ticket-fields"],
  queryFn: () => call(api.GET("/api/v1/ticket-fields")),
  staleTime: 5 * 60_000,
});

const path = (id: string) => ({ params: { path: { id } } });

export const ticketActions = {
  assign: (id: string, body: Schemas["AssignmentRequest"]) =>
    call(api.PUT("/api/v1/tickets/{id}/assignment", { ...path(id), body })),
  status: (id: string, body: Schemas["StatusRequest"]) =>
    call(api.POST("/api/v1/tickets/{id}/status", { ...path(id), body })),
  update: (id: string, body: Schemas["TicketRequest"]) =>
    call(api.PUT("/api/v1/tickets/{id}", { ...path(id), body })),
  note: (id: string, body: Schemas["NoteRequest"]) =>
    call(api.POST("/api/v1/tickets/{id}/notes", { ...path(id), body })),
  reply: (id: string, body: Schemas["ReplyRequest"]) =>
    call(api.POST("/api/v1/tickets/{id}/replies", { ...path(id), body })),
  snooze: (id: string, body: Schemas["SnoozeRequest"]) =>
    call(api.PUT("/api/v1/tickets/{id}/snooze", { ...path(id), body })),
  wake: (id: string) => call(api.DELETE("/api/v1/tickets/{id}/snooze", path(id))),
  logTime: (id: string, body: Schemas["TimeEntryRequest"]) =>
    call(api.POST("/api/v1/tickets/{id}/time-entries", { ...path(id), body })),
  remove: (id: string) => call(api.DELETE("/api/v1/tickets/{id}", path(id))),
  restore: (id: string) => call(api.POST("/api/v1/tickets/{id}/restore", path(id))),
  create: (body: Schemas["TicketRequest"]) => call(api.POST("/api/v1/tickets", { body })),
  bulk: (body: Schemas["BulkRequest"]) => call(api.POST("/api/v1/tickets/bulk", { body })),
  renderTemplate: (id: string, templateId: string) =>
    call(
      api.GET("/api/v1/tickets/{id}/reply-templates/{template_id}", {
        params: { path: { id, template_id: templateId } },
      }),
    ),
  upload: (id: string, file: File) =>
    uploaded<Schemas["AttachmentDto"]>(upload(`/api/v1/tickets/${id}/attachments`, file)),
};

/** The request that keeps everything as it is, for edits that change some details. */
export function ticketRequest(t: Ticket): Schemas["TicketRequest"] {
  return {
    subject: t.subject,
    description: t.description,
    priority_id: t.priority_id,
    type_id: t.type_id ?? null,
    category_id: t.category_id ?? null,
    origin_id: t.origin_id ?? null,
    department_id: t.department_id ?? null,
    requester_id: t.requester_id ?? null,
    sensitivity: t.sensitivity,
    first_time_fix: t.first_time_fix,
    training_provided: t.training_provided,
  };
}
