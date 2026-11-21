import * as RadixDialog from "@radix-ui/react-dialog";
import { useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { Menu as MenuIcon, PanelLeftClose, PanelLeftOpen, Plus, Search, X } from "lucide-react";
import { useEffect, useState } from "react";
import { buttonClass, IconButton } from "@/components/Button";
import { Kbd } from "@/components/Layout";
import { useLiveUpdates } from "@/features/live/live";
import { analystSessionQuery, useCan, useModuleOn, useSession } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { useHotkeys } from "@/lib/hooks";
import { setPreferences, usePreferences } from "@/lib/preferences";
import { AccountMenu } from "./AccountMenu";
import { CommandPalette } from "./CommandPalette";
import { useFocusOnNavigate } from "./focus";
import { Logo } from "./Logo";
import { NotificationsBell } from "./NotificationsBell";
import { NAV_GROUPS, visibleNav } from "./nav";
import { ShortcutsDialog } from "./ShortcutsDialog";

/**
 * The Analyst console frame: a dark navigation rail on the left (collapsible to icons, a
 * drawer on narrow screens), a top bar with search, New ticket, Notifications and the
 * account menu, and the screen. The rail never changes between screens (H4).
 */
export function AnalystLayout() {
  const t = useText();
  const navigate = useNavigate();
  const session = useQuery(analystSessionQuery).data;
  const { nav } = usePreferences();
  const [palette, setPalette] = useState(false);
  const [shortcuts, setShortcuts] = useState(false);
  const [drawer, setDrawer] = useState(false);
  useLiveUpdates(Boolean(session));
  useFocusOnNavigate();

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPalette((open) => !open);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!session) return null;
  const collapsed = nav === "collapsed";
  return (
    <div className="flex h-dvh bg-canvas">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-[70] focus:rounded-md focus:bg-surface focus:px-3 focus:py-2 focus:shadow-overlay"
      >
        {t("app.skipToContent")}
      </a>
      <aside
        className={cn(
          "hidden shrink-0 flex-col bg-nav text-nav-fg lg:flex",
          collapsed ? "w-16" : "w-60",
        )}
      >
        <Sidebar collapsed={collapsed} />
        <div className={cn("border-t border-white/10 p-2", collapsed && "flex justify-center")}>
          <button
            type="button"
            onClick={() => setPreferences({ nav: collapsed ? "expanded" : "collapsed" })}
            aria-label={collapsed ? t("shell.expandNav") : t("shell.collapseNav")}
            aria-expanded={!collapsed}
            className="flex h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-[0.8125rem] text-nav-muted hover:bg-nav-hover hover:text-nav-fg [&_svg]:size-[1.0625rem]"
          >
            {collapsed ? (
              <PanelLeftOpen aria-hidden="true" />
            ) : (
              <PanelLeftClose aria-hidden="true" />
            )}
            {collapsed ? null : <span>{t("shell.collapseNav")}</span>}
          </button>
        </div>
      </aside>
      <RadixDialog.Root open={drawer} onOpenChange={setDrawer}>
        <RadixDialog.Portal>
          <RadixDialog.Overlay className="fixed inset-0 z-40 bg-overlay lg:hidden" />
          <RadixDialog.Content
            aria-describedby={undefined}
            className="fixed inset-y-0 left-0 z-50 flex w-72 max-w-[85vw] flex-col bg-nav text-nav-fg shadow-overlay lg:hidden"
          >
            <RadixDialog.Title className="sr-only">{t("shell.mainNav")}</RadixDialog.Title>
            <RadixDialog.Close asChild>
              <button
                type="button"
                aria-label={t("shell.closeNav")}
                className="absolute right-2 top-3 grid size-9 place-items-center rounded-md text-nav-muted hover:bg-nav-hover hover:text-nav-fg"
              >
                <X aria-hidden="true" className="size-4" />
              </button>
            </RadixDialog.Close>
            <Sidebar collapsed={false} onNavigate={() => setDrawer(false)} />
          </RadixDialog.Content>
        </RadixDialog.Portal>
      </RadixDialog.Root>

      <div className="flex min-w-0 flex-1 flex-col">
        <TopBar
          onOpenPalette={() => setPalette(true)}
          onOpenDrawer={() => setDrawer(true)}
          onShowShortcuts={() => setShortcuts(true)}
        />
        <main id="main" className="min-h-0 flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>

      <GlobalHotkeys
        onHelp={() => setShortcuts(true)}
        onNewTicket={() => void navigate({ to: "/tickets/new", search: { view: "open" } })}
      />
      <CommandPalette
        open={palette}
        onOpenChange={setPalette}
        onShowShortcuts={() => setShortcuts(true)}
      />
      <ShortcutsDialog open={shortcuts} onOpenChange={setShortcuts} />
    </div>
  );
}

function GlobalHotkeys({ onHelp, onNewTicket }: { onHelp: () => void; onNewTicket: () => void }) {
  const navigate = useNavigate();
  const session = useSession();
  const moduleOn = useModuleOn();
  const can = useCan();
  const bindings: Record<string, () => void> = { "?": onHelp };
  if (can("tickets.create") && moduleOn("tickets")) bindings.c = onNewTicket;
  for (const item of visibleNav(session, moduleOn)) {
    bindings[`g ${item.shortcut}`] = () => void navigate({ to: item.to });
  }
  useHotkeys(bindings);
  return null;
}

function Sidebar({ collapsed, onNavigate }: { collapsed: boolean; onNavigate?: () => void }) {
  const t = useText();
  const session = useSession();
  const moduleOn = useModuleOn();
  const items = visibleNav(session, moduleOn);
  return (
    <>
      <div
        className={cn(
          "flex h-14 shrink-0 items-center gap-2.5",
          collapsed ? "justify-center" : "px-4",
        )}
      >
        <Logo className="size-7" />
        {collapsed ? null : <span className="text-[0.9375rem] font-semibold">{t("app.name")}</span>}
      </div>
      <nav
        aria-label={t("shell.mainNav")}
        className={cn("flex-1 overflow-y-auto pb-4", collapsed ? "px-2" : "px-3")}
      >
        {NAV_GROUPS.map((group) => {
          const entries = items.filter((i) => i.group === group.id);
          if (entries.length === 0) return null;
          return (
            <div key={group.id} className="mt-4 first:mt-1">
              {collapsed ? (
                <div aria-hidden="true" className="mx-2 mb-2 h-px bg-white/10" />
              ) : (
                <p className="px-2.5 pb-1.5 text-[0.6875rem] font-semibold uppercase tracking-wider text-nav-muted">
                  {t(group.label)}
                </p>
              )}
              <ul className="grid gap-0.5">
                {entries.map((item) => (
                  <li key={String(item.to)}>
                    <Link
                      to={item.to}
                      onClick={onNavigate}
                      activeOptions={{ exact: item.to === "/" }}
                      activeProps={{ "aria-current": "page" }}
                      aria-label={collapsed ? t(item.label) : undefined}
                      data-tip={collapsed ? t(item.label) : undefined}
                      className={cn(
                        "flex h-9 items-center gap-3 rounded-md text-sm font-medium text-nav-muted hover:bg-nav-hover hover:text-nav-fg aria-[current=page]:bg-nav-active aria-[current=page]:text-nav-fg [&_svg]:size-[1.125rem] [&_svg]:shrink-0",
                        collapsed ? "tip justify-center px-0" : "px-2.5",
                      )}
                    >
                      <item.icon aria-hidden="true" />
                      {collapsed ? null : <span className="truncate">{t(item.label)}</span>}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
      </nav>
    </>
  );
}

function TopBar({
  onOpenPalette,
  onOpenDrawer,
  onShowShortcuts,
}: {
  onOpenPalette: () => void;
  onOpenDrawer: () => void;
  onShowShortcuts: () => void;
}) {
  const t = useText();
  const can = useCan();
  const moduleOn = useModuleOn();
  return (
    <header className="flex h-14 shrink-0 items-center gap-2 border-b border-line bg-surface px-3 sm:px-4">
      <IconButton
        label={t("shell.openNav")}
        icon={<MenuIcon aria-hidden="true" />}
        className="lg:hidden"
        onClick={onOpenDrawer}
      />
      <button
        type="button"
        onClick={onOpenPalette}
        aria-keyshortcuts="Control+K Meta+K"
        className="flex h-9 w-full max-w-lg items-center gap-2 rounded-md border border-line bg-canvas px-3 text-left text-sm text-fg-muted hover:border-control"
      >
        <Search aria-hidden="true" className="size-4 shrink-0" />
        <span className="flex-1 truncate">{t("shell.search")}</span>
        <span className="hidden sm:inline">
          <Kbd>{t("shell.searchShortcut")}</Kbd>
        </span>
      </button>
      <div className="ml-auto flex items-center gap-1">
        {can("tickets.create") && moduleOn("tickets") ? (
          <Link
            to="/tickets/new"
            search={{ view: "open" }}
            aria-keyshortcuts="c"
            className={buttonClass({ variant: "primary", className: "max-sm:w-9 max-sm:px-0" })}
          >
            <Plus aria-hidden="true" />
            <span className="max-sm:sr-only">{t("shell.newTicket")}</span>
          </Link>
        ) : null}
        <NotificationsBell />
        <AccountMenu onShowShortcuts={onShowShortcuts} />
      </div>
    </header>
  );
}
