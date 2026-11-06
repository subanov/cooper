import { type ComponentProps, useId } from "react";
import { cn } from "@/lib/utils";
import { Input } from "./input";
import { Label } from "./label";

type FieldProps = ComponentProps<typeof Input> & {
  /** Visible label. */
  label: string;
  /** Help text under the input. */
  hint?: string | undefined;
  /** Error for this field, announced and linked with aria-describedby. */
  error?: string | undefined;
};

/** A labelled input with optional hint and error, wired for screen readers. */
export function Field({ label, hint, error, className, id, ...props }: FieldProps) {
  const generated = useId();
  const inputId = id ?? generated;
  const hintId = `${inputId}-hint`;
  const errorId = `${inputId}-error`;
  const describedBy = [hint ? hintId : null, error ? errorId : null].filter(Boolean).join(" ");
  return (
    <div className={cn("grid gap-1.5", className)}>
      <Label htmlFor={inputId}>{label}</Label>
      <Input
        id={inputId}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy || undefined}
        {...props}
      />
      {hint ? (
        <p id={hintId} className="text-xs text-muted-foreground">
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className="text-xs text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  );
}
