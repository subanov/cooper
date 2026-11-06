import { createMemoryHistory } from "@tanstack/react-router";
import { render } from "@testing-library/react";
import axe from "axe-core";
import { App } from "@/App";
import { makeQueryClient, makeRouter } from "@/router";

/** Renders the whole app at `path` with fresh router and query state. */
export function renderApp(path: string) {
  const queryClient = makeQueryClient();
  queryClient.setDefaultOptions({ queries: { retry: false } });
  const router = makeRouter(queryClient, {
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  return render(<App router={router} queryClient={queryClient} />);
}

/** WCAG 2.1 AA violations in `node` (axe-core). */
export async function axeViolations(node: Element) {
  const result = await axe.run(node, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
    // jsdom does not compute colours; contrast is checked by Playwright in a browser.
    rules: { "color-contrast": { enabled: false } },
  });
  return result.violations.map((v) => `${v.id}: ${v.help}`);
}

type Reply = { status: number; body?: unknown; headers?: Record<string, string> };

/**
 * Answers fetch calls by `"METHOD /path"` or `"/path"` (any method). A function gets the
 * request (with its JSON body) and returns the reply, for multi-step flows.
 */
export function mockFetch(
  routes: Record<string, Reply | ((request: Request, body: unknown) => Reply)>,
) {
  const calls: { method: string; path: string; body: unknown; headers: Headers }[] = [];
  vi.stubGlobal("fetch", async (input: Request | string) => {
    const request =
      typeof input === "string" ? new Request(new URL(input, "http://localhost")) : input;
    const url = new URL(request.url);
    const text = request.method === "GET" ? "" : await request.clone().text();
    const body = text ? JSON.parse(text) : undefined;
    calls.push({ method: request.method, path: url.pathname, body, headers: request.headers });
    const route = routes[`${request.method} ${url.pathname}`] ?? routes[url.pathname];
    const reply = typeof route === "function" ? route(request, body) : route;
    if (!reply) {
      return new Response(null, { status: 404 });
    }
    const isProblem = reply.status >= 400 && reply.body !== undefined;
    return new Response(reply.body === undefined ? null : JSON.stringify(reply.body), {
      status: reply.status,
      headers: {
        "content-type": isProblem ? "application/problem+json" : "application/json",
        ...reply.headers,
      },
    });
  });
  return calls;
}
