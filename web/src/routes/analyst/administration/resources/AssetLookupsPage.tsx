import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import {
  type AssetLookupKind,
  assetLookupQuery,
  createAssetLookup,
  locationPaths,
  updateAssetLookup,
} from "@/features/assets/api";
import { problemText } from "@/features/result";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { CheckField, FormActions, ResourceAdmin, useFlash, useInvalidateAdmin } from "./shared";

type Row = Schemas["AssetLookupDto"];

const KINDS: Record<AssetLookupKind, { title: MessageId; add: MessageId; intro: MessageId }> = {
  "asset-types": {
    title: "admin.assetTypes",
    add: "admin.assetTypes.add",
    intro: "admin.assetTypes.intro",
  },
  "asset-statuses": {
    title: "admin.assetStatuses",
    add: "admin.assetStatuses.add",
    intro: "admin.assetStatuses.intro",
  },
  "asset-locations": {
    title: "admin.locations",
    add: "admin.locations.add",
    intro: "admin.locations.intro",
  },
};

/** Asset Types, Asset Statuses (with Retired) and the Location tree (FR-AST-002). */
export function AssetLookupsPage({ kind }: { kind: AssetLookupKind }) {
  const t = useText();
  const config = KINDS[kind];
  const rows = useQuery(assetLookupQuery(kind));
  const paths = locationPaths(rows.data);
  const label = (r: Row) => (kind === "asset-locations" ? (paths.get(r.id) ?? r.name) : r.name);
  const sorted = [...(rows.data ?? [])].sort((a, b) =>
    kind === "asset-locations"
      ? label(a).localeCompare(label(b))
      : a.sort_order - b.sort_order || a.name.localeCompare(b.name),
  );
  return (
    <ResourceAdmin
      title={t(config.title)}
      intro={t(config.intro)}
      rows={rows.data ? sorted : undefined}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Row) => r.id}
      getName={label}
      addLabel={t(config.add)}
      columns={[
        {
          header: t("admin.res.name"),
          cell: (r) => <span className="font-medium">{label(r)}</span>,
        },
        ...(kind === "asset-statuses"
          ? [
              {
                header: t("admin.lookup.properties"),
                cell: (r: Row) =>
                  r.is_retired ? <Badge>{t("admin.assetStatuses.retired")}</Badge> : null,
              },
            ]
          : []),
        { header: t("admin.res.order"), cell: (r) => r.sort_order },
        {
          header: t("admin.res.status"),
          cell: (r) => (
            <Badge tone={r.is_active ? "ok" : "muted"}>
              {t(r.is_active ? "admin.res.active" : "admin.res.inactive")}
            </Badge>
          ),
        },
      ]}
      renderForm={(row, done) => (
        <AssetLookupForm kind={kind} row={row} rows={rows.data ?? []} done={done} />
      )}
    />
  );
}

function AssetLookupForm({
  kind,
  row,
  rows,
  done,
}: {
  kind: AssetLookupKind;
  row: Row | null;
  rows: readonly Row[];
  done: () => void;
}) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const paths = locationPaths(rows);
  const [name, setName] = useState(row?.name ?? "");
  const [order, setOrder] = useState(String(row?.sort_order ?? 0));
  const [active, setActive] = useState(row?.is_active ?? true);
  const [retired, setRetired] = useState(row?.is_retired ?? false);
  const [colour, setColour] = useState(row?.colour ?? "");
  const [parentId, setParentId] = useState(row?.parent_id ?? "");

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["AssetLookupRequest"] = {
      name: name.trim(),
      sort_order: Number(order) || 0,
      is_active: active,
      is_retired: kind === "asset-statuses" ? retired : false,
      colour: kind === "asset-statuses" ? colour.trim() || null : null,
      parent_id: kind === "asset-locations" ? parentId || null : null,
    };
    const outcome = row
      ? await updateAssetLookup(kind, row.id, body)
      : await createAssetLookup(kind, body);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await invalidate();
    done();
  }

  return (
    <form onSubmit={submit} noValidate className="grid max-w-xl gap-3">
      <Field
        label={t("admin.res.name")}
        value={name}
        onChange={(e) => setName(e.target.value)}
        required
      />
      {kind === "asset-locations" ? (
        <SelectField
          label={t("admin.locations.parent")}
          value={parentId}
          onChange={(e) => setParentId(e.target.value)}
        >
          <option value="">{t("admin.locations.top")}</option>
          {rows
            .filter((r) => r.id !== row?.id && (r.is_active || r.id === parentId))
            .map((r) => [r.id, paths.get(r.id) ?? r.name] as const)
            .sort((a, b) => a[1].localeCompare(b[1]))
            .map(([id, path]) => (
              <option key={id} value={id}>
                {path}
              </option>
            ))}
        </SelectField>
      ) : null}
      {kind === "asset-statuses" ? (
        <>
          <CheckField
            label={t("admin.assetStatuses.retired")}
            checked={retired}
            onChange={setRetired}
          />
          <Field
            label={t("admin.assetStatuses.colour")}
            hint={t("admin.assetStatuses.colourHint")}
            value={colour}
            onChange={(e) => setColour(e.target.value)}
          />
        </>
      ) : null}
      <Field
        label={t("admin.res.order")}
        type="number"
        value={order}
        onChange={(e) => setOrder(e.target.value)}
      />
      <CheckField label={t("admin.res.active")} checked={active} onChange={setActive} />
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
    </form>
  );
}
