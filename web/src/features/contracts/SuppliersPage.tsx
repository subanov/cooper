import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { Building2, Plus, Search } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { Card } from "@/components/Card";
import { DataTable, rowLinkClass } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { Input } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { FilterBar, Page, PageHeader } from "@/components/Page";
import { LoadMore } from "@/components/QueryState";
import { namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { contractLookupQuery, suppliersQuery } from "./api";
import { ContractsTabs } from "./ContractsTabs";

/** Suppliers by legal name (FR-CON-005). */
export function SuppliersPage({ search }: { search: { q?: string } }) {
  const t = useText();
  const can = useCan();
  const navigate = useNavigate();
  const list = useInfiniteQuery(suppliersQuery(search.q));
  const types = useQuery(contractLookupQuery("supplier-types"));
  const statuses = useQuery(contractLookupQuery("supplier-statuses"));
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  return (
    <Page>
      <PageHeader
        title={t("contracts.title")}
        description={t("contracts.intro")}
        actions={
          can("contracts.write") ? (
            <Link to="/suppliers/new" className={buttonClass({ variant: "primary" })}>
              <Plus aria-hidden="true" className="size-4" />
              {t("suppliers.new")}
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
            void navigate({ to: "/suppliers", search: { q: q.trim() || undefined } });
          }}
        >
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-fg-muted"
          />
          <Input
            type="search"
            aria-label={t("suppliers.search")}
            placeholder={t("suppliers.searchPlaceholder")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            className="pl-8"
          />
        </form>
      </FilterBar>
      <ProblemNotice error={list.error} />
      <Card flush>
        <DataTable
          caption={t("suppliers.list")}
          rows={list.data ? rows : undefined}
          loading={list.isPending}
          rowKey={(s) => s.id}
          empty={
            <EmptyState
              icon={<Building2 aria-hidden="true" />}
              title={search.q ? t("suppliers.empty") : t("suppliers.emptyStart")}
            />
          }
          columns={[
            {
              id: "name",
              header: t("suppliers.legalName"),
              rowHeader: true,
              cell: (s) => (
                <span className="grid">
                  <Link to="/suppliers/$id" params={{ id: s.id }} className={rowLinkClass}>
                    {s.legal_name}
                  </Link>
                  {s.trading_name ? (
                    <span className="text-xs text-fg-muted">{s.trading_name}</span>
                  ) : null}
                </span>
              ),
            },
            {
              id: "type",
              header: t("suppliers.type"),
              cell: (s) =>
                (s.supplier_type_id && namesById(types.data).get(s.supplier_type_id)) ||
                t("common.noneDash"),
            },
            {
              id: "status",
              header: t("suppliers.status"),
              wide: true,
              cell: (s) =>
                (s.supplier_status_id && namesById(statuses.data).get(s.supplier_status_id)) ||
                t("common.noneDash"),
            },
            {
              id: "active",
              header: t("common.status"),
              cell: (s) =>
                s.is_active ? (
                  <Badge tone="success">{t("common.active")}</Badge>
                ) : (
                  <Badge tone="muted">{t("common.inactive")}</Badge>
                ),
            },
          ]}
        />
        <LoadMore query={list} />
      </Card>
    </Page>
  );
}
