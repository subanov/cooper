import type { ReactNode } from "react";
import { IntlProvider as ReactIntlProvider } from "react-intl";
import { defaultLocale, en } from "./messages";

/** The user's locale for dates and numbers; text comes from the English catalogue. */
function userLocale(): string {
  if (typeof navigator === "undefined") return defaultLocale;
  try {
    return Intl.getCanonicalLocales(navigator.language)[0] ?? defaultLocale;
  } catch {
    return defaultLocale;
  }
}

/**
 * Provides the message catalogue (FR-GEN-009). Text is English until other catalogues
 * exist; dates and numbers follow the browser's locale and time zone (H2).
 */
export function IntlProvider({ children, locale }: { children: ReactNode; locale?: string }) {
  return (
    <ReactIntlProvider
      locale={locale ?? userLocale()}
      defaultLocale={defaultLocale}
      messages={en}
      onError={(error) => {
        // Only the English catalogue exists, so other locales "miss" every message.
        if (error.code !== "MISSING_TRANSLATION") throw error;
      }}
    >
      {children}
    </ReactIntlProvider>
  );
}
