import { cn } from "@/lib/cn";

/** A small busy indicator. Decorative: pair it with text or `aria-busy`. */
export function Spinner({ className }: { className?: string | undefined }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "inline-block size-4 shrink-0 animate-spin rounded-full border-2 border-current border-r-transparent",
        className,
      )}
    />
  );
}

/** A grey placeholder block shown while content loads. */
export function Skeleton({ className }: { className?: string | undefined }) {
  return (
    <span
      aria-hidden="true"
      className={cn("block animate-pulse rounded-md bg-sunken", className)}
    />
  );
}
