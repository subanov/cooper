import { cn } from "@/lib/cn";

/** The Cooper mark: a barrel hoop, drawn as two arcs. Decorative; the name is next to it. */
export function Logo({ className }: { className?: string | undefined }) {
  return (
    <svg viewBox="0 0 32 32" aria-hidden="true" className={cn("size-8 shrink-0", className)}>
      <rect width="32" height="32" rx="8" fill="var(--color-accent)" />
      <path
        d="M10 9.5c-2 1.8-3 4-3 6.5s1 4.7 3 6.5M22 9.5c2 1.8 3 4 3 6.5s-1 4.7-3 6.5M12 11h8M12 21h8M11 16h10"
        fill="none"
        stroke="var(--color-accent-fg)"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
    </svg>
  );
}
