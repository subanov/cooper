import { type ComponentProps, useId } from "react";
import { cn } from "@/lib/utils";
import { Label } from "./label";
import { Select } from "./select";

type SelectFieldProps = ComponentProps<typeof Select> & {
  /** Visible label. */
  label: string;
  /** Error for this field. */
  error?: string | undefined;
};

/** A labelled select, the counterpart of `Field` for choices. */
export function SelectField({ label, error, className, id, children, ...props }: SelectFieldProps) {
  const generated = useId();
  const selectId = id ?? generated;
  return (
    <div className={cn("grid gap-1.5", className)}>
      <Label htmlFor={selectId}>{label}</Label>
      <Select
        id={selectId}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? `${selectId}-error` : undefined}
        {...props}
      >
        {children}
      </Select>
      {error ? (
        <p id={`${selectId}-error`} className="text-xs text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  );
}
