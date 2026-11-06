import { createMemoryHistory } from "@tanstack/react-router";
import { render } from "@testing-library/react";
import axe from "axe-core";
import { App } from "@/App";
import { makeQueryClient, makeRouter } from "@/router";

/** Renders the whole app at `path` with fresh router and query state. */
export function renderApp(path: string) {
  const router = makeRouter({ history: createMemoryHistory({ initialEntries: [path] }) });
  const queryClient = makeQueryClient();
  queryClient.setDefaultOptions({ queries: { retry: false } });
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

/** Answers fetch calls by path. */
export function mockFetch(routes: Record<string, { status: number; body?: unknown }>) {
  vi.stubGlobal("fetch", async (input: Request | string) => {
    const url = new URL(typeof input === "string" ? input : input.url, "http://localhost");
    const route = routes[url.pathname];
    if (!route) {
      return new Response(null, { status: 404 });
    }
    return new Response(route.body === undefined ? null : JSON.stringify(route.body), {
      status: route.status,
      headers: { "content-type": "application/json" },
    });
  });
}
