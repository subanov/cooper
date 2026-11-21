import { Link, type LinkProps } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { type ReactNode, useRef } from "react";
import { useClaimFocus } from "@/features/shell/useFocusOnNavigate";
import { cn } from "@/lib/cn";

/**
 * The top of every screen: an optional Back link, the title (`h1`, focused after
 * navigation), a one-line description and the screen's actions, with the primary action
 * last and closest to where the eye ends (Fitts's Law).
 */
export function PageHeader({
  title,
  description,
  actions,
  back,
  meta,
  className,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  back?: { to: LinkProps["to"]; label: string; search?: LinkProps["search"] } | undefined;
  meta?: ReactNode;
  className?: string | undefined;
}) {
  const heading = useRef<HTMLHeadingElement>(null);
  useClaimFocus(heading);
  return (
    <header className={cn("grid gap-2", className)}>
      {back ? (
        <Link
          to={back.to}
          search={back.search}
          className="inline-flex w-fit items-center gap-1 rounded-sm text-[0.8125rem] font-medium text-accent-text hover:underline"
        >
          <ArrowLeft aria-hidden="true" className="size-3.5" />
          {back.label}
        </Link>
      ) : null}
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
        <div className="grid min-w-0 gap-1">
          <h1
            ref={heading}
            tabIndex={-1}
            data-page-heading
            className="text-xl font-semibold leading-tight tracking-tight text-fg [overflow-wrap:anywhere]"
          >
            {title}
          </h1>
          {description ? <p className="max-w-prose text-sm text-fg-muted">{description}</p> : null}
          {meta ? <div className="flex flex-wrap items-center gap-2 pt-1">{meta}</div> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
    </header>
  );
}

/**
 * The heading of a screen inside a section with tabs (the section owns the `h1`). It is
 * focused after navigation like a page title.
 */
export function SectionHeader({
  title,
  description,
  actions,
  back,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  back?: { to: LinkProps["to"]; label: string; search?: LinkProps["search"] } | undefined;
}) {
  const heading = useRef<HTMLHeadingElement>(null);
  useClaimFocus(heading);
  return (
    <header className="grid gap-2">
      {back ? (
        <Link
          to={back.to}
          search={back.search}
          className="inline-flex w-fit items-center gap-1 rounded-sm text-[0.8125rem] font-medium text-accent-text hover:underline"
        >
          <ArrowLeft aria-hidden="true" className="size-3.5" />
          {back.label}
        </Link>
      ) : null}
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
        <div className="grid min-w-0 gap-1">
          <h2
            ref={heading}
            tabIndex={-1}
            data-page-heading
            className="text-lg font-semibold [overflow-wrap:anywhere]"
          >
            {title}
          </h2>
          {description ? <p className="max-w-prose text-sm text-fg-muted">{description}</p> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
    </header>
  );
}

/** The content column of a screen. `width` keeps forms readable on wide screens. */
export function Page({
  children,
  width = "wide",
  className,
}: {
  children: ReactNode;
  width?: "narrow" | "medium" | "wide" | "full" | undefined;
  className?: string | undefined;
}) {
  return (
    <div
      className={cn(
        "mx-auto grid w-full content-start gap-6 px-4 py-6 sm:px-6",
        width === "narrow" && "max-w-2xl",
        width === "medium" && "max-w-4xl",
        width === "wide" && "max-w-7xl",
        className,
      )}
    >
      {children}
    </div>
  );
}

/** A row of filters above a list. Wraps on narrow screens. */
export function FilterBar({ children, label }: { children: ReactNode; label: string }) {
  return (
    <search aria-label={label} className="flex flex-wrap items-end gap-3">
      {children}
    </search>
  );
}
