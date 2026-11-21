import { Loader2, X } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { Field, type FieldShellProps } from "./Field";

/** One choice of a search-as-you-type picker. */
export interface PickerOption {
  id: string;
  label: string;
  description?: string | undefined;
}

export interface ComboboxProps extends Omit<FieldShellProps, "id"> {
  value: PickerOption | null;
  onChange: (option: PickerOption | null) => void;
  /**
   * Choices for the current text. With `onQueryChange` the parent fetches them as the user
   * types; without it, the picker filters this (short) list itself.
   */
  options: readonly PickerOption[];
  onQueryChange?: ((query: string) => void) | undefined;
  loading?: boolean | undefined;
  placeholder?: string | undefined;
  disabled?: boolean | undefined;
}

/**
 * A searchable picker for long lists (Hick's Law): type, then choose with the arrow keys
 * and Enter, or the pointer. ARIA 1.2 combobox pattern; Escape closes the list.
 */
export function Combobox({
  label,
  hint,
  error,
  required,
  className,
  value,
  onChange,
  options,
  onQueryChange,
  loading,
  placeholder,
  disabled,
}: ComboboxProps) {
  const t = useText();
  const listId = useId();
  const [text, setText] = useState(value?.label ?? "");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const shownValue = useRef(value?.id);

  useEffect(() => {
    if (shownValue.current !== value?.id) {
      shownValue.current = value?.id;
      setText(value?.label ?? "");
    }
  }, [value]);

  function choose(option: PickerOption) {
    shownValue.current = option.id;
    onChange(option);
    setText(option.label);
    setOpen(false);
  }

  const needle = text.trim().toLowerCase();
  const shown =
    onQueryChange || !needle || (value && text === value.label)
      ? options
      : options.filter((o) => `${o.label} ${o.description ?? ""}`.toLowerCase().includes(needle));
  const report = (q: string) => onQueryChange?.(q);
  const expanded = open && (shown.length > 0 || (text.trim() !== "" && !loading));
  const activeOption = expanded ? shown[active] : undefined;

  return (
    <Field label={label} hint={hint} error={error} required={required} className={className}>
      {(c) => (
        <div className="relative">
          <input
            {...c}
            ref={input}
            type="text"
            role="combobox"
            aria-autocomplete="list"
            aria-expanded={expanded}
            aria-controls={listId}
            aria-activedescendant={activeOption ? `${listId}-${activeOption.id}` : undefined}
            autoComplete="off"
            disabled={disabled}
            placeholder={placeholder ?? t("picker.placeholder")}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setActive(0);
              setOpen(true);
              report(e.target.value.trim());
              if (value && e.target.value !== value.label) {
                shownValue.current = undefined;
                onChange(null);
              }
            }}
            onFocus={() => {
              setOpen(true);
              report(value ? "" : text.trim());
            }}
            onBlur={() => setTimeout(() => setOpen(false), 120)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setOpen(true);
                setActive((i) => Math.min(i + 1, Math.max(shown.length - 1, 0)));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((i) => Math.max(i - 1, 0));
              } else if (e.key === "Enter" && expanded && activeOption) {
                e.preventDefault();
                choose(activeOption);
              } else if (e.key === "Escape" && open) {
                e.preventDefault();
                e.stopPropagation();
                setOpen(false);
              }
            }}
            className={cn(
              "h-9 w-full rounded-md border border-control bg-surface pl-2.5 pr-16 text-sm text-fg placeholder:text-fg-muted hover:border-fg-muted disabled:cursor-not-allowed disabled:bg-sunken aria-invalid:border-danger-text",
            )}
          />
          <div className="absolute inset-y-0 right-1 flex items-center gap-0.5">
            {loading ? (
              <Loader2 aria-hidden="true" className="size-4 animate-spin text-fg-muted" />
            ) : null}
            {value && !disabled ? (
              <button
                type="button"
                aria-label={t("picker.clear", {
                  label: typeof label === "string" ? label : value.label,
                })}
                onClick={() => {
                  shownValue.current = undefined;
                  onChange(null);
                  setText("");
                  report("");
                  input.current?.focus();
                }}
                className="grid size-7 place-items-center rounded text-fg-muted hover:bg-sunken hover:text-fg"
              >
                <X aria-hidden="true" className="size-4" />
              </button>
            ) : null}
          </div>
          {/* ARIA 1.2 combobox: focus stays in the input; options are announced through
              aria-activedescendant, so they are not focusable themselves. */}
          <div
            id={listId}
            role="listbox"
            aria-label={typeof label === "string" ? label : undefined}
            className={cn(
              "absolute inset-x-0 top-full z-50 mt-1 max-h-64 overflow-y-auto rounded-md border border-line bg-raised p-1 shadow-overlay",
              expanded ? "block" : "hidden",
            )}
          >
            {shown.map((option, index) => (
              // biome-ignore lint/a11y/useFocusableInteractive: options are reached with aria-activedescendant
              // biome-ignore lint/a11y/useKeyWithClickEvents: the input handles the keys (arrows, Enter)
              <div
                key={option.id}
                id={`${listId}-${option.id}`}
                role="option"
                aria-selected={index === active}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => choose(option)}
                onMouseEnter={() => setActive(index)}
                className="grid cursor-pointer gap-0.5 rounded px-2 py-1.5 text-sm aria-selected:bg-accent-soft"
              >
                <span className="truncate font-medium">{option.label}</span>
                {option.description ? (
                  <span className="truncate text-xs text-fg-muted">{option.description}</span>
                ) : null}
              </div>
            ))}
            {shown.length === 0 && !loading ? (
              // biome-ignore lint/a11y/useFocusableInteractive: an informative, disabled option
              <div
                role="option"
                aria-disabled="true"
                aria-selected="false"
                className="px-2 py-2 text-sm text-fg-muted"
              >
                {t("picker.noMatches")}
              </div>
            ) : null}
          </div>
        </div>
      )}
    </Field>
  );
}
