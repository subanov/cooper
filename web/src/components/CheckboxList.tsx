import { useId, useState } from "react";
import { useText } from "@/i18n/useText";
import { Input } from "./Field";

export interface CheckOption {
  value: string;
  label: string;
  description?: string | undefined;
}

/**
 * Several choices out of a list (Roles, Capabilities, members), with a filter for long
 * lists and a count of what is chosen (recognition over recall: H6).
 */
export function CheckboxList({
  legend,
  options,
  value,
  onChange,
  disabled,
  columns = 1,
}: {
  legend: string;
  options: readonly CheckOption[];
  value: readonly string[];
  onChange: (value: string[]) => void;
  disabled?: boolean | undefined;
  columns?: 1 | 2 | undefined;
}) {
  const t = useText();
  const id = useId();
  const [filter, setFilter] = useState("");
  const chosen = new Set(value);
  const needle = filter.trim().toLowerCase();
  const shown = needle
    ? options.filter((o) => `${o.label} ${o.description ?? ""}`.toLowerCase().includes(needle))
    : options;
  return (
    <fieldset className="grid min-w-0 gap-2" disabled={disabled}>
      <legend className="sr-only">{legend}</legend>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-xs text-fg-muted" aria-live="polite">
          {t("checkboxList.count", { selected: value.length, total: options.length })}
        </span>
        {options.length > 8 ? (
          <Input
            type="search"
            aria-label={t("checkboxList.filter", { legend })}
            placeholder={t("checkboxList.filterPlaceholder")}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="h-8 w-48"
          />
        ) : null}
      </div>
      <ul
        className={
          columns === 2
            ? "grid max-h-80 gap-x-4 gap-y-1 overflow-y-auto rounded-md border border-line p-2 sm:grid-cols-2"
            : "grid max-h-80 gap-1 overflow-y-auto rounded-md border border-line p-2"
        }
      >
        {shown.map((o) => (
          <li key={o.value}>
            <label
              htmlFor={`${id}-${o.value}`}
              className="flex cursor-pointer items-start gap-2.5 rounded px-1.5 py-1 hover:bg-sunken"
            >
              <input
                id={`${id}-${o.value}`}
                type="checkbox"
                checked={chosen.has(o.value)}
                onChange={(e) =>
                  onChange(
                    e.target.checked ? [...value, o.value] : value.filter((v) => v !== o.value),
                  )
                }
                className="mt-0.5 size-4 shrink-0 accent-accent"
              />
              <span className="grid min-w-0">
                <span className="text-sm">{o.label}</span>
                {o.description ? (
                  <span className="text-xs text-fg-muted">{o.description}</span>
                ) : null}
              </span>
            </label>
          </li>
        ))}
      </ul>
    </fieldset>
  );
}
