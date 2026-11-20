import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export interface Fact {
  term: ReactNode;
  value: ReactNode;
  key?: string;
}

/** Label and value pairs (a description list), for the facts of a record. */
export function Facts({
  items,
  className,
  layout = "stacked",
}: {
  items: readonly (Fact | null | false)[];
  className?: string | undefined;
  /** `stacked`: term above value. `inline`: term beside value. */
  layout?: "stacked" | "inline" | undefined;
}) {
  const shown = items.filter((i): i is Fact => Boolean(i));
  return (
    <dl
      className={cn(
        layout === "inline"
          ? "grid grid-cols-[minmax(7rem,max-content)_1fr] gap-x-4 gap-y-2"
          : "grid gap-3",
        className,
      )}
    >
      {shown.map((item, index) =>
        layout === "inline" ? (
          <div key={item.key ?? index} className="contents">
            <dt className="text-[0.8125rem] text-fg-muted">{item.term}</dt>
            <dd className="min-w-0 text-[0.8125rem] [overflow-wrap:anywhere]">{item.value}</dd>
          </div>
        ) : (
          <div key={item.key ?? index} className="grid gap-0.5">
            <dt className="text-xs font-medium text-fg-muted">{item.term}</dt>
            <dd className="min-w-0 text-sm [overflow-wrap:anywhere]">{item.value}</dd>
          </div>
        ),
      )}
    </dl>
  );
}

/** A keyboard key, for shortcut hints. */
export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded-sm border border-line bg-sunken px-1 font-mono text-[0.6875rem] text-fg-muted">
      {children}
    </kbd>
  );
}
