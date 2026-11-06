import createClient, { type Middleware } from "openapi-fetch";
import type { components, paths } from "@/api/schema";

/** Response and request bodies from the OpenAPI description. */
export type Schemas = components["schemas"];

/**
 * CSRF tokens returned by the session endpoints, kept in memory only (never in
 * localStorage; ADR-0010). The Analyst console and the Portal have separate sessions.
 */
const csrf: { analyst: string | undefined; portal: string | undefined } = {
  analyst: undefined,
  portal: undefined,
};

/** Remembers the CSRF token of the Analyst or Portal session. */
export function setCsrfToken(area: "analyst" | "portal", token: string | undefined): void {
  csrf[area] = token;
}

const UNSAFE = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/** Adds the session's CSRF token to unsafe requests (AGENTS.md §6). */
export const csrfMiddleware: Middleware = {
  onRequest({ request }) {
    if (!UNSAFE.has(request.method)) {
      return request;
    }
    const area = new URL(request.url).pathname.startsWith("/api/v1/portal") ? "portal" : "analyst";
    const token = csrf[area];
    if (token) {
      request.headers.set("X-CSRF-Token", token);
    }
    return request;
  },
};

/** The typed `/api/v1` client. Same origin, cookies included. */
export const api = createClient<paths>({
  baseUrl: typeof window === "undefined" ? "http://localhost" : window.location.origin,
  credentials: "same-origin",
  // Look fetch up per request, so tests (and polyfills) can replace it.
  fetch: (request) => globalThis.fetch(request),
});
api.use(csrfMiddleware);
