import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Badge, ColourLabel } from "@/components/Badge";
import type { Column } from "@/components/DataTable";
import { FormDialog } from "@/components/Dialog";
import { Notice, ProblemNotice } from "@/components/Feedback";
import { Checkbox, FormGrid, SelectField, TextField } from "@/components/Field";
import { useCan } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";
import { AdminList } from "./OrganisationList";

/** A row of any reference list: the union of the lookup DTOs. */
interface Row {
  id: string;
  name: string;
  is_active: boolean;
  sort_order?: number;
  is_default?: boolean;
  is_closed?: boolean;
  pauses_sla?: boolean;
  response_minutes?: number | null;
  resolution_minutes?: number | null;
  number_prefix?: string | null;
  colour?: string | null;
  is_retired?: boolean;
  parent_id?: string | null;
  system_key?: string | null;
}

type Flag = "is_default" | "is_closed" | "pauses_sla" | "is_retired";
type Extra = "targets" | "prefix" | "colour" | "parent";

interface Config {
  title: MessageId;
  intro: MessageId;
  capability: string;
  queryKey: string;
  list: () => Promise<Row[]>;
  save: (id: string | undefined, body: Record<string, unknown>) => Promise<unknown>;
  flags: readonly { key: Flag; label: MessageId; hint?: MessageId }[];
  extras: readonly Extra[];
  /** Calendar Categories have no order. */
  ordered: boolean;
}

/* The API checks each body against its own schema; this generic editor only shapes it. */
const lookup = (path: string) => ({
  list: async () => (await call(api.GET(path as "/api/v1/ticket-statuses"))) as Row[],
  save: (id: string | undefined, body: Record<string, unknown>) =>
    id
      ? call(
          api.PUT(`${path}/{id}` as "/api/v1/ticket-statuses/{id}", {
            params: { path: { id } },
            body: body as never,
          }),
        )
      : call(api.POST(path as "/api/v1/ticket-statuses", { body: body as never })),
});

const TICKET = {
  capability: "tickets.configure",
  intro: "ref.lookupIntro",
  ordered: true,
} as const;
const ASSET = { capability: "assets.configure", intro: "ref.lookupIntro", ordered: true } as const;
const CONTRACT = {
  capability: "contracts.configure",
  intro: "ref.contractIntro",
  ordered: true,
} as const;

const CONFIGS: Record<string, Config> = {
  "ticket-statuses": {
    ...TICKET,
    ...lookup("/api/v1/ticket-statuses"),
    title: "admin.ticketStatuses",
    queryKey: "ticket-statuses",
    flags: [
      { key: "is_default", label: "ref.isDefault" },
      { key: "is_closed", label: "ref.isClosed" },
      { key: "pauses_sla", label: "ref.pausesSla" },
    ],
    extras: [],
  },
  "ticket-priorities": {
    ...TICKET,
    ...lookup("/api/v1/ticket-priorities"),
    title: "admin.ticketPriorities",
    queryKey: "ticket-priorities",
    flags: [{ key: "is_default", label: "ref.isDefault" }],
    extras: ["targets"],
  },
  "ticket-types": {
    ...TICKET,
    ...lookup("/api/v1/ticket-types"),
    title: "admin.ticketTypes",
    queryKey: "ticket-types",
    flags: [{ key: "is_default", label: "ref.isDefault" }],
    extras: ["prefix"],
  },
  "ticket-origins": {
    ...TICKET,
    ...lookup("/api/v1/ticket-origins"),
    title: "admin.ticketOrigins",
    queryKey: "ticket-origins",
    flags: [{ key: "is_default", label: "ref.isDefault" }],
    extras: [],
  },
  "resolution-codes": {
    ...TICKET,
    ...lookup("/api/v1/resolution-codes"),
    title: "admin.resolutionCodes",
    queryKey: "resolution-codes",
    flags: [],
    extras: [],
  },
  "asset-types": {
    ...ASSET,
    ...lookup("/api/v1/asset-types"),
    title: "admin.assetTypes",
    queryKey: "asset-types",
    flags: [],
    extras: [],
  },
  "asset-statuses": {
    ...ASSET,
    ...lookup("/api/v1/asset-statuses"),
    title: "admin.assetStatuses",
    queryKey: "asset-statuses",
    flags: [{ key: "is_retired", label: "ref.retired", hint: "ref.retiredHint" }],
    extras: ["colour"],
  },
  "asset-locations": {
    ...ASSET,
    ...lookup("/api/v1/asset-locations"),
    title: "admin.assetLocations",
    intro: "admin.assetLocationsIntro",
    queryKey: "asset-locations",
    flags: [],
    extras: ["parent"],
  },
  "supplier-types": {
    ...CONTRACT,
    ...lookup("/api/v1/supplier-types"),
    title: "admin.supplierTypes",
    queryKey: "supplier-types",
    flags: [],
    extras: [],
  },
  "supplier-statuses": {
    ...CONTRACT,
    ...lookup("/api/v1/supplier-statuses"),
    title: "admin.supplierStatuses",
    queryKey: "supplier-statuses",
    flags: [],
    extras: [],
  },
  "contract-statuses": {
    ...CONTRACT,
    ...lookup("/api/v1/contract-statuses"),
    title: "admin.contractStatuses",
    queryKey: "contract-statuses",
    flags: [{ key: "is_closed", label: "ref.closedContract" }],
    extras: [],
  },
  "payment-schedules": {
    ...CONTRACT,
    ...lookup("/api/v1/payment-schedules"),
    title: "admin.paymentSchedules",
    queryKey: "payment-schedules",
    flags: [],
    extras: [],
  },
  "calendar-categories": {
    ...lookup("/api/v1/calendar/categories"),
    capability: "calendar.configure",
    intro: "admin.calendarCategoriesIntro",
    ordered: false,
    title: "admin.calendarCategories",
    queryKey: "calendar-categories",
    flags: [],
    extras: ["colour"],
  },
};

/** Every kind of reference data this page edits (the route paths under Administration). */
export const REFERENCE_KINDS = Object.keys(CONFIGS);

/**
 * Reference data (statuses, Priorities, types, …): one generic list and form, so every
 * lookup works the same way (H4). Rows are deactivated, never deleted.
 */
export function ReferencePage({ kind }: { kind: string }) {
  const config = CONFIGS[kind];
  if (!config) return null;
  return <ReferenceList key={kind} config={config} />;
}

function ReferenceList({ config }: { config: Config }) {
  const t = useText();
  const can = useCan();
  const rows = useQuery({ queryKey: ["reference", config.queryKey], queryFn: config.list });
  const byId = new Map((rows.data ?? []).map((r) => [r.id, r]));
  const sorted = [...(rows.data ?? [])].sort(
    (a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.name.localeCompare(b.name),
  );
  const columns: Column<Row>[] = [];
  if (config.extras.includes("parent")) {
    columns.push({
      key: "parent",
      header: t("ref.parent"),
      cell: (r) => (r.parent_id ? (byId.get(r.parent_id)?.name ?? "…") : t("ref.topLevel")),
    });
  }
  if (config.flags.length || config.extras.some((e) => e === "targets" || e === "prefix")) {
    columns.push({
      key: "props",
      header: t("ref.properties"),
      cell: (r) => (
        <span className="flex flex-wrap gap-1">
          {config.flags
            .filter((f) => r[f.key])
            .map((f) => (
              <Badge key={f.key} tone={f.key === "is_default" ? "accent" : "neutral"}>
                {f.key === "is_default" ? t("ref.default") : t(f.label)}
              </Badge>
            ))}
          {config.extras.includes("prefix") && r.number_prefix ? (
            <Badge tone="neutral">{r.number_prefix}</Badge>
          ) : null}
          {config.extras.includes("targets") && (r.response_minutes || r.resolution_minutes) ? (
            <span className="text-xs text-fg-muted">
              {t("ref.targets", {
                response: r.response_minutes ?? "—",
                resolution: r.resolution_minutes ?? "—",
              })}
            </span>
          ) : null}
        </span>
      ),
    });
  }
  if (config.ordered)
    columns.push({ key: "order", header: t("ref.order"), cell: (r) => r.sort_order ?? 0 });
  return (
    <AdminList
      title={t(config.title)}
      intro={t(config.intro)}
      rows={rows.data ? sorted : undefined}
      loading={rows.isPending}
      error={rows.error}
      columns={columns}
      canEdit={can(config.capability)}
      name={(r) => (
        <span className="flex flex-wrap items-center gap-1.5 font-medium">
          {config.extras.includes("colour") ? (
            <ColourLabel colour={r.colour}>{r.name}</ColourLabel>
          ) : (
            r.name
          )}
          {r.system_key ? <Badge tone="muted">{t("ref.system")}</Badge> : null}
        </span>
      )}
      renderEditor={(row, close) => (
        <RowDialog config={config} row={row} rows={rows.data ?? []} onClose={close} />
      )}
    />
  );
}

function RowDialog({
  config,
  row,
  rows,
  onClose,
}: {
  config: Config;
  row?: Row | undefined;
  rows: Row[];
  onClose: () => void;
}) {
  const t = useText();
  const [form, setForm] = useState({
    name: row?.name ?? "",
    sort_order: String(row?.sort_order ?? 0),
    is_active: row?.is_active ?? true,
    is_default: row?.is_default ?? false,
    is_closed: row?.is_closed ?? false,
    pauses_sla: row?.pauses_sla ?? false,
    is_retired: row?.is_retired ?? false,
    response_minutes: row?.response_minutes != null ? String(row.response_minutes) : "",
    resolution_minutes: row?.resolution_minutes != null ? String(row.resolution_minutes) : "",
    number_prefix: row?.number_prefix ?? "",
    colour: row?.colour ?? "#3d3ac4",
    parent_id: row?.parent_id ?? "",
  });
  const text = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const body = () => {
    const out: Record<string, unknown> = { name: form.name.trim(), is_active: form.is_active };
    if (config.ordered) out.sort_order = Number(form.sort_order) || 0;
    for (const f of config.flags) out[f.key] = form[f.key];
    if (config.extras.includes("targets")) {
      out.response_minutes = form.response_minutes ? Number(form.response_minutes) : null;
      out.resolution_minutes = form.resolution_minutes ? Number(form.resolution_minutes) : null;
    }
    if (config.extras.includes("prefix")) out.number_prefix = form.number_prefix.trim() || null;
    if (config.extras.includes("colour")) out.colour = form.colour;
    if (config.extras.includes("parent")) out.parent_id = form.parent_id || null;
    return out;
  };
  const save = useAction(() => config.save(row?.id, body()), {
    invalidate: [["reference", config.queryKey], ["calendar"]],
    success: () => t("ref.saved", { name: form.name.trim() }),
    onSuccess: onClose,
  });
  const system = Boolean(row?.system_key);
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={row ? t("ref.edit", { name: row.name }) : `${t("ref.add")}: ${t(config.title)}`}
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!form.name.trim()}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      {system ? (
        <Notice tone="info" live={false}>
          {t("ref.systemHint")}
        </Notice>
      ) : null}
      <FormGrid>
        <TextField
          label={t("ref.name")}
          required
          autoFocus
          value={form.name}
          onChange={text("name")}
        />
        {config.ordered ? (
          <TextField
            label={t("ref.order")}
            type="number"
            value={form.sort_order}
            onChange={text("sort_order")}
          />
        ) : null}
        {config.extras.includes("prefix") ? (
          <TextField
            label={t("ref.prefix")}
            hint={t("ref.prefixHint")}
            maxLength={8}
            value={form.number_prefix}
            onChange={text("number_prefix")}
          />
        ) : null}
        {config.extras.includes("targets") ? (
          <>
            <TextField
              label={t("ref.responseMinutes")}
              type="number"
              min={0}
              value={form.response_minutes}
              onChange={text("response_minutes")}
            />
            <TextField
              label={t("ref.resolutionMinutes")}
              type="number"
              min={0}
              value={form.resolution_minutes}
              onChange={text("resolution_minutes")}
            />
          </>
        ) : null}
        {config.extras.includes("colour") ? (
          <TextField
            label={t("ref.colour")}
            type="color"
            className="[&_input]:h-9 [&_input]:w-24 [&_input]:p-1"
            value={form.colour}
            onChange={text("colour")}
          />
        ) : null}
        {config.extras.includes("parent") ? (
          <SelectField
            label={t("ref.parent")}
            value={form.parent_id}
            placeholder={t("ref.topLevel")}
            onChange={text("parent_id")}
            options={rows
              .filter((r) => r.id !== row?.id && (r.is_active || r.id === row?.parent_id))
              .map((r) => ({ value: r.id, label: r.name }))}
          />
        ) : null}
      </FormGrid>
      <div className="grid gap-3">
        {config.flags.map((f) => (
          <Checkbox
            key={f.key}
            label={t(f.label)}
            hint={f.hint ? t(f.hint) : undefined}
            checked={form[f.key]}
            onChange={(e) => setForm((x) => ({ ...x, [f.key]: e.target.checked }))}
          />
        ))}
        <Checkbox
          label={t("common.active")}
          checked={form.is_active}
          disabled={system}
          onChange={(e) => setForm((x) => ({ ...x, is_active: e.target.checked }))}
        />
      </div>
    </FormDialog>
  );
}
