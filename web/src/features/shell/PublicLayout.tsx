import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";
import { Logo } from "./Logo";

/**
 * The frame of signed-out screens (sign-in, password links, registration, setup): one
 * focused form and nothing else to distract (aesthetic and minimalist design). Wide screens
 * show what the area is for beside it, so people know they are in the right place.
 */
export function PublicLayout({
  title,
  intro,
  children,
  footer,
  area = "analyst",
}: {
  title: string;
  intro?: ReactNode | undefined;
  children: ReactNode;
  footer?: ReactNode | undefined;
  area?: "analyst" | "portal" | undefined;
}) {
  const t = useText();
  return (
    <div className="grid min-h-dvh lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:z-50 focus:rounded-md focus:bg-surface focus:px-3 focus:py-2 focus:shadow-overlay"
      >
        {t("app.skipToContent")}
      </a>
      <aside className="hidden flex-col justify-between bg-nav p-10 text-nav-fg lg:flex">
        <div className="flex items-center gap-3">
          <Logo className="size-9" />
          <span className="text-lg font-semibold">{t("app.name")}</span>
        </div>
        <div className="grid max-w-sm gap-3">
          <p className="text-2xl font-semibold leading-snug">
            {area === "portal" ? t("public.portalHeadline") : t("public.headline")}
          </p>
          <p className="text-sm text-nav-muted">
            {area === "portal" ? t("public.portalBody") : t("public.body")}
          </p>
        </div>
        <p className="text-xs text-nav-muted">
          {area === "portal" ? t("portal.name") : t("public.tagline")}
        </p>
      </aside>
      <div className="flex flex-col items-center justify-center bg-canvas px-4 py-10">
        <div className="mb-6 flex items-center gap-2.5 lg:hidden">
          <Logo />
          <span className="grid leading-tight">
            <span className="text-base font-semibold">{t("app.name")}</span>
            <span className="text-xs text-fg-muted">
              {area === "portal" ? t("portal.name") : t("public.tagline")}
            </span>
          </span>
        </div>
        <main
          id="main"
          className="w-full max-w-md rounded-lg border border-line bg-surface p-6 shadow-raised sm:p-8"
        >
          <h1 tabIndex={-1} data-page-heading className="text-xl font-semibold tracking-tight">
            {title}
          </h1>
          {intro ? <p className="mt-1.5 text-sm text-fg-muted">{intro}</p> : null}
          <div className="mt-6 grid gap-5">{children}</div>
        </main>
        {footer ? <footer className="mt-6 grid gap-2 text-center text-sm">{footer}</footer> : null}
      </div>
    </div>
  );
}
