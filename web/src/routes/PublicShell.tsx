import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";

/** Frame of the signed-out screens: a centred card with the product name. */
export function PublicShell({ title, children }: { title: string; children: ReactNode }) {
  const t = useText();
  return (
    <div className="grid min-h-dvh place-items-center bg-muted px-4 py-10">
      <main id="main" className="w-full max-w-sm">
        <div className="mb-6 flex items-center justify-center gap-2">
          <img src="/favicon.svg" alt="" className="size-8" />
          <span className="text-lg font-semibold">{t("app.name")}</span>
        </div>
        <div className="rounded-lg border border-border bg-background p-6 shadow-sm">
          <h1 className="mb-4 text-xl font-semibold">{title}</h1>
          {children}
        </div>
      </main>
    </div>
  );
}
