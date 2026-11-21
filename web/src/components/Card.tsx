import { type ReactNode, useId } from "react";
import { cn } from "@/lib/cn";

/**
 * A surface that groups related content (Gestalt: common region). With a `title` it is a
 * labelled region, so screen reader users can jump to it.
 */
export function Card({
  title,
  description,
  actions,
  children,
  className,
  bodyClassName,
  headingLevel = 2,
  flush = false,
}: {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
  className?: string | undefined;
  bodyClassName?: string | undefined;
  headingLevel?: 2 | 3 | undefined;
  /** No body padding (tables and lists that run edge to edge). */
  flush?: boolean | undefined;
}) {
  const id = useId();
  const Heading = headingLevel === 2 ? "h2" : "h3";
  return (
    <section
      aria-labelledby={title ? id : undefined}
      className={cn("min-w-0 rounded-lg border border-line bg-surface shadow-raised", className)}
    >
      {title || actions ? (
        <header className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 border-b border-line px-4 py-3">
          <div className="grid min-w-0 gap-0.5">
            {title ? (
              <Heading id={id} className="text-[0.9375rem] font-semibold leading-6">
                {title}
              </Heading>
            ) : null}
            {description ? <p className="text-xs text-fg-muted">{description}</p> : null}
          </div>
          {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
        </header>
      ) : null}
      {children !== undefined ? (
        <div className={cn(flush ? "" : "p-4", bodyClassName)}>{children}</div>
      ) : null}
    </section>
  );
}
