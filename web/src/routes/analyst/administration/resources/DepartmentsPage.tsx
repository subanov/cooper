import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import {
  createDepartment,
  departmentRowsQuery,
  teamRowsQuery,
  updateDepartment,
} from "@/features/administration/resources";
import { problemText } from "@/features/result";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import {
  CheckField,
  CheckList,
  FormActions,
  ResourceAdmin,
  useFlash,
  useInvalidateAdmin,
} from "./shared";

type Department = Schemas["DepartmentDto"];

/** Departments: which Teams serve them and their default Sensitivity. */
export function DepartmentsPage() {
  const t = useText();
  const rows = useQuery(departmentRowsQuery);
  return (
    <ResourceAdmin
      title={t("admin.departments")}
      intro={t("admin.departments.intro")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Department) => r.id}
      getName={(r) => r.name}
      addLabel={t("admin.departments.add")}
      columns={[
        { header: t("admin.res.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
        {
          header: t("newTicket.sensitivity"),
          cell: (r) =>
            t(
              r.default_sensitivity === "confidential"
                ? "tickets.confidential"
                : "newTicket.normal",
            ),
        },
        {
          header: t("admin.res.status"),
          cell: (r) => (
            <Badge tone={r.is_active ? "ok" : "muted"}>
              {t(r.is_active ? "admin.res.active" : "admin.res.inactive")}
            </Badge>
          ),
        },
      ]}
      renderForm={(row, done) => <DepartmentForm row={row} done={done} />}
    />
  );
}

function DepartmentForm({ row, done }: { row: Department | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const teams = useQuery(teamRowsQuery);
  const [name, setName] = useState(row?.name ?? "");
  const [sensitivity, setSensitivity] = useState(row?.default_sensitivity ?? "normal");
  const [active, setActive] = useState(row?.is_active ?? true);
  const [teamIds, setTeamIds] = useState(new Set(row?.team_ids ?? []));

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["DepartmentRequest"] = {
      name: name.trim(),
      default_sensitivity: sensitivity,
      is_active: active,
      team_ids: [...teamIds],
    };
    const outcome = row ? await updateDepartment(row.id, body) : await createDepartment(body);
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
      <SelectField
        label={t("admin.departments.defaultSensitivity")}
        value={sensitivity}
        onChange={(e) => setSensitivity(e.target.value)}
      >
        <option value="normal">{t("newTicket.normal")}</option>
        <option value="confidential">{t("tickets.confidential")}</option>
      </SelectField>
      <CheckField label={t("admin.res.active")} checked={active} onChange={setActive} />
      <CheckList
        legend={t("admin.departments.teams")}
        options={(teams.data ?? []).map((x) => ({ id: x.id, label: x.name }))}
        selected={teamIds}
        onChange={setTeamIds}
      />
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
    </form>
  );
}
