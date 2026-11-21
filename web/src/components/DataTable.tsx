import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { LoadingBlock } from "./Feedback";

export interface Column<T> {
  key: string;
  header: ReactNode;
  cell: (row: T) => ReactNode;
  className?: string | undefined;
  /** The header is for screen readers only (an actions column). */
  hideHeader?: boolean | undefined;
}

/** A plain, accessible table for admin lists and history (scrolls sideways when narrow). */
export function DataTable<T>({
  caption,
  columns,
  rows,
  rowKey,
  empty,
  loading,
  className,
}: {
  caption: string;
  columns: readonly Column<T>[];
  rows: readonly T[] | undefined;
  rowKey: (row: T) => string;
  empty?: ReactNode | undefined;
  loading?: boolean | undefined;
  className?: string | undefined;
}) {
  if (loading) return <LoadingBlock lines={4} className="p-4" />;
  if (rows && rows.length === 0 && empty) return <>{empty}</>;
  return (
    <div className={cn("overflow-x-auto", className)}>
      <table className="w-full border-collapse text-left text-sm">
        <caption className="sr-only">{caption}</caption>
        <thead>
          <tr className="border-b border-line bg-sunken/60">
            {columns.map((c) => (
              <th
                key={c.key}
                scope="col"
                className={cn(
                  "whitespace-nowrap px-4 py-2 text-xs font-semibold text-fg-muted",
                  c.className,
                )}
              >
                {c.hideHeader ? <span className="sr-only">{c.header}</span> : c.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {(rows ?? []).map((row) => (
            <tr
              key={rowKey(row)}
              className="border-b border-line last:border-b-0 hover:bg-sunken/40"
            >
              {columns.map((c) => (
                <td key={c.key} className={cn("px-4 py-2.5 align-top", c.className)}>
                  {c.cell(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
