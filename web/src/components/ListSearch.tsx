import { Search } from "lucide-react";
import { type Ref, useEffect, useState } from "react";
import { cn } from "@/lib/cn";
import { Input, type Option, Select } from "./Field";

/** A search box for a list: Enter applies it (kept in the URL by the caller). */
export function SearchBox({
  label,
  placeholder,
  value,
  onSubmit,
  inputRef,
  shortcut,
  className,
  id,
}: {
  label: string;
  placeholder?: string | undefined;
  value: string | undefined;
  onSubmit: (q: string | undefined) => void;
  inputRef?: Ref<HTMLInputElement> | undefined;
  shortcut?: string | undefined;
  className?: string | undefined;
  id?: string | undefined;
}) {
  const [q, setQ] = useState(value ?? "");
  useEffect(() => setQ(value ?? ""), [value]);
  return (
    <search className={cn("min-w-48 flex-1", className)}>
      <form
        className="relative"
        onSubmit={(e) => {
          e.preventDefault();
          onSubmit(q.trim() || undefined);
        }}
      >
        <Search
          aria-hidden="true"
          className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-fg-muted"
        />
        <Input
          ref={inputRef}
          id={id}
          type="search"
          aria-label={label}
          aria-keyshortcuts={shortcut}
          placeholder={placeholder}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          className="pl-8"
        />
      </form>
    </search>
  );
}

/** A compact filter: its first choice means "no filter" and names what it filters. */
export function FilterSelect({
  label,
  allLabel,
  value,
  options,
  onChange,
  className,
}: {
  label: string;
  allLabel: string;
  value: string | undefined;
  options: readonly Option[];
  onChange: (value: string | undefined) => void;
  className?: string | undefined;
}) {
  return (
    <Select
      aria-label={label}
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value || undefined)}
      className={cn(
        "h-8 w-auto max-w-[12rem] text-[0.8125rem]",
        value ? "border-accent text-accent-text" : undefined,
        className,
      )}
    >
      <option value="">{allLabel}</option>
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </Select>
  );
}
