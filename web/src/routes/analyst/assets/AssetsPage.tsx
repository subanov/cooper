import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import {
  assetListQuery,
  assetLookupQuery,
  assetSummaryQuery,
  locationPaths,
} from "@/features/assets/api";
import { can } from "@/features/navigation/navigation";
import { analystSessionQuery } from "@/features/session/queries";
import { namesById } from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

type Asset = Schemas["AssetDto"];

/** The Asset list (FR-AST-001) with filters; retired Assets only on request (FR-AST-005). */
export function AssetsPage() {
  const t = useText();
  const session = useQuery(analystSessionQuery).data;
  const [q, setQ] = useState("");
  const [typeId, setTypeId] = useState("");
  const [statusId, setStatusId] = useState("");
  const [locationId, setLocationId] = useState("");
  const [includeRetired, setIncludeRetired] = useState(false);
  const types = useQuery(assetLookupQuery("asset-types"));
  const statuses = useQuery(assetLookupQuery("asset-statuses"));
  const locations = useQuery(assetLookupQuery("asset-locations"));
  const summary = useQuery(assetSummaryQuery);
  const list = useInfiniteQuery(
    assetListQuery({
      ...(q.trim() ? { q: q.trim() } : {}),
      ...(typeId ? { assetTypeId: typeId } : {}),
      ...(statusId ? { assetStatusId: statusId } : {}),
      ...(locationId ? { locationId } : {}),
      includeRetired,
    }),
  );
  const typeName = namesById(types.data, (r) => r.name);
  const statusName = namesById(statuses.data, (r) => r.name);
  const locationName = locationPaths(locations.data);
  const assets = list.data?.pages.flatMap((p) => p.items) ?? [];
  const canCreate = session ? can(session, "assets.create") : false;

  return (
    <div className="grid gap-4">
      <div className="flex items-center gap-3">
        <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
          {t("nav.assets")}
        </h1>
        {canCreate ? (
          <Button asChild size="sm" className="ml-auto">
            <Link to="/assets/new">{t("assets.new")}</Link>
          </Button>
        ) : null}
      </div>
      {summary.data ? (
        <p className="text-sm text-muted-foreground">
          {t("assets.summary", {
            active: summary.data.active,
            checkedOut: summary.data.checked_out,
            retired: summary.data.retired,
          })}
        </p>
      ) : null}
      <div className="flex flex-wrap items-end gap-3">
        <Field
          label={t("assets.search")}
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <SelectField
          label={t("assets.type")}
          value={typeId}
          onChange={(e) => setTypeId(e.target.value)}
        >
          <option value="">{t("assets.any")}</option>
          {(types.data ?? []).map((r) => (
            <option key={r.id} value={r.id}>
              {r.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("assets.status")}
          value={statusId}
          onChange={(e) => setStatusId(e.target.value)}
        >
          <option value="">{t("assets.any")}</option>
          {(statuses.data ?? []).map((r) => (
            <option key={r.id} value={r.id}>
              {r.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("assets.location")}
          value={locationId}
          onChange={(e) => setLocationId(e.target.value)}
        >
          <option value="">{t("assets.any")}</option>
          {[...locationName.entries()]
            .sort((a, b) => a[1].localeCompare(b[1]))
            .map(([id, name]) => (
              <option key={id} value={id}>
                {name}
              </option>
            ))}
        </SelectField>
        <label className="flex items-center gap-2 pb-2 text-sm">
          <input
            type="checkbox"
            checked={includeRetired}
            onChange={(e) => setIncludeRetired(e.target.checked)}
          />
          {t("assets.includeRetired")}
        </label>
      </div>
      {list.isPending ? <p>{t("common.loading")}</p> : null}
      {list.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {list.isSuccess && assets.length === 0 ? (
        <p className="text-muted-foreground">{t("assets.empty")}</p>
      ) : null}
      {assets.length > 0 ? (
        <table className="w-full text-left text-sm">
          <caption className="sr-only">{t("nav.assets")}</caption>
          <thead>
            <tr className="border-b border-border">
              <th scope="col" className="py-2 pr-3">
                {t("assets.asset")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("assets.type")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("assets.status")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("assets.location")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("assets.holder")}
              </th>
            </tr>
          </thead>
          <tbody>
            {assets.map((a) => (
              <tr key={a.id} className="border-b border-border align-top">
                <td className="py-2 pr-3">
                  <Link
                    to="/assets/$id"
                    params={{ id: a.id }}
                    className="font-medium underline-offset-2 hover:underline"
                  >
                    {assetLabel(a)}
                  </Link>
                  {a.is_active ? null : (
                    <Badge tone="muted" className="ml-2">
                      {t("assets.retired")}
                    </Badge>
                  )}
                </td>
                <td className="py-2 pr-3">
                  {a.asset_type_id ? typeName.get(a.asset_type_id) : ""}
                </td>
                <td className="py-2 pr-3">
                  {a.asset_status_id ? statusName.get(a.asset_status_id) : ""}
                </td>
                <td className="py-2 pr-3">
                  {a.location_id ? locationName.get(a.location_id) : ""}
                </td>
                <td className="py-2 pr-3">
                  {a.checkout ? t("assets.checkedOut") : t("assets.available")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
      {list.hasNextPage ? (
        <Button variant="outline" size="sm" onClick={() => void list.fetchNextPage()}>
          {t("users.more")}
        </Button>
      ) : null}
    </div>
  );
}

/** How an Asset is named on screen: Asset Tag, hostname, else Service Tag. */
export function assetLabel(a: Pick<Asset, "asset_tag" | "hostname" | "service_tag">): string {
  return [a.asset_tag, a.hostname].filter(Boolean).join(" · ") || a.service_tag || "";
}
