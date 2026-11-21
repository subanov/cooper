import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { Skeleton } from "./Spinner";

export interface Column<T> {
  id: string;
  header: ReactNode;
  cell: (row: T) => ReactNode;
  className?: string | undefined;
  /** Hidden below the `sm` breakpoint, to keep narrow screens readable. */
  wide?: boolean | undefined;
  /** Makes the cell a row header (the record's name). */
  rowHeader?: boolean | undefined;
}

/**
 * A semantic, scannable table. The record's name cell is a link to it, so rows work with
 * the keyboard and screen readers without custom row handlers.
 */
export function DataTable<T>({
  columns,
  rows,
  rowKey,
  caption,
  captionHidden = true,
  loading = false,
  empty,
  selectedKey,
  className,
}: {
  columns: readonly Column<T>[];
  rows: readonly T[] | undefined;
  rowKey: (row: T) => string;
  caption: string;
  captionHidden?: boolean | undefined;
  loading?: boolean | undefined;
  /** Shown instead of rows when there are none. */
  empty?: ReactNode;
  selectedKey?: string | undefined;
  className?: string | undefined;
}) {
  const hide = (c: Column<T>) => (c.wide ? "hidden sm:table-cell" : "");
  return (
    <div className={cn("overflow-x-auto", className)}>
      <table className="w-full border-collapse text-left text-sm">
        <caption className={cn("pb-2 text-left text-sm font-medium", captionHidden && "sr-only")}>
          {caption}
        </caption>
        <thead>
          <tr className="border-b border-line">
            {columns.map((c) => (
              <th
                key={c.id}
                scope="col"
                className={cn(
                  "h-9 whitespace-nowrap px-3 text-xs font-semibold text-fg-muted first:pl-4 last:pr-4",
                  hide(c),
                  c.className,
                )}
              >
                {c.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {loading && !rows
            ? [0, 1, 2, 3].map((n) => (
                <tr key={n} className="border-b border-line last:border-0">
                  {columns.map((c) => (
                    <td key={c.id} className={cn("px-3 py-3 first:pl-4 last:pr-4", hide(c))}>
                      <Skeleton className="h-4 w-3/4" />
                    </td>
                  ))}
                </tr>
              ))
            : (rows ?? []).map((row) => {
                const key = rowKey(row);
                return (
                  <tr
                    key={key}
                    aria-current={selectedKey === key ? "true" : undefined}
                    className="border-b border-line last:border-0 hover:bg-canvas aria-[current=true]:bg-accent-soft"
                  >
                    {columns.map((c) => {
                      const Cell = c.rowHeader ? "th" : "td";
                      return (
                        <Cell
                          key={c.id}
                          scope={c.rowHeader ? "row" : undefined}
                          className={cn(
                            "px-3 py-2.5 align-middle font-normal first:pl-4 last:pr-4",
                            hide(c),
                            c.className,
                          )}
                        >
                          {c.cell(row)}
                        </Cell>
                      );
                    })}
                  </tr>
                );
              })}
        </tbody>
      </table>
      {!loading && rows && rows.length === 0 ? empty : null}
    </div>
  );
}

/** The link in a table's name cell. */
export const rowLinkClass =
  "font-medium text-fg underline-offset-2 hover:text-accent-text hover:underline [overflow-wrap:anywhere]";
