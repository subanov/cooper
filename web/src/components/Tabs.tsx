import { Link, type LinkProps } from "@tanstack/react-router";
import { type KeyboardEvent, type ReactNode, useId, useRef, useState } from "react";
import { cn } from "@/lib/cn";

export interface TabLink {
  to: LinkProps["to"];
  label: string;
  search?: LinkProps["search"];
  params?: LinkProps["params"];
  exact?: boolean;
}

/** Sections of a screen as links (each has a URL, so Back and sharing work: H3). */
export function TabNav({
  label,
  items,
  className,
}: {
  label: string;
  items: readonly TabLink[];
  className?: string;
}) {
  return (
    <nav aria-label={label} className={cn("-mb-px overflow-x-auto", className)}>
      <ul className="flex min-w-max gap-1">
        {items.map((item) => (
          <li key={String(item.to)}>
            <Link
              to={item.to}
              {...(item.params ? { params: item.params } : {})}
              {...(item.search ? { search: item.search } : {})}
              activeOptions={{ exact: item.exact ?? false, includeSearch: false }}
              activeProps={{ "aria-current": "page" }}
              className="inline-flex h-10 items-center border-b-2 border-transparent px-3 text-sm font-medium text-fg-muted hover:text-fg aria-[current=page]:border-accent aria-[current=page]:text-fg"
            >
              {item.label}
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  );
}

/** One choice out of a few, shown at once (radio group with arrow keys). */
export function Segmented<T extends string>({
  label,
  value,
  onChange,
  options,
  className,
  showLabel,
}: {
  label: string;
  value: T;
  onChange: (value: T) => void;
  options: readonly { value: T; label: string }[];
  className?: string | undefined;
  /** Shows the label above the choices (in forms). */
  showLabel?: boolean | undefined;
}) {
  const name = useId();
  const group = (
    <div
      role="radiogroup"
      aria-label={showLabel ? undefined : label}
      aria-labelledby={showLabel ? `${name}-label` : undefined}
      className={cn("inline-flex w-fit rounded-md border border-line bg-sunken p-0.5", className)}
    >
      {options.map((o) => (
        <label
          key={o.value}
          className="relative inline-flex h-7 cursor-pointer items-center rounded px-2.5 text-[0.8125rem] font-medium text-fg-muted has-[:checked]:bg-surface has-[:checked]:text-fg has-[:checked]:shadow-raised has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-ring"
        >
          <input
            type="radio"
            name={name}
            value={o.value}
            checked={value === o.value}
            onChange={() => onChange(o.value)}
            className="sr-only"
          />
          {o.label}
        </label>
      ))}
    </div>
  );
  if (!showLabel) return group;
  return (
    <div className="grid gap-1.5">
      <span id={`${name}-label`} className="text-[0.8125rem] font-medium">
        {label}
      </span>
      {group}
    </div>
  );
}

/** Tabs inside a record (ARIA tabs: arrow keys move, Home and End jump). */
export function Tabs({
  label,
  tabs,
  value,
  onChange,
}: {
  label: string;
  tabs: readonly { id: string; label: ReactNode; content: ReactNode }[];
  value?: string | undefined;
  onChange?: ((id: string) => void) | undefined;
}) {
  const base = useId();
  const [own, setOwn] = useState(tabs[0]?.id ?? "");
  const current = value ?? own;
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});
  const select = (id: string) => {
    setOwn(id);
    onChange?.(id);
  };
  function onKey(e: KeyboardEvent) {
    const index = tabs.findIndex((t) => t.id === current);
    let next = -1;
    if (e.key === "ArrowRight") next = (index + 1) % tabs.length;
    if (e.key === "ArrowLeft") next = (index - 1 + tabs.length) % tabs.length;
    if (e.key === "Home") next = 0;
    if (e.key === "End") next = tabs.length - 1;
    const tab = tabs[next];
    if (tab) {
      e.preventDefault();
      select(tab.id);
      refs.current[tab.id]?.focus();
    }
  }
  return (
    <div className="grid gap-4">
      <div
        role="tablist"
        aria-label={label}
        onKeyDown={onKey}
        className="flex gap-1 overflow-x-auto border-b border-line"
      >
        {tabs.map((tab) => (
          <button
            key={tab.id}
            ref={(el) => {
              refs.current[tab.id] = el;
            }}
            type="button"
            role="tab"
            id={`${base}-tab-${tab.id}`}
            aria-selected={tab.id === current}
            aria-controls={`${base}-panel-${tab.id}`}
            tabIndex={tab.id === current ? 0 : -1}
            onClick={() => select(tab.id)}
            className="-mb-px inline-flex h-10 shrink-0 items-center gap-1.5 border-b-2 border-transparent px-3 text-sm font-medium text-fg-muted hover:text-fg aria-selected:border-accent aria-selected:text-fg"
          >
            {tab.label}
          </button>
        ))}
      </div>
      {tabs.map((tab) =>
        tab.id === current ? (
          <div
            key={tab.id}
            role="tabpanel"
            id={`${base}-panel-${tab.id}`}
            aria-labelledby={`${base}-tab-${tab.id}`}
            className="min-w-0"
          >
            {tab.content}
          </div>
        ) : null,
      )}
    </div>
  );
}
