import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { Boxes, Plus } from "lucide-react";
import { Badge } from "@/components/Badge";
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
import {
  type AssetSearch,
  assetLookupQuery,
  assetName,
  assetSummaryQuery,
  assetsQuery,
  locationOptions,
} from "./api";
import { HolderName } from "./HolderName";

/** The Asset register (FR-AST-001, FR-AST-005) beside the open Asset; filters in the URL. */
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
  const openId = useOpenId();
  const detailOpen = useDetailOpen("/assets");
  const setSearch = (next: SearchPatch<AssetSearch>) =>
    void navigate({ to: "/assets", search: { ...search, ...next } });
  const filtered = Boolean(search.q || search.type || search.status || search.location);
  const locationNames = new Map(locationOptions(locations.data).map((o) => [o.value, o.label]));
  useListKeys(
    rows.map((a) => a.id),
    openId,
    (id) => void navigate({ to: "/assets/$id", params: { id }, search }),
  );
  return (
    <SplitView
      detailOpen={detailOpen}
      placeholder={
        <PickOne icon={<Boxes />} title={t("assets.pickOne")}>
          {t("assets.intro")}
        </PickOne>
      }
      list={
        <ListPane
          title={t("assets.title")}
          label={t("assets.title")}
          actions={
            can("assets.create") ? (
              <Link
                to="/assets/new"
                search={search}
                className={buttonClass({ variant: "primary", size: "sm" })}
              >
                <Plus aria-hidden="true" />
                {t("assets.new")}
              </Link>
            ) : undefined
          }
          filters={
            <>
              <SearchBox
                label={t("assets.search")}
                placeholder={t("assets.searchPlaceholder")}
                value={search.q}
                onSubmit={(q) => setSearch({ q })}
              />
              <div className="flex flex-wrap items-center gap-2">
                <FilterSelect
                  label={t("assets.type")}
                  allLabel={t("assets.allTypes")}
                  value={search.type}
                  options={toOptions(types.data, (x) => x.name)}
                  onChange={(type) => setSearch({ type })}
                />
                <FilterSelect
                  label={t("assets.status")}
                  allLabel={t("assets.allStatuses")}
                  value={search.status}
                  options={toOptions(statuses.data, (x) => x.name)}
                  onChange={(status) => setSearch({ status })}
                />
                <FilterSelect
                  label={t("assets.location")}
                  allLabel={t("assets.allLocations")}
                  value={search.location}
                  options={locationOptions(locations.data)}
                  onChange={(location) => setSearch({ location })}
                />
                <Checkbox
                  label={t("assets.includeRetired")}
                  checked={Boolean(search.retired)}
                  onChange={(e) => setSearch({ retired: e.target.checked ? true : undefined })}
                />
              </div>
            </>
          }
          status={
            summary.data ? (
              <p className="border-b border-line px-4 py-1.5 text-xs text-fg-muted">
                {t("assets.summary", {
                  active: summary.data.active,
                  checkedOut: summary.data.checked_out,
                  retired: summary.data.retired,
                })}
              </p>
            ) : null
          }
        >
          {list.isError ? (
            <ProblemNotice error={list.error} className="m-4" onRetry={() => void list.refetch()} />
          ) : null}
          {list.isPending ? <LoadingBlock className="p-4" lines={6} /> : null}
          {list.isSuccess && rows.length === 0 ? (
            <EmptyState
              icon={<Boxes aria-hidden="true" />}
              title={filtered ? t("assets.empty") : t("assets.emptyStart")}
            />
          ) : null}
          {rows.length > 0 ? (
            <ul aria-label={t("assets.list")}>
              {rows.map((a) => (
                <RecordRow key={a.id} selected={a.id === openId}>
                  <div className="flex items-center justify-between gap-2">
                    <Link
                      to="/assets/$id"
                      params={{ id: a.id }}
                      search={search}
                      aria-current={a.id === openId ? "page" : undefined}
                      className={rowLinkClass}
                    >
                      {assetName(a) ?? t("assets.unnamed")}
                    </Link>
                    {a.retired_on ? (
                      <Badge tone="muted">{t("assets.retired")}</Badge>
                    ) : a.checkout ? (
                      <Badge tone="accent">{t("assets.checkedOut")}</Badge>
                    ) : (
                      <Badge tone="success">{t("assets.available")}</Badge>
                    )}
                  </div>
                  <p className="truncate text-xs text-fg-muted">
                    {[
                      a.hostname && a.asset_tag ? a.hostname : null,
                      a.asset_type_id ? namesById(types.data).get(a.asset_type_id) : null,
                      a.asset_status_id ? namesById(statuses.data).get(a.asset_status_id) : null,
                      a.location_id ? locationNames.get(a.location_id) : null,
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </p>
                  <p className="flex flex-wrap gap-x-3 text-xs text-fg-muted">
                    {a.checkout ? (
                      <span>
                        {t("assets.holder")}: <HolderName holder={a.checkout.holder} />
                      </span>
                    ) : null}
                    {a.warranty_expires_on ? (
                      <span>
                        {t("assets.warrantyExpires")}: {f.day(a.warranty_expires_on)}
                      </span>
                    ) : null}
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
