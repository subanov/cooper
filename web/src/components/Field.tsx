import { ChevronDown } from "lucide-react";
import {
  type InputHTMLAttributes,
  type ReactNode,
  type Ref,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
  useId,
} from "react";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";

/** Classes of text inputs, selects and text areas. Borders keep 3:1 contrast (WCAG 1.4.11). */
export const controlClass =
  "w-full min-w-0 rounded-md border border-control bg-surface px-2.5 text-sm text-fg shadow-[inset_0_1px_1px_rgb(0_0_0/0.04)] placeholder:text-fg-muted disabled:cursor-not-allowed disabled:bg-sunken disabled:opacity-70 aria-invalid:border-danger-text aria-invalid:ring-1 aria-invalid:ring-danger-text";

/** What a control inside a `Field` receives to stay linked to its label, hint and error. */
export interface ControlProps {
  id: string;
  "aria-describedby": string | undefined;
  "aria-invalid": true | undefined;
  required: boolean | undefined;
}

export interface FieldShellProps {
  label: ReactNode;
  /** Short guidance under the label (H10: help in context). */
  hint?: ReactNode;
  /** A field error from the API or form feedback, shown next to the field (H9). */
  error?: string | undefined;
  required?: boolean | undefined;
  /** Hides the label visually; it stays the accessible name. */
  labelHidden?: boolean | undefined;
  className?: string | undefined;
  id?: string | undefined;
}

/** Label, control, hint and error, linked for assistive technology. */
export function Field({
  label,
  hint,
  error,
  required,
  labelHidden,
  className,
  id: givenId,
  children,
}: FieldShellProps & { children: (control: ControlProps) => ReactNode }) {
  const t = useText();
  const autoId = useId();
  const id = givenId ?? autoId;
  const hintId = hint ? `${id}-hint` : undefined;
  const errorId = error ? `${id}-error` : undefined;
  const describedBy = [errorId, hintId].filter(Boolean).join(" ") || undefined;
  return (
    <div className={cn("grid content-start gap-1.5", className)}>
      <label
        htmlFor={id}
        className={cn("text-[0.8125rem] font-medium text-fg", labelHidden && "sr-only")}
      >
        {label}
        {required ? (
          <span className="ml-0.5 text-danger-text" aria-hidden="true">
            *
          </span>
        ) : null}
        {required ? <span className="sr-only"> ({t("form.required")})</span> : null}
      </label>
      {children({
        id,
        "aria-describedby": describedBy,
        "aria-invalid": error ? true : undefined,
        required,
      })}
      {hint ? (
        <p id={hintId} className="text-xs text-fg-muted">
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className="text-xs font-medium text-danger-text">
          {error}
        </p>
      ) : null}
    </div>
  );
}

type InputProps = InputHTMLAttributes<HTMLInputElement> & { ref?: Ref<HTMLInputElement> };

/** A bare text input. Prefer `TextField`, which adds the label. */
export function Input({ className, ...props }: InputProps) {
  return <input className={cn(controlClass, "h-9 read-only:bg-sunken", className)} {...props} />;
}

type TextareaProps = TextareaHTMLAttributes<HTMLTextAreaElement> & {
  ref?: Ref<HTMLTextAreaElement>;
};

/** A bare text area. */
export function Textarea({ className, ...props }: TextareaProps) {
  return (
    <textarea
      className={cn(controlClass, "min-h-24 py-2 leading-relaxed read-only:bg-sunken", className)}
      {...props}
    />
  );
}

type SelectProps = SelectHTMLAttributes<HTMLSelectElement> & { ref?: Ref<HTMLSelectElement> };

const WIDTH = /^(?:[a-z]+:)?(?:w-|min-w-|max-w-|flex-1|hidden$|block$)/;

/**
 * A bare native select: constrained input, keyboard and screen reader friendly (H5).
 * Styled the same in every browser, with its own chevron.
 */
export function Select({ className, ...props }: SelectProps) {
  const classes = (className ?? "").split(/\s+/).filter(Boolean);
  const outer = classes.filter((c) => WIDTH.test(c));
  const inner = classes.filter((c) => !WIDTH.test(c));
  return (
    <span className={cn("relative block", outer.length ? outer : "w-full")}>
      <select
        className={cn(controlClass, "h-9 w-full cursor-pointer appearance-none pr-8", inner)}
        {...props}
      />
      <ChevronDown
        aria-hidden="true"
        className="pointer-events-none absolute right-2.5 top-1/2 size-4 -translate-y-1/2 text-fg-muted"
      />
    </span>
  );
}

type Shell = Omit<FieldShellProps, "id">;

/** A labelled text input. */
export function TextField({
  label,
  hint,
  error,
  labelHidden,
  className,
  required,
  ...props
}: Shell & Omit<InputProps, "className">) {
  return (
    <Field {...{ label, hint, error, labelHidden, className, required }} id={props.id}>
      {(control) => <Input {...props} {...control} />}
    </Field>
  );
}

/** A labelled text area. */
export function TextAreaField({
  label,
  hint,
  error,
  labelHidden,
  className,
  required,
  ...props
}: Shell & Omit<TextareaProps, "className">) {
  return (
    <Field {...{ label, hint, error, labelHidden, className, required }} id={props.id}>
      {(control) => <Textarea {...props} {...control} />}
    </Field>
  );
}

export interface Option {
  value: string;
  label: string;
  disabled?: boolean | undefined;
}

/** A labelled select. `placeholder` adds an empty first option (for "none" or "choose"). */
export function SelectField({
  label,
  hint,
  error,
  labelHidden,
  className,
  required,
  options,
  placeholder,
  ...props
}: Shell &
  Omit<SelectProps, "className" | "children"> & {
    options: readonly Option[];
    placeholder?: string | undefined;
  }) {
  return (
    <Field {...{ label, hint, error, labelHidden, className, required }} id={props.id}>
      {(control) => (
        <Select {...props} {...control}>
          {placeholder !== undefined ? <option value="">{placeholder}</option> : null}
          {options.map((o) => (
            <option key={o.value} value={o.value} disabled={o.disabled}>
              {o.label}
            </option>
          ))}
        </Select>
      )}
    </Field>
  );
}

/** A checkbox with its label on the right and an optional hint. */
export function Checkbox({
  label,
  hint,
  className,
  ...props
}: Omit<InputProps, "type"> & { label: ReactNode; hint?: ReactNode }) {
  const autoId = useId();
  const id = props.id ?? autoId;
  return (
    <div className={cn("flex items-start gap-2.5", className)}>
      <input
        type="checkbox"
        id={id}
        aria-describedby={hint ? `${id}-hint` : undefined}
        className="mt-0.5 size-4 shrink-0 cursor-pointer accent-accent disabled:cursor-not-allowed"
        {...props}
      />
      <div className="grid gap-0.5">
        <label htmlFor={id} className="cursor-pointer text-sm leading-5">
          {label}
        </label>
        {hint ? (
          <p id={`${id}-hint`} className="text-xs text-fg-muted">
            {hint}
          </p>
        ) : null}
      </div>
    </div>
  );
}

/** An on/off switch for settings that apply at once. State is never shown by colour alone. */
export function Switch({
  checked,
  onChange,
  label,
  id,
  describedBy,
  disabled,
  stateLabels,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** The accessible name, unless a <label htmlFor={id}> names the switch. */
  label?: string | undefined;
  id?: string | undefined;
  describedBy?: string | undefined;
  disabled?: boolean | undefined;
  /** Visible "On"/"Off" text next to the switch. */
  stateLabels?: [on: string, off: string] | undefined;
}) {
  return (
    <span className="inline-flex items-center gap-2">
      <button
        type="button"
        role="switch"
        id={id}
        aria-checked={checked}
        aria-label={label}
        aria-describedby={describedBy}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cn(
          "relative inline-flex h-6 w-10 shrink-0 cursor-pointer items-center rounded-full border transition-colors disabled:cursor-not-allowed disabled:opacity-55",
          checked ? "border-accent bg-accent" : "border-control bg-sunken",
        )}
      >
        <span
          aria-hidden="true"
          className={cn(
            "inline-block size-4 rounded-full shadow-raised transition-transform",
            checked ? "translate-x-5 bg-accent-fg" : "translate-x-1 bg-control",
          )}
        />
      </button>
      {stateLabels ? (
        <span aria-hidden="true" className="text-xs font-medium text-fg-muted">
          {checked ? stateLabels[0] : stateLabels[1]}
        </span>
      ) : null}
    </span>
  );
}

/** Groups related fields under a heading (Gestalt: common region). */
export function Fieldset({
  legend,
  description,
  children,
  className,
}: {
  legend: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <fieldset className={cn("grid min-w-0 gap-4", className)}>
      <legend className="mb-1 w-full border-b border-line pb-1.5 text-xs font-semibold uppercase tracking-wider text-fg-muted">
        {legend}
      </legend>
      {description ? <p className="-mt-2 text-xs text-fg-muted">{description}</p> : null}
      {children}
    </fieldset>
  );
}

/** A responsive grid of fields: one column on narrow screens, two (or three) on wide ones. */
export function FormGrid({
  children,
  columns = 2,
  className,
}: {
  children: ReactNode;
  columns?: 1 | 2 | 3 | undefined;
  className?: string | undefined;
}) {
  return (
    <div
      className={cn(
        "grid gap-4",
        columns === 2 && "sm:grid-cols-2",
        columns === 3 && "sm:grid-cols-2 lg:grid-cols-3",
        className,
      )}
    >
      {children}
    </div>
  );
}
