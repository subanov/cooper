import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { ToastProvider } from "@/components/Toast";
import { IntlProvider } from "@/i18n/IntlProvider";
import type { makeRouter } from "./router";

/** Providers every screen shares: messages, server state, toasts and routing. */
export function App({
  router,
  queryClient,
  locale,
}: {
  router: ReturnType<typeof makeRouter>;
  queryClient: QueryClient;
  locale?: string;
}) {
  return (
    <IntlProvider {...(locale ? { locale } : {})}>
      <QueryClientProvider client={queryClient}>
        <ToastProvider>
          <RouterProvider router={router} />
        </ToastProvider>
      </QueryClientProvider>
    </IntlProvider>
  );
}
