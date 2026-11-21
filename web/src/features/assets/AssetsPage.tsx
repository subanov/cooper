import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { Boxes, Plus, Search } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { Badge } from "@/components/Badge";
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
import {
  type AssetSearch,
  assetLookupQuery,
  assetName,
  assetSummaryQuery,
  assetsQuery,
  locationOptions,
} from "./api";
import { HolderName } from "./HolderName";

/** The Asset register (FR-AST-001, FR-AST-005): filters in the URL, totals on top. */
export function AssetsPage({ search }: { search: AssetSearch }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const navigate = useNavigate();
  const list = useInfiniteQuery(assetsQuery(search));
  const summary = useQuery(assetSummaryQuery);
  const types = useQuery(assetLookupQuery("asset-types"));
  const statuses = useQuery(assetLookupQuery("asset-statuses"));
  const locations = useQuery(assetLookupQuery("asset-locations"));
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  const setSearch = (next: SearchPatch<AssetSearch>) =>
    void navigate({ to: "/assets", search: { ...search, ...next } });
  const filtered = Boolean(search.q || search.type || search.status || search.location);
  const locationNames = new Map(locationOptions(locations.data).map((o) => [o.value, o.label]));
  return (
    <Page>
      <PageHeader
        title={t("assets.title")}
        description={
          summary.data
            ? t("assets.summary", {
                active: summary.data.active,
                checkedOut: summary.data.checked_out,
                retired: summary.data.retired,
              })
            : t("assets.intro")
        }
        actions={
          can("assets.create") ? (
            <Link to="/assets/new" className={buttonClass({ variant: "primary" })}>
              <Plus aria-hidden="true" className="size-4" />
              {t("assets.new")}
            </Link>
          ) : undefined
        }
      />
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
            aria-label={t("assets.search")}
            placeholder={t("assets.searchPlaceholder")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            className="pl-8"
          />
        </form>
        <SelectField
          className="w-44"
          label={t("assets.type")}
          value={search.type ?? ""}
          placeholder={t("common.any")}
          options={toOptions(types.data, (x) => x.name)}
          onChange={(e) => setSearch({ type: e.target.value || undefined })}
        />
        <SelectField
          className="w-44"
          label={t("assets.status")}
          value={search.status ?? ""}
          placeholder={t("common.any")}
          options={toOptions(statuses.data, (x) => x.name)}
          onChange={(e) => setSearch({ status: e.target.value || undefined })}
        />
        <SelectField
          className="w-52"
          label={t("assets.location")}
          value={search.location ?? ""}
          placeholder={t("common.any")}
          options={locationOptions(locations.data)}
          onChange={(e) => setSearch({ location: e.target.value || undefined })}
        />
        <Checkbox
          className="h-9 items-center"
          label={t("assets.includeRetired")}
          checked={Boolean(search.retired)}
          onChange={(e) => setSearch({ retired: e.target.checked ? true : undefined })}
        />
      </FilterBar>
      <ProblemNotice error={list.error} />
      <Card flush>
        <DataTable
          caption={t("assets.list")}
          rows={list.data ? rows : undefined}
          loading={list.isPending}
          rowKey={(a) => a.id}
          empty={
            <EmptyState
              icon={<Boxes aria-hidden="true" />}
              title={filtered ? t("assets.empty") : t("assets.emptyStart")}
            />
          }
          columns={[
            {
              id: "asset",
              header: t("assets.asset"),
              rowHeader: true,
              cell: (a) => (
                <span className="grid">
                  <Link to="/assets/$id" params={{ id: a.id }} className={rowLinkClass}>
                    {assetName(a) ?? t("assets.unnamed")}
                  </Link>
                  {a.hostname && a.asset_tag ? (
                    <span className="text-xs text-fg-muted">{a.hostname}</span>
                  ) : null}
                </span>
              ),
            },
            {
              id: "type",
              header: t("assets.type"),
              cell: (a) =>
                (a.asset_type_id && namesById(types.data).get(a.asset_type_id)) ||
                t("common.noneDash"),
            },
            {
              id: "status",
              header: t("assets.status"),
              cell: (a) =>
                a.retired_on ? (
                  <Badge tone="muted">{t("assets.retired")}</Badge>
                ) : (
                  (a.asset_status_id && namesById(statuses.data).get(a.asset_status_id)) ||
                  t("common.noneDash")
                ),
            },
            {
              id: "location",
              header: t("assets.location"),
              wide: true,
              cell: (a) =>
                (a.location_id && locationNames.get(a.location_id)) || t("common.noneDash"),
            },
            {
              id: "holder",
              header: t("assets.holder"),
              cell: (a) =>
                a.checkout ? (
                  <HolderName holder={a.checkout.holder} />
                ) : (
                  <span className="text-fg-muted">{t("assets.available")}</span>
                ),
            },
            {
              id: "warranty",
              header: t("assets.warrantyExpires"),
              wide: true,
              cell: (a) =>
                a.warranty_expires_on ? f.day(a.warranty_expires_on) : t("common.noneDash"),
            },
          ]}
        />
        <LoadMore query={list} />
      </Card>
    </Page>
  );
}
