import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { Building2, Plus } from "lucide-react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { EmptyState, LoadingBlock, LoadMore, ProblemNotice } from "@/components/Feedback";
import { SearchBox } from "@/components/ListSearch";
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
import { namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { contractLookupQuery, suppliersQuery } from "./api";
import { ContractsTabs } from "./ContractsTabs";

/** Suppliers (FR-CON-005) beside the open Supplier. */
export function SuppliersPage({ search }: { search: { q?: string } }) {
  const t = useText();
  const can = useCan();
  const navigate = useNavigate();
  const list = useInfiniteQuery(suppliersQuery(search.q));
  const types = useQuery(contractLookupQuery("supplier-types"));
  const statuses = useQuery(contractLookupQuery("supplier-statuses"));
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const openId = useOpenId();
  const detailOpen = useDetailOpen("/suppliers");
  useListKeys(
    rows.map((s) => s.id),
    openId,
    (id) => void navigate({ to: "/suppliers/$id", params: { id }, search }),
  );
  return (
    <SplitView
      detailOpen={detailOpen}
      placeholder={
        <PickOne icon={<Building2 />} title={t("suppliers.pickOne")}>
          {t("suppliers.intro")}
        </PickOne>
      }
      list={
        <ListPane
          title={t("contracts.title")}
          label={t("contracts.suppliers")}
          actions={
            can("contracts.write") ? (
              <Link
                to="/suppliers/new"
                search={search}
                className={buttonClass({ variant: "primary", size: "sm" })}
              >
                <Plus aria-hidden="true" />
                {t("suppliers.new")}
              </Link>
            ) : undefined
          }
          filters={
            <>
              <ContractsTabs />
              <SearchBox
                label={t("suppliers.search")}
                placeholder={t("suppliers.searchPlaceholder")}
                value={search.q}
                onSubmit={(q) => void navigate({ to: "/suppliers", search: q ? { q } : {} })}
              />
            </>
          }
        >
          {list.isError ? (
            <ProblemNotice error={list.error} className="m-4" onRetry={() => void list.refetch()} />
          ) : null}
          {list.isPending ? <LoadingBlock className="p-4" lines={6} /> : null}
          {list.isSuccess && rows.length === 0 ? (
            <EmptyState
              icon={<Building2 aria-hidden="true" />}
              title={search.q ? t("suppliers.empty") : t("suppliers.emptyStart")}
            />
          ) : null}
          {rows.length > 0 ? (
            <ul aria-label={t("suppliers.list")}>
              {rows.map((s) => (
                <RecordRow key={s.id} selected={s.id === openId}>
                  <div className="flex items-center justify-between gap-2">
                    <Link
                      to="/suppliers/$id"
                      params={{ id: s.id }}
                      search={search}
                      aria-current={s.id === openId ? "page" : undefined}
                      className={rowLinkClass}
                    >
                      {s.legal_name}
                    </Link>
                    {s.is_active ? null : <Badge tone="muted">{t("common.inactive")}</Badge>}
                  </div>
                  <p className="truncate text-xs text-fg-muted">
                    {[
                      s.trading_name,
                      s.supplier_type_id ? namesById(types.data).get(s.supplier_type_id) : null,
                      s.supplier_status_id
                        ? namesById(statuses.data).get(s.supplier_status_id)
                        : null,
                    ]
                      .filter(Boolean)
                      .join(" · ")}
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
