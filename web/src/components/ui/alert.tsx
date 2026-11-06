import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

const alertVariants = cva("rounded-md border px-4 py-3 text-sm", {
  variants: {
    variant: {
      info: "border-border bg-muted text-foreground",
      error: "border-destructive/50 bg-destructive/10 text-foreground",
      success: "border-primary/40 bg-primary/10 text-foreground",
    },
  },
  defaultVariants: { variant: "info" },
});

/**
 * A message box. Errors use `role="alert"` so screen readers announce them; other
 * variants use `role="status"`.
 */
export function Alert({
  className,
  variant,
  ...props
}: ComponentProps<"div"> & VariantProps<typeof alertVariants>) {
  return (
    <div
      role={variant === "error" ? "alert" : "status"}
      className={cn(alertVariants({ variant }), className)}
      {...props}
    />
  );
}
