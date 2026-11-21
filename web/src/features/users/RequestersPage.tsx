import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
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
import { companiesQuery, namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";
import { PeopleFilters, type PeopleSearch } from "./PeopleList";

/** Requesters of the Companies the Analyst sees (FR-IAM-006). */
export function RequestersPage({ search }: { search: PeopleSearch }) {
  const t = useText();
  const can = useCan();
  const companies = useQuery(companiesQuery);
  const list = useInfiniteQuery({
    queryKey: ["users", "requesters", search],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/requesters", {
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
        title={t("users.requesters")}
        actions={
          can("requesters.manage") ? (
            <Link to="/users/requesters/new" className={buttonClass({ variant: "primary" })}>
              <Plus aria-hidden="true" className="size-4" />
              {t("users.newRequester")}
            </Link>
          ) : undefined
        }
      />
      <PeopleFilters to="/users/requesters" search={search} />
      <ProblemNotice error={list.error} />
      <Card flush>
        <DataTable
          caption={t("users.requesters")}
          rows={rows}
          loading={list.isPending}
          rowKey={(r) => r.id}
          empty={<EmptyState icon={<Users aria-hidden="true" />} title={t("users.empty")} />}
          columns={[
            {
              id: "name",
              header: t("users.name"),
              rowHeader: true,
              cell: (r) => (
                <Link to="/users/requesters/$id" params={{ id: r.id }} className={rowLinkClass}>
                  {r.full_name}
                </Link>
              ),
            },
            { id: "email", header: t("users.email"), cell: (r) => r.email },
            {
              id: "company",
              header: t("users.company"),
              wide: true,
              cell: (r) => namesById(companies.data).get(r.company_id) ?? "…",
            },
            {
              id: "status",
              header: t("users.status"),
              cell: (r) =>
                r.is_active ? (
                  <Badge tone="success">{t("users.active")}</Badge>
                ) : (
                  <Badge tone="muted">{t("users.deactivated")}</Badge>
                ),
            },
          ]}
        />
        <LoadMore query={list} />
      </Card>
    </div>
  );
}
