import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import { costCentreRowsQuery } from "@/features/administration/resources";
import {
  assetLookupQuery,
  assetQuery,
  createAsset,
  locationPaths,
  updateAsset,
} from "@/features/assets/api";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import { analystSessionQuery } from "@/features/session/queries";
import { companiesQuery, defaultCompanyId } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";
import { AssetCustody, AssetHistory, AssetLinks } from "./AssetSections";
import { assetLabel } from "./AssetsPage";

type Asset = Schemas["AssetDto"];

/** Registers an Asset (`id` absent) or shows and edits one, with custody, links and history. */
export function AssetEditor({ id }: { id?: string | undefined }) {
  const t = useText();
  const existing = useQuery({ ...assetQuery(id ?? ""), enabled: Boolean(id) });
  const session = useQuery(analystSessionQuery).data;
  if (id && existing.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (id && !existing.data) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  const asset = existing.data;
  const canUpdate = session ? can(session, "assets.update") : false;
  return (
    <div className="grid max-w-3xl gap-6">
      <Link to="/assets" className="text-sm underline-offset-2 hover:underline">
        {t("assets.back")}
      </Link>
      <AssetForm key={asset?.updated_at ?? "new"} asset={asset} canUpdate={!asset || canUpdate} />
      {asset ? <AssetCustody asset={asset} canUpdate={canUpdate} /> : null}
      {asset ? <AssetLinks asset={asset} canUpdate={canUpdate} /> : null}
      {asset ? <AssetHistory assetId={asset.id} /> : null}
    </div>
  );
}

function AssetForm({ asset, canUpdate }: { asset: Asset | undefined; canUpdate: boolean }) {
  const t = useText();
  const intl = useIntl();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const companies = useQuery(companiesQuery);
  const types = useQuery(assetLookupQuery("asset-types"));
  const statuses = useQuery(assetLookupQuery("asset-statuses"));
  const locations = useQuery(assetLookupQuery("asset-locations"));
  const costCentres = useQuery(costCentreRowsQuery);
  const [company, setCompany] = useState(asset?.company_id ?? "");
  const [hostname, setHostname] = useState(asset?.hostname ?? "");
  const [assetTag, setAssetTag] = useState(asset?.asset_tag ?? "");
  const [serviceTag, setServiceTag] = useState(asset?.service_tag ?? "");
  const [typeId, setTypeId] = useState(asset?.asset_type_id ?? "");
  const [statusId, setStatusId] = useState(asset?.asset_status_id ?? "");
  const [locationId, setLocationId] = useState(asset?.location_id ?? "");
  const [costCentreId, setCostCentreId] = useState(asset?.cost_centre_id ?? "");
  const [purchaseDate, setPurchaseDate] = useState(asset?.purchase_date ?? "");
  const [cost, setCost] = useState(asset?.purchase_cost ?? "");
  const [currency, setCurrency] = useState(asset?.purchase_currency ?? "");
  const [warranty, setWarranty] = useState(asset?.warranty_expires_on ?? "");
  const [leaseStart, setLeaseStart] = useState(asset?.lease_starts_on ?? "");
  const [leaseEnd, setLeaseEnd] = useState(asset?.lease_expires_on ?? "");
  const [idError, setIdError] = useState<string | undefined>();
  const companyId = company || defaultCompanyId(companies.data);
  const locationName = locationPaths(locations.data);
  // Inactive rows stay selectable only when the Asset already has them.
  const usable = (rows: readonly Schemas["AssetLookupDto"][] | undefined, current: string) =>
    (rows ?? []).filter((r) => r.is_active || r.id === current);
  // Retired statuses come only through Retire; a retired Asset keeps one.
  const statusChoices = usable(statuses.data, statusId).filter(
    (s) => s.is_retired === (asset ? !asset.is_active : false),
  );

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!hostname.trim() && !assetTag.trim() && !serviceTag.trim()) {
      setIdError(t("assets.identifierRequired"));
      return;
    }
    setIdError(undefined);
    const text = (v: string) => v.trim() || null;
    const body: Schemas["AssetRequest"] = {
      hostname: text(hostname),
      asset_tag: text(assetTag),
      service_tag: text(serviceTag),
      asset_type_id: typeId || null,
      asset_status_id: statusId || null,
      location_id: locationId || null,
      supplier_id: asset?.supplier_id ?? null,
      cost_centre_id: costCentreId || null,
      purchase_date: purchaseDate || null,
      purchase_cost: text(cost),
      purchase_currency: text(currency),
      warranty_expires_on: warranty || null,
      lease_starts_on: leaseStart || null,
      lease_expires_on: leaseEnd || null,
      ...(asset ? {} : { company_id: companyId }),
    };
    const outcome = asset ? await updateAsset(asset.id, body) : await createAsset(body);
    if (!outcome.ok) {
      flash.fail(t("assets.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["assets"] });
    if (asset) {
      flash.ok(t("users.saved"));
    } else {
      await navigate({ to: "/assets/$id", params: { id: outcome.data.id } });
    }
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-3" aria-labelledby="asset-title">
      <div className="flex flex-wrap items-center gap-2">
        <h1 id="asset-title" tabIndex={-1} className="text-2xl font-semibold outline-none">
          {asset ? assetLabel(asset) : t("assets.new")}
        </h1>
        {asset && !asset.is_active ? (
          <Badge tone="muted">
            {t("assets.retiredOn", {
              date: intl.formatDate(asset.retired_on ?? "", { dateStyle: "medium" }),
            })}
          </Badge>
        ) : null}
      </div>
      {asset?.retirement_reason ? (
        <p className="text-sm text-muted-foreground">
          {t("assets.retiredBecause", { reason: asset.retirement_reason })}
        </p>
      ) : null}
      <fieldset disabled={!canUpdate} className="grid gap-3">
        {asset ? null : (
          <SelectField
            label={t("users.company")}
            value={companyId}
            onChange={(e) => setCompany(e.target.value)}
          >
            {(companies.data ?? [])
              .filter((c) => c.is_active)
              .map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
          </SelectField>
        )}
        <div className="grid gap-3 sm:grid-cols-3">
          <Field
            label={t("assets.assetTag")}
            value={assetTag}
            onChange={(e) => setAssetTag(e.target.value)}
            error={idError}
          />
          <Field
            label={t("assets.hostname")}
            value={hostname}
            onChange={(e) => setHostname(e.target.value)}
          />
          <Field
            label={t("assets.serviceTag")}
            value={serviceTag}
            onChange={(e) => setServiceTag(e.target.value)}
          />
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <SelectField
            label={t("assets.type")}
            value={typeId}
            onChange={(e) => setTypeId(e.target.value)}
          >
            <option value="">{t("assets.none")}</option>
            {usable(types.data, typeId).map((r) => (
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
            <option value="">{t("assets.none")}</option>
            {statusChoices.map((r) => (
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
            <option value="">{t("assets.none")}</option>
            {usable(locations.data, locationId)
              .map((r) => [r.id, locationName.get(r.id) ?? r.name] as const)
              .sort((a, b) => a[1].localeCompare(b[1]))
              .map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
          </SelectField>
          <SelectField
            label={t("assets.costCentre")}
            value={costCentreId}
            onChange={(e) => setCostCentreId(e.target.value)}
          >
            <option value="">{t("assets.none")}</option>
            {(costCentres.data ?? [])
              .filter(
                (c) =>
                  c.company_id === (asset?.company_id ?? companyId) &&
                  (c.is_active || c.id === costCentreId),
              )
              .map((c) => (
                <option key={c.id} value={c.id}>
                  {`${c.code} – ${c.name}`}
                </option>
              ))}
          </SelectField>
        </div>
        <div className="grid gap-3 sm:grid-cols-3">
          <Field
            label={t("assets.purchaseDate")}
            type="date"
            value={purchaseDate}
            onChange={(e) => setPurchaseDate(e.target.value)}
          />
          <Field
            label={t("assets.purchaseCost")}
            hint={t("assets.purchaseCostHint")}
            inputMode="decimal"
            value={cost}
            onChange={(e) => setCost(e.target.value)}
          />
          <Field
            label={t("assets.currency")}
            hint={t("assets.currencyHint")}
            maxLength={3}
            value={currency}
            onChange={(e) => setCurrency(e.target.value.toUpperCase())}
          />
          <Field
            label={t("assets.warrantyExpires")}
            type="date"
            value={warranty}
            onChange={(e) => setWarranty(e.target.value)}
          />
          <Field
            label={t("assets.leaseStarts")}
            type="date"
            value={leaseStart}
            onChange={(e) => setLeaseStart(e.target.value)}
          />
          <Field
            label={t("assets.leaseExpires")}
            type="date"
            value={leaseEnd}
            onChange={(e) => setLeaseEnd(e.target.value)}
          />
        </div>
        {canUpdate ? (
          <div className="flex items-center gap-3">
            <Button type="submit">{asset ? t("users.save") : t("users.create")}</Button>
            {flash.element}
          </div>
        ) : null}
      </fieldset>
    </form>
  );
}
