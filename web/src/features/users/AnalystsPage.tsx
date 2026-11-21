import { useInfiniteQuery } from "@tanstack/react-query";
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
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";
import { PeopleFilters, type PeopleSearch } from "./PeopleList";

/** Analysts (FR-IAM-001) beside the open Analyst. */
export function AnalystsPage({ search }: { search: PeopleSearch }) {
  const t = useText();
  const navigate = useNavigate();
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
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const openId = useOpenId();
  const detailOpen = useDetailOpen("/users/analysts");
  useListKeys(
    rows.map((a) => a.id),
    openId,
    (id) => void navigate({ to: "/users/analysts/$id", params: { id }, search }),
  );
  return (
    <SplitView
      detailOpen={detailOpen}
      placeholder={<PickOne icon={<Users />} title={t("users.pickAnalyst")} />}
      list={
        <ListPane
          level={2}
          title={t("users.analysts")}
          label={t("users.analysts")}
          actions={
            <Link
              to="/users/analysts/new"
              search={search}
              className={buttonClass({ variant: "primary", size: "sm" })}
            >
              <Plus aria-hidden="true" />
              {t("users.newAnalyst")}
            </Link>
          }
          filters={<PeopleFilters to="/users/analysts" search={search} />}
        >
          {list.isError ? (
            <ProblemNotice error={list.error} className="m-4" onRetry={() => void list.refetch()} />
          ) : null}
          {list.isPending ? <LoadingBlock className="p-4" lines={6} /> : null}
          {list.isSuccess && rows.length === 0 ? (
            <EmptyState icon={<Users aria-hidden="true" />} title={t("users.empty")} />
          ) : null}
          {rows.length > 0 ? (
            <ul aria-label={t("users.analysts")}>
              {rows.map((a) => (
                <RecordRow key={a.id} selected={a.id === openId}>
                  <div className="flex items-center justify-between gap-2">
                    <Link
                      to="/users/analysts/$id"
                      params={{ id: a.id }}
                      search={search}
                      aria-current={a.id === openId ? "page" : undefined}
                      className={rowLinkClass}
                    >
                      {a.full_name}
                    </Link>
                    <span className="flex gap-1">
                      {a.is_admin ? <Badge tone="accent">{t("users.administrator")}</Badge> : null}
                      {a.is_active ? null : <Badge tone="muted">{t("users.deactivated")}</Badge>}
                    </span>
                  </div>
                  <p className="truncate text-xs text-fg-muted">
                    {a.username} · {a.email} · {t("users.twoFactor")}:{" "}
                    {a.totp_enabled ? t("common.on") : t("common.off")}
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
