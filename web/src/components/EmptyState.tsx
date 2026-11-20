import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/**
 * What to show when a list is empty: why it is empty and the next step (H10, Paradox of
 * the Active User). Not an error.
 */
export function EmptyState({
  icon,
  title,
  children,
  action,
  className,
}: {
  icon?: ReactNode;
  title: ReactNode;
  children?: ReactNode;
  action?: ReactNode;
  className?: string | undefined;
}) {
  return (
    <div
      className={cn(
        "grid justify-items-center gap-2 px-6 py-10 text-center [&>svg]:size-8 [&>svg]:text-fg-muted",
        className,
      )}
    >
      {icon}
      <p className="text-[0.9375rem] font-semibold">{title}</p>
      {children ? <div className="max-w-md text-sm text-fg-muted">{children}</div> : null}
      {action ? <div className="pt-2">{action}</div> : null}
    </div>
  );
}
