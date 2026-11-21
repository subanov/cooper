import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";
import { Logo } from "./Logo";

/**
 * The frame of signed-out screens (sign-in, password links, registration): one focused
 * card, nothing else to distract (Aesthetic and minimalist design).
 */
export function PublicLayout({
  title,
  intro,
  children,
  footer,
  area = "analyst",
}: {
  title: string;
  intro?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  area?: "analyst" | "portal" | undefined;
}) {
  const t = useText();
  return (
    <div className="flex min-h-dvh flex-col items-center bg-canvas px-4 py-10 sm:py-16">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-md focus:bg-surface focus:px-3 focus:py-2 focus:shadow-overlay"
      >
        {t("app.skipToContent")}
      </a>
      <header className="mb-6 flex items-center gap-2.5">
        <Logo />
        <span className="grid leading-tight">
          <span className="text-base font-semibold">{t("app.name")}</span>
          <span className="text-xs text-fg-muted">
            {area === "portal" ? t("portal.name") : t("public.tagline")}
          </span>
        </span>
      </header>
      <main
        id="main"
        className="w-full max-w-md rounded-lg border border-line bg-surface p-6 shadow-raised sm:p-8"
      >
        <h1 tabIndex={-1} data-page-heading className="text-lg font-semibold">
          {title}
        </h1>
        {intro ? <p className="mt-1 text-sm text-fg-muted">{intro}</p> : null}
        <div className="mt-6 grid gap-5">{children}</div>
      </main>
      {footer ? <footer className="mt-6 grid gap-2 text-center text-sm">{footer}</footer> : null}
    </div>
  );
}
