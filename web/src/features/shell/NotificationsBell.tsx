import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { Bell } from "lucide-react";
import { Button } from "@/components/Button";
import { Menu, MenuItem, MenuLabel, MenuSeparator } from "@/components/Menu";
import {
  markRead,
  notificationHref,
  recentNotificationsQuery,
  unreadCountQuery,
} from "@/features/notifications/api";
import { useFormat, useText } from "@/i18n/useText";

/** The bell (FR-GEN-006): unread count at a glance, the newest items one click away. */
export function NotificationsBell() {
  const t = useText();
  const f = useFormat();
  const navigate = useNavigate();
  const router = useRouter();
  const queryClient = useQueryClient();
  const unread = useQuery(unreadCountQuery).data?.unread ?? 0;
  const recent = useQuery(recentNotificationsQuery);
  const items = recent.data?.items ?? [];
  return (
    <Menu
      label={t("notifications.recent")}
      trigger={
        <Button
          variant="ghost"
          className="relative w-9 px-0"
          aria-label={unread > 0 ? t("bell.unread", { count: unread }) : t("bell.label")}
        >
          <Bell aria-hidden="true" className="size-[1.125rem]" />
          {unread > 0 ? (
            <span
              aria-hidden="true"
              className="absolute right-0.5 top-0.5 grid h-4 min-w-4 place-items-center rounded-full bg-danger px-1 text-[0.625rem] font-semibold leading-none text-danger-fg"
            >
              {unread > 99 ? "99+" : unread}
            </span>
          ) : null}
        </Button>
      }
    >
      <MenuLabel>{t("notifications.recent")}</MenuLabel>
      {items.length === 0 ? (
        <p className="px-2.5 py-3 text-sm text-fg-muted">{t("notifications.empty")}</p>
      ) : null}
      {items.map((n) => (
        <MenuItem
          key={n.id}
          onSelect={async () => {
            if (!n.read_at) {
              await markRead(n.id).catch(() => undefined);
              void queryClient.invalidateQueries({ queryKey: ["notifications"] });
            }
            const href = notificationHref(n);
            if (href) router.history.push(href);
          }}
        >
          <span className="grid max-w-72 gap-0.5 py-1.5">
            <span className="flex items-center gap-1.5 font-medium">
              {!n.read_at ? (
                <span className="size-2 shrink-0 rounded-full bg-accent" aria-hidden="true" />
              ) : null}
              {!n.read_at ? <span className="sr-only">{t("notifications.unread")}:</span> : null}
              <span className="truncate">{n.title}</span>
            </span>
            <span className="truncate text-xs text-fg-muted">
              {f.relative(n.created_at)} · {n.body}
            </span>
          </span>
        </MenuItem>
      ))}
      <MenuSeparator />
      <MenuItem onSelect={() => void navigate({ to: "/notifications" })}>
        {t("notifications.seeAll")}
      </MenuItem>
    </Menu>
  );
}
