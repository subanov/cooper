import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/** A native select with the input styles: keyboard and screen reader support for free. */
export function Select({ className, ...props }: ComponentProps<"select">) {
  return (
    <select
      className={cn(
        "flex h-9 w-full rounded-md border border-input bg-background px-2 py-1 text-sm",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}
