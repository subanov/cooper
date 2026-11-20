import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { CheckboxList } from "@/components/CheckboxList";
import { FormDialog } from "@/components/Dialog";
import { Checkbox, SelectField, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { departmentsQuery, namesById, teamsQuery } from "@/features/reference/queries";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { OrganisationList } from "./OrganisationPages";

type Department = Schemas["DepartmentDto"];

/** Departments: default Sensitivity and the Teams that serve them (FR-IAM-010). */
export function DepartmentsPage() {
  const t = useText();
  const rows = useQuery(departmentsQuery);
  const teams = useQuery(teamsQuery);
  return (
    <OrganisationList
      title={t("admin.departments")}
      intro={t("admin.departments.intro")}
      rows={rows.data}
      loading={rows.isPending}
      error={rows.error}
      columns={[
        {
          id: "sensitivity",
          header: t("admin.departments.defaultSensitivity"),
          cell: (d) =>
            t(
              d.default_sensitivity === "confidential"
                ? "ticket.sensitivity.confidential"
                : "ticket.sensitivity.normal",
            ),
        },
        {
          id: "teams",
          header: t("admin.departments.teams"),
          wide: true,
          cell: (d) =>
            d.team_ids.map((id) => namesById(teams.data).get(id) ?? "…").join(", ") ||
            t("common.noneDash"),
        },
      ]}
      renderEditor={(row, close) => <DepartmentDialog department={row} onClose={close} />}
    />
  );
}

function DepartmentDialog({
  department,
  onClose,
}: {
  department?: Department | undefined;
  onClose: () => void;
}) {
  const t = useText();
  const teams = useQuery(teamsQuery);
  const [form, setForm] = useState({
    name: department?.name ?? "",
    default_sensitivity: department?.default_sensitivity ?? "normal",
    is_active: department?.is_active ?? true,
    team_ids: department?.team_ids ?? [],
  });
  const body = (): Schemas["DepartmentRequest"] => ({ ...form, name: form.name.trim() });
  const save = useAction(
    () =>
      department
        ? call(
            api.PUT("/api/v1/departments/{id}", {
              params: { path: { id: department.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/departments", { body: body() })),
    {
      invalidate: [["reference", "departments"]],
      success: () => t("ref.saved", { name: form.name.trim() }),
      onSuccess: onClose,
    },
  );
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={department ? t("ref.edit", { name: department.name }) : t("ref.add")}
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!form.name.trim()}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <TextField
        label={t("ref.name")}
        required
        value={form.name}
        onChange={(e) => setForm((x) => ({ ...x, name: e.target.value }))}
      />
      <SelectField
        label={t("admin.departments.defaultSensitivity")}
        value={form.default_sensitivity}
        onChange={(e) => setForm((x) => ({ ...x, default_sensitivity: e.target.value }))}
        options={[
          { value: "normal", label: t("ticket.sensitivity.normal") },
          { value: "confidential", label: t("ticket.sensitivity.confidential") },
        ]}
      />
      <CheckboxList
        legend={t("admin.departments.teams")}
        items={(teams.data ?? [])
          .filter((q) => q.is_active)
          .map((q) => ({ id: q.id, label: q.name }))}
        selected={form.team_ids}
        onChange={(team_ids) => setForm((x) => ({ ...x, team_ids }))}
      />
      <Checkbox
        label={t("common.active")}
        checked={form.is_active}
        onChange={(e) => setForm((x) => ({ ...x, is_active: e.target.checked }))}
      />
    </FormDialog>
  );
}
