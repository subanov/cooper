import { useQuery } from "@tanstack/react-query";
import { ClipboardList, Pencil, Plus } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { Card } from "@/components/Card";
import { DataTable } from "@/components/DataTable";
import { FormDialog } from "@/components/Dialog";
import { EmptyState } from "@/components/EmptyState";
import { Checkbox, FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import {
  analystsQuery,
  companiesQuery,
  namesById,
  teamsQuery,
  toOptions,
} from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { checkGroupsQuery, checksQuery } from "./api";

type Check = Schemas["CheckDto"];

/** The Checks of the Round, active or not, in Round order (FR-MCK-001). */
export function ChecksPage() {
  const t = useText();
  const can = useCan();
  const checks = useQuery(checksQuery);
  const analysts = useQuery(analystsQuery);
  const teams = useQuery(teamsQuery);
  const [editing, setEditing] = useState<Check | "new" | null>(null);
  const editable = can("morning-checks.configure");
  const routing = (c: Check) =>
    c.routing.analyst_id
      ? namesById(analysts.data, (a) => a.full_name).get(c.routing.analyst_id)
      : c.routing.team_id
        ? namesById(teams.data).get(c.routing.team_id)
        : undefined;
  return (
    <div className="grid gap-4">
      <SectionHeader
        title={t("checks.checks")}
        actions={
          editable ? (
            <Button
              variant="primary"
              icon={<Plus aria-hidden="true" className="size-4" />}
              onClick={() => setEditing("new")}
            >
              {t("checks.addCheck")}
            </Button>
          ) : undefined
        }
      />
      <ProblemNotice error={checks.error} />
      <Card flush>
        <DataTable
          caption={t("checks.list")}
          rows={checks.data}
          loading={checks.isPending}
          rowKey={(c) => c.id}
          empty={
            <EmptyState icon={<ClipboardList aria-hidden="true" />} title={t("checks.emptyList")} />
          }
          columns={[
            {
              id: "name",
              header: t("checks.name"),
              rowHeader: true,
              cell: (c) => (
                <span className="grid">
                  <span className="font-medium">{c.name}</span>
                  {c.description ? (
                    <span className="text-xs text-fg-muted">{c.description}</span>
                  ) : null}
                </span>
              ),
            },
            {
              id: "group",
              header: t("checks.group"),
              cell: (c) => c.check_group_name ?? t("common.noneDash"),
            },
            {
              id: "routing",
              header: t("checks.routing"),
              wide: true,
              cell: (c) => routing(c) ?? t("common.noneDash"),
            },
            {
              id: "round",
              header: t("common.status"),
              cell: (c) =>
                c.in_round ? (
                  <Badge tone="success">{t("checks.inRound")}</Badge>
                ) : (
                  <Badge tone="muted">
                    {c.is_active ? t("checks.notInRound") : t("common.inactive")}
                  </Badge>
                ),
            },
            ...(editable
              ? [
                  {
                    id: "edit",
                    header: <span className="sr-only">{t("common.actions")}</span>,
                    className: "w-12 text-right",
                    cell: (c: Check) => (
                      <IconButton
                        size="sm"
                        tooltip="top"
                        label={t("checks.editCheck", { name: c.name })}
                        icon={<Pencil aria-hidden="true" className="size-3.5" />}
                        onClick={() => setEditing(c)}
                      />
                    ),
                  },
                ]
              : []),
          ]}
        />
      </Card>
      {editing ? (
        <CheckDialog
          check={editing === "new" ? undefined : editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
    </div>
  );
}

function CheckDialog({ check, onClose }: { check?: Check | undefined; onClose: () => void }) {
  const t = useText();
  const groups = useQuery(checkGroupsQuery);
  const analysts = useQuery(analystsQuery);
  const companies = useQuery(companiesQuery);
  const [form, setForm] = useState({
    name: check?.name ?? "",
    description: check?.description ?? "",
    check_group_id: check?.check_group_id ?? "",
    assigned_analyst_id: check?.assigned_analyst_id ?? "",
    sort_order: String(check?.sort_order ?? 0),
    is_active: check?.is_active ?? true,
    company_id: check?.company_id ?? "",
  });
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const body = (): Schemas["CheckRequest"] => ({
    name: form.name.trim(),
    description: form.description,
    check_group_id: form.check_group_id || null,
    assigned_analyst_id: form.assigned_analyst_id || null,
    sort_order: Number(form.sort_order) || 0,
    is_active: form.is_active,
    ...(check ? {} : { company_id: form.company_id || null }),
  });
  const save = useAction(
    () =>
      check
        ? call(
            api.PUT("/api/v1/morning-checks/checks/{id}", {
              params: { path: { id: check.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/morning-checks/checks", { body: body() })),
    { invalidate: [["morning-checks"]], success: "checks.saved", onSuccess: onClose },
  );
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={check ? t("checks.editCheck", { name: check.name }) : t("checks.addCheck")}
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!form.name.trim()}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <TextField label={t("checks.name")} required value={form.name} onChange={set("name")} />
      <TextAreaField
        label={t("common.description")}
        rows={2}
        value={form.description}
        onChange={set("description")}
      />
      <FormGrid>
        {!check && activeCompanies.length > 1 ? (
          <SelectField
            label={t("common.company")}
            value={form.company_id}
            placeholder={t("common.choose")}
            onChange={set("company_id")}
            options={activeCompanies.map((c) => ({ value: c.id, label: c.name }))}
          />
        ) : null}
        <SelectField
          label={t("checks.group")}
          value={form.check_group_id}
          placeholder={t("checks.noGroup")}
          onChange={set("check_group_id")}
          options={toOptions(groups.data, (g) => g.name, check?.check_group_id)}
        />
        <SelectField
          label={t("checks.analyst")}
          hint={t("checks.analystHint")}
          value={form.assigned_analyst_id}
          placeholder={t("common.none")}
          onChange={set("assigned_analyst_id")}
          options={toOptions(analysts.data, (a) => a.full_name, check?.assigned_analyst_id)}
        />
        <TextField
          label={t("common.order")}
          type="number"
          value={form.sort_order}
          onChange={set("sort_order")}
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
