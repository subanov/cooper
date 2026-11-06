import { queryOptions } from "@tanstack/react-query";
import { api, type Schemas, setCsrfToken } from "@/lib/api";

/** The Analyst session, or `null` when not signed in. */
export const analystSessionQuery = queryOptions({
  queryKey: ["session", "analyst"],
  queryFn: async (): Promise<Schemas["SessionDto"] | null> => {
    const { data, response } = await api.GET("/api/v1/session");
    if (response.status === 401) {
      setCsrfToken("analyst", undefined);
      return null;
    }
    if (!data) {
      throw new Error(`session request failed: ${response.status}`);
    }
    setCsrfToken("analyst", data.csrf_token);
    return data;
  },
});

/** The Portal session of a Requester, or `null` when not signed in. */
export const portalSessionQuery = queryOptions({
  queryKey: ["session", "portal"],
  queryFn: async (): Promise<Schemas["PortalSessionDto"] | null> => {
    const { data, response } = await api.GET("/api/v1/portal/session");
    if (response.status === 401) {
      setCsrfToken("portal", undefined);
      return null;
    }
    if (!data) {
      throw new Error(`Portal session request failed: ${response.status}`);
    }
    setCsrfToken("portal", data.csrf_token);
    return data;
  },
});
