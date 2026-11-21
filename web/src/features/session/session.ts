import { queryOptions, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { api, type Schemas, setCsrfToken } from "@/lib/api";
import { call } from "@/lib/request";

export type Session = Schemas["SessionDto"];
export type PortalSession = Schemas["PortalSessionDto"];

/** The Analyst session, or `null` when signed out. Holds the CSRF token in memory. */
export const analystSessionQuery = queryOptions({
  queryKey: ["session", "analyst"],
  queryFn: async (): Promise<Session | null> => {
    const { data, response } = await api.GET("/api/v1/session");
    if (response.status === 401) {
      setCsrfToken("analyst", undefined);
      return null;
    }
    if (!data) throw new Error(`session: ${response.status}`);
    setCsrfToken("analyst", data.csrf_token);
    return data;
  },
  staleTime: 60_000,
});

/** The Portal session of a Requester, or `null` when signed out. */
export const portalSessionQuery = queryOptions({
  queryKey: ["session", "portal"],
  queryFn: async (): Promise<PortalSession | null> => {
    const { data, response } = await api.GET("/api/v1/portal/session");
    if (response.status === 401) {
      setCsrfToken("portal", undefined);
      return null;
    }
    if (!data) throw new Error(`portal session: ${response.status}`);
    setCsrfToken("portal", data.csrf_token);
    return data;
  },
  staleTime: 60_000,
});

/** Every Module and whether it is on (FR-GEN-005). */
export const modulesQuery = queryOptions({
  queryKey: ["modules"],
  queryFn: () => call(api.GET("/api/v1/modules")),
  staleTime: 5 * 60_000,
});

/**
 * The signed-in Analyst. Only used below the Analyst layout, which redirects to sign-in
 * before rendering, so the session is always there.
 */
export function useSession(): Session {
  const { data } = useQuery(analystSessionQuery);
  if (!data) throw new Error("useSession outside the signed-in Analyst area");
  return data;
}

/**
 * Whether the session holds a Capability (Administrators hold all). Hiding what someone
 * cannot do only tidies the screen; the API checks every request.
 */
export function can(session: Session, capability: string): boolean {
  return session.is_admin || session.capabilities.includes(capability);
}

/** `can` for the signed-in Analyst. */
export function useCan(): (capability: string) => boolean {
  const session = useSession();
  return (capability) => can(session, capability);
}

/** Whether a Module is on. Unknown (still loading) counts as on, to avoid flicker. */
export function useModuleOn(): (key: string) => boolean {
  const modules = useQuery(modulesQuery);
  return (key) => {
    if (!modules.data) return true;
    return modules.data.some((m) => m.key === key && m.is_enabled);
  };
}

/** Signs out of the Analyst console and forgets every cached record. */
export function useSignOut(): () => Promise<void> {
  const queryClient = useQueryClient();
  const router = useRouter();
  return async () => {
    await api.DELETE("/api/v1/session").catch(() => undefined);
    setCsrfToken("analyst", undefined);
    queryClient.clear();
    await router.navigate({ to: "/sign-in" });
  };
}
