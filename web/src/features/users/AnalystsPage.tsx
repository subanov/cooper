import { useInfiniteQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Plus, Users } from "lucide-react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { Card } from "@/components/Card";
import { DataTable, rowLinkClass } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import { LoadMore } from "@/components/QueryState";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";
import { PeopleFilters, type PeopleSearch } from "./PeopleList";

/** Analysts (FR-IAM-001). */
export function AnalystsPage({ search }: { search: PeopleSearch }) {
  const t = useText();
  const list = useInfiniteQuery({
    queryKey: ["users", "analysts", search],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/analysts", {
          params: {
            query: {
              limit: 50,
              ...(search.q ? { q: search.q } : {}),
              ...(search.inactive ? { include_inactive: true } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
  const rows = list.data?.pages.flatMap((p) => p.items);
  return (
    <div className="grid gap-4">
      <SectionHeader
        title={t("users.analysts")}
        actions={
          <Link to="/users/analysts/new" className={buttonClass({ variant: "primary" })}>
            <Plus aria-hidden="true" className="size-4" />
            {t("users.newAnalyst")}
          </Link>
        }
      />
      <PeopleFilters to="/users/analysts" search={search} />
      <ProblemNotice error={list.error} />
      <Card flush>
        <DataTable
          caption={t("users.analysts")}
          rows={rows}
          loading={list.isPending}
          rowKey={(a) => a.id}
          empty={<EmptyState icon={<Users aria-hidden="true" />} title={t("users.empty")} />}
          columns={[
            {
              id: "name",
              header: t("users.name"),
              rowHeader: true,
              cell: (a) => (
                <span className="grid">
                  <Link to="/users/analysts/$id" params={{ id: a.id }} className={rowLinkClass}>
                    {a.full_name}
                  </Link>
                  <span className="text-xs text-fg-muted">{a.username}</span>
                </span>
              ),
            },
            { id: "email", header: t("users.email"), wide: true, cell: (a) => a.email },
            {
              id: "status",
              header: t("users.status"),
              cell: (a) => (
                <span className="flex flex-wrap gap-1">
                  {a.is_active ? (
                    <Badge tone="success">{t("users.active")}</Badge>
                  ) : (
                    <Badge tone="muted">{t("users.deactivated")}</Badge>
                  )}
                  {a.is_admin ? <Badge tone="accent">{t("users.administrator")}</Badge> : null}
                </span>
              ),
            },
            {
              id: "totp",
              header: t("users.twoFactor"),
              wide: true,
              cell: (a) => (a.totp_enabled ? t("common.on") : t("common.off")),
            },
          ]}
        />
        <LoadMore query={list} />
      </Card>
    </div>
  );
}
