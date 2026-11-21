import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export type Tone = "neutral" | "muted" | "accent" | "success" | "warning" | "danger" | "info";

const TONES: Record<Tone, string> = {
  neutral: "bg-sunken text-fg border-line",
  muted: "bg-transparent text-fg-muted border-line",
  accent: "bg-accent-soft text-accent-text border-transparent",
  success: "bg-success-soft text-success-text border-transparent",
  warning: "bg-warning-soft text-warning-text border-transparent",
  danger: "bg-danger-soft text-danger-text border-transparent",
  info: "bg-info-soft text-info-text border-transparent",
};

/**
 * A short status label. Meaning is always in the text (and often an icon), never in the
 * colour alone (WCAG 1.4.1).
 */
export function Badge({
  tone = "neutral",
  icon,
  children,
  className,
}: {
  tone?: Tone | undefined;
  icon?: ReactNode | undefined;
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-6 max-w-full items-center gap-1 whitespace-nowrap rounded-full border px-2 text-xs font-medium [&_svg]:size-3.5 [&_svg]:shrink-0",
        TONES[tone],
        className,
      )}
    >
      {icon}
      <span className="truncate">{children}</span>
    </span>
  );
}

/** A name with the colour an Administrator gave it (statuses, categories): dot plus text. */
export function ColourLabel({
  colour,
  children,
  className,
}: {
  colour?: string | null | undefined;
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5", className)}>
      <span
        aria-hidden="true"
        className="size-2.5 shrink-0 rounded-full border border-line-strong"
        style={colour ? { backgroundColor: colour, borderColor: colour } : undefined}
      />
      <span className="truncate">{children}</span>
    </span>
  );
}
