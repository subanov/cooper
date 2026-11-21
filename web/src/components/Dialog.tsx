import * as RadixDialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { Button, IconButton } from "./Button";

const SIZES = { sm: "max-w-md", md: "max-w-lg", lg: "max-w-2xl", xl: "max-w-4xl" } as const;

export interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode | undefined;
  size?: keyof typeof SIZES | undefined;
  children: ReactNode;
  footer?: ReactNode | undefined;
}

/**
 * A modal dialog: focus moves in and is trapped, Escape and the close button cancel, focus
 * returns to the opener (H3, WCAG 2.4.3). Use only for focused, short tasks.
 */
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  size = "md",
  children,
  footer,
}: DialogProps) {
  const t = useText();
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-50 bg-overlay" />
        <RadixDialog.Content
          {...(description ? {} : { "aria-describedby": undefined })}
          className={cn(
            "fixed left-1/2 top-[6vh] z-50 flex max-h-[88vh] w-[calc(100vw-1.5rem)] -translate-x-1/2 flex-col rounded-lg border border-line bg-surface shadow-overlay",
            SIZES[size],
          )}
        >
          <div className="flex items-start gap-3 border-b border-line px-5 py-4">
            <div className="grid min-w-0 flex-1 gap-1">
              <RadixDialog.Title className="text-base font-semibold leading-snug [overflow-wrap:anywhere]">
                {title}
              </RadixDialog.Title>
              {description ? (
                <RadixDialog.Description className="text-sm text-fg-muted">
                  {description}
                </RadixDialog.Description>
              ) : null}
            </div>
            <RadixDialog.Close asChild>
              <IconButton
                size="sm"
                label={t("common.close")}
                tooltip="none"
                icon={<X aria-hidden="true" />}
                className="-mr-2 -mt-1"
              />
            </RadixDialog.Close>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
          {footer ? (
            <div className="flex flex-wrap justify-end gap-2 border-t border-line px-5 py-3">
              {footer}
            </div>
          ) : null}
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

/** A dialog holding a form: Enter submits, Cancel and Escape close, input survives errors. */
export function FormDialog({
  open,
  onOpenChange,
  title,
  description,
  size,
  children,
  submitLabel,
  onSubmit,
  busy,
  submitDisabled,
  tone = "primary",
}: Omit<DialogProps, "footer"> & {
  submitLabel: string;
  onSubmit: () => void;
  busy?: boolean | undefined;
  submitDisabled?: boolean | undefined;
  tone?: "primary" | "danger" | undefined;
}) {
  const t = useText();
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-50 bg-overlay" />
        <RadixDialog.Content
          {...(description ? {} : { "aria-describedby": undefined })}
          className={cn(
            "fixed left-1/2 top-[6vh] z-50 flex max-h-[88vh] w-[calc(100vw-1.5rem)] -translate-x-1/2 flex-col rounded-lg border border-line bg-surface shadow-overlay",
            SIZES[size ?? "md"],
          )}
        >
          <form
            noValidate
            className="flex min-h-0 flex-1 flex-col"
            onSubmit={(e) => {
              e.preventDefault();
              e.stopPropagation();
              if (!busy && !submitDisabled) onSubmit();
            }}
          >
            <div className="flex items-start gap-3 border-b border-line px-5 py-4">
              <div className="grid min-w-0 flex-1 gap-1">
                <RadixDialog.Title className="text-base font-semibold leading-snug [overflow-wrap:anywhere]">
                  {title}
                </RadixDialog.Title>
                {description ? (
                  <RadixDialog.Description className="text-sm text-fg-muted">
                    {description}
                  </RadixDialog.Description>
                ) : null}
              </div>
              <RadixDialog.Close asChild>
                <IconButton
                  size="sm"
                  label={t("common.close")}
                  tooltip="none"
                  icon={<X aria-hidden="true" />}
                  className="-mr-2 -mt-1"
                />
              </RadixDialog.Close>
            </div>
            <div className="grid min-h-0 flex-1 content-start gap-4 overflow-y-auto px-5 py-4">
              {children}
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-line px-5 py-3">
              <RadixDialog.Close asChild>
                <Button variant="ghost">{t("common.cancel")}</Button>
              </RadixDialog.Close>
              <Button type="submit" variant={tone} loading={busy} disabled={submitDisabled}>
                {submitLabel}
              </Button>
            </div>
          </form>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

/**
 * Confirms a destructive, irreversible action (H5). The title names the object; reversible
 * actions use Undo instead.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  children,
  confirmLabel,
  onConfirm,
  busy,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  children?: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  busy?: boolean | undefined;
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
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button variant="danger" loading={busy} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Dialog>
  );
}
