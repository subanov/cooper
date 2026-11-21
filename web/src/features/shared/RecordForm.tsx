import type { ReactNode } from "react";
import { Button } from "@/components/Button";
import { ProblemNotice } from "@/components/Feedback";
import { useText } from "@/i18n/useText";

/**
 * The form of a record: fields, the problem (if any), and Save at the end where the eye
 * finishes (Fitts's Law). Unsaved changes are flagged (Von Restorff). Enter submits.
 */
export function RecordForm({
  children,
  onSubmit,
  busy,
  error,
  submitLabel,
  dirty = false,
  secondary,
  disabled,
}: {
  children: ReactNode;
  onSubmit: () => void;
  busy: boolean;
  error: unknown;
  submitLabel: string;
  dirty?: boolean | undefined;
  secondary?: ReactNode | undefined;
  disabled?: boolean | undefined;
}) {
  const t = useText();
  return (
    <form
      noValidate
      className="grid gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        if (!disabled) onSubmit();
      }}
    >
      <fieldset disabled={disabled} className="grid min-w-0 gap-5">
        {children}
      </fieldset>
      <ProblemNotice error={error} />
      {disabled ? null : (
        <div className="flex flex-wrap items-center justify-end gap-2 border-t border-line pt-4">
          {dirty ? (
            <span
              role="status"
              className="mr-auto inline-flex items-center gap-1.5 text-xs font-medium text-warning-text"
            >
              <span aria-hidden="true" className="size-2 rounded-full bg-warning-text" />
              {t("common.unsaved")}
            </span>
          ) : null}
          {secondary}
          <Button type="submit" variant="primary" loading={busy}>
            {submitLabel}
          </Button>
        </div>
      )}
    </form>
  );
}

/** Tags typed as "a, b, c" (Postel's Law: spaces and empty entries are fine). */
export function parseTags(value: string): string[] {
  return [
    ...new Set(
      value
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
    ),
  ];
}
