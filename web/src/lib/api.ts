import createClient, { type Middleware } from "openapi-fetch";
import type { components, paths } from "@/api/schema";

/** Request and response bodies from the generated OpenAPI types (never edited by hand). */
export type Schemas = components["schemas"];

/** The two signed-in areas. Each has its own session cookie and CSRF token. */
export type Area = "analyst" | "portal";

/**
 * CSRF tokens from the session endpoints, held in memory only. Session cookies are
 * HttpOnly; nothing secret is ever written to localStorage (ADR-0010).
 */
const csrf: Record<Area, string | undefined> = { analyst: undefined, portal: undefined };

/** Remembers (or forgets) the CSRF token of a session. */
export function setCsrfToken(area: Area, token: string | undefined): void {
  csrf[area] = token;
}

const UNSAFE = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/** The area a request belongs to: Portal endpoints live under `/api/v1/portal`. */
export function areaOf(url: string): Area {
  return new URL(url, "http://localhost").pathname.startsWith("/api/v1/portal")
    ? "portal"
    : "analyst";
}

/** Adds the session's CSRF token to every unsafe request (AGENTS.md §6). */
export const csrfMiddleware: Middleware = {
  onRequest({ request }) {
    if (UNSAFE.has(request.method)) {
      const token = csrf[areaOf(request.url)];
      if (token) {
        request.headers.set("X-CSRF-Token", token);
      }
    }
    return request;
  },
};

/** The typed `/api/v1` client: same origin, session cookie included. */
export const api = createClient<paths>({
  baseUrl: baseUrl(),
  credentials: "same-origin",
  // Looked up per request, so tests can replace fetch.
  fetch: (request) => globalThis.fetch(request),
});
api.use(csrfMiddleware);

/** Sends a multipart upload (attachments) with the CSRF token of its area. */
export async function upload(path: string, file: File): Promise<Response> {
  const body = new FormData();
  body.append("file", file);
  const headers = new Headers();
  const token = csrf[areaOf(path)];
  if (token) {
    headers.set("X-CSRF-Token", token);
  }
  return globalThis.fetch(
    new Request(new URL(path, baseUrl()), {
      method: "POST",
      body,
      headers,
      credentials: "same-origin",
    }),
  );
}

function baseUrl(): string {
  return typeof window === "undefined" ? "http://localhost" : window.location.origin;
}
