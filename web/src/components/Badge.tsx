import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export type Tone = "neutral" | "accent" | "success" | "warning" | "danger" | "muted";

const TONES: Record<Tone, string> = {
  neutral: "border-line bg-surface text-fg",
  accent: "border-transparent bg-accent-soft text-accent-text",
  success: "border-transparent bg-success-soft text-success-text",
  warning: "border-transparent bg-warning-soft text-warning-text",
  danger: "border-transparent bg-danger-soft text-danger-text",
  muted: "border-transparent bg-sunken text-fg-muted",
};

/**
 * A short label for a state (status, Priority, SLA). The text always names the state;
 * tone and icon only reinforce it (never colour alone).
 */
export function Badge({
  tone = "neutral",
  icon,
  children,
  className,
  title,
}: {
  tone?: Tone | undefined;
  icon?: ReactNode;
  children: ReactNode;
  className?: string | undefined;
  title?: string | undefined;
}) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex h-6 max-w-full shrink-0 items-center gap-1 truncate rounded-full border px-2 text-xs font-medium leading-none [&_svg]:size-3.5",
        TONES[tone],
        className,
      )}
    >
      {icon}
      {children}
    </span>
  );
}

/** A round swatch of a configured colour (Check Status, category). Decorative only. */
export function ColourDot({
  colour,
  className,
}: {
  colour?: string | null | undefined;
  className?: string | undefined;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "inline-block size-2.5 shrink-0 rounded-full border border-black/15",
        className,
      )}
      ref={(node) => {
        // Set through the CSSOM: the Content-Security-Policy forbids inline style attributes.
        if (node) node.style.backgroundColor = colour ?? "transparent";
      }}
    />
  );
}

/** A label with a configured colour shown as a dot next to the name. */
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
    <span className={cn("inline-flex items-center gap-1.5", className)}>
      <ColourDot colour={colour} />
      {children}
    </span>
  );
}
