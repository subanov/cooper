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

/** One choice of a select. */
export interface Option {
  value: string;
  label: string;
}

/** What a labelled field shows around its control. */
export interface FieldShellProps {
  label: ReactNode;
  hint?: ReactNode | undefined;
  /** Shown under the control and announced with it (H9: next to the field). */
  error?: string | undefined;
  required?: boolean | undefined;
  className?: string | undefined;
  id?: string | undefined;
  /** Keeps the label for screen readers only, when a heading already names the field. */
  labelHidden?: boolean | undefined;
}

/** Props a control gets from its field: its id and descriptions. */
export interface ControlProps {
  id: string;
  "aria-describedby": string | undefined;
  "aria-invalid": true | undefined;
  required: boolean | undefined;
}

const control =
  "w-full rounded-md border border-control bg-surface px-2.5 text-sm text-fg placeholder:text-fg-muted transition-colors hover:border-fg-muted disabled:cursor-not-allowed disabled:bg-sunken disabled:opacity-70 aria-invalid:border-danger-text";

/** Label, hint and error around a control. The control gets ids that tie them together. */
export function Field({
  label,
  hint,
  error,
  required,
  className,
  id: given,
  labelHidden,
  children,
}: FieldShellProps & { children: (control: ControlProps) => ReactNode }) {
  const t = useText();
  const auto = useId();
  const id = given ?? auto;
  const hintId = hint ? `${id}-hint` : undefined;
  const errorId = error ? `${id}-error` : undefined;
  const described = [errorId, hintId].filter(Boolean).join(" ") || undefined;
  return (
    <div className={cn("grid content-start gap-1.5", className)}>
      <label
        htmlFor={id}
        className={cn("text-[0.8125rem] font-medium text-fg", labelHidden && "sr-only")}
      >
        {label}
        {required ? (
          <span className="ml-1 font-normal text-fg-muted">({t("form.required")})</span>
        ) : null}
      </label>
      {children({
        id,
        "aria-describedby": described,
        "aria-invalid": error ? true : undefined,
        required,
      })}
      {error ? (
        <p id={errorId} className="text-xs font-medium text-danger-text">
          {error}
        </p>
      ) : null}
      {hint ? (
        <p id={hintId} className="text-xs text-fg-muted">
          {hint}
        </p>
      ) : null}
    </div>
  );
}

export function Input({
  className,
  ref,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { ref?: Ref<HTMLInputElement> | undefined }) {
  return <input ref={ref} className={cn(control, "h-9", className)} {...props} />;
}

export function Textarea({
  className,
  ref,
  ...props
}: TextareaHTMLAttributes<HTMLTextAreaElement> & { ref?: Ref<HTMLTextAreaElement> | undefined }) {
  return (
    <textarea
      ref={ref}
      className={cn(control, "min-h-20 py-2 leading-relaxed", className)}
      {...props}
    />
  );
}

export function Select({
  className,
  ref,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement> & { ref?: Ref<HTMLSelectElement> | undefined }) {
  return <select ref={ref} className={cn(control, "h-9 pr-8", className)} {...props} />;
}

type InputProps = Omit<InputHTMLAttributes<HTMLInputElement>, "id"> & {
  ref?: Ref<HTMLInputElement> | undefined;
};

/** A labelled text input. */
export function TextField({
  label,
  hint,
  error,
  required,
  className,
  id,
  labelHidden,
  ...input
}: FieldShellProps & InputProps) {
  return (
    <Field
      label={label}
      hint={hint}
      error={error}
      required={required}
      className={className}
      id={id}
      labelHidden={labelHidden}
    >
      {(c) => <Input {...c} {...input} />}
    </Field>
  );
}

/** A labelled multi-line input. */
export function TextAreaField({
  label,
  hint,
  error,
  required,
  className,
  id,
  labelHidden,
  ...input
}: FieldShellProps & Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "id">) {
  return (
    <Field
      label={label}
      hint={hint}
      error={error}
      required={required}
      className={className}
      id={id}
      labelHidden={labelHidden}
    >
      {(c) => <Textarea {...c} {...input} />}
    </Field>
  );
}

/** A labelled select. `placeholder` adds an empty first choice ("None", "Choose…"). */
export function SelectField({
  label,
  hint,
  error,
  required,
  className,
  id,
  options,
  placeholder,
  ...select
}: FieldShellProps &
  Omit<SelectHTMLAttributes<HTMLSelectElement>, "id"> & {
    options: readonly Option[];
    placeholder?: string | undefined;
  }) {
  return (
    <Field
      label={label}
      hint={hint}
      error={error}
      required={required}
      className={className}
      id={id}
    >
      {(c) => (
        <Select {...c} {...select}>
          {placeholder !== undefined ? <option value="">{placeholder}</option> : null}
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </Select>
      )}
    </Field>
  );
}

/** A checkbox with its label to the right (a big hit target: Fitts's Law). */
export function Checkbox({
  label,
  hint,
  className,
  ...input
}: Omit<InputHTMLAttributes<HTMLInputElement>, "type"> & {
  label: ReactNode;
  hint?: ReactNode | undefined;
}) {
  const id = useId();
  return (
    <div className={cn("flex items-start gap-2.5", className)}>
      <input
        id={id}
        type="checkbox"
        aria-describedby={hint ? `${id}-hint` : undefined}
        className="mt-0.5 size-4 shrink-0 cursor-pointer accent-accent"
        {...input}
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

/** An on/off switch that acts at once (settings, Modules). */
export function Switch({
  label,
  checked,
  onCheckedChange,
  disabled,
  describedBy,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean | undefined;
  describedBy?: string | undefined;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      aria-describedby={describedBy}
      disabled={disabled}
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "relative inline-flex h-6 w-11 shrink-0 items-center rounded-full border transition-colors disabled:cursor-not-allowed disabled:opacity-50",
        checked ? "border-accent bg-accent" : "border-control bg-sunken",
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          "inline-block size-4.5 rounded-full shadow-raised transition-transform",
          checked ? "translate-x-5.5 bg-accent-fg" : "translate-x-0.5 bg-fg-muted",
        )}
      />
    </button>
  );
}

/** Related fields under a legend (chunking: Miller's Law). */
export function Fieldset({
  legend,
  description,
  children,
  className,
}: {
  legend: ReactNode;
  description?: ReactNode | undefined;
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <fieldset className={cn("grid min-w-0 gap-4", className)}>
      <legend className="mb-1 text-[0.9375rem] font-semibold">{legend}</legend>
      {description ? <p className="-mt-3 text-xs text-fg-muted">{description}</p> : null}
      {children}
    </fieldset>
  );
}

/** Two columns of fields on wide screens, one on narrow ones. */
export function FormGrid({
  children,
  className,
  columns = 2,
}: {
  children: ReactNode;
  className?: string | undefined;
  columns?: 2 | 3 | undefined;
}) {
  return (
    <div
      className={cn("grid gap-4", columns === 3 ? "sm:grid-cols-3" : "sm:grid-cols-2", className)}
    >
      {children}
    </div>
  );
}
