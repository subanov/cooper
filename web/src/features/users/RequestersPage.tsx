import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { Plus, Users } from "lucide-react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { EmptyState, LoadingBlock, LoadMore, ProblemNotice } from "@/components/Feedback";
import {
  ListPane,
  PickOne,
  RecordRow,
  rowLinkClass,
  SplitView,
  useDetailOpen,
  useListKeys,
  useOpenId,
} from "@/components/MasterDetail";
import { companiesQuery, namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";
import { PeopleFilters, type PeopleSearch } from "./PeopleList";

/** Requesters of the Companies the Analyst sees (FR-IAM-006), beside the open one. */
export function RequestersPage({ search }: { search: PeopleSearch }) {
  const t = useText();
  const can = useCan();
  const navigate = useNavigate();
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
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const openId = useOpenId();
  const detailOpen = useDetailOpen("/users/requesters");
  useListKeys(
    rows.map((r) => r.id),
    openId,
    (id) => void navigate({ to: "/users/requesters/$id", params: { id }, search }),
  );
  return (
    <SplitView
      detailOpen={detailOpen}
      placeholder={<PickOne icon={<Users />} title={t("users.pickRequester")} />}
      list={
        <ListPane
          level={2}
          title={t("users.requesters")}
          label={t("users.requesters")}
          actions={
            can("requesters.manage") ? (
              <Link
                to="/users/requesters/new"
                search={search}
                className={buttonClass({ variant: "primary", size: "sm" })}
              >
                <Plus aria-hidden="true" />
                {t("users.newRequester")}
              </Link>
            ) : undefined
          }
          filters={<PeopleFilters to="/users/requesters" search={search} />}
        >
          {list.isError ? (
            <ProblemNotice error={list.error} className="m-4" onRetry={() => void list.refetch()} />
          ) : null}
          {list.isPending ? <LoadingBlock className="p-4" lines={6} /> : null}
          {list.isSuccess && rows.length === 0 ? (
            <EmptyState icon={<Users aria-hidden="true" />} title={t("users.empty")} />
          ) : null}
          {rows.length > 0 ? (
            <ul aria-label={t("users.requesters")}>
              {rows.map((r) => (
                <RecordRow key={r.id} selected={r.id === openId}>
                  <div className="flex items-center justify-between gap-2">
                    <Link
                      to="/users/requesters/$id"
                      params={{ id: r.id }}
                      search={search}
                      aria-current={r.id === openId ? "page" : undefined}
                      className={rowLinkClass}
                    >
                      {r.full_name}
                    </Link>
                    {r.is_active ? null : <Badge tone="muted">{t("users.deactivated")}</Badge>}
                  </div>
                  <p className="truncate text-xs text-fg-muted">
                    {r.email} · {namesById(companies.data).get(r.company_id) ?? "…"}
                  </p>
                </RecordRow>
              ))}
            </ul>
          ) : null}
          <LoadMore query={list} />
        </ListPane>
      }
    >
      <Outlet />
    </SplitView>
  );
}
