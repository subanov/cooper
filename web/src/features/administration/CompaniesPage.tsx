import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button } from "@/components/Button";
import { FormDialog } from "@/components/Dialog";
import { FormGrid, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { companiesQuery } from "@/features/reference/queries";
import { fieldError, useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { OrganisationList } from "./OrganisationPages";

type Company = Schemas["CompanyDto"];

/** Companies (Tenants): name, Ticket Code and Sender Domains (FR-IAM-008). */
export function CompaniesPage() {
  const t = useText();
  const companies = useQuery(companiesQuery);
  return (
    <OrganisationList
      title={t("admin.companies")}
      intro={t("admin.companies.intro")}
      rows={companies.data}
      loading={companies.isPending}
      error={companies.error}
      columns={[
        {
          id: "code",
          header: t("admin.companies.ticketCode"),
          cell: (c) => <span className="font-mono">{c.ticket_code}</span>,
        },
        {
          id: "domains",
          header: t("admin.companies.senderDomains"),
          wide: true,
          cell: (c) => c.sender_domains.join(", ") || t("common.noneDash"),
        },
        {
          id: "default",
          header: <span className="sr-only">{t("admin.companies.default")}</span>,
          cell: (c) =>
            c.is_default ? <Badge tone="accent">{t("admin.companies.default")}</Badge> : null,
        },
      ]}
      renderEditor={(row, close) => <CompanyDialog company={row} onClose={close} />}
    />
  );
}

function CompanyDialog({
  company,
  onClose,
}: {
  company?: Company | undefined;
  onClose: () => void;
}) {
  const t = useText();
  const [name, setName] = useState(company?.name ?? "");
  const [code, setCode] = useState(company?.ticket_code ?? "");
  const [domains, setDomains] = useState(company?.sender_domains.join(", ") ?? "");
  const body = (): Schemas["CompanyRequest"] => ({
    name: name.trim(),
    ticket_code: code.trim().toUpperCase(),
    sender_domains: domains
      .split(",")
      .map((d) => d.trim())
      .filter(Boolean),
  });
  const save = useAction(
    () =>
      company
        ? call(
            api.PUT("/api/v1/companies/{id}", {
              params: { path: { id: company.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/companies", { body: body() })),
    {
      invalidate: [["reference", "companies"]],
      success: () => t("ref.saved", { name: name.trim() }),
      onSuccess: onClose,
    },
  );
  const toggle = useAction(
    () =>
      company?.is_active
        ? call(
            api.POST("/api/v1/companies/{id}/deactivate", { params: { path: { id: company.id } } }),
          )
        : call(
            api.POST("/api/v1/companies/{id}/reactivate", {
              params: { path: { id: company?.id ?? "" } },
            }),
          ),
    {
      invalidate: [["reference", "companies"]],
      success: () =>
        t(company?.is_active ? "admin.companies.deactivated" : "admin.companies.reactivated", {
          name: company?.name ?? "",
        }),
      onSuccess: onClose,
    },
  );
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={company ? t("ref.edit", { name: company.name }) : t("ref.add")}
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!name.trim() || !code.trim()}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error ?? toggle.error} />
      <FormGrid>
        <TextField
          label={t("ref.name")}
          required
          value={name}
          error={fieldError(save.error, "name")}
          onChange={(e) => setName(e.target.value)}
        />
        <TextField
          label={t("admin.companies.ticketCode")}
          hint={t("admin.companies.ticketCodeHint")}
          required
          maxLength={5}
          value={code}
          error={fieldError(save.error, "ticket_code")}
          onChange={(e) => setCode(e.target.value)}
        />
      </FormGrid>
      <TextField
        label={t("admin.companies.senderDomains")}
        hint={t("admin.companies.senderDomainsHint")}
        value={domains}
        error={fieldError(save.error, "sender_domains")}
        onChange={(e) => setDomains(e.target.value)}
      />
      {company && !company.is_default ? (
        <div className="border-t border-line pt-3">
          <Button
            variant={company.is_active ? "ghost" : "secondary"}
            className={company.is_active ? "text-danger-text" : undefined}
            loading={toggle.isPending}
            onClick={() => toggle.mutate()}
          >
            {company.is_active ? t("admin.companies.deactivate") : t("admin.companies.reactivate")}
          </Button>
        </div>
      ) : null}
    </FormDialog>
  );
}
