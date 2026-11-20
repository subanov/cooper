import { AlertTriangle, CheckCircle2, Info, XCircle } from "lucide-react";
import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { problemHeadline, problemOf } from "@/lib/problem";

type NoticeTone = "info" | "success" | "warning" | "danger";

const STYLES: Record<NoticeTone, string> = {
  info: "border-accent/30 bg-accent-soft text-fg [&>svg]:text-accent-text",
  success: "border-success-text/30 bg-success-soft text-fg [&>svg]:text-success-text",
  warning: "border-warning-text/30 bg-warning-soft text-fg [&>svg]:text-warning-text",
  danger: "border-danger-text/30 bg-danger-soft text-fg [&>svg]:text-danger-text",
};

const ICONS: Record<NoticeTone, ReactNode> = {
  info: <Info aria-hidden="true" />,
  success: <CheckCircle2 aria-hidden="true" />,
  warning: <AlertTriangle aria-hidden="true" />,
  danger: <XCircle aria-hidden="true" />,
};

/**
 * An inline message next to the content it is about (never a banner that looks like an
 * ad). Danger notices are announced at once; the others politely.
 */
export function Notice({
  tone = "info",
  title,
  children,
  action,
  className,
  live = true,
}: {
  tone?: NoticeTone | undefined;
  title?: ReactNode;
  children?: ReactNode;
  action?: ReactNode;
  className?: string | undefined;
  /** Announce to screen readers (off for static guidance). */
  live?: boolean | undefined;
}) {
  return (
    <div
      role={live ? (tone === "danger" ? "alert" : "status") : undefined}
      className={cn(
        "flex items-start gap-3 rounded-lg border px-3.5 py-3 text-sm [&>svg]:mt-0.5 [&>svg]:size-4 [&>svg]:shrink-0",
        STYLES[tone],
        className,
      )}
    >
      {ICONS[tone]}
      <div className="grid min-w-0 flex-1 gap-1">
        {title ? <p className="font-semibold">{title}</p> : null}
        {children ? <div className="text-fg [&_a]:underline">{children}</div> : null}
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  );
}

/**
 * A failed request in plain language: what happened (catalogue headline) and why (the
 * server's detail). Never a raw status code (H9).
 */
export function ProblemNotice({
  error,
  action,
  className,
}: {
  error: unknown;
  action?: ReactNode;
  className?: string | undefined;
}) {
  const t = useText();
  if (!error) return null;
  const problem = problemOf(error);
  return (
    <Notice tone="danger" title={t(problemHeadline(problem))} action={action} className={className}>
      {problem.detail ?? t("problem.tryAgain")}
    </Notice>
  );
}
