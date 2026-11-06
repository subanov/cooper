import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { IntlProvider } from "@/i18n/IntlProvider";
import type { makeRouter } from "./router";

/** Providers shared by every screen: messages, server state and routing. */
export function App({
  router,
  queryClient,
}: {
  router: ReturnType<typeof makeRouter>;
  queryClient: QueryClient;
}) {
  return (
    <IntlProvider>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </IntlProvider>
  );
}
