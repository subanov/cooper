import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import {
  createLookup,
  type LookupKind,
  lookupRowsQuery,
  updateLookup,
} from "@/features/administration/resources";
import { problemText } from "@/features/result";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { CheckField, FormActions, ResourceAdmin, useFlash, useInvalidateAdmin } from "./shared";

type Lookup = Schemas["LookupDto"];

interface KindConfig {
  title: MessageId;
  add: MessageId;
  flags: readonly ("is_default" | "is_closed" | "pauses_sla")[];
  targets?: true;
  prefix?: true;
}

const KINDS: Record<LookupKind, KindConfig> = {
  "ticket-statuses": {
    title: "admin.statuses",
    add: "admin.statuses.add",
    flags: ["is_default", "is_closed", "pauses_sla"],
  },
  "ticket-priorities": {
    title: "admin.priorities",
    add: "admin.priorities.add",
    flags: ["is_default"],
    targets: true,
  },
  "ticket-types": { title: "admin.types", add: "admin.types.add", flags: [], prefix: true },
  "ticket-origins": { title: "admin.origins", add: "admin.origins.add", flags: [] },
  "resolution-codes": { title: "admin.codes", add: "admin.codes.add", flags: [] },
};

const FLAG_LABELS: Record<string, MessageId> = {
  is_default: "admin.lookup.isDefault",
  is_closed: "admin.lookup.isClosed",
  pauses_sla: "admin.lookup.pausesSla",
};

/** Edits one kind of ticket reference data: Statuses, Priorities, Types, Origins, Codes. */
export function LookupAdminPage({ kind }: { kind: LookupKind }) {
  const t = useText();
  const config = KINDS[kind];
  const rows = useQuery(lookupRowsQuery(kind));
  const sorted = [...(rows.data ?? [])].sort((a, b) => a.sort_order - b.sort_order);
  return (
    <ResourceAdmin
      title={t(config.title)}
      intro={t("admin.lookup.intro")}
      rows={rows.data ? sorted : undefined}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Lookup) => r.id}
      getName={(r) => r.name}
      addLabel={t(config.add)}
      columns={[
        { header: t("admin.res.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
        {
          header: t("admin.lookup.properties"),
          cell: (r) => (
            <span className="flex flex-wrap gap-1">
              {config.flags
                .filter((f) => r[f])
                .map((f) => (
                  <Badge key={f}>{t(FLAG_LABELS[f] as MessageId)}</Badge>
                ))}
              {config.targets && (r.response_minutes ?? r.resolution_minutes) !== null ? (
                <Badge>
                  {t("admin.lookup.targetsSummary", {
                    response: r.response_minutes ?? "–",
                    resolution: r.resolution_minutes ?? "–",
                  })}
                </Badge>
              ) : null}
              {config.prefix && r.number_prefix ? <Badge>{r.number_prefix}</Badge> : null}
            </span>
          ),
        },
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
      renderForm={(row, done) => <LookupForm kind={kind} row={row} done={done} />}
    />
  );
}

function LookupForm({
  kind,
  row,
  done,
}: {
  kind: LookupKind;
  row: Lookup | null;
  done: () => void;
}) {
  const t = useText();
  const config = KINDS[kind];
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const [name, setName] = useState(row?.name ?? "");
  const [order, setOrder] = useState(String(row?.sort_order ?? 0));
  const [active, setActive] = useState(row?.is_active ?? true);
  const [flags, setFlags] = useState({
    is_default: row?.is_default ?? false,
    is_closed: row?.is_closed ?? false,
    pauses_sla: row?.pauses_sla ?? false,
  });
  const [response, setResponse] = useState(String(row?.response_minutes ?? ""));
  const [resolution, setResolution] = useState(String(row?.resolution_minutes ?? ""));
  const [prefix, setPrefix] = useState(row?.number_prefix ?? "");

  async function submit(event: FormEvent) {
    event.preventDefault();
    const minutes = (v: string) => (v.trim() === "" ? null : Number(v));
    const body: Schemas["LookupRequest"] = {
      name: name.trim(),
      sort_order: Number(order) || 0,
      is_active: active,
      is_default: config.flags.includes("is_default")
        ? flags.is_default
        : (row?.is_default ?? false),
      is_closed: config.flags.includes("is_closed") ? flags.is_closed : (row?.is_closed ?? false),
      pauses_sla: config.flags.includes("pauses_sla")
        ? flags.pauses_sla
        : (row?.pauses_sla ?? false),
      number_prefix: config.prefix ? prefix.trim() || null : (row?.number_prefix ?? null),
      response_minutes: config.targets ? minutes(response) : (row?.response_minutes ?? null),
      resolution_minutes: config.targets ? minutes(resolution) : (row?.resolution_minutes ?? null),
      sla_calendar_id: row?.sla_calendar_id ?? null,
    };
    const outcome = row ? await updateLookup(kind, row.id, body) : await createLookup(kind, body);
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
      <Field
        label={t("admin.res.order")}
        type="number"
        value={order}
        onChange={(e) => setOrder(e.target.value)}
      />
      {config.flags.map((f) => (
        <CheckField
          key={f}
          label={t(FLAG_LABELS[f] as MessageId)}
          checked={flags[f]}
          onChange={(c) => setFlags({ ...flags, [f]: c })}
        />
      ))}
      {config.targets ? (
        <div className="grid gap-3 sm:grid-cols-2">
          <Field
            label={t("admin.lookup.responseMinutes")}
            type="number"
            min={0}
            value={response}
            onChange={(e) => setResponse(e.target.value)}
          />
          <Field
            label={t("admin.lookup.resolutionMinutes")}
            type="number"
            min={0}
            value={resolution}
            onChange={(e) => setResolution(e.target.value)}
          />
        </div>
      ) : null}
      {config.prefix ? (
        <Field
          label={t("admin.lookup.prefix")}
          hint={t("admin.lookup.prefixHint")}
          value={prefix}
          onChange={(e) => setPrefix(e.target.value)}
        />
      ) : null}
      <CheckField label={t("admin.res.active")} checked={active} onChange={setActive} />
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
    </form>
  );
}
