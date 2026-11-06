import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

/** A bordered surface for grouped content (shadcn/ui). */
export function Card({ className, ...props }: ComponentProps<"section">) {
  return (
    <section
      className={cn("rounded-lg border border-border bg-card text-card-foreground", className)}
      {...props}
    />
  );
}

/** Card heading area. */
export function CardHeader({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("flex flex-col gap-1.5 p-6", className)} {...props} />;
}

/** Card title. */
export function CardTitle({ className, ...props }: ComponentProps<"h2">) {
  return <h2 className={cn("text-lg font-semibold leading-none", className)} {...props} />;
}

/** Card body. */
export function CardContent({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("p-6 pt-0", className)} {...props} />;
}
