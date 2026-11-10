import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import {
  companyRowsQuery,
  createCompany,
  setCompanyActive,
  updateCompany,
} from "@/features/administration/resources";
import { problemText } from "@/features/result";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { FormActions, ResourceAdmin, useFlash, useInvalidateAdmin } from "./shared";

type Company = Schemas["CompanyDto"];

/** Companies (tenants): name, Ticket Code, Sender Domains, active state. */
export function CompaniesPage() {
  const t = useText();
  const rows = useQuery(companyRowsQuery);
  return (
    <ResourceAdmin
      title={t("admin.companies")}
      intro={t("admin.companies.intro")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Company) => r.id}
      getName={(r) => r.name}
      addLabel={t("admin.companies.add")}
      columns={[
        {
          header: t("admin.res.name"),
          cell: (r) => (
            <span className="font-medium">
              {r.name}
              {r.is_default ? <Badge className="ml-2">{t("admin.companies.default")}</Badge> : null}
            </span>
          ),
        },
        { header: t("admin.companies.ticketCode"), cell: (r) => r.ticket_code },
        { header: t("admin.companies.senderDomains"), cell: (r) => r.sender_domains.join(", ") },
        {
          header: t("admin.res.status"),
          cell: (r) => (
            <Badge tone={r.is_active ? "ok" : "muted"}>
              {t(r.is_active ? "admin.res.active" : "admin.res.inactive")}
            </Badge>
          ),
        },
      ]}
      renderForm={(row, done) => <CompanyForm row={row} done={done} />}
    />
  );
}

function CompanyForm({ row, done }: { row: Company | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const [name, setName] = useState(row?.name ?? "");
  const [code, setCode] = useState(row?.ticket_code ?? "");
  const [domains, setDomains] = useState((row?.sender_domains ?? []).join(", "));

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["CompanyRequest"] = {
      name: name.trim(),
      ticket_code: code.trim(),
      sender_domains: domains
        .split(/[\s,]+/)
        .map((d) => d.trim())
        .filter(Boolean),
    };
    const outcome = row ? await updateCompany(row.id, body) : await createCompany(body);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await invalidate();
    done();
  }

  async function toggle() {
    if (!row) {
      return;
    }
    const outcome = await setCompanyActive(row.id, !row.is_active);
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
        label={t("admin.companies.ticketCode")}
        hint={t("admin.companies.ticketCodeHint")}
        value={code}
        onChange={(e) => setCode(e.target.value)}
        required
      />
      <Field
        label={t("admin.companies.senderDomains")}
        hint={t("admin.companies.senderDomainsHint")}
        value={domains}
        onChange={(e) => setDomains(e.target.value)}
      />
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
      {row ? (
        <Button
          type="button"
          size="sm"
          variant={row.is_active ? "destructive" : "outline"}
          className="justify-self-start"
          onClick={() => void toggle()}
        >
          {t(row.is_active ? "users.deactivate" : "users.reactivate")}
        </Button>
      ) : null}
    </form>
  );
}
