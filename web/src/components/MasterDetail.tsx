import { useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { useHotkeys } from "@/lib/hooks";
import { type TabLink, TabNav } from "./Tabs";

/**
 * Master-detail: the list on the left and the open record on the right, as in an inbox
 * (Jakob's Law). On narrow screens only one of them shows, with "Back to the list" in the
 * detail. Filters and selection live in the URL, so a round trip keeps the user's place.
 */
export function SplitView({
  list,
  children,
  detailOpen,
  placeholder,
  listWidth = "md",
}: {
  list: ReactNode;
  /** The detail (usually the router Outlet). */
  children: ReactNode;
  detailOpen: boolean;
  /** What the detail area says when nothing is open (wide screens only). */
  placeholder?: ReactNode | undefined;
  listWidth?: "md" | "lg" | undefined;
}) {
  return (
    <div className="flex h-full min-h-0">
      <div
        className={cn(
          "min-h-0 w-full flex-col border-line bg-surface xl:flex xl:shrink-0 xl:border-r",
          listWidth === "lg" ? "xl:w-[34rem]" : "xl:w-[26rem]",
          detailOpen ? "hidden" : "flex",
        )}
      >
        {list}
      </div>
      <div
        className={cn(
          "min-h-0 min-w-0 flex-1",
          detailOpen ? "block overflow-y-auto" : "hidden xl:grid xl:place-items-center",
        )}
      >
        {detailOpen ? children : placeholder}
      </div>
    </div>
  );
}

/** Whether a child of `base` (a record or a New form) is open. */
export function useDetailOpen(base: string): boolean {
  return useRouterState({
    select: (s) => {
      const path = s.location.pathname.replace(/\/+$/, "") || "/";
      return path !== base && path.startsWith(`${base}/`);
    },
  });
}

/** The list column: title and actions, filters, the scrolling list and its footer. */
export function ListPane({
  title,
  actions,
  filters,
  status,
  children,
  label,
  level = 1,
}: {
  title: ReactNode;
  actions?: ReactNode | undefined;
  filters?: ReactNode | undefined;
  /** A line under the filters: counts, selection bar. */
  status?: ReactNode | undefined;
  children: ReactNode;
  label?: string | undefined;
  /** 2 inside a section that already has the page's h1. */
  level?: 1 | 2 | undefined;
}) {
  const Heading = level === 1 ? "h1" : "h2";
  return (
    <section aria-label={label} className="flex min-h-0 flex-1 flex-col">
      <div className="grid gap-3 border-b border-line px-4 pb-3 pt-4">
        <div className="flex items-center justify-between gap-2">
          <Heading
            tabIndex={-1}
            data-page-heading
            className={
              level === 1 ? "text-lg font-semibold tracking-tight" : "text-base font-semibold"
            }
          >
            {title}
          </Heading>
          {actions ? <div className="flex items-center gap-1.5">{actions}</div> : null}
        </div>
        {filters}
      </div>
      {status}
      <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
    </section>
  );
}

/** Classes for the link that makes a whole list row clickable (stretched link). */
export const rowLinkClass =
  "font-medium text-fg after:absolute after:inset-0 hover:underline [overflow-wrap:anywhere]";

/** A selectable row of a record list. The open one is marked (aria-current on its link). */
export function RecordRow({
  selected,
  leading,
  children,
  className,
}: {
  selected?: boolean | undefined;
  /** A control before the content (a checkbox); it sits above the stretched link. */
  leading?: ReactNode | undefined;
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <li
      className={cn(
        "relative flex gap-3 border-b border-line px-4 py-3 hover:bg-sunken/60",
        selected && "bg-accent-soft hover:bg-accent-soft",
        className,
      )}
    >
      {selected ? (
        <span aria-hidden="true" className="absolute inset-y-0 left-0 w-1 bg-accent" />
      ) : null}
      {leading ? <div className="relative z-10 shrink-0 pt-0.5">{leading}</div> : null}
      <div className="grid min-w-0 flex-1 gap-1">{children}</div>
    </li>
  );
}

/** `j` and `k` open the next and previous record of a list (keyboard-first triage). */
export function useListKeys(
  ids: readonly string[],
  openId: string | undefined,
  open: (id: string) => void,
) {
  function step(delta: number) {
    if (ids.length === 0) return;
    const index = openId ? ids.indexOf(openId) : -1;
    const next = ids[Math.min(Math.max(index + delta, 0), ids.length - 1)];
    if (next && next !== openId) open(next);
  }
  useHotkeys({ j: () => step(1), k: () => step(-1) });
}

/** The id of the open record (`$id` or `$ticketId` of the deepest match). */
export function useOpenId(): string | undefined {
  return useRouterState({
    select: (s) => {
      const leaf = s.matches[s.matches.length - 1];
      const params = (leaf?.params ?? {}) as Record<string, string | undefined>;
      return params.ticketId ?? params.id;
    },
  });
}

/** The heading block of a record in the detail pane: who/what at a glance, then actions. */
export function DetailHeader({
  eyebrow,
  title,
  badges,
  actions,
  back,
  actionsLabel,
}: {
  eyebrow?: ReactNode | undefined;
  title: ReactNode;
  badges?: ReactNode | undefined;
  actions?: ReactNode | undefined;
  back?: ReactNode | undefined;
  actionsLabel?: string | undefined;
}) {
  return (
    <header className="grid gap-3">
      {back ? <div className="xl:hidden">{back}</div> : null}
      {eyebrow ? (
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[0.8125rem] text-fg-muted">
          {eyebrow}
        </div>
      ) : null}
      <h2
        tabIndex={-1}
        data-page-heading
        className="text-xl font-semibold leading-snug tracking-tight [overflow-wrap:anywhere]"
      >
        {title}
      </h2>
      {badges ? <div className="flex flex-wrap items-center gap-1.5">{badges}</div> : null}
      {actions ? (
        <div role="toolbar" aria-label={actionsLabel} className="flex flex-wrap items-center gap-2">
          {actions}
        </div>
      ) : null}
    </header>
  );
}

/** Shown in the detail area of a split view when nothing is open. */
export function PickOne({
  icon,
  title,
  children,
}: {
  icon: ReactNode;
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="grid justify-items-center gap-2 p-8 text-center text-fg-muted">
      <span
        aria-hidden="true"
        className="grid size-12 place-items-center rounded-full bg-sunken [&_svg]:size-6"
      >
        {icon}
      </span>
      <p className="text-[0.9375rem] font-semibold text-fg">{title}</p>
      {children ? <p className="max-w-xs text-sm">{children}</p> : null}
    </div>
  );
}

/**
 * A section with its own tabs (Service Status, Morning Checks, Users): the heading and tabs
 * stay put, the content below scrolls or holds a split view.
 */
export function SectionFrame({
  title,
  description,
  tabsLabel,
  tabs,
  actions,
  children,
}: {
  title: ReactNode;
  description?: ReactNode | undefined;
  tabsLabel: string;
  tabs: readonly TabLink[];
  actions?: ReactNode | undefined;
  children: ReactNode;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="border-b border-line bg-surface px-4 pt-4 sm:px-6">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="grid gap-0.5">
            <h1 tabIndex={-1} data-page-heading className="text-xl font-semibold tracking-tight">
              {title}
            </h1>
            {description ? <p className="text-sm text-fg-muted">{description}</p> : null}
          </div>
          {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
        </div>
        <TabNav label={tabsLabel} items={tabs} className="mt-2" />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
    </div>
  );
}
