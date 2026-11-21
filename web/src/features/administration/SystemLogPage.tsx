import { useInfiniteQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Badge } from "@/components/Badge";
import { type Column, DataTable } from "@/components/DataTable";
import { EmptyState, LoadMore, ProblemNotice } from "@/components/Feedback";
import { PageHeader, Panel } from "@/components/Layout";
import { FilterSelect, SearchBox } from "@/components/ListSearch";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

export interface LogSearch {
  level?: "warn" | "error";
  q?: string;
}
type Entry = Schemas["LogEntryDto"];

/** Warnings and errors from the server and the worker (FR-SYS-007); filters in the URL. */
export function SystemLogPage({ search }: { search: LogSearch }) {
  const t = useText();
  const f = useFormat();
  const navigate = useNavigate();
  const log = useInfiniteQuery({
    queryKey: ["administration", "log", search],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/system-log", {
          params: {
            query: {
              limit: 50,
              ...(search.level ? { level: search.level } : {}),
              ...(search.q ? { q: search.q } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
  const rows = log.data?.pages.flatMap((p) => p.items);
  const columns: Column<Entry>[] = [
    {
      key: "when",
      header: t("admin.log.when"),
      className: "whitespace-nowrap",
      cell: (r) => <time dateTime={r.occurred_at}>{f.dateTime(r.occurred_at)}</time>,
    },
    {
      key: "level",
      header: t("admin.log.level"),
      cell: (r) => (
        <Badge tone={r.level === "error" ? "danger" : "warning"}>
          {r.level === "error" ? t("admin.log.error") : t("admin.log.warn")}
        </Badge>
      ),
    },
    {
      key: "source",
      header: t("admin.log.source"),
      cell: (r) => (
        <span className="grid text-xs">
          <span>{r.source}</span>
          <span className="font-mono text-fg-muted [overflow-wrap:anywhere]">{r.target}</span>
        </span>
      ),
    },
    {
      key: "message",
      header: t("admin.log.message"),
      cell: (r) => (
        <span className="grid gap-0.5">
          <span className="[overflow-wrap:anywhere]">{r.message}</span>
          {r.request_id ? (
            <span className="font-mono text-xs text-fg-muted">
              {t("admin.log.request", { id: r.request_id })}
            </span>
          ) : null}
        </span>
      ),
    },
  ];
  return (
    <>
      <PageHeader level={2} title={t("admin.log")} description={t("admin.log.intro")} />
      <div className="flex flex-wrap items-center gap-2">
        <FilterSelect
          label={t("admin.log.level")}
          allLabel={t("admin.log.levelAll")}
          value={search.level}
          options={[
            { value: "warn", label: t("admin.log.warn") },
            { value: "error", label: t("admin.log.error") },
          ]}
          onChange={(level) =>
            void navigate({
              to: "/administration/log",
              search: { ...search, level: level as LogSearch["level"] },
            })
          }
        />
        <SearchBox
          className="max-w-sm"
          label={t("admin.log.search")}
          placeholder={t("admin.log.search")}
          value={search.q}
          onSubmit={(q) => void navigate({ to: "/administration/log", search: { ...search, q } })}
        />
      </div>
      <ProblemNotice error={log.error} />
      <Panel flush>
        <DataTable
          caption={t("admin.log")}
          rows={rows}
          loading={log.isPending}
          rowKey={(r) => r.id}
          empty={<EmptyState title={t("admin.log.empty")} />}
          columns={columns}
        />
        <LoadMore query={log} />
      </Panel>
    </>
  );
}
