import { useId, useState } from "react";
import { useText } from "@/i18n/useText";
import { Input } from "./Field";

export interface CheckboxItem {
  id: string;
  label: string;
  description?: string | undefined;
}

/**
 * Many-of-many selection (Roles, Capabilities, members, Ticket Types). Long lists get a
 * filter box, and the count of chosen items stays visible (H6).
 */
export function CheckboxList({
  legend,
  description,
  items,
  selected,
  onChange,
  disabled,
  columns = 1,
}: {
  legend: string;
  description?: string | undefined;
  items: readonly CheckboxItem[];
  selected: readonly string[];
  onChange: (selected: string[]) => void;
  disabled?: boolean | undefined;
  columns?: 1 | 2 | undefined;
}) {
  const t = useText();
  const id = useId();
  const [filter, setFilter] = useState("");
  const chosen = new Set(selected);
  const q = filter.trim().toLowerCase();
  const shown = q
    ? items.filter((i) => `${i.label} ${i.description ?? ""}`.toLowerCase().includes(q))
    : items;
  function toggle(itemId: string, on: boolean) {
    const next = new Set(chosen);
    if (on) next.add(itemId);
    else next.delete(itemId);
    onChange(items.map((i) => i.id).filter((i) => next.has(i)));
  }
  return (
    <fieldset className="grid min-w-0 gap-2" disabled={disabled}>
      <legend className="mb-1 flex w-full items-baseline justify-between gap-2 text-[0.8125rem] font-medium">
        <span>{legend}</span>
        <span className="text-xs font-normal text-fg-muted">
          {t("checkboxList.count", { selected: chosen.size, total: items.length })}
        </span>
      </legend>
      {description ? <p className="-mt-1 text-xs text-fg-muted">{description}</p> : null}
      {items.length > 8 ? (
        <Input
          type="search"
          aria-label={t("checkboxList.filter", { legend })}
          placeholder={t("checkboxList.filterPlaceholder")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          className="h-8"
        />
      ) : null}
      <ul
        className={
          columns === 2
            ? "grid max-h-72 gap-x-4 gap-y-1 overflow-y-auto rounded-md border border-line p-2 sm:grid-cols-2"
            : "grid max-h-72 gap-1 overflow-y-auto rounded-md border border-line p-2"
        }
      >
        {shown.map((item) => (
          <li key={item.id} className="flex items-start gap-2 rounded-sm px-1 py-1 hover:bg-canvas">
            <input
              type="checkbox"
              id={`${id}-${item.id}`}
              checked={chosen.has(item.id)}
              onChange={(e) => toggle(item.id, e.target.checked)}
              className="mt-0.5 size-4 shrink-0 cursor-pointer accent-accent"
            />
            <label
              htmlFor={`${id}-${item.id}`}
              className="grid cursor-pointer gap-0.5 text-sm leading-5"
            >
              <span className="[overflow-wrap:anywhere]">{item.label}</span>
              {item.description ? (
                <span className="text-xs text-fg-muted">{item.description}</span>
              ) : null}
            </label>
          </li>
        ))}
        {shown.length === 0 ? (
          <li className="px-1 py-1 text-sm text-fg-muted">{t("picker.noMatches")}</li>
        ) : null}
      </ul>
    </fieldset>
  );
}
