import { useInfiniteQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, useEffect, useState } from "react";
import { Badge } from "@/components/Badge";
import { Card } from "@/components/Card";
import { DataTable } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { SelectField, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { FilterBar, SectionHeader } from "@/components/Page";
import { LoadMore } from "@/components/QueryState";
import { useFormat, useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";

interface LogSearch {
  level?: "warn" | "error";
  q?: string;
}

/** Warnings and errors from the server and the worker (FR-SYS-007). */
export function SystemLogPage({ search }: { search: LogSearch }) {
  const t = useText();
  const f = useFormat();
  const navigate = useNavigate();
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
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
  return (
    <>
      <SectionHeader title={t("admin.log")} description={t("admin.log.intro")} />
      <FilterBar label={t("common.filters")}>
        <SelectField
          className="w-52"
          label={t("admin.log.level")}
          value={search.level ?? ""}
          placeholder={t("admin.log.levelAll")}
          options={[
            { value: "warn", label: t("admin.log.warn") },
            { value: "error", label: t("admin.log.error") },
          ]}
          onChange={(e) =>
            void navigate({
              to: "/administration/log",
              search: { ...search, level: (e.target.value || undefined) as LogSearch["level"] },
            })
          }
        />
        <form
          className="w-full max-w-xs"
          onSubmit={(e: FormEvent) => {
            e.preventDefault();
            void navigate({
              to: "/administration/log",
              search: { ...search, q: q.trim() || undefined },
            });
          }}
        >
          <TextField
            type="search"
            label={t("admin.log.search")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </form>
      </FilterBar>
      <ProblemNotice error={log.error} />
      <Card flush>
        <DataTable
          caption={t("admin.log")}
          rows={rows}
          loading={log.isPending}
          rowKey={(r) => r.id}
          empty={<EmptyState title={t("admin.log.empty")} />}
          columns={[
            {
              id: "when",
              header: t("admin.log.when"),
              className: "whitespace-nowrap",
              cell: (r) => <time dateTime={r.occurred_at}>{f.dateTime(r.occurred_at)}</time>,
            },
            {
              id: "level",
              header: t("admin.log.level"),
              cell: (r) => (
                <Badge tone={r.level === "error" ? "danger" : "warning"}>
                  {r.level === "error" ? t("admin.log.error") : t("admin.log.warn")}
                </Badge>
              ),
            },
            {
              id: "source",
              header: t("admin.log.source"),
              wide: true,
              cell: (r) => (
                <span className="grid text-xs">
                  <span>{r.source}</span>
                  <span className="font-mono text-fg-muted [overflow-wrap:anywhere]">
                    {r.target}
                  </span>
                </span>
              ),
            },
            {
              id: "message",
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
          ]}
        />
        <LoadMore query={log} />
      </Card>
    </>
  );
}
