import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas, upload } from "@/lib/api";
import { call, uploaded } from "@/lib/request";

export type PortalTicket = Schemas["PortalTicketDto"];

export const catalogueQuery = queryOptions({
  queryKey: ["portal", "catalogue"],
  queryFn: () => call(api.GET("/api/v1/portal/catalogue")),
  staleTime: 5 * 60_000,
});

export function portalTicketsQuery(show: "open" | "closed" | "managed") {
  return infiniteQueryOptions({
    queryKey: ["portal", "tickets", show],
    queryFn: ({ pageParam }) => {
      const query = {
        limit: 30,
        ...(show === "managed" ? {} : { open: show === "open" }),
        ...(pageParam ? { cursor: pageParam } : {}),
      };
      return show === "managed"
        ? call(api.GET("/api/v1/portal/managed-tickets", { params: { query } }))
        : call(api.GET("/api/v1/portal/tickets", { params: { query } }));
    },
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

export const portalTicketQuery = (id: string) =>
  queryOptions({
    queryKey: ["portal", "ticket", id],
    queryFn: () => call(api.GET("/api/v1/portal/tickets/{id}", { params: { path: { id } } })),
  });
export const portalTimelineQuery = (id: string) =>
  queryOptions({
    queryKey: ["portal", "ticket", id, "timeline"],
    queryFn: () =>
      call(api.GET("/api/v1/portal/tickets/{id}/timeline", { params: { path: { id } } })),
  });
export const portalAttachmentsQuery = (id: string) =>
  queryOptions({
    queryKey: ["portal", "ticket", id, "attachments"],
    queryFn: () =>
      call(api.GET("/api/v1/portal/tickets/{id}/attachments", { params: { path: { id } } })),
  });

export function portalArticlesQuery(q: string | undefined) {
  return queryOptions({
    queryKey: ["portal", "knowledge", q ?? ""],
    queryFn: () =>
      call(
        api.GET("/api/v1/portal/knowledge", {
          params: { query: { limit: 30, ...(q ? { q } : {}) } },
        }),
      ),
  });
}

export const portalStatusQuery = queryOptions({
  queryKey: ["portal", "service-status"],
  queryFn: () => call(api.GET("/api/v1/portal/service-status")),
  refetchInterval: 60_000,
});

export const portalUnreadQuery = queryOptions({
  queryKey: ["portal", "notifications", "unread"],
  queryFn: () => call(api.GET("/api/v1/portal/notifications/unread-count")),
  refetchInterval: 60_000,
});

export const portalActions = {
  create: (body: Schemas["PortalTicketRequest"]) =>
    call(api.POST("/api/v1/portal/tickets", { body })),
  reply: (id: string, body: string) =>
    call(
      api.POST("/api/v1/portal/tickets/{id}/replies", { params: { path: { id } }, body: { body } }),
    ),
  upload: (id: string, file: File) =>
    uploaded<Schemas["AttachmentDto"]>(upload(`/api/v1/portal/tickets/${id}/attachments`, file)),
};
