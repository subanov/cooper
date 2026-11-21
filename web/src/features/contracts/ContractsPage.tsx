import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { FileText, Plus } from "lucide-react";
import { buttonClass } from "@/components/Button";
import { EmptyState, LoadingBlock, LoadMore, ProblemNotice } from "@/components/Feedback";
import { Checkbox } from "@/components/Field";
import { FilterSelect, SearchBox } from "@/components/ListSearch";
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
import { namesById, toOptions } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import type { SearchPatch } from "@/routes/search";
import { allSuppliersQuery, type ContractSearch, contractLookupQuery, contractsQuery } from "./api";
import { ContractsTabs } from "./ContractsTabs";
import { NoticeBadge } from "./NoticeBadge";

/** Contracts (FR-CON-001) beside the open Contract; filters in the URL. */
export function ContractsPage({ search }: { search: ContractSearch }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const navigate = useNavigate();
  const list = useInfiniteQuery(contractsQuery(search));
  const statuses = useQuery(contractLookupQuery("contract-statuses"));
  const suppliers = useQuery(allSuppliersQuery);
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const openId = useOpenId();
  const detailOpen = useDetailOpen("/contracts");
  const setSearch = (next: SearchPatch<ContractSearch>) =>
    void navigate({ to: "/contracts", search: { ...search, ...next } });
  const supplierNames = namesById(suppliers.data, (s) => s.legal_name);
  const filtered = Boolean(search.q || search.status || search.supplier || search.notice);
  useListKeys(
    rows.map((c) => c.id),
    openId,
    (id) => void navigate({ to: "/contracts/$id", params: { id }, search }),
  );
  return (
    <SplitView
      detailOpen={detailOpen}
      placeholder={
        <PickOne icon={<FileText />} title={t("contracts.pickOne")}>
          {t("contracts.intro")}
        </PickOne>
      }
      list={
        <ListPane
          title={t("contracts.title")}
          label={t("contracts.title")}
          actions={
            can("contracts.write") ? (
              <Link
                to="/contracts/new"
                search={search}
                className={buttonClass({ variant: "primary", size: "sm" })}
              >
                <Plus aria-hidden="true" />
                {t("contracts.new")}
              </Link>
            ) : undefined
          }
          filters={
            <>
              <ContractsTabs />
              <SearchBox
                label={t("contracts.search")}
                placeholder={t("contracts.searchPlaceholder")}
                value={search.q}
                onSubmit={(q) => setSearch({ q })}
              />
              <div className="flex flex-wrap items-center gap-2">
                <FilterSelect
                  label={t("contracts.status")}
                  allLabel={t("contracts.allStatuses")}
                  value={search.status}
                  options={toOptions(statuses.data, (s) => s.name)}
                  onChange={(status) => setSearch({ status })}
                />
                <FilterSelect
                  label={t("contracts.supplier")}
                  allLabel={t("contracts.allSuppliers")}
                  value={search.supplier}
                  options={toOptions(suppliers.data, (s) => s.legal_name)}
                  onChange={(supplier) => setSearch({ supplier })}
                />
                <Checkbox
                  label={t("contracts.noticeDueOnly")}
                  checked={Boolean(search.notice)}
                  onChange={(e) => setSearch({ notice: e.target.checked ? true : undefined })}
                />
              </div>
            </>
          }
        >
          {list.isError ? (
            <ProblemNotice error={list.error} className="m-4" onRetry={() => void list.refetch()} />
          ) : null}
          {list.isPending ? <LoadingBlock className="p-4" lines={6} /> : null}
          {list.isSuccess && rows.length === 0 ? (
            <EmptyState
              icon={<FileText aria-hidden="true" />}
              title={filtered ? t("contracts.empty") : t("contracts.emptyStart")}
            />
          ) : null}
          {rows.length > 0 ? (
            <ul aria-label={t("contracts.list")}>
              {rows.map((c) => (
                <RecordRow key={c.id} selected={c.id === openId}>
                  <div className="flex items-center justify-between gap-2 text-xs text-fg-muted">
                    <span className="font-mono">{c.contract_number}</span>
                    <NoticeBadge contract={c} />
                  </div>
                  <Link
                    to="/contracts/$id"
                    params={{ id: c.id }}
                    search={search}
                    aria-current={c.id === openId ? "page" : undefined}
                    className={rowLinkClass}
                  >
                    {c.title}
                  </Link>
                  <p className="truncate text-xs text-fg-muted">
                    {[
                      c.supplier_id ? supplierNames.get(c.supplier_id) : null,
                      c.status_id ? namesById(statuses.data).get(c.status_id) : null,
                      c.end_date ? `${t("contracts.ends")} ${f.day(c.end_date)}` : null,
                      c.notice_date ? `${t("contracts.noticeDate")} ${f.day(c.notice_date)}` : null,
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
