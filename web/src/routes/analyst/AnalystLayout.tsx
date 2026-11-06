import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { modulesQuery } from "@/features/navigation/modules";
import { NAV_ITEMS, visibleNavItems } from "@/features/navigation/navigation";
import { signOut } from "@/features/session/actions";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import { UserMenu } from "../UserMenu";
import { useFocusOnNavigate } from "../useFocusOnNavigate";
import { CommandPalette } from "./CommandPalette";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { useShortcuts } from "./useShortcuts";

/**
 * The signed-in Analyst console: header with the command palette and account menu, the
 * navigation (enabled Modules and held Capabilities only, FR-GEN-005) and the page.
 */
export function AnalystLayout() {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const session = useQuery(analystSessionQuery);
  const modules = useQuery(modulesQuery);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);

  const items = useMemo(
    () => (session.data ? visibleNavItems(session.data, modules.data) : NAV_ITEMS.slice(0, 1)),
    [session.data, modules.data],
  );

  async function handleSignOut() {
    await signOut();
    queryClient.clear();
    await navigate({ to: "/sign-in" });
  }

  useShortcuts({
    palette: () => setPaletteOpen(true),
    help: () => setHelpOpen(true),
    goTo: (key) => {
      const item = items.find((i) => i.shortcut === key);
      if (item) {
        void navigate({ to: item.to });
      }
    },
  });

  useFocusOnNavigate();

  return (
    <div className="flex min-h-dvh flex-col">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-50 focus:rounded-md focus:bg-background focus:px-3 focus:py-2"
      >
        {t("app.skipToContent")}
      </a>
      <header className="flex items-center gap-3 border-b border-border px-4 py-2">
        <Link to="/" className="flex items-center gap-2 font-semibold">
          <img src="/favicon.svg" alt="" className="size-7" />
          {t("app.name")}
        </Link>
        <Button
          variant="outline"
          size="sm"
          className="ml-4 w-64 justify-between text-muted-foreground"
          onClick={() => setPaletteOpen(true)}
          aria-keyshortcuts="Control+K Meta+K"
        >
          {t("palette.open")}
          <kbd className="font-mono text-xs">Ctrl K</kbd>
        </Button>
        <div className="ml-auto">
          {session.data ? (
            <UserMenu name={session.data.analyst.full_name} onSignOut={handleSignOut} />
          ) : null}
        </div>
      </header>
      <div className="flex flex-1">
        <nav aria-label={t("nav.label")} className="w-56 shrink-0 border-r border-border p-3">
          <ul className="grid gap-1">
            {items.map((item) => (
              <li key={item.to}>
                <Link
                  to={item.to}
                  activeOptions={{ exact: item.to === "/" }}
                  className="block rounded-md px-3 py-2 text-sm hover:bg-muted aria-[current=page]:bg-muted aria-[current=page]:font-medium"
                >
                  {t(item.label)}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
        <main id="main" className="flex-1 px-8 py-6">
          <Outlet />
        </main>
      </div>
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        items={items}
        onShowShortcuts={() => setHelpOpen(true)}
        onSignOut={handleSignOut}
      />
      <ShortcutsDialog open={helpOpen} onOpenChange={setHelpOpen} items={items} />
    </div>
  );
}
