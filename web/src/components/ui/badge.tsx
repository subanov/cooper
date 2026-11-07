import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex items-center gap-1 whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium",
  {
    variants: {
      tone: {
        neutral: "border-border bg-muted text-foreground",
        ok: "border-success/40 bg-success/10 text-success",
        warning: "border-warning/50 bg-warning/10 text-warning",
        breached: "border-destructive/50 bg-destructive/10 text-destructive",
        muted: "border-border text-muted-foreground",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

/** A small label, for example a Status or an SLA state. Never colour alone: always text. */
export function Badge({
  className,
  tone,
  ...props
}: ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}
