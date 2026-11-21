import { Link, type LinkProps } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { type ReactNode, useEffect, useRef } from "react";
import { cn } from "@/lib/cn";

const WIDTHS = {
  narrow: "max-w-3xl",
  default: "max-w-6xl",
  wide: "max-w-[96rem]",
  full: "max-w-none",
} as const;

/** A scrolling screen with consistent padding. */
export function Page({
  children,
  width = "default",
  className,
}: {
  children: ReactNode;
  width?: keyof typeof WIDTHS | undefined;
  className?: string | undefined;
}) {
  return (
    <div
      className={cn("mx-auto grid w-full content-start gap-6 p-4 sm:p-6", WIDTHS[width], className)}
    >
      {children}
    </div>
  );
}

/** "Back to …" that keeps the list's filters (H3: the user keeps their place). */
export function BackLink({
  to,
  search,
  params,
  children,
  className,
}: {
  to: LinkProps["to"];
  search?: LinkProps["search"];
  params?: LinkProps["params"];
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <Link
      to={to}
      {...(search !== undefined ? { search } : {})}
      {...(params ? { params } : {})}
      className={cn(
        "inline-flex w-fit items-center gap-1 text-[0.8125rem] font-medium text-accent-text hover:underline",
        className,
      )}
    >
      <ArrowLeft aria-hidden="true" className="size-3.5" />
      {children}
    </Link>
  );
}

/**
 * The heading of a screen. It takes focus after navigation (WCAG 2.4.3), so keyboard and
 * screen reader users land on the content.
 */
export function PageHeader({
  title,
  description,
  actions,
  eyebrow,
  back,
  level = 1,
  className,
}: {
  title: ReactNode;
  description?: ReactNode | undefined;
  actions?: ReactNode | undefined;
  eyebrow?: ReactNode | undefined;
  back?: ReactNode | undefined;
  level?: 1 | 2 | undefined;
  className?: string | undefined;
}) {
  const Heading = level === 1 ? "h1" : "h2";
  return (
    <header className={cn("grid gap-2", className)}>
      {back}
      <div className="flex flex-wrap items-start justify-between gap-x-4 gap-y-3">
        <div className="grid min-w-0 gap-1">
          {eyebrow ? <div className="text-[0.8125rem] text-fg-muted">{eyebrow}</div> : null}
          <Heading
            tabIndex={-1}
            data-page-heading
            className="text-xl font-semibold leading-tight tracking-tight [overflow-wrap:anywhere]"
          >
            {title}
          </Heading>
          {description ? <p className="max-w-prose text-sm text-fg-muted">{description}</p> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
    </header>
  );
}

/** A titled region with a border: things that belong together (Common Region). */
export function Panel({
  title,
  description,
  actions,
  children,
  flush,
  headingLevel = 2,
  className,
  id,
}: {
  title?: ReactNode | undefined;
  description?: ReactNode | undefined;
  actions?: ReactNode | undefined;
  children: ReactNode;
  /** No padding around the content (lists, tables). */
  flush?: boolean | undefined;
  headingLevel?: 2 | 3 | undefined;
  className?: string | undefined;
  id?: string | undefined;
}) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  const headingId = id ? `${id}-title` : undefined;
  return (
    <section
      aria-labelledby={headingId}
      className={cn("min-w-0 rounded-lg border border-line bg-surface shadow-raised", className)}
    >
      {title || actions ? (
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-line px-4 py-2.5">
          <div className="grid min-w-0 gap-0.5">
            {title ? (
              <Heading id={headingId} className="text-sm font-semibold">
                {title}
              </Heading>
            ) : null}
            {description ? <p className="text-xs text-fg-muted">{description}</p> : null}
          </div>
          {actions ? <div className="flex flex-wrap items-center gap-1.5">{actions}</div> : null}
        </div>
      ) : null}
      <div className={flush ? undefined : "p-4"}>{children}</div>
    </section>
  );
}

export interface Fact {
  key: string;
  term: ReactNode;
  value: ReactNode;
}

/** Label and value pairs (a description list). */
export function Facts({
  items,
  layout = "inline",
  className,
}: {
  items: readonly (Fact | null | false | undefined)[];
  layout?: "inline" | "stacked" | "grid" | undefined;
  className?: string | undefined;
}) {
  const shown = items.filter((i): i is Fact => Boolean(i));
  return (
    <dl
      className={cn(
        layout === "inline" && "grid grid-cols-[minmax(7rem,auto)_1fr] gap-x-4 gap-y-2.5 text-sm",
        layout === "stacked" && "grid gap-3 text-sm",
        layout === "grid" && "grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2 lg:grid-cols-3",
        className,
      )}
    >
      {shown.map((item) =>
        layout === "inline" ? (
          <div key={item.key} className="contents">
            <dt className="text-fg-muted">{item.term}</dt>
            <dd className="min-w-0 [overflow-wrap:anywhere]">{item.value}</dd>
          </div>
        ) : (
          <div key={item.key} className="grid min-w-0 gap-0.5">
            <dt className="text-xs text-fg-muted">{item.term}</dt>
            <dd className="min-w-0 [overflow-wrap:anywhere]">{item.value}</dd>
          </div>
        ),
      )}
    </dl>
  );
}

/** A keyboard key. */
export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded border border-line-strong bg-sunken px-1 font-sans text-[0.6875rem] font-medium text-fg-muted">
      {children}
    </kbd>
  );
}

/** A big number with its label; the whole tile can be a link. */
export function Stat({
  label,
  value,
  tone,
  to,
  search,
}: {
  label: string;
  value: ReactNode;
  tone?: "warning" | "danger" | undefined;
  to?: LinkProps["to"] | undefined;
  search?: LinkProps["search"] | undefined;
}) {
  return (
    <div className={cn("relative grid gap-0.5 rounded-md p-2.5", to && "hover:bg-sunken")}>
      <span
        className={cn(
          "text-2xl font-semibold tabular-nums leading-tight",
          tone === "danger" && "text-danger-text",
          tone === "warning" && "text-warning-text",
        )}
      >
        {value}
      </span>
      {to ? (
        <Link
          to={to}
          {...(search !== undefined ? { search } : {})}
          className="text-xs text-fg-muted after:absolute after:inset-0 hover:underline"
        >
          {label}
        </Link>
      ) : (
        <span className="text-xs text-fg-muted">{label}</span>
      )}
    </div>
  );
}

/** Moves focus to a heading that appears after its data has loaded, if focus was lost. */
export function useFocusWhenReady<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  useEffect(() => {
    if (document.activeElement === document.body || document.activeElement === null) {
      ref.current?.focus({ preventScroll: true });
    }
  }, []);
  return ref;
}
