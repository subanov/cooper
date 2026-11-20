import { useInfiniteQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { BellOff, CheckCheck } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/Button";
import { Card } from "@/components/Card";
import { EmptyState } from "@/components/EmptyState";
import { ProblemNotice } from "@/components/Notice";
import { Page, PageHeader } from "@/components/Page";
import { LoadingBlock, LoadMore } from "@/components/QueryState";
import { Segmented } from "@/components/Tabs";
import { useAction } from "@/features/shared/mutation";
import { useFormat, useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { markAllRead, markRead, notificationHref, notificationsQuery } from "./api";

/** Every Notification of the signed-in Analyst (FR-GEN-006), newest first. */
export function NotificationsPage() {
  const t = useText();
  const f = useFormat();
  const router = useRouter();
  const queryClient = useQueryClient();
  const [show, setShow] = useState<"unread" | "all">("unread");
  const list = useInfiniteQuery(notificationsQuery(show === "unread"));
  const items = list.data?.pages.flatMap((p) => p.items) ?? [];
  const readAll = useAction(markAllRead, {
    invalidate: [["notifications"]],
    success: "notifications.allRead",
  });
  async function open(id: string, href: string | undefined, unread: boolean) {
    if (unread) {
      await markRead(id).catch(() => undefined);
      void queryClient.invalidateQueries({ queryKey: ["notifications"] });
    }
    if (href) router.history.push(href);
  }
  return (
    <Page width="medium">
      <PageHeader
        title={t("notifications.title")}
        description={t("notifications.intro")}
        actions={
          <Button
            icon={<CheckCheck aria-hidden="true" className="size-4" />}
            loading={readAll.isPending}
            onClick={() => readAll.mutate()}
          >
            {t("notifications.markAllRead")}
          </Button>
        }
      />
      <Segmented
        label={t("common.filters")}
        value={show}
        onChange={setShow}
        options={[
          { value: "unread", label: t("notifications.unreadOnly") },
          { value: "all", label: t("notifications.all") },
        ]}
      />
      <ProblemNotice error={list.error ?? readAll.error} />
      <Card flush>
        {list.isPending ? (
          <div className="p-4">
            <LoadingBlock />
          </div>
        ) : items.length === 0 ? (
          <EmptyState
            icon={<BellOff aria-hidden="true" />}
            title={show === "unread" ? t("notifications.emptyUnread") : t("notifications.empty")}
          />
        ) : (
          <ul className="divide-y divide-line">
            {items.map((n) => {
              const href = notificationHref(n);
              const unread = !n.read_at;
              return (
                <li
                  key={n.id}
                  className={cn("flex items-start gap-3 px-4 py-3", unread && "bg-accent-soft/40")}
                >
                  <span
                    aria-hidden="true"
                    className={cn(
                      "mt-1.5 size-2 shrink-0 rounded-full",
                      unread ? "bg-accent" : "bg-transparent",
                    )}
                  />
                  <div className="grid min-w-0 flex-1 gap-0.5">
                    <p className="font-medium">
                      {unread ? (
                        <span className="sr-only">{t("notifications.unread")}: </span>
                      ) : null}
                      {n.title}
                    </p>
                    <p className="text-sm text-fg-muted [overflow-wrap:anywhere]">{n.body}</p>
                    <time dateTime={n.created_at} className="text-xs text-fg-muted">
                      {f.dateTime(n.created_at)}
                    </time>
                  </div>
                  <div className="flex shrink-0 gap-1.5">
                    {href ? (
                      <Button size="sm" onClick={() => void open(n.id, href, unread)}>
                        {t("notifications.open")}
                      </Button>
                    ) : null}
                    {unread ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => void open(n.id, undefined, true)}
                      >
                        {t("notifications.markRead")}
                      </Button>
                    ) : null}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        <LoadMore query={list} />
      </Card>
    </Page>
  );
}
