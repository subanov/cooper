import { createMemoryHistory } from "@tanstack/react-router";
import { render } from "@testing-library/react";
import axe from "axe-core";
import type { ReactNode } from "react";
import { App } from "@/App";
import { ToastProvider } from "@/components/Toast";
import { IntlProvider } from "@/i18n/IntlProvider";
import { makeQueryClient, makeRouter } from "@/router";

/** Renders the whole app at `path` with a fresh router and query cache. */
export function renderApp(path: string) {
  const queryClient = makeQueryClient();
  queryClient.setDefaultOptions({ queries: { retry: false, refetchOnWindowFocus: false } });
  const router = makeRouter(queryClient, {
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const view = render(<App router={router} queryClient={queryClient} locale="en" />);
  return { ...view, router, queryClient };
}

/** Renders a design-system component with the providers it may need. */
export function renderUi(ui: ReactNode) {
  return render(
    <IntlProvider locale="en">
      <ToastProvider>{ui}</ToastProvider>
    </IntlProvider>,
  );
}

/** WCAG 2.1 AA violations in `node` (axe-core). Colour contrast is checked in a browser. */
export async function axeViolations(node: Element): Promise<string[]> {
  const result = await axe.run(node, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
    rules: { "color-contrast": { enabled: false } },
  });
  return result.violations.map(
    (v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(" ")).join(", ")})`,
  );
}

type Reply = { status: number; body?: unknown; headers?: Record<string, string> };
type Route = Reply | ((request: Request, body: unknown) => Reply);

/**
 * Answers fetch calls by `"METHOD /path"` or `"/path"` (any method); anything else is a
 * 404. Returns the calls made, with their JSON bodies, for assertions.
 */
export function mockApi(routes: Record<string, Route>) {
  const calls: { method: string; path: string; search: string; body: unknown; headers: Headers }[] =
    [];
  vi.stubGlobal("fetch", async (input: Request | string) => {
    const request =
      typeof input === "string" ? new Request(new URL(input, "http://localhost")) : input;
    const url = new URL(request.url);
    const text = request.method === "GET" ? "" : await request.clone().text();
    let body: unknown;
    try {
      body = text ? JSON.parse(text) : undefined;
    } catch {
      body = text;
    }
    calls.push({
      method: request.method,
      path: url.pathname,
      search: url.search,
      body,
      headers: request.headers,
    });
    const route = routes[`${request.method} ${url.pathname}`] ?? routes[url.pathname];
    const reply = typeof route === "function" ? route(request, body) : route;
    if (!reply) return new Response(null, { status: 404 });
    const problem = reply.status >= 400 && reply.body !== undefined;
    return new Response(reply.body === undefined ? null : JSON.stringify(reply.body), {
      status: reply.status,
      headers: {
        "content-type": problem ? "application/problem+json" : "application/json",
        ...reply.headers,
      },
    });
  });
  return calls;
}
