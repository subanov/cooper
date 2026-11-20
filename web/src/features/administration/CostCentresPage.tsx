import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { FormDialog } from "@/components/Dialog";
import { Checkbox, FormGrid, SelectField, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { companiesQuery, costCentresQuery, namesById } from "@/features/reference/queries";
import { fieldError, useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { OrganisationList } from "./OrganisationPages";

type CostCentre = Schemas["CostCentreDto"];

/** Cost Centres of each Company (FR-IAM-009). */
export function CostCentresPage() {
  const t = useText();
  const rows = useQuery(costCentresQuery);
  const companies = useQuery(companiesQuery);
  return (
    <OrganisationList
      title={t("admin.costCentres")}
      intro={t("admin.costCentres.intro")}
      rows={rows.data}
      loading={rows.isPending}
      error={rows.error}
      columns={[
        {
          id: "code",
          header: t("admin.costCentres.code"),
          cell: (c) => <span className="font-mono">{c.code}</span>,
        },
        {
          id: "company",
          header: t("common.company"),
          wide: true,
          cell: (c) => namesById(companies.data).get(c.company_id) ?? "…",
        },
      ]}
      renderEditor={(row, close) => <CostCentreDialog costCentre={row} onClose={close} />}
    />
  );
}

function CostCentreDialog({
  costCentre,
  onClose,
}: {
  costCentre?: CostCentre | undefined;
  onClose: () => void;
}) {
  const t = useText();
  const companies = useQuery(companiesQuery);
  const [form, setForm] = useState({
    name: costCentre?.name ?? "",
    code: costCentre?.code ?? "",
    company_id: costCentre?.company_id ?? "",
    is_active: costCentre?.is_active ?? true,
  });
  const body = (): Schemas["CostCentreRequest"] => ({
    name: form.name.trim(),
    code: form.code.trim(),
    company_id: form.company_id || null,
    is_active: form.is_active,
  });
  const save = useAction(
    () =>
      costCentre
        ? call(
            api.PUT("/api/v1/cost-centres/{id}", {
              params: { path: { id: costCentre.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/cost-centres", { body: body() })),
    {
      invalidate: [["reference", "cost-centres"]],
      success: () => t("ref.saved", { name: form.name.trim() }),
      onSuccess: onClose,
    },
  );
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={costCentre ? t("ref.edit", { name: costCentre.name }) : t("ref.add")}
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!form.name.trim() || !form.code.trim()}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <FormGrid>
        <TextField
          label={t("ref.name")}
          required
          value={form.name}
          onChange={(e) => setForm((x) => ({ ...x, name: e.target.value }))}
        />
        <TextField
          label={t("admin.costCentres.code")}
          required
          value={form.code}
          error={fieldError(save.error, "code")}
          onChange={(e) => setForm((x) => ({ ...x, code: e.target.value }))}
        />
        <SelectField
          label={t("common.company")}
          required
          disabled={Boolean(costCentre)}
          value={form.company_id}
          placeholder={t("common.choose")}
          onChange={(e) => setForm((x) => ({ ...x, company_id: e.target.value }))}
          options={(companies.data ?? [])
            .filter((c) => c.is_active || c.id === costCentre?.company_id)
            .map((c) => ({ value: c.id, label: c.name }))}
        />
      </FormGrid>
      <Checkbox
        label={t("common.active")}
        checked={form.is_active}
        onChange={(e) => setForm((x) => ({ ...x, is_active: e.target.checked }))}
      />
    </FormDialog>
  );
}
