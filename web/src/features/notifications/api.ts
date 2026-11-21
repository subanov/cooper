import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

export type Notification = Schemas["NotificationDto"];

/** The bell badge: refreshed every minute and after Live Updates. */
export const unreadCountQuery = queryOptions({
  queryKey: ["notifications", "unread-count"],
  queryFn: () => call(api.GET("/api/v1/notifications/unread-count")),
  refetchInterval: 60_000,
});

/** The newest Notifications, for the bell menu. */
export const recentNotificationsQuery = queryOptions({
  queryKey: ["notifications", "recent"],
  queryFn: () => call(api.GET("/api/v1/notifications", { params: { query: { limit: 8 } } })),
});

/** Every Notification, page by page. */
export function notificationsQuery(unread: boolean) {
  return infiniteQueryOptions({
    queryKey: ["notifications", "list", unread],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/notifications", {
          params: {
            query: {
              limit: 30,
              ...(unread ? { unread } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

export const markRead = (id: string) =>
  call(api.POST("/api/v1/notifications/{id}/read", { params: { path: { id } } }));
export const markAllRead = () => call(api.POST("/api/v1/notifications/read-all"));

/** Where a Notification leads: the record it is about, when there is a screen for it. */
export function notificationHref(n: Notification): string | undefined {
  if (n.ticket_id) return `/tickets/${n.ticket_id}`;
  if (n.task_id) return `/tasks/${n.task_id}`;
  if (n.contract_id) return `/contracts/${n.contract_id}`;
  if (n.article_id) return `/knowledge/${n.article_id}`;
  return undefined;
}
