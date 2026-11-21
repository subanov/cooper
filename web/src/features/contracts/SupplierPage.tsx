import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { Pencil, Plus } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { FormDialog } from "@/components/Dialog";
import { LoadingBlock, ProblemNotice, QueryState } from "@/components/Feedback";
import { Checkbox, FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { BackLink, Panel } from "@/components/Layout";
import { DetailHeader } from "@/components/MasterDetail";
import { companiesQuery, namesById, toOptions } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { HistoryTable } from "@/features/shared/History";
import { fieldError, orNull, useAction } from "@/features/shared/mutation";
import { RecordForm } from "@/features/shared/RecordForm";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import {
  contactsQuery,
  contractActions,
  contractLookupQuery,
  contractsQuery,
  type Supplier,
  supplierHistoryQuery,
  supplierQuery,
} from "./api";
import { NoticeBadge } from "./NoticeBadge";

type Contact = Schemas["ContactDto"];

/** One Supplier in the detail pane, with its Contacts and Contracts (FR-CON-005). */
export function SupplierPage({ id }: { id?: string | undefined }) {
  const t = useText();
  const query = useQuery({ ...supplierQuery(id ?? ""), enabled: Boolean(id) });
  if (!id) return <SupplierView />;
  if (!query.data) {
    return (
      <div className="mx-auto grid max-w-6xl gap-4 p-4 sm:p-6">
        <DetailHeader
          back={<BackToSuppliers />}
          title={query.isError ? t("suppliers.unavailable") : t("common.loading")}
        />
        {query.isError ? (
          <ProblemNotice error={query.error} onRetry={() => void query.refetch()} />
        ) : (
          <LoadingBlock lines={8} />
        )}
      </div>
    );
  }
  return <SupplierView key={query.data.updated_at} supplier={query.data} />;
}

function BackToSuppliers() {
  const t = useText();
  return (
    <BackLink to="/suppliers" search={true}>
      {t("suppliers.back")}
    </BackLink>
  );
}

function toForm(s?: Supplier) {
  return {
    legal_name: s?.legal_name ?? "",
    trading_name: s?.trading_name ?? "",
    registration_number: s?.registration_number ?? "",
    vat_number: s?.vat_number ?? "",
    supplier_type_id: s?.supplier_type_id ?? "",
    supplier_status_id: s?.supplier_status_id ?? "",
    notes: s?.notes ?? "",
    is_active: s?.is_active ?? true,
    company_id: s?.company_id ?? "",
  };
}

function SupplierView({ supplier }: { supplier?: Supplier }) {
  const t = useText();
  const can = useCan();
  const navigate = useNavigate();
  const types = useQuery(contractLookupQuery("supplier-types"));
  const statuses = useQuery(contractLookupQuery("supplier-statuses"));
  const companies = useQuery(companiesQuery);
  const [form, setForm] = useState(() => toForm(supplier));
  const [tried, setTried] = useState(false);
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const dirty = JSON.stringify(form) !== JSON.stringify(toForm(supplier));
  const body = (): Schemas["SupplierRequest"] => ({
    legal_name: form.legal_name.trim(),
    trading_name: orNull(form.trading_name),
    registration_number: orNull(form.registration_number),
    vat_number: orNull(form.vat_number),
    supplier_type_id: form.supplier_type_id || null,
    supplier_status_id: form.supplier_status_id || null,
    notes: form.notes,
    is_active: form.is_active,
    ...(supplier ? {} : { company_id: form.company_id || null }),
  });
  const save = useAction(
    () =>
      supplier
        ? contractActions.updateSupplier(supplier.id, body())
        : contractActions.createSupplier(body()),
    {
      invalidate: [["contracts"]],
      success: supplier ? "suppliers.saved" : "suppliers.created",
      onSuccess: (saved) => {
        if (!supplier)
          void navigate({ to: "/suppliers/$id", params: { id: saved.id }, search: true });
      },
    },
  );
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);
  const nameError =
    tried && !form.legal_name.trim()
      ? t("contracts.required")
      : fieldError(save.error, "legal_name");
  return (
    <article className="mx-auto grid max-w-6xl gap-5 p-4 sm:p-6">
      <DetailHeader
        back={<BackToSuppliers />}
        eyebrow={
          supplier ? (
            <>
              {supplier.trading_name ? <span>{supplier.trading_name}</span> : null}
              <span>{namesById(companies.data).get(supplier.company_id) ?? ""}</span>
            </>
          ) : undefined
        }
        title={supplier ? supplier.legal_name : t("suppliers.newTitle")}
        badges={
          supplier ? (
            <>
              <Badge tone={supplier.is_active ? "success" : "muted"}>
                {supplier.is_active ? t("common.active") : t("common.inactive")}
              </Badge>
              {supplier.supplier_status_id ? (
                <Badge tone="neutral">
                  {namesById(statuses.data).get(supplier.supplier_status_id) ?? "…"}
                </Badge>
              ) : null}
            </>
          ) : undefined
        }
      />
      <div
        className={
          supplier ? "grid gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]" : "grid max-w-3xl gap-5"
        }
      >
        <div className="grid min-w-0 content-start gap-5">
          <Panel title={t("suppliers.details")}>
            <RecordForm
              busy={save.isPending}
              error={nameError ? null : save.error}
              dirty={dirty && Boolean(supplier)}
              disabled={!can("contracts.write")}
              submitLabel={supplier ? t("common.saveChanges") : t("common.create")}
              onSubmit={() => {
                setTried(true);
                if (form.legal_name.trim()) save.mutate();
                else document.getElementById("supplier-name")?.focus();
              }}
            >
              <FormGrid>
                <TextField
                  id="supplier-name"
                  label={t("suppliers.legalName")}
                  required
                  value={form.legal_name}
                  error={nameError}
                  onChange={set("legal_name")}
                />
                <TextField
                  label={t("suppliers.tradingName")}
                  value={form.trading_name}
                  onChange={set("trading_name")}
                />
                <TextField
                  label={t("suppliers.registrationNumber")}
                  value={form.registration_number}
                  onChange={set("registration_number")}
                />
                <TextField
                  label={t("suppliers.vatNumber")}
                  value={form.vat_number}
                  onChange={set("vat_number")}
                />
                <SelectField
                  label={t("suppliers.type")}
                  value={form.supplier_type_id}
                  placeholder={t("common.none")}
                  onChange={set("supplier_type_id")}
                  options={toOptions(types.data, (x) => x.name, supplier?.supplier_type_id)}
                />
                <SelectField
                  label={t("suppliers.status")}
                  value={form.supplier_status_id}
                  placeholder={t("common.none")}
                  onChange={set("supplier_status_id")}
                  options={toOptions(statuses.data, (x) => x.name, supplier?.supplier_status_id)}
                />
                {!supplier && activeCompanies.length > 1 ? (
                  <SelectField
                    label={t("common.company")}
                    value={form.company_id}
                    placeholder={t("common.choose")}
                    onChange={set("company_id")}
                    options={activeCompanies.map((c) => ({ value: c.id, label: c.name }))}
                  />
                ) : null}
              </FormGrid>
              <TextAreaField
                label={t("suppliers.notes")}
                rows={4}
                value={form.notes}
                onChange={set("notes")}
              />
              <Checkbox
                label={t("suppliers.activeLabel")}
                checked={form.is_active}
                onChange={(e) => setForm((x) => ({ ...x, is_active: e.target.checked }))}
              />
            </RecordForm>
          </Panel>
          {supplier ? (
            <Panel title={t("suppliers.history")} flush>
              <SupplierHistory id={supplier.id} />
            </Panel>
          ) : null}
        </div>
        {supplier ? (
          <aside className="grid content-start gap-4" aria-label={t("suppliers.contacts")}>
            <Contacts supplier={supplier} />
            <SupplierContracts supplierId={supplier.id} />
          </aside>
        ) : null}
      </div>
    </article>
  );
}

function Contacts({ supplier }: { supplier: Supplier }) {
  const t = useText();
  const can = useCan();
  const contacts = useQuery(contactsQuery(supplier.id));
  const [editing, setEditing] = useState<Contact | "new" | null>(null);
  return (
    <Panel
      title={t("suppliers.contacts")}
      actions={
        can("contracts.write") ? (
          <Button
            size="sm"
            variant="ghost"
            icon={<Plus aria-hidden="true" />}
            onClick={() => setEditing("new")}
          >
            {t("suppliers.addContact")}
          </Button>
        ) : undefined
      }
    >
      <QueryState query={contacts}>
        {(list) =>
          list.length === 0 ? (
            <p className="text-sm text-fg-muted">{t("suppliers.noContacts")}</p>
          ) : (
            <ul className="grid gap-3">
              {list.map((c) => (
                <li key={c.id} className="flex items-start gap-2 text-sm">
                  <div className="grid min-w-0 flex-1 gap-0.5">
                    <span className="font-medium">
                      {c.full_name}
                      {!c.is_active ? (
                        <span className="text-fg-muted"> · {t("common.inactive")}</span>
                      ) : null}
                    </span>
                    {c.job_title ? (
                      <span className="text-xs text-fg-muted">{c.job_title}</span>
                    ) : null}
                    {c.email ? (
                      <a
                        href={`mailto:${c.email}`}
                        className="truncate text-xs text-accent-text hover:underline"
                      >
                        {c.email}
                      </a>
                    ) : null}
                    {c.phone || c.mobile ? (
                      <span className="text-xs text-fg-muted">
                        {[c.phone, c.mobile].filter(Boolean).join(" · ")}
                      </span>
                    ) : null}
                  </div>
                  {can("contracts.write") ? (
                    <IconButton
                      size="sm"
                      tooltip="top"
                      label={t("suppliers.editContact", { name: c.full_name })}
                      icon={<Pencil aria-hidden="true" />}
                      onClick={() => setEditing(c)}
                    />
                  ) : null}
                </li>
              ))}
            </ul>
          )
        }
      </QueryState>
      {editing ? (
        <ContactDialog
          supplierId={supplier.id}
          contact={editing === "new" ? undefined : editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
    </Panel>
  );
}

function ContactDialog({
  supplierId,
  contact,
  onClose,
}: {
  supplierId: string;
  contact?: Contact | undefined;
  onClose: () => void;
}) {
  const t = useText();
  const [form, setForm] = useState({
    full_name: contact?.full_name ?? "",
    job_title: contact?.job_title ?? "",
    email: contact?.email ?? "",
    phone: contact?.phone ?? "",
    mobile: contact?.mobile ?? "",
    notes: contact?.notes ?? "",
    is_active: contact?.is_active ?? true,
  });
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const body = (): Schemas["ContactRequest"] => ({
    full_name: form.full_name.trim(),
    job_title: orNull(form.job_title),
    email: orNull(form.email),
    phone: orNull(form.phone),
    mobile: orNull(form.mobile),
    notes: form.notes,
    is_active: form.is_active,
  });
  const save = useAction(
    () =>
      contact
        ? contractActions.updateContact(contact.id, body())
        : contractActions.addContact(supplierId, body()),
    {
      invalidate: [["contracts", "supplier", supplierId]],
      success: "suppliers.contactSaved",
      onSuccess: onClose,
    },
  );
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={
        contact
          ? t("suppliers.editContact", { name: contact.full_name })
          : t("suppliers.addContact")
      }
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!form.full_name.trim()}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <FormGrid>
        <TextField
          label={t("contacts.fullName")}
          required
          value={form.full_name}
          onChange={set("full_name")}
        />
        <TextField
          label={t("contacts.jobTitle")}
          value={form.job_title}
          onChange={set("job_title")}
        />
        <TextField
          label={t("contacts.email")}
          type="email"
          value={form.email}
          error={fieldError(save.error, "email")}
          onChange={set("email")}
        />
        <TextField
          label={t("contacts.phone")}
          type="tel"
          value={form.phone}
          onChange={set("phone")}
        />
        <TextField
          label={t("contacts.mobile")}
          type="tel"
          value={form.mobile}
          onChange={set("mobile")}
        />
      </FormGrid>
      <TextAreaField
        label={t("contacts.notes")}
        rows={3}
        value={form.notes}
        onChange={set("notes")}
      />
      <Checkbox
        label={t("contacts.active")}
        checked={form.is_active}
        onChange={(e) => setForm((x) => ({ ...x, is_active: e.target.checked }))}
      />
    </FormDialog>
  );
}

function SupplierContracts({ supplierId }: { supplierId: string }) {
  const t = useText();
  const list = useInfiniteQuery(contractsQuery({ supplier: supplierId }));
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  return (
    <Panel title={t("suppliers.contracts")}>
      {list.isPending ? (
        <LoadingBlock lines={2} />
      ) : rows.length === 0 ? (
        <p className="text-sm text-fg-muted">{t("suppliers.noContracts")}</p>
      ) : (
        <ul className="grid gap-1.5 text-sm">
          {rows.map((c) => (
            <li key={c.id} className="flex flex-wrap items-center gap-2">
              <span className="font-mono text-xs text-fg-muted">{c.contract_number}</span>
              <Link
                to="/contracts/$id"
                params={{ id: c.id }}
                className="min-w-0 flex-1 truncate hover:underline"
              >
                {c.title}
              </Link>
              <NoticeBadge contract={c} />
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}

function SupplierHistory({ id }: { id: string }) {
  const t = useText();
  const history = useQuery(supplierHistoryQuery(id));
  return (
    <QueryState query={history}>
      {(entries) => <HistoryTable entries={entries} caption={t("suppliers.history")} />}
    </QueryState>
  );
}
