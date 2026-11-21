import { useQuery } from "@tanstack/react-query";
import { Activity, Pencil, Plus } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { type Column, DataTable } from "@/components/DataTable";
import { FormDialog } from "@/components/Dialog";
import { EmptyState, ProblemNotice } from "@/components/Feedback";
import { Checkbox, FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { Page, PageHeader, Panel } from "@/components/Layout";
import { companiesQuery, namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { servicesQuery } from "./api";

type Service = Schemas["StatusServiceDto"];

/** The Services the board shows (FR-SST-001); a few fields, so they are edited in place. */
export function ServicesPage() {
  const t = useText();
  const can = useCan();
  const services = useQuery(servicesQuery);
  const companies = useQuery(companiesQuery);
  const [editing, setEditing] = useState<Service | "new" | null>(null);
  const editable = can("service-status.configure");
  const columns: Column<Service>[] = [
    {
      key: "name",
      header: t("common.name"),
      cell: (s) => (
        <span className="grid">
          <span className="font-medium">{s.name}</span>
          {s.description ? <span className="text-xs text-fg-muted">{s.description}</span> : null}
        </span>
      ),
    },
    {
      key: "company",
      header: t("common.company"),
      cell: (s) =>
        s.company_id ? namesById(companies.data).get(s.company_id) : t("status.shared"),
    },
    { key: "order", header: t("status.sortOrder"), cell: (s) => s.sort_order },
    {
      key: "active",
      header: t("common.status"),
      cell: (s) =>
        s.is_active ? (
          <Badge tone="success">{t("common.active")}</Badge>
        ) : (
          <Badge tone="muted">{t("common.inactive")}</Badge>
        ),
    },
  ];
  if (editable) {
    columns.push({
      key: "edit",
      header: t("common.actions"),
      hideHeader: true,
      className: "w-12 text-right",
      cell: (s) => (
        <IconButton
          size="sm"
          tooltip="top"
          label={t("status.editService", { name: s.name })}
          icon={<Pencil aria-hidden="true" />}
          onClick={() => setEditing(s)}
        />
      ),
    });
  }
  return (
    <Page>
      <PageHeader
        level={2}
        title={t("status.services")}
        actions={
          editable ? (
            <Button
              variant="primary"
              icon={<Plus aria-hidden="true" />}
              onClick={() => setEditing("new")}
            >
              {t("status.addService")}
            </Button>
          ) : undefined
        }
      />
      <ProblemNotice error={services.error} />
      <Panel flush>
        <DataTable
          caption={t("status.services")}
          rows={
            services.data
              ? [...services.data].sort((a, b) => a.sort_order - b.sort_order)
              : undefined
          }
          loading={services.isPending}
          rowKey={(s) => s.id}
          empty={
            <EmptyState icon={<Activity aria-hidden="true" />} title={t("status.noServices")} />
          }
          columns={columns}
        />
      </Panel>
      {editing ? (
        <ServiceDialog
          service={editing === "new" ? undefined : editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
    </Page>
  );
}

function ServiceDialog({
  service,
  onClose,
}: {
  service?: Service | undefined;
  onClose: () => void;
}) {
  const t = useText();
  const companies = useQuery(companiesQuery);
  const [form, setForm] = useState({
    name: service?.name ?? "",
    description: service?.description ?? "",
    company_id: service?.company_id ?? "",
    sort_order: String(service?.sort_order ?? 0),
    is_active: service?.is_active ?? true,
  });
  const body = (): Schemas["StatusServiceRequest"] => ({
    name: form.name.trim(),
    description: form.description,
    company_id: form.company_id || null,
    sort_order: Number(form.sort_order) || 0,
    is_active: form.is_active,
  });
  const save = useAction(
    () =>
      service
        ? call(
            api.PUT("/api/v1/service-status/services/{id}", {
              params: { path: { id: service.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/service-status/services", { body: body() })),
    { invalidate: [["service-status"]], success: "status.serviceSaved", onSuccess: onClose },
  );
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={service ? t("status.editService", { name: service.name }) : t("status.addService")}
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!form.name.trim()}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <TextField
        label={t("common.name")}
        required
        autoFocus
        value={form.name}
        onChange={(e) => setForm((x) => ({ ...x, name: e.target.value }))}
      />
      <TextAreaField
        label={t("common.description")}
        rows={3}
        value={form.description}
        onChange={(e) => setForm((x) => ({ ...x, description: e.target.value }))}
      />
      <FormGrid>
        <SelectField
          label={t("common.company")}
          hint={t("status.companyHint")}
          value={form.company_id}
          placeholder={t("status.shared")}
          disabled={Boolean(service)}
          onChange={(e) => setForm((x) => ({ ...x, company_id: e.target.value }))}
          options={(companies.data ?? [])
            .filter((c) => c.is_active)
            .map((c) => ({ value: c.id, label: c.name }))}
        />
        <TextField
          label={t("status.sortOrder")}
          type="number"
          value={form.sort_order}
          onChange={(e) => setForm((x) => ({ ...x, sort_order: e.target.value }))}
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
