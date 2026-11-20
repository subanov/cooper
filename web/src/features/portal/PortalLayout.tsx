import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { Bell, Building2, LogOut, Menu as MenuIcon } from "lucide-react";
import { useState } from "react";
import { Button, buttonClass } from "@/components/Button";
import { Menu, MenuItem, MenuLabel, MenuRadioGroup, MenuSeparator } from "@/components/Menu";
import { useToast } from "@/components/Toast";
import { portalSessionQuery } from "@/features/session/session";
import { Logo } from "@/features/shell/Logo";
import { useFocusOnNavigate } from "@/features/shell/useFocusOnNavigate";
import { useText } from "@/i18n/useText";
import { api, setCsrfToken } from "@/lib/api";
import { cn } from "@/lib/cn";
import { call } from "@/lib/request";
import { portalUnreadQuery } from "./api";

/** The signed-in Requester. Only used below the Portal layout, which requires a session. */
export function usePortalSession() {
  const { data } = useQuery(portalSessionQuery);
  if (!data) throw new Error("usePortalSession outside the Portal");
  return data;
}

const NAV = [
  { to: "/portal", label: "portal.nav.home", exact: true },
  { to: "/portal/tickets", label: "portal.nav.tickets", exact: false },
  { to: "/portal/knowledge", label: "portal.nav.knowledge", exact: false },
  { to: "/portal/service-status", label: "portal.nav.status", exact: false },
] as const;

/**
 * The Self-Service Portal frame (FR-PRT-001): plain navigation for occasional users,
 * large touch targets, and the Company the session acts for always visible.
 */
export function PortalLayout() {
  const t = useText();
  const session = useQuery(portalSessionQuery).data;
  const unread = useQuery({ ...portalUnreadQuery, enabled: Boolean(session) }).data?.unread ?? 0;
  const [menuOpen, setMenuOpen] = useState(false);
  useFocusOnNavigate();
  if (!session) return null;
  return (
    <div className="flex min-h-dvh flex-col bg-canvas">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-[70] focus:rounded-md focus:bg-surface focus:px-3 focus:py-2 focus:shadow-overlay"
      >
        {t("app.skipToContent")}
      </a>
      <header className="border-b border-line bg-surface">
        <div className="mx-auto flex h-16 max-w-5xl items-center gap-3 px-4">
          <Link to="/portal" className="flex items-center gap-2.5 rounded-md">
            <Logo />
            <span className="grid leading-tight">
              <span className="font-semibold">{t("app.name")}</span>
              <span className="text-xs text-fg-muted">{t("portal.name")}</span>
            </span>
          </Link>
          <nav aria-label={t("portal.nav")} className="ml-6 hidden md:block">
            <PortalNav />
          </nav>
          <div className="ml-auto flex items-center gap-1">
            <Link
              to="/portal/notifications"
              aria-label={
                unread > 0
                  ? t("portal.notificationsUnread", { count: unread })
                  : t("portal.notifications")
              }
              className={buttonClass({
                variant: "ghost",
                size: "lg",
                className: "relative w-11 px-0",
              })}
            >
              <Bell aria-hidden="true" className="size-5" />
              {unread > 0 ? (
                <span
                  aria-hidden="true"
                  className="absolute right-1 top-1.5 grid h-4 min-w-4 place-items-center rounded-full bg-danger px-1 text-[0.625rem] font-semibold text-danger-fg"
                >
                  {unread > 99 ? "99+" : unread}
                </span>
              ) : null}
            </Link>
            <AccountMenu />
            <Button
              variant="ghost"
              size="lg"
              className="w-11 px-0 md:hidden"
              aria-label={t("portal.nav")}
              aria-expanded={menuOpen}
              aria-controls="portal-mobile-nav"
              onClick={() => setMenuOpen((o) => !o)}
            >
              <MenuIcon aria-hidden="true" className="size-5" />
            </Button>
          </div>
        </div>
        {menuOpen ? (
          <nav
            id="portal-mobile-nav"
            aria-label={t("portal.nav")}
            className="border-t border-line px-4 py-2 md:hidden"
          >
            <PortalNav vertical onNavigate={() => setMenuOpen(false)} />
          </nav>
        ) : null}
      </header>
      <main id="main" className="mx-auto w-full max-w-5xl flex-1 px-4 py-8">
        <Outlet />
      </main>
      <footer className="border-t border-line bg-surface">
        <p className="mx-auto max-w-5xl px-4 py-5 text-sm text-fg-muted">{t("portal.footer")}</p>
      </footer>
    </div>
  );
}

function PortalNav({
  vertical = false,
  onNavigate,
}: {
  vertical?: boolean;
  onNavigate?: () => void;
}) {
  const t = useText();
  return (
    <ul className={cn("flex gap-1", vertical && "flex-col")}>
      {NAV.map((item) => (
        <li key={item.to}>
          <Link
            to={item.to}
            onClick={onNavigate}
            activeOptions={{ exact: item.exact }}
            activeProps={{ "aria-current": "page" }}
            className="flex h-11 items-center rounded-md px-3 text-[0.9375rem] font-medium text-fg-muted hover:bg-sunken hover:text-fg aria-[current=page]:text-accent-text aria-[current=page]:underline aria-[current=page]:decoration-2 aria-[current=page]:underline-offset-8"
          >
            {t(item.label)}
          </Link>
        </li>
      ))}
    </ul>
  );
}

function AccountMenu() {
  const t = useText();
  const session = usePortalSession();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const toast = useToast();
  async function switchTo(requesterId: string) {
    try {
      const next = await call(
        api.PUT("/api/v1/portal/session/requester", { body: { requester_id: requesterId } }),
      );
      setCsrfToken("portal", next.csrf_token);
      queryClient.setQueryData(portalSessionQuery.queryKey, next);
      await queryClient.invalidateQueries({ queryKey: ["portal"] });
      toast({
        tone: "success",
        message: t("portal.switched", { company: next.requester.company_name }),
      });
    } catch {
      toast({ tone: "danger", message: t("problem.server") });
    }
  }
  async function signOut() {
    await api.DELETE("/api/v1/portal/session").catch(() => undefined);
    setCsrfToken("portal", undefined);
    queryClient.clear();
    await navigate({ to: "/portal/sign-in" });
  }
  const name = session.requester.full_name;
  return (
    <Menu
      label={t("portal.account", { name })}
      trigger={
        <Button
          variant="ghost"
          size="lg"
          className="gap-2 px-2"
          title={t("portal.account", { name })}
        >
          <span
            aria-hidden="true"
            className="grid size-8 place-items-center rounded-full bg-accent-soft text-xs font-semibold text-accent-text"
          >
            {name
              .split(/\s+/)
              .map((p) => p[0] ?? "")
              .join("")
              .slice(0, 2)
              .toUpperCase()}
          </span>
          <span className="sr-only">{name}, </span>
          <span className="sr-only max-w-40 truncate text-sm sm:not-sr-only">
            {session.requester.company_name}
          </span>
        </Button>
      }
    >
      <MenuLabel>
        <span className="grid gap-0.5">
          <span className="text-sm text-fg">{name}</span>
          <span>{session.email}</span>
          <span className="inline-flex items-center gap-1">
            <Building2 aria-hidden="true" className="size-3" />
            {t("portal.actingFor", { company: session.requester.company_name })}
          </span>
        </span>
      </MenuLabel>
      {session.requesters.length > 1 ? (
        <>
          <MenuSeparator />
          <MenuRadioGroup
            label={t("portal.switchCompany")}
            value={session.requester.id}
            onChange={(id) => void switchTo(id)}
            options={session.requesters.map((r) => ({ value: r.id, label: r.company_name }))}
          />
        </>
      ) : null}
      <MenuSeparator />
      <MenuItem icon={<LogOut />} onSelect={() => void signOut()}>
        {t("portal.signOut")}
      </MenuItem>
    </Menu>
  );
}
