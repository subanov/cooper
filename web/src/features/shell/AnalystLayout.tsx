import * as RadixDialog from "@radix-ui/react-dialog";
import { useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { Menu as MenuIcon, Plus, Search, X } from "lucide-react";
import { useEffect, useState } from "react";
import { buttonClass, IconButton } from "@/components/Button";
import { Kbd } from "@/components/Facts";
import { useLiveUpdates } from "@/features/live/live";
import { analystSessionQuery, useCan, useModuleOn, useSession } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { useHotkeys } from "@/lib/hooks";
import { AccountMenu } from "./AccountMenu";
import { CommandPalette } from "./CommandPalette";
import { Logo } from "./Logo";
import { NotificationsBell } from "./NotificationsBell";
import { NAV_GROUPS, visibleNav } from "./nav";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { useFocusOnNavigate } from "./useFocusOnNavigate";

/**
 * The Analyst console frame: navigation on the left (a drawer on narrow screens), a top
 * bar with search, New ticket, Notifications and the account menu, and the screen.
 */
export function AnalystLayout() {
  const t = useText();
  const navigate = useNavigate();
  const session = useQuery(analystSessionQuery).data;
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
  return (
    <div className="flex h-dvh bg-canvas">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-[70] focus:rounded-md focus:bg-surface focus:px-3 focus:py-2 focus:shadow-overlay"
      >
        {t("app.skipToContent")}
      </a>
      <aside className="hidden w-60 shrink-0 border-r border-line bg-surface lg:flex lg:flex-col">
        <Sidebar />
      </aside>
      <RadixDialog.Root open={drawer} onOpenChange={setDrawer}>
        <RadixDialog.Portal>
          <RadixDialog.Overlay className="fixed inset-0 z-40 bg-overlay lg:hidden" />
          <RadixDialog.Content
            aria-describedby={undefined}
            className="fixed inset-y-0 left-0 z-50 flex w-72 max-w-[85vw] flex-col border-r border-line bg-surface shadow-overlay lg:hidden"
          >
            <RadixDialog.Title className="sr-only">{t("shell.mainNav")}</RadixDialog.Title>
            <RadixDialog.Close asChild>
              <IconButton
                label={t("shell.closeNav")}
                icon={<X aria-hidden="true" className="size-4" />}
                tooltip="none"
                className="absolute right-2 top-3"
              />
            </RadixDialog.Close>
            <Sidebar onNavigate={() => setDrawer(false)} />
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
  if (can("tickets.create")) bindings.c = onNewTicket;
  for (const item of visibleNav(session, moduleOn)) {
    bindings[`g ${item.shortcut}`] = () => void navigate({ to: item.to });
  }
  useHotkeys(bindings);
  return null;
}

function Sidebar({ onNavigate }: { onNavigate?: () => void }) {
  const t = useText();
  const session = useSession();
  const moduleOn = useModuleOn();
  const items = visibleNav(session, moduleOn);
  return (
    <>
      <div className="flex h-14 shrink-0 items-center gap-2.5 px-4">
        <Logo className="size-7" />
        <span className="text-[0.9375rem] font-semibold">{t("app.name")}</span>
      </div>
      <nav aria-label={t("shell.mainNav")} className="flex-1 overflow-y-auto px-3 pb-4">
        {NAV_GROUPS.map((group) => {
          const entries = items.filter((i) => i.group === group.id);
          if (entries.length === 0) return null;
          return (
            <div key={group.id} className="mt-4 first:mt-1">
              <p className="px-2 pb-1 text-[0.6875rem] font-semibold uppercase tracking-wider text-fg-muted">
                {t(group.label)}
              </p>
              <ul className="grid gap-0.5">
                {entries.map((item) => (
                  <li key={String(item.to)}>
                    <Link
                      to={item.to}
                      onClick={onNavigate}
                      activeOptions={{ exact: item.to === "/" }}
                      activeProps={{ "aria-current": "page" }}
                      className="flex h-9 items-center gap-2.5 rounded-md px-2 text-sm font-medium text-fg-muted hover:bg-sunken hover:text-fg aria-[current=page]:bg-accent-soft aria-[current=page]:text-accent-text [&_svg]:size-[1.0625rem]"
                    >
                      <item.icon aria-hidden="true" />
                      {t(item.label)}
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
  return (
    <header className="flex h-14 shrink-0 items-center gap-2 border-b border-line bg-surface px-3 sm:px-4">
      <IconButton
        label={t("shell.openNav")}
        icon={<MenuIcon aria-hidden="true" className="size-[1.125rem]" />}
        className="lg:hidden"
        onClick={onOpenDrawer}
      />
      <button
        type="button"
        onClick={onOpenPalette}
        aria-keyshortcuts="Control+K Meta+K"
        className="flex h-9 w-full max-w-md items-center gap-2 rounded-md border border-line bg-canvas px-3 text-left text-sm text-fg-muted hover:border-control"
      >
        <Search aria-hidden="true" className="size-4 shrink-0" />
        <span className="flex-1 truncate">{t("shell.search")}</span>
        <span className="hidden sm:inline">
          <Kbd>{t("shell.searchShortcut")}</Kbd>
        </span>
      </button>
      <div className="ml-auto flex items-center gap-1.5">
        {can("tickets.create") ? (
          <Link
            to="/tickets/new"
            search={{ view: "open" }}
            aria-keyshortcuts="c"
            className={buttonClass({ variant: "primary", className: "max-sm:w-9 max-sm:px-0" })}
          >
            <Plus aria-hidden="true" className="size-4" />
            <span className="max-sm:sr-only">{t("shell.newTicket")}</span>
          </Link>
        ) : null}
        <NotificationsBell />
        <AccountMenu onShowShortcuts={onShowShortcuts} />
      </div>
    </header>
  );
}
