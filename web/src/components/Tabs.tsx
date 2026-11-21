import { Link, type LinkProps } from "@tanstack/react-router";
import { type ReactNode, useId } from "react";
import { cn } from "@/lib/cn";

export interface NavTab {
  to: LinkProps["to"];
  label: string;
  /** Shown after the label, for example a count. */
  badge?: ReactNode;
  search?: LinkProps["search"];
}

/**
 * Section navigation between sibling screens, as links (so Back, new tabs and sharing
 * work, H3). The current one carries `aria-current="page"`.
 */
export function NavTabs({
  label,
  items,
  className,
}: {
  label: string;
  items: readonly NavTab[];
  className?: string | undefined;
}) {
  return (
    <nav aria-label={label} className={cn("-mb-px overflow-x-auto", className)}>
      <ul className="flex min-w-max gap-1 border-b border-line">
        {items.map((item) => (
          <li key={String(item.to)}>
            <Link
              to={item.to}
              search={item.search}
              activeProps={{ "aria-current": "page" }}
              className="relative inline-flex h-10 items-center gap-2 rounded-t-md px-3 text-sm font-medium text-fg-muted hover:text-fg aria-[current=page]:text-fg aria-[current=page]:after:absolute aria-[current=page]:after:inset-x-2 aria-[current=page]:after:bottom-0 aria-[current=page]:after:h-0.5 aria-[current=page]:after:rounded-full aria-[current=page]:after:bg-accent"
            >
              {item.label}
              {item.badge}
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  );
}

/**
 * A small set of exclusive options shown side by side (views, modes). Native radio
 * buttons underneath: arrow keys move, the group has a name (WCAG 4.1.2).
 */
export function Segmented<T extends string>({
  label,
  value,
  onChange,
  options,
  className,
  hideLabel = true,
}: {
  label: string;
  value: T;
  onChange: (value: T) => void;
  options: readonly { value: T; label: string }[];
  className?: string | undefined;
  hideLabel?: boolean | undefined;
}) {
  const name = useId();
  return (
    <fieldset className={cn("grid gap-1.5", className)}>
      <legend className={cn("mb-1.5 text-[0.8125rem] font-medium", hideLabel && "sr-only")}>
        {label}
      </legend>
      <div className="inline-flex w-fit flex-wrap gap-0.5 rounded-md border border-line bg-sunken p-0.5">
        {options.map((o) => (
          <label
            key={o.value}
            className="relative inline-flex h-8 cursor-pointer items-center rounded-[5px] px-3 text-[0.8125rem] font-medium text-fg-muted has-[:checked]:bg-surface has-[:checked]:text-fg has-[:checked]:shadow-raised has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-ring"
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
    </fieldset>
  );
}
