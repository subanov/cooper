import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import type { paths } from "@/api/schema";

/** The Server-Sent Events endpoint (ADR-0022). */
export const LIVE_PATH = "/api/v1/live" satisfies keyof paths;

const FIRST_RETRY_MS = 2_000;
const MAX_RETRY_MS = 60_000;

/**
 * Marks the cached data an event names as stale, so screens showing it refetch. Events
 * only say *what* changed, never the data itself (ADR-0010, ADR-0022).
 */
export function applyLiveEvent(queryClient: QueryClient, event: string, data: string): void {
  if (event === "resync") {
    void queryClient.invalidateQueries({ queryKey: ["tickets"] });
    return;
  }
  if (event !== "invalidate") {
    return;
  }
  let change: { topic?: unknown; id?: unknown };
  try {
    change = JSON.parse(data);
  } catch {
    return;
  }
  if (change.topic === "ticket_presence" && typeof change.id === "string") {
    void queryClient.invalidateQueries({ queryKey: ["tickets", "presence", change.id] });
    return;
  }
  if (change.topic === "ticket" && typeof change.id === "string") {
    void queryClient.invalidateQueries({ queryKey: ["tickets", "list"] });
    void queryClient.invalidateQueries({ queryKey: ["tickets", "detail", change.id] });
    void queryClient.invalidateQueries({ queryKey: ["tickets", "timeline", change.id] });
  }
}

/**
 * Keeps ticket screens current while the Analyst is signed in: subscribes to Live
 * Updates and invalidates the TanStack Query caches they name. After a reconnect it
 * refetches everything, because changes may have been missed meanwhile.
 */
export function useLiveUpdates(enabled: boolean): void {
  const queryClient = useQueryClient();
  useEffect(() => {
    if (!enabled || typeof EventSource === "undefined") {
      return;
    }
    let source: EventSource | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let readyCount = 0;
    let retryMs = FIRST_RETRY_MS;
    let stopped = false;

    const connect = () => {
      const current = new EventSource(LIVE_PATH);
      source = current;
      current.addEventListener("ready", () => {
        retryMs = FIRST_RETRY_MS;
        if (readyCount++ > 0) {
          applyLiveEvent(queryClient, "resync", "{}");
        }
      });
      for (const name of ["invalidate", "resync"]) {
        current.addEventListener(name, (e) =>
          applyLiveEvent(queryClient, name, (e as MessageEvent<string>).data),
        );
      }
      current.addEventListener("error", () => {
        // The browser retries dropped streams itself; it gives up (CLOSED) on an error
        // response such as 401 or 503, so retry those with backoff.
        if (stopped || current.readyState !== EventSource.CLOSED) {
          return;
        }
        current.close();
        timer = setTimeout(connect, retryMs);
        retryMs = Math.min(retryMs * 2, MAX_RETRY_MS);
      });
    };

    connect();
    return () => {
      stopped = true;
      clearTimeout(timer);
      source?.close();
    };
  }, [enabled, queryClient]);
}
