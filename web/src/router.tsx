import { QueryClient } from "@tanstack/react-query";
import { createRootRoute, createRoute, createRouter, Outlet } from "@tanstack/react-router";
import { AnalystHome } from "./routes/AnalystHome";
import { NotFound } from "./routes/NotFound";
import { PortalHome } from "./routes/PortalHome";

/**
 * Two route trees share one app (ADR-0010): the Analyst console at `/` and the
 * Self-Service Portal at `/portal`. List filters, sort and selection will live in the URL.
 */
const rootRoute = createRootRoute({ component: Outlet, notFoundComponent: NotFound });

const analystRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: AnalystHome,
});

const portalRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/portal",
  component: PortalHome,
});

export const routeTree = rootRoute.addChildren([analystRoute, portalRoute]);

/** Creates the router; tests pass their own history. */
export function makeRouter(options: Partial<Parameters<typeof createRouter>[0]> = {}) {
  return createRouter({ routeTree, defaultPreload: "intent", ...options });
}

/** Server state: no global client store (ADR-0010). */
export function makeQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { staleTime: 30_000, retry: 1, refetchOnWindowFocus: true } },
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof makeRouter>;
  }
}
