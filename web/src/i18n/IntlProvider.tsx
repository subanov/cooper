import type { ReactNode } from "react";
import { IntlProvider as ReactIntlProvider } from "react-intl";
import { defaultLocale, en } from "./messages";

/** Provides the message catalogue to every screen. */
export function IntlProvider({ children }: { children: ReactNode }) {
  return (
    <ReactIntlProvider locale={defaultLocale} defaultLocale={defaultLocale} messages={en}>
      {children}
    </ReactIntlProvider>
  );
}
