import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";

/** Page frame shared by the Analyst console and the Portal: skip link, header, main. */
export function AppShell({ title, children }: { title: string; children: ReactNode }) {
  const t = useText();
  return (
    <div className="min-h-dvh">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:rounded-md focus:bg-background focus:px-3 focus:py-2"
      >
        {t("app.skipToContent")}
      </a>
      <header className="border-b border-border">
        <div className="mx-auto flex max-w-6xl items-center gap-3 px-6 py-4">
          <img src="/favicon.svg" alt="" className="size-7" />
          <span className="font-semibold">{t("app.name")}</span>
          <span className="text-muted-foreground">{title}</span>
        </div>
      </header>
      <main id="main" className="mx-auto max-w-6xl px-6 py-8">
        {children}
      </main>
    </div>
  );
}
