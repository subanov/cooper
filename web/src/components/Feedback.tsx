import type { InfiniteData, UseInfiniteQueryResult, UseQueryResult } from "@tanstack/react-query";
import { CheckCircle2, Info, Loader2, RefreshCw, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { problemHeadline, problemOf } from "@/lib/problem";
import { Button } from "./Button";

export type NoticeTone = "info" | "success" | "warning" | "danger";

const NOTICE: Record<NoticeTone, { box: string; icon: ReactNode }> = {
  info: {
    box: "border-info-text/30 bg-info-soft text-fg",
    icon: <Info className="text-info-text" />,
  },
  success: {
    box: "border-success-text/30 bg-success-soft text-fg",
    icon: <CheckCircle2 className="text-success-text" />,
  },
  warning: {
    box: "border-warning-text/30 bg-warning-soft text-fg",
    icon: <TriangleAlert className="text-warning-text" />,
  },
  danger: {
    box: "border-danger-text/40 bg-danger-soft text-fg",
    icon: <TriangleAlert className="text-danger-text" />,
  },
};

/**
 * A message inline, next to what it is about (not a banner that looks like an ad).
 * `live` announces it when it appears: "status" for info, "alert" for errors.
 */
export function Notice({
  tone = "info",
  title,
  children,
  action,
  live = true,
  className,
}: {
  tone?: NoticeTone | undefined;
  title?: ReactNode | undefined;
  children?: ReactNode;
  action?: ReactNode | undefined;
  live?: boolean | undefined;
  className?: string | undefined;
}) {
  const role = live ? (tone === "danger" ? "alert" : "status") : undefined;
  return (
    <div
      role={role}
      className={cn(
        "flex items-start gap-2.5 rounded-md border px-3.5 py-3 text-sm [&_svg]:size-4 [&_svg]:shrink-0",
        NOTICE[tone].box,
        className,
      )}
    >
      <span aria-hidden="true" className="mt-0.5">
        {NOTICE[tone].icon}
      </span>
      <div className="grid min-w-0 flex-1 gap-1 [overflow-wrap:anywhere]">
        {title ? <p className="font-semibold">{title}</p> : null}
        {children ? <div className="leading-relaxed">{children}</div> : null}
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  );
}

/**
 * An API problem in plain language (H9): what happened (catalogue headline), why (the
 * server's detail) and what to do next. Never a status code.
 */
export function ProblemNotice({
  error,
  className,
  onRetry,
}: {
  error: unknown;
  className?: string | undefined;
  onRetry?: (() => void) | undefined;
}) {
  const t = useText();
  if (!error) return null;
  const problem = problemOf(error);
  const headline = problemHeadline(problem);
  const generic = headline === "problem.server" || headline === "problem.network";
  return (
    <Notice
      tone="danger"
      title={t(headline)}
      className={className}
      action={
        onRetry ? (
          <Button size="sm" icon={<RefreshCw aria-hidden="true" />} onClick={onRetry}>
            {t("common.retry")}
          </Button>
        ) : undefined
      }
    >
      {problem.detail && !generic ? problem.detail : generic ? t("problem.tryAgain") : null}
    </Notice>
  );
}

/** What an empty list or screen means, and the next step (H10, Paradox of the Active User). */
export function EmptyState({
  icon,
  title,
  children,
  action,
  className,
}: {
  icon?: ReactNode | undefined;
  title: ReactNode;
  children?: ReactNode;
  action?: ReactNode | undefined;
  className?: string | undefined;
}) {
  return (
    <div className={cn("grid justify-items-center gap-2 px-6 py-10 text-center", className)}>
      {icon ? (
        <span
          aria-hidden="true"
          className="mb-1 grid size-11 place-items-center rounded-full bg-sunken text-fg-muted [&_svg]:size-5"
        >
          {icon}
        </span>
      ) : null}
      <p className="text-[0.9375rem] font-semibold">{title}</p>
      {children ? <div className="max-w-sm text-sm text-fg-muted">{children}</div> : null}
      {action ? <div className="mt-2">{action}</div> : null}
    </div>
  );
}

export function Spinner({
  label,
  className,
}: {
  label?: string | undefined;
  className?: string | undefined;
}) {
  const t = useText();
  return (
    <span
      role="status"
      className={cn("inline-flex items-center gap-2 text-sm text-fg-muted", className)}
    >
      <Loader2 aria-hidden="true" className="size-4 animate-spin" />
      <span className={label ? undefined : "sr-only"}>{label ?? t("common.loading")}</span>
    </span>
  );
}

export function Skeleton({ className }: { className?: string | undefined }) {
  return (
    <span aria-hidden="true" className={cn("block animate-pulse rounded bg-sunken", className)} />
  );
}

/** Placeholder lines while a block loads (skeletons keep the layout still: Doherty). */
export function LoadingBlock({
  lines = 3,
  className,
}: {
  lines?: number;
  className?: string | undefined;
}) {
  const t = useText();
  return (
    <div role="status" className={cn("grid gap-2.5", className)}>
      <span className="sr-only">{t("common.loading")}</span>
      {Array.from({ length: lines }, (_, i) => `line-${i}`).map((key, i) => (
        <Skeleton
          key={key}
          className={cn("h-4", i % 3 === 2 ? "w-2/3" : i % 2 ? "w-5/6" : "w-full")}
        />
      ))}
    </div>
  );
}

/** Renders a query: skeleton while loading, the problem with Try again, or the data. */
export function QueryState<T>({
  query,
  children,
  lines,
  loading,
}: {
  query: UseQueryResult<T>;
  children: (data: T) => ReactNode;
  lines?: number;
  /** What shows while loading, when the default skeleton does not fit. */
  loading?: ReactNode;
}) {
  if (query.isPending) return <>{loading ?? <LoadingBlock lines={lines ?? 3} />}</>;
  if (query.isError)
    return <ProblemNotice error={query.error} onRetry={() => void query.refetch()} />;
  return <>{children(query.data)}</>;
}

/** "Load more" for cursor-paged lists; keeps the place in the list. */
export function LoadMore<T>({
  query,
  className,
}: {
  query: UseInfiniteQueryResult<InfiniteData<T>>;
  className?: string | undefined;
}) {
  const t = useText();
  if (!query.hasNextPage) return null;
  return (
    <div className={cn("flex justify-center p-3", className)}>
      <Button
        size="sm"
        loading={query.isFetchingNextPage}
        onClick={() => void query.fetchNextPage()}
      >
        {t("common.loadMore")}
      </Button>
    </div>
  );
}

/** A quiet "Updating…" while cached data refreshes in the background (H1). */
export function Refreshing({ active }: { active: boolean }) {
  const t = useText();
  return (
    <span role="status" className="inline-flex h-5 items-center gap-1.5 text-xs text-fg-muted">
      {active ? (
        <>
          <Loader2 aria-hidden="true" className="size-3.5 animate-spin" />
          {t("common.updating")}
        </>
      ) : null}
    </span>
  );
}
