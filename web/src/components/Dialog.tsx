import * as RadixDialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { Button } from "./Button";

/**
 * A modal dialog. Focus moves in and is trapped, Escape and Cancel close it, and focus
 * returns to the control that opened it (H3, WCAG 2.4.3).
 */
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  size = "md",
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg" | undefined;
}) {
  const t = useText();
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-40 bg-overlay" />
        <RadixDialog.Content
          {...(description ? {} : { "aria-describedby": undefined })}
          className={cn(
            "fixed left-1/2 top-[8vh] z-50 grid max-h-[84vh] w-[calc(100vw-2rem)] -translate-x-1/2 grid-rows-[auto_1fr_auto] overflow-hidden rounded-lg border border-line bg-surface shadow-overlay",
            size === "sm" && "max-w-md",
            size === "md" && "max-w-xl",
            size === "lg" && "max-w-3xl",
          )}
        >
          <div className="flex items-start justify-between gap-4 border-b border-line px-5 py-4">
            <div className="grid gap-1">
              <RadixDialog.Title className="text-base font-semibold">{title}</RadixDialog.Title>
              {description ? (
                <RadixDialog.Description className="text-sm text-fg-muted">
                  {description}
                </RadixDialog.Description>
              ) : null}
            </div>
            <RadixDialog.Close
              aria-label={t("common.close")}
              className="-mr-2 -mt-1 grid size-9 shrink-0 place-items-center rounded-md text-fg-muted hover:bg-sunken"
            >
              <X aria-hidden="true" className="size-4" />
            </RadixDialog.Close>
          </div>
          <div className="overflow-y-auto px-5 py-4">{children}</div>
          {footer ? (
            <div className="flex flex-wrap justify-end gap-2 border-t border-line bg-canvas px-5 py-3">
              {footer}
            </div>
          ) : null}
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

/**
 * Confirms a destructive, irreversible action. The title names the object; the
 * confirm button repeats the action (H5). Reversible actions use Undo instead.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  children,
  confirmLabel,
  onConfirm,
  busy = false,
  tone = "danger",
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  children?: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  busy?: boolean | undefined;
  tone?: "danger" | "primary" | undefined;
}) {
  const t = useText();
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      size="sm"
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>{t("common.cancel")}</Button>
          <Button variant={tone} loading={busy} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Dialog>
  );
}

/** A form inside a dialog: Enter submits, the footer holds Cancel and the submit button. */
export function FormDialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  submitLabel,
  onSubmit,
  busy = false,
  size,
  submitDisabled = false,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  submitLabel: string;
  onSubmit: () => void;
  busy?: boolean | undefined;
  size?: "sm" | "md" | "lg" | undefined;
  submitDisabled?: boolean | undefined;
}) {
  const t = useText();
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-40 bg-overlay" />
        <RadixDialog.Content
          {...(description ? {} : { "aria-describedby": undefined })}
          className={cn(
            "fixed left-1/2 top-[8vh] z-50 w-[calc(100vw-2rem)] -translate-x-1/2 overflow-hidden rounded-lg border border-line bg-surface shadow-overlay",
            size === "sm" && "max-w-md",
            (size ?? "md") === "md" && "max-w-xl",
            size === "lg" && "max-w-3xl",
          )}
        >
          <form
            noValidate
            className="grid max-h-[84vh] grid-rows-[auto_1fr_auto]"
            onSubmit={(event) => {
              event.preventDefault();
              onSubmit();
            }}
          >
            <div className="flex items-start justify-between gap-4 border-b border-line px-5 py-4">
              <div className="grid gap-1">
                <RadixDialog.Title className="text-base font-semibold">{title}</RadixDialog.Title>
                {description ? (
                  <RadixDialog.Description className="text-sm text-fg-muted">
                    {description}
                  </RadixDialog.Description>
                ) : null}
              </div>
              <RadixDialog.Close
                type="button"
                aria-label={t("common.close")}
                className="-mr-2 -mt-1 grid size-9 shrink-0 place-items-center rounded-md text-fg-muted hover:bg-sunken"
              >
                <X aria-hidden="true" className="size-4" />
              </RadixDialog.Close>
            </div>
            <div className="grid content-start gap-4 overflow-y-auto px-5 py-4">{children}</div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-line bg-canvas px-5 py-3">
              <Button onClick={() => onOpenChange(false)}>{t("common.cancel")}</Button>
              <Button type="submit" variant="primary" loading={busy} disabled={submitDisabled}>
                {submitLabel}
              </Button>
            </div>
          </form>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}
