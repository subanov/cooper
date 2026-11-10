import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import {
  costCentreRowsQuery,
  createCostCentre,
  updateCostCentre,
} from "@/features/administration/resources";
import { problemText } from "@/features/result";
import { companiesQuery, defaultCompanyId } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { CheckField, FormActions, ResourceAdmin, useFlash, useInvalidateAdmin } from "./shared";

type CostCentre = Schemas["CostCentreDto"];

/** Cost Centres of a Company. */
export function CostCentresPage() {
  const t = useText();
  const rows = useQuery(costCentreRowsQuery);
  const companies = useQuery(companiesQuery);
  const companyName = new Map((companies.data ?? []).map((c) => [c.id, c.name]));
  return (
    <ResourceAdmin
      title={t("admin.costCentres")}
      intro={t("admin.costCentres.intro")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: CostCentre) => r.id}
      getName={(r) => r.name}
      addLabel={t("admin.costCentres.add")}
      columns={[
        { header: t("admin.costCentres.code"), cell: (r) => r.code },
        { header: t("admin.res.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
        { header: t("users.company"), cell: (r) => companyName.get(r.company_id) ?? "" },
        {
          header: t("admin.res.status"),
          cell: (r) => (
            <Badge tone={r.is_active ? "ok" : "muted"}>
              {t(r.is_active ? "admin.res.active" : "admin.res.inactive")}
            </Badge>
          ),
        },
      ]}
      renderForm={(row, done) => <CostCentreForm row={row} done={done} />}
    />
  );
}

function CostCentreForm({ row, done }: { row: CostCentre | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const companies = useQuery(companiesQuery);
  const [code, setCode] = useState(row?.code ?? "");
  const [name, setName] = useState(row?.name ?? "");
  const [companyId, setCompanyId] = useState("");
  const [active, setActive] = useState(row?.is_active ?? true);
  const company = companyId || defaultCompanyId(companies.data);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["CostCentreRequest"] = {
      code: code.trim(),
      name: name.trim(),
      is_active: active,
      ...(!row && company ? { company_id: company } : {}),
    };
    const outcome = row ? await updateCostCentre(row.id, body) : await createCostCentre(body);
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
        label={t("admin.costCentres.code")}
        value={code}
        onChange={(e) => setCode(e.target.value)}
        required
      />
      <Field
        label={t("admin.res.name")}
        value={name}
        onChange={(e) => setName(e.target.value)}
        required
      />
      {row ? null : (
        <SelectField
          label={t("users.company")}
          value={company}
          onChange={(e) => setCompanyId(e.target.value)}
        >
          {(companies.data ?? []).map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </SelectField>
      )}
      <CheckField label={t("admin.res.active")} checked={active} onChange={setActive} />
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
    </form>
  );
}
