import type { ReactNode } from "react";
import { Button } from "@/components/Button";
import { ProblemNotice } from "@/components/Notice";
import { useText } from "@/i18n/useText";

/**
 * The form of a record screen: fields, the problem (if any) and Save at the end, where
 * the eye finishes (Fitts's Law). Enter submits.
 */
export function RecordForm({
  children,
  onSubmit,
  busy,
  error,
  submitLabel,
  dirty = true,
  secondary,
}: {
  children: ReactNode;
  onSubmit: () => void;
  busy: boolean;
  error: unknown;
  submitLabel: string;
  dirty?: boolean | undefined;
  secondary?: ReactNode;
}) {
  const t = useText();
  return (
    <form
      noValidate
      className="grid gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit();
      }}
    >
      {children}
      <ProblemNotice error={error} />
      <div className="flex flex-wrap items-center justify-end gap-2 border-t border-line pt-4">
        {dirty ? (
          <span className="mr-auto text-xs font-medium text-warning-text">
            {t("common.unsaved")}
          </span>
        ) : null}
        {secondary}
        <Button type="submit" variant="primary" loading={busy}>
          {submitLabel}
        </Button>
      </div>
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
