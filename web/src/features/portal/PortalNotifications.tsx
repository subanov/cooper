import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { BellOff } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/Button";
import {
  EmptyState,
  LoadingBlock,
  LoadMore,
  ProblemNotice,
  QueryState,
} from "@/components/Feedback";
import { PageHeader, Panel } from "@/components/Layout";
import { useAction } from "@/features/shared/mutation";
import type { MessageId } from "@/i18n/messages";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { cn } from "@/lib/cn";
import { call } from "@/lib/request";

/** Portal Notifications and Notification Preferences (FR-PRT-007). */
export function PortalNotificationsPage() {
  const t = useText();
  const f = useFormat();
  const queryClient = useQueryClient();
  const list = useInfiniteQuery({
    queryKey: ["portal", "notifications", "list"],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/portal/notifications", {
          params: { query: { limit: 30, ...(pageParam ? { cursor: pageParam } : {}) } },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
  const items = list.data?.pages.flatMap((p) => p.items) ?? [];
  const readAll = useAction(() => call(api.POST("/api/v1/portal/notifications/read-all")), {
    invalidate: [["portal", "notifications"]],
    success: "portalNotifications.allRead",
  });
  async function markRead(id: string) {
    await api
      .POST("/api/v1/portal/notifications/{id}/read", { params: { path: { id } } })
      .catch(() => undefined);
    void queryClient.invalidateQueries({ queryKey: ["portal", "notifications"] });
  }
  return (
    <div className="grid gap-6">
      <PageHeader
        title={t("portalNotifications.title")}
        description={t("portalNotifications.intro")}
        actions={
          items.some((n) => !n.read_at) ? (
            <Button size="lg" loading={readAll.isPending} onClick={() => readAll.mutate()}>
              {t("portalNotifications.markAll")}
            </Button>
          ) : undefined
        }
      />
      <ProblemNotice error={list.error ?? readAll.error} />
      <Panel flush>
        {list.isPending ? (
          <LoadingBlock className="p-4" />
        ) : items.length === 0 ? (
          <EmptyState
            icon={<BellOff aria-hidden="true" />}
            title={t("portalNotifications.empty")}
          />
        ) : (
          <ul className="divide-y divide-line">
            {items.map((n) => (
              <li
                key={n.id}
                className={cn(
                  "flex flex-wrap items-start gap-3 px-4 py-3",
                  !n.read_at && "bg-accent-soft/50",
                )}
              >
                <span
                  aria-hidden="true"
                  className={cn(
                    "mt-2 size-2 shrink-0 rounded-full",
                    n.read_at ? "bg-transparent" : "bg-accent",
                  )}
                />
                <div className="grid min-w-0 flex-1 gap-0.5">
                  <p className="font-medium">
                    {!n.read_at ? (
                      <span className="sr-only">{t("portalNotifications.unread")}: </span>
                    ) : null}
                    {n.title}
                  </p>
                  <p className="text-sm text-fg-muted">{n.body}</p>
                  <time dateTime={n.created_at} className="text-xs text-fg-muted">
                    {f.dateTime(n.created_at)}
                  </time>
                </div>
                {n.ticket_id ? (
                  <Link
                    to="/portal/tickets/$id"
                    params={{ id: n.ticket_id }}
                    onClick={() => void markRead(n.id)}
                    className="inline-flex h-11 items-center rounded-md px-3 text-sm font-medium text-accent-text hover:bg-accent-soft"
                  >
                    {t("portalNotifications.open")}
                  </Link>
                ) : null}
              </li>
            ))}
          </ul>
        )}
        <LoadMore query={list} />
      </Panel>
      <Preferences />
    </div>
  );
}

const KINDS = ["ticket_created", "ticket_replied", "ticket_closed"] as const;

function Preferences() {
  const t = useText();
  const prefs = useQuery({
    queryKey: ["portal", "notification-preferences"],
    queryFn: () => call(api.GET("/api/v1/portal/notification-preferences")),
  });
  const [draft, setDraft] = useState<Schemas["NotificationPreferenceDto"][]>([]);
  useEffect(() => {
    if (prefs.data) setDraft(prefs.data.preferences);
  }, [prefs.data]);
  const save = useAction(
    () =>
      call(api.PUT("/api/v1/portal/notification-preferences", { body: { preferences: draft } })),
    {
      invalidate: [["portal", "notification-preferences"]],
      success: "portalNotifications.saved",
    },
  );
  const get = (kind: string) =>
    draft.find((p) => p.kind === kind) ?? { kind, in_app: true, email: true };
  const set = (kind: string, key: "in_app" | "email", on: boolean) =>
    setDraft((d) => [...d.filter((p) => p.kind !== kind), { ...get(kind), [key]: on }]);
  return (
    <Panel
      title={t("portalNotifications.preferences")}
      description={t("portalNotifications.preferencesIntro")}
    >
      <QueryState query={prefs}>
        {() => (
          <form
            className="grid gap-4"
            onSubmit={(e) => {
              e.preventDefault();
              save.mutate();
            }}
          >
            <table className="w-full text-left text-sm">
              <caption className="sr-only">{t("portalNotifications.preferences")}</caption>
              <thead>
                <tr className="border-b border-line text-xs text-fg-muted">
                  <th scope="col" className="py-2 font-semibold">
                    {t("portalNotifications.kind")}
                  </th>
                  <th scope="col" className="w-28 py-2 text-center font-semibold">
                    {t("portalNotifications.inApp")}
                  </th>
                  <th scope="col" className="w-28 py-2 text-center font-semibold">
                    {t("portalNotifications.email")}
                  </th>
                </tr>
              </thead>
              <tbody>
                {KINDS.map((kind) => {
                  const p = get(kind);
                  const label = t(`portalNotifications.kind.${kind}` as MessageId);
                  return (
                    <tr key={kind} className="border-b border-line last:border-0">
                      <th scope="row" className="py-2 font-normal">
                        {label}
                      </th>
                      <td className="py-2 text-center">
                        <input
                          type="checkbox"
                          aria-label={`${label}: ${t("portalNotifications.inApp")}`}
                          checked={p.in_app}
                          onChange={(e) => set(kind, "in_app", e.target.checked)}
                          className="size-5 cursor-pointer accent-accent"
                        />
                      </td>
                      <td className="py-2 text-center">
                        <input
                          type="checkbox"
                          aria-label={`${label}: ${t("portalNotifications.email")}`}
                          checked={p.email}
                          onChange={(e) => set(kind, "email", e.target.checked)}
                          className="size-5 cursor-pointer accent-accent"
                        />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <ProblemNotice error={save.error} />
            <div>
              <Button type="submit" variant="primary" size="lg" loading={save.isPending}>
                {t("common.save")}
              </Button>
            </div>
          </form>
        )}
      </QueryState>
    </Panel>
  );
}
