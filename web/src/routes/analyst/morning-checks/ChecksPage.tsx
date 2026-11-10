import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import {
  checkGroupsQuery,
  checksQuery,
  createCheck,
  updateCheck,
} from "@/features/morning-checks/api";
import { problemText } from "@/features/result";
import { companiesQuery, defaultCompanyId } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import {
  CheckField,
  FormActions,
  ResourceAdmin,
  useFlash,
  useInvalidateAdmin,
} from "../administration/resources/shared";

type Check = Schemas["CheckDto"];

/** The Checks themselves: name, group, order, active. */
export function ChecksPage() {
  const t = useText();
  const rows = useQuery(checksQuery);
  return (
    <ResourceAdmin
      title={t("checks.checks")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Check) => r.id}
      getName={(r) => r.name}
      addLabel={t("checks.addCheck")}
      columns={[
        { header: t("admin.res.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
        { header: t("checks.group"), cell: (r) => r.check_group_name ?? "" },
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
      renderForm={(row, done) => <CheckForm row={row} done={done} />}
    />
  );
}

function CheckForm({ row, done }: { row: Check | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const groups = useQuery(checkGroupsQuery);
  const companies = useQuery(companiesQuery);
  const [name, setName] = useState(row?.name ?? "");
  const [description, setDescription] = useState(row?.description ?? "");
  const [groupId, setGroupId] = useState(row?.check_group_id ?? "");
  const [order, setOrder] = useState(String(row?.sort_order ?? 0));
  const [active, setActive] = useState(row?.is_active ?? true);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const company = defaultCompanyId(companies.data);
    const body: Schemas["CheckRequest"] = {
      name: name.trim(),
      description: description.trim(),
      check_group_id: groupId || null,
      sort_order: Number(order) || 0,
      is_active: active,
      ...(!row && company ? { company_id: company } : {}),
    };
    const outcome = row ? await updateCheck(row.id, body) : await createCheck(body);
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
        label={t("admin.res.description")}
        value={description}
        onChange={(e) => setDescription(e.target.value)}
      />
      <SelectField
        label={t("checks.group")}
        value={groupId}
        onChange={(e) => setGroupId(e.target.value)}
      >
        <option value="">{t("checks.noGroup")}</option>
        {(groups.data ?? []).map((g) => (
          <option key={g.id} value={g.id}>
            {g.name}
          </option>
        ))}
      </SelectField>
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
