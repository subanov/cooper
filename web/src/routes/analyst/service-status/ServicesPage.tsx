import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import { problemText } from "@/features/result";
import { createService, servicesQuery, updateService } from "@/features/service-status/api";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import {
  CheckField,
  FormActions,
  ResourceAdmin,
  useFlash,
  useInvalidateAdmin,
} from "../administration/resources/shared";

type Service = Schemas["StatusServiceDto"];

/** The services shown on the status page. */
export function ServicesPage() {
  const t = useText();
  const rows = useQuery(servicesQuery);
  return (
    <ResourceAdmin
      title={t("status.services")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Service) => r.id}
      getName={(r) => r.name}
      addLabel={t("status.addService")}
      columns={[
        { header: t("admin.res.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
        { header: t("admin.res.description"), cell: (r) => r.description },
        {
          header: t("admin.res.status"),
          cell: (r) => (
            <Badge tone={r.is_active ? "ok" : "muted"}>
              {t(r.is_active ? "admin.res.active" : "admin.res.inactive")}
            </Badge>
          ),
        },
      ]}
      renderForm={(row, done) => <ServiceForm row={row} done={done} />}
    />
  );
}

function ServiceForm({ row, done }: { row: Service | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const [name, setName] = useState(row?.name ?? "");
  const [description, setDescription] = useState(row?.description ?? "");
  const [order, setOrder] = useState(String(row?.sort_order ?? 0));
  const [active, setActive] = useState(row?.is_active ?? true);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["StatusServiceRequest"] = {
      name: name.trim(),
      description: description.trim(),
      sort_order: Number(order) || 0,
      is_active: active,
    };
    const outcome = row ? await updateService(row.id, body) : await createService(body);
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
