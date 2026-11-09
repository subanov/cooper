import { queryOptions, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { api, type Schemas } from "@/lib/api";

/** How often an open ticket tells the server the Analyst is still there (FR-TKT-025). */
export const HEARTBEAT_MS = 10_000;

/** Other Analysts who have the ticket open. Refreshed by heartbeats and Live Updates. */
export function presenceQuery(id: string) {
  return queryOptions({
    queryKey: ["tickets", "presence", id],
    queryFn: async (): Promise<Schemas["ViewerDto"][]> => {
      const { data } = await api.GET("/api/v1/tickets/{id}/presence", {
        params: { path: { id } },
      });
      return data ?? [];
    },
    // Viewers who close their tab drop out after the server's stale window.
    refetchInterval: 15_000,
  });
}

/**
 * Announces that the signed-in Analyst has ticket `id` open (and whether they are
 * writing), and returns the other Analysts who do. Presence warns; it never locks.
 */
export function useTicketPresence(id: string, composing: boolean): Schemas["ViewerDto"][] {
  const queryClient = useQueryClient();
  useEffect(() => {
    let stopped = false;
    const beat = async () => {
      try {
        const { data } = await api.PUT("/api/v1/tickets/{id}/presence", {
          params: { path: { id } },
          body: { composing },
        });
        if (!stopped && data) {
          queryClient.setQueryData(presenceQuery(id).queryKey, data);
        }
      } catch {
        // Offline: the next heartbeat tries again.
      }
    };
    void beat();
    const timer = setInterval(() => void beat(), HEARTBEAT_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [id, composing, queryClient]);
  useEffect(
    () => () => {
      // Leaving is only an optimisation: the heartbeat goes stale on its own.
      api.DELETE("/api/v1/tickets/{id}/presence", { params: { path: { id } } }).catch(() => {});
    },
    [id],
  );
  return useQuery(presenceQuery(id)).data ?? [];
}
