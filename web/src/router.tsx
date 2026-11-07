import { QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  redirect,
} from "@tanstack/react-router";
import { analystSessionQuery, portalSessionQuery } from "./features/session/queries";
import { parseTicketSearch } from "./features/tickets/views";
import { AnalystLayout } from "./routes/analyst/AnalystLayout";
import { ComingSoon } from "./routes/analyst/ComingSoon";
import { Home } from "./routes/analyst/Home";
import { TicketDetail } from "./routes/analyst/tickets/TicketDetail";
import { TicketsPage } from "./routes/analyst/tickets/TicketsPage";
import { ResetPasswordPage } from "./routes/auth/ResetPassword";
import { SignInPage } from "./routes/auth/SignIn";
import { TwoFactorSetupPage } from "./routes/auth/TwoFactorSetup";
import { NotFound } from "./routes/NotFound";
import { PortalHome } from "./routes/portal/PortalHome";
import { PortalLayout } from "./routes/portal/PortalLayout";
import { PortalResetPage, PortalSetPasswordPage } from "./routes/portal/PortalPassword";
import { PortalRegisterPage } from "./routes/portal/PortalRegister";
import { PortalSignInPage } from "./routes/portal/PortalSignIn";

/** What every route can use: the query cache, for loading data before rendering. */
export interface RouterContext {
  queryClient: QueryClient;
}

/**
 * Two route trees share one app (ADR-0010): the Analyst console at `/` and the
 * Self-Service Portal at `/portal`. List filters, sort and selection will live in the URL.
 */
const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: Outlet,
  notFoundComponent: NotFound,
});

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  validateSearch: (search: Record<string, unknown>): { redirect?: string } =>
    typeof search.redirect === "string" ? { redirect: search.redirect } : {},
  component: function SignInRoute() {
    const { redirect: target } = signInRoute.useSearch();
    return <SignInPage redirect={target} />;
  },
});

const resetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/reset-password",
  validateSearch: (search: Record<string, unknown>): { token?: string } =>
    typeof search.token === "string" ? { token: search.token } : {},
  component: function ResetRoute() {
    const { token } = resetRoute.useSearch();
    return <ResetPasswordPage token={token} />;
  },
});

const twoFactorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/two-factor/setup",
  beforeLoad: async ({ context }) => {
    const session = await context.queryClient.ensureQueryData(analystSessionQuery);
    if (!session) {
      throw redirect({ to: "/sign-in" });
    }
  },
  component: TwoFactorSetupPage,
});

/** Signed-in Analyst screens: sign-in and required two-factor setup come first. */
const analystRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "analyst",
  beforeLoad: async ({ context, location }) => {
    const session = await context.queryClient.ensureQueryData(analystSessionQuery);
    if (!session) {
      throw redirect({ to: "/sign-in", search: { redirect: location.href } });
    }
    if (session.totp_enrollment_required) {
      throw redirect({ to: "/two-factor/setup" });
    }
  },
  component: AnalystLayout,
});

const homeRoute = createRoute({ getParentRoute: () => analystRoute, path: "/", component: Home });

/** The ticket list; view, search and sort are in the URL (FR-TKT-040). */
const ticketsRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/tickets",
  validateSearch: parseTicketSearch,
  component: TicketsPage,
});

/** A ticket opened beside the list (split pane). */
const ticketRoute = createRoute({
  getParentRoute: () => ticketsRoute,
  path: "$ticketId",
  component: function TicketRoute() {
    const { ticketId } = ticketRoute.useParams();
    return <TicketDetail key={ticketId} ticketId={ticketId} />;
  },
});

const administrationRoute = createRoute({
  getParentRoute: () => analystRoute,
  path: "/administration",
  component: () => <ComingSoon section="nav.administration" />,
});

const portalSignInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/sign-in",
  validateSearch: (search: Record<string, unknown>): { redirect?: string } =>
    typeof search.redirect === "string" ? { redirect: search.redirect } : {},
  component: function PortalSignInRoute() {
    const { redirect: target } = portalSignInRoute.useSearch();
    return <PortalSignInPage redirect={target} />;
  },
});

const portalResetRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/reset-password",
  component: PortalResetPage,
});

/** The link in Portal Invitation, reset and Self-Registration emails. */
const portalSetPasswordRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/set-password",
  validateSearch: (search: Record<string, unknown>): { token?: string } =>
    typeof search.token === "string" ? { token: search.token } : {},
  component: function PortalSetPasswordRoute() {
    const { token } = portalSetPasswordRoute.useSearch();
    return <PortalSetPasswordPage token={token} />;
  },
});

const portalRegisterRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal/register",
  component: PortalRegisterPage,
});

/** Signed-in Portal screens (Requesters, FR-PRT-001). */
const portalRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal",
  beforeLoad: async ({ context, location }) => {
    const session = await context.queryClient.ensureQueryData(portalSessionQuery);
    if (!session) {
      throw redirect({ to: "/portal/sign-in", search: { redirect: location.href } });
    }
  },
  component: PortalLayout,
});

const portalHomeRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/",
  component: PortalHome,
});

const portalTicketsRoute = createRoute({
  getParentRoute: () => portalRoute,
  path: "/tickets",
  component: () => <ComingSoon section="portalNav.tickets" />,
});

export const routeTree = rootRoute.addChildren([
  signInRoute,
  resetRoute,
  twoFactorRoute,
  analystRoute.addChildren([
    homeRoute,
    ticketsRoute.addChildren([ticketRoute]),
    administrationRoute,
  ]),
  portalSignInRoute,
  portalResetRoute,
  portalSetPasswordRoute,
  portalRegisterRoute,
  portalRoute.addChildren([portalHomeRoute, portalTicketsRoute]),
]);

/** Server state: no global client store (ADR-0010). */
export function makeQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { staleTime: 30_000, retry: 1, refetchOnWindowFocus: true } },
  });
}

/** Creates the router; tests pass their own history and query client. */
export function makeRouter(
  queryClient: QueryClient,
  options: Partial<Omit<Parameters<typeof createRouter>[0], "routeTree" | "context">> = {},
) {
  return createRouter({
    routeTree,
    context: { queryClient },
    defaultPreload: "intent",
    ...options,
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof makeRouter>;
  }
}
