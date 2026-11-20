import { Check, ChevronsUpDown, X } from "lucide-react";
import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { useDebounced } from "@/lib/hooks";
import { controlClass, Field, type FieldShellProps } from "./Field";
import { Spinner } from "./Spinner";

export interface PickerOption {
  id: string;
  label: string;
  /** A second line that tells similar options apart (email, Company, Ticket Number). */
  description?: string | undefined;
}

/**
 * Search-as-you-type selection from a long list (Requesters, tickets, Tasks), so nobody
 * has to remember IDs (H6, Hick's Law). Follows the ARIA 1.2 combobox pattern: arrow
 * keys move, Enter picks, Escape closes.
 */
export function Combobox({
  label,
  hint,
  error,
  required,
  labelHidden,
  className,
  value,
  onChange,
  options: given,
  loading = false,
  onQueryChange,
  placeholder,
  disabled,
  clearable = true,
}: Omit<FieldShellProps, "id"> & {
  value: PickerOption | null;
  onChange: (option: PickerOption | null) => void;
  /** The options to offer. Without `onQueryChange` they are filtered here as the user types. */
  options: readonly PickerOption[];
  loading?: boolean | undefined;
  /** Called (debounced) with the typed text, for options searched on the server. */
  onQueryChange?: ((query: string) => void) | undefined;
  placeholder?: string | undefined;
  disabled?: boolean | undefined;
  clearable?: boolean | undefined;
}) {
  const t = useText();
  const listId = useId();
  const [text, setText] = useState(value?.label ?? "");
  const [open, setOpen] = useState(false);
  const [activeRaw, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const query = useDebounced(open ? text.trim() : "", 200);
  const searching = open && value?.label !== text;
  const effective = searching ? query : "";
  useEffect(() => {
    onQueryChange?.(effective);
  }, [effective, onQueryChange]);
  const options = onQueryChange ? given : filterOptions(given, effective);
  const active = Math.min(activeRaw, Math.max(options.length - 1, 0));

  useEffect(() => {
    setText(value?.label ?? "");
  }, [value?.label]);

  function pick(option: PickerOption) {
    onChange(option);
    setText(option.label);
    setOpen(false);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setOpen(true);
      setActive((i) => Math.min(i + 1, Math.max(options.length - 1, 0)));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((i) => Math.max(i - 1, 0));
    } else if (event.key === "Enter" && open) {
      const option = options[active];
      if (option) {
        event.preventDefault();
        pick(option);
      }
    } else if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      setText(value?.label ?? "");
    }
  }

  const expanded = open && (options.length > 0 || loading || effective !== "");
  const activeId = expanded && options[active] ? `${listId}-${active}` : undefined;

  return (
    <Field {...{ label, hint, error, required, labelHidden, className }}>
      {(control) => (
        <div className="relative">
          <input
            {...control}
            ref={input}
            type="text"
            role="combobox"
            autoComplete="off"
            aria-autocomplete="list"
            aria-expanded={expanded}
            aria-controls={listId}
            aria-activedescendant={activeId}
            disabled={disabled}
            placeholder={placeholder ?? t("picker.placeholder")}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setOpen(true);
            }}
            onFocus={() => setOpen(true)}
            onClick={() => setOpen(true)}
            onBlur={() => {
              // Let a click on an option land first.
              setTimeout(() => {
                setOpen(false);
                if (!text.trim() && value && clearable) onChange(null);
                else setText(value?.label ?? "");
              }, 150);
            }}
            onKeyDown={onKeyDown}
            className={cn(controlClass, "h-9 pr-16")}
          />
          <span className="absolute inset-y-0 right-1 flex items-center gap-0.5">
            {loading ? <Spinner className="mr-1 text-fg-muted" /> : null}
            {value && clearable && !disabled ? (
              <button
                type="button"
                aria-label={t("picker.clear", { label: value.label })}
                className="grid size-7 place-items-center rounded-sm text-fg-muted hover:bg-sunken"
                onClick={() => {
                  onChange(null);
                  setText("");
                  input.current?.focus();
                }}
              >
                <X aria-hidden="true" className="size-3.5" />
              </button>
            ) : (
              <ChevronsUpDown aria-hidden="true" className="mr-2 size-3.5 text-fg-muted" />
            )}
          </span>
          <div
            id={listId}
            role="listbox"
            aria-label={typeof label === "string" ? label : undefined}
            hidden={!expanded}
            className="absolute inset-x-0 top-full z-30 mt-1 max-h-72 overflow-y-auto rounded-md border border-line bg-surface p-1 shadow-overlay"
          >
            {options.map((option, index) => (
              // The input keeps focus and handles the keyboard (aria-activedescendant).
              // biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/useFocusableInteractive: ARIA combobox pattern
              <div
                key={option.id}
                id={`${listId}-${index}`}
                role="option"
                aria-selected={value?.id === option.id}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => pick(option)}
                onMouseEnter={() => setActive(index)}
                className={cn(
                  "flex cursor-pointer items-start gap-2 rounded-sm px-2 py-1.5",
                  index === active && "bg-sunken",
                )}
              >
                <Check
                  aria-hidden="true"
                  className={cn(
                    "mt-0.5 size-4 shrink-0",
                    value?.id === option.id ? "text-accent-text" : "invisible",
                  )}
                />
                <span className="grid min-w-0">
                  <span className="truncate">{option.label}</span>
                  {option.description ? (
                    <span className="truncate text-xs text-fg-muted">{option.description}</span>
                  ) : null}
                </span>
              </div>
            ))}
            {!loading && options.length === 0 && effective !== "" ? (
              <div className="px-2 py-1.5 text-fg-muted">{t("picker.noMatches")}</div>
            ) : null}
          </div>
        </div>
      )}
    </Field>
  );
}

/** Filters options by the typed text, case-insensitively (Postel's Law). */
function filterOptions(all: readonly PickerOption[], query: string): PickerOption[] {
  const q = query.toLowerCase();
  return (
    q
      ? all.filter(
          (o) => o.label.toLowerCase().includes(q) || o.description?.toLowerCase().includes(q),
        )
      : all
  ).slice(0, 50);
}
