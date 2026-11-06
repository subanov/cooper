import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { portalSignOut } from "@/features/portal/actions";
import { portalSessionQuery } from "@/features/session/queries";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import { UserMenu } from "../UserMenu";
import { useFocusOnNavigate } from "../useFocusOnNavigate";
import { CompanySwitcher } from "./CompanySwitcher";

const NAV: { to: "/portal" | "/portal/tickets"; label: MessageId }[] = [
  { to: "/portal", label: "portalNav.home" },
  { to: "/portal/tickets", label: "portalNav.tickets" },
];

/**
 * The signed-in Self-Service Portal: header with the Company switcher and account menu,
 * a short navigation and the page. Same design system as the Analyst console, simpler
 * layout for occasional users.
 */
export function PortalLayout() {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const session = useQuery(portalSessionQuery);
  useFocusOnNavigate();

  async function handleSignOut() {
    await portalSignOut();
    queryClient.clear();
    await navigate({ to: "/portal/sign-in" });
  }

  return (
    <div className="flex min-h-dvh flex-col">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-50 focus:rounded-md focus:bg-background focus:px-3 focus:py-2"
      >
        {t("app.skipToContent")}
      </a>
      <header className="border-b border-border">
        <div className="mx-auto flex max-w-5xl flex-wrap items-center gap-x-6 gap-y-2 px-4 py-2">
          <Link to="/portal" className="flex items-center gap-2 font-semibold">
            <img src="/favicon.svg" alt="" className="size-7" />
            <span>{t("app.name")}</span>
            <span className="font-normal text-muted-foreground">{t("portal.title")}</span>
          </Link>
          <nav aria-label={t("nav.label")}>
            <ul className="flex gap-1">
              {NAV.map((item) => (
                <li key={item.to}>
                  <Link
                    to={item.to}
                    activeOptions={{ exact: item.to === "/portal" }}
                    className="block rounded-md px-3 py-2 text-sm hover:bg-muted aria-[current=page]:bg-muted aria-[current=page]:font-medium"
                  >
                    {t(item.label)}
                  </Link>
                </li>
              ))}
            </ul>
          </nav>
          {session.data ? (
            <div className="ml-auto flex items-center gap-2">
              <CompanySwitcher session={session.data} />
              <UserMenu name={session.data.requester.full_name} onSignOut={handleSignOut} />
            </div>
          ) : null}
        </div>
      </header>
      <main id="main" className="mx-auto w-full max-w-5xl flex-1 px-4 py-6">
        <Outlet />
      </main>
    </div>
  );
}
