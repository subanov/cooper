import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import {
  type ContractLookupKind,
  contractLookupQuery,
  createContractLookup,
  updateContractLookup,
} from "@/features/contracts/api";
import { problemText } from "@/features/result";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { CheckField, FormActions, ResourceAdmin, useFlash, useInvalidateAdmin } from "./shared";

type Lookup = Schemas["ContractLookupDto"];

const KINDS: Record<ContractLookupKind, { title: MessageId; add: MessageId }> = {
  "supplier-types": { title: "admin.supplierTypes", add: "admin.supplierTypes.add" },
  "supplier-statuses": { title: "admin.supplierStatuses", add: "admin.supplierStatuses.add" },
  "contract-statuses": { title: "admin.contractStatuses", add: "admin.contractStatuses.add" },
  "payment-schedules": { title: "admin.paymentSchedules", add: "admin.paymentSchedules.add" },
};

/** Edits one Contracts lookup: Supplier Types and Statuses, Contract Statuses, Payment Schedules. */
export function ContractLookupPage({ kind }: { kind: ContractLookupKind }) {
  const t = useText();
  const config = KINDS[kind];
  const rows = useQuery(contractLookupQuery(kind));
  return (
    <ResourceAdmin
      title={t(config.title)}
      intro={t("admin.contractLookup.intro")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Lookup) => r.id}
      getName={(r) => r.name}
      addLabel={t(config.add)}
      columns={[
        { header: t("admin.res.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
        {
          header: t("admin.lookup.properties"),
          cell: (r) => (r.is_closed ? <Badge>{t("admin.contractLookup.isClosed")}</Badge> : null),
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
  kind: ContractLookupKind;
  row: Lookup | null;
  done: () => void;
}) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const [name, setName] = useState(row?.name ?? "");
  const [order, setOrder] = useState(String(row?.sort_order ?? 0));
  const [active, setActive] = useState(row?.is_active ?? true);
  const [closed, setClosed] = useState(row?.is_closed ?? false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["ContractLookupRequest"] = {
      name: name.trim(),
      sort_order: Number(order) || 0,
      is_active: active,
      is_closed: kind === "contract-statuses" ? closed : false,
    };
    const outcome = row
      ? await updateContractLookup(kind, row.id, body)
      : await createContractLookup(kind, body);
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
      {kind === "contract-statuses" ? (
        <CheckField
          label={t("admin.contractLookup.isClosed")}
          checked={closed}
          onChange={setClosed}
        />
      ) : null}
      <CheckField label={t("admin.res.active")} checked={active} onChange={setActive} />
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
    </form>
  );
}
