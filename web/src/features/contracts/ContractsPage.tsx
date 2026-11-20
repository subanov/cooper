import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { FileText, Plus, Search } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { buttonClass } from "@/components/Button";
import { Card } from "@/components/Card";
import { DataTable, rowLinkClass } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { Checkbox, Input, SelectField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { FilterBar, Page, PageHeader } from "@/components/Page";
import { LoadMore } from "@/components/QueryState";
import { namesById, toOptions } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import type { SearchPatch } from "@/routes/search";
import { allSuppliersQuery, type ContractSearch, contractLookupQuery, contractsQuery } from "./api";
import { ContractsTabs } from "./ContractsTabs";
import { NoticeBadge } from "./NoticeBadge";

/** Contracts by number, with their Notice Date and where it stands (FR-CON-001). */
export function ContractsPage({ search }: { search: ContractSearch }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const navigate = useNavigate();
  const list = useInfiniteQuery(contractsQuery(search));
  const statuses = useQuery(contractLookupQuery("contract-statuses"));
  const suppliers = useQuery(allSuppliersQuery);
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  const setSearch = (next: SearchPatch<ContractSearch>) =>
    void navigate({ to: "/contracts", search: { ...search, ...next } });
  const supplierNames = namesById(suppliers.data, (s) => s.legal_name);
  const filtered = Boolean(search.q || search.status || search.supplier || search.notice);
  return (
    <Page>
      <PageHeader
        title={t("contracts.title")}
        description={t("contracts.intro")}
        actions={
          can("contracts.write") ? (
            <Link to="/contracts/new" className={buttonClass({ variant: "primary" })}>
              <Plus aria-hidden="true" className="size-4" />
              {t("contracts.new")}
            </Link>
          ) : undefined
        }
      />
      <ContractsTabs />
      <FilterBar label={t("common.filters")}>
        <form
          className="relative w-full max-w-xs"
          onSubmit={(e: FormEvent) => {
            e.preventDefault();
            setSearch({ q: q.trim() || undefined });
          }}
        >
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-fg-muted"
          />
          <Input
            type="search"
            aria-label={t("contracts.search")}
            placeholder={t("contracts.searchPlaceholder")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            className="pl-8"
          />
        </form>
        <SelectField
          className="w-48"
          label={t("contracts.status")}
          value={search.status ?? ""}
          placeholder={t("common.any")}
          options={toOptions(statuses.data, (s) => s.name)}
          onChange={(e) => setSearch({ status: e.target.value || undefined })}
        />
        <SelectField
          className="w-56"
          label={t("contracts.supplier")}
          value={search.supplier ?? ""}
          placeholder={t("common.any")}
          options={toOptions(suppliers.data, (s) => s.legal_name)}
          onChange={(e) => setSearch({ supplier: e.target.value || undefined })}
        />
        <Checkbox
          className="h-9 items-center"
          label={t("contracts.noticeDueOnly")}
          checked={Boolean(search.notice)}
          onChange={(e) => setSearch({ notice: e.target.checked ? true : undefined })}
        />
      </FilterBar>
      <ProblemNotice error={list.error} />
      <Card flush>
        <DataTable
          caption={t("contracts.list")}
          rows={list.data ? rows : undefined}
          loading={list.isPending}
          rowKey={(c) => c.id}
          empty={
            <EmptyState
              icon={<FileText aria-hidden="true" />}
              title={filtered ? t("contracts.empty") : t("contracts.emptyStart")}
            />
          }
          columns={[
            {
              id: "contract",
              header: t("contracts.contracts"),
              rowHeader: true,
              cell: (c) => (
                <span className="grid">
                  <span className="font-mono text-xs text-fg-muted">{c.contract_number}</span>
                  <Link to="/contracts/$id" params={{ id: c.id }} className={rowLinkClass}>
                    {c.title}
                  </Link>
                </span>
              ),
            },
            {
              id: "supplier",
              header: t("contracts.supplier"),
              cell: (c) =>
                (c.supplier_id && supplierNames.get(c.supplier_id)) || t("common.noneDash"),
            },
            {
              id: "status",
              header: t("contracts.status"),
              wide: true,
              cell: (c) =>
                (c.status_id && namesById(statuses.data).get(c.status_id)) || t("common.noneDash"),
            },
            {
              id: "end",
              header: t("contracts.ends"),
              wide: true,
              cell: (c) => (c.end_date ? f.day(c.end_date) : t("common.noneDash")),
            },
            {
              id: "notice",
              header: t("contracts.noticeDate"),
              cell: (c) => (
                <span className="flex flex-wrap items-center gap-1.5">
                  {c.notice_date ? f.day(c.notice_date) : t("common.noneDash")}
                  <NoticeBadge contract={c} />
                </span>
              ),
            },
          ]}
        />
        <LoadMore query={list} />
      </Card>
    </Page>
  );
}
