import { Loader2 } from "lucide-react";
import { type ReactNode, useId } from "react";
import { cn } from "@/lib/cn";
import type { Option } from "./Field";

/**
 * The properties of a record, each editable in place (as in Jira or Linear: Jakob's Law).
 * A change saves at once and shows that it is saving (H1); the parent offers Undo.
 */
export function PropertyList({
  children,
  className,
}: {
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <dl className={cn("grid gap-x-3 gap-y-1 text-sm sm:grid-cols-[8rem_1fr]", className)}>
      {children}
    </dl>
  );
}

/** One read-only property. */
export function Property({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <div className="contents">
      <dt className="pt-1.5 text-[0.8125rem] text-fg-muted">{label}</dt>
      <dd className="min-w-0 py-1.5 [overflow-wrap:anywhere]">{children}</dd>
    </div>
  );
}

/** A property changed with a select that looks like text until hovered or focused. */
export function PropertySelect({
  label,
  value,
  options,
  placeholder,
  onChange,
  saving,
  disabled,
  groups,
}: {
  label: string;
  value: string;
  options: readonly Option[];
  /** An empty choice ("None", "Unassigned"). */
  placeholder?: string | undefined;
  onChange: (value: string) => void;
  saving?: boolean | undefined;
  disabled?: boolean | undefined;
  /** Optional option groups, shown instead of `options`. */
  groups?: readonly { label: string; options: readonly Option[] }[] | undefined;
}) {
  const id = useId();
  return (
    <div className="contents">
      <dt className="pt-1.5 text-[0.8125rem] text-fg-muted">
        <label htmlFor={id}>{label}</label>
      </dt>
      <dd className="relative min-w-0">
        <select
          id={id}
          value={value}
          disabled={disabled || saving}
          aria-busy={saving || undefined}
          onChange={(e) => onChange(e.target.value)}
          className="h-8 w-full truncate rounded-md border border-transparent bg-transparent px-2 pr-7 text-sm text-fg hover:border-control focus-visible:border-control disabled:cursor-default disabled:opacity-100 disabled:hover:border-transparent"
        >
          {placeholder !== undefined ? <option value="">{placeholder}</option> : null}
          {groups
            ? groups.map((g) => (
                <optgroup key={g.label} label={g.label}>
                  {g.options.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </optgroup>
              ))
            : options.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
        </select>
        {saving ? (
          <Loader2
            aria-hidden="true"
            className="pointer-events-none absolute right-7 top-2 size-4 animate-spin text-fg-muted"
          />
        ) : null}
      </dd>
    </div>
  );
}
