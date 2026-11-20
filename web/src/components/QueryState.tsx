import type { UseInfiniteQueryResult, UseQueryResult } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";
import { Button } from "./Button";
import { ProblemNotice } from "./Notice";
import { Skeleton, Spinner } from "./Spinner";

/**
 * Renders a query's data, or a loading skeleton, or the problem with a Retry button
 * (H1: system status is always visible; H9: errors say what to do next).
 */
export function QueryState<T>({
  query,
  children,
  loading,
}: {
  query: UseQueryResult<T>;
  children: (data: T) => ReactNode;
  loading?: ReactNode;
}) {
  const t = useText();
  if (query.isPending) {
    return <>{loading ?? <LoadingBlock />}</>;
  }
  if (query.isError) {
    return (
      <ProblemNotice
        error={query.error}
        action={
          <Button size="sm" onClick={() => void query.refetch()}>
            {t("common.retry")}
          </Button>
        }
      />
    );
  }
  return <>{children(query.data)}</>;
}

/** A neutral loading placeholder, announced once to screen readers. */
export function LoadingBlock({ lines = 3 }: { lines?: number | undefined }) {
  const t = useText();
  return (
    <div role="status" className="grid gap-2.5 py-2">
      <span className="sr-only">{t("common.loading")}</span>
      {Array.from({ length: lines }, (_, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: static placeholders
        <Skeleton key={i} className={i === lines - 1 ? "h-4 w-1/2" : "h-4 w-full"} />
      ))}
    </div>
  );
}

/** "Load more" for cursor-paged lists, with its own busy state. */
export function LoadMore({ query }: { query: UseInfiniteQueryResult<unknown> }) {
  const t = useText();
  if (!query.hasNextPage) return null;
  return (
    <div className="flex justify-center p-3">
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

/** A small "updating" marker for background refetches. */
export function Refreshing({ active }: { active: boolean }) {
  const t = useText();
  if (!active) return null;
  return (
    <span role="status" className="inline-flex items-center gap-1.5 text-xs text-fg-muted">
      <Spinner className="size-3" />
      {t("common.updating")}
    </span>
  );
}
