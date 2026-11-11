import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { SelectField } from "@/components/ui/select-field";
import { Textarea } from "@/components/ui/textarea";
import {
  contractListQuery,
  contractLookupQuery,
  createContact,
  createSupplier,
  supplierContactsQuery,
  supplierQuery,
  updateContact,
  updateSupplier,
} from "@/features/contracts/api";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import { analystSessionQuery } from "@/features/session/queries";
import { companiesQuery, defaultCompanyId } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";
import { NoticeBadge } from "./ContractsPage";

type Supplier = Schemas["SupplierDto"];
type Contact = Schemas["ContactDto"];

/** Creates a Supplier (`id` absent) or edits one, with its Contacts and Contracts. */
export function SupplierEditor({ id }: { id?: string | undefined }) {
  const t = useText();
  const existing = useQuery({ ...supplierQuery(id ?? ""), enabled: Boolean(id) });
  if (id && existing.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (id && !existing.data) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  return (
    <div className="grid max-w-3xl gap-6">
      <Link to="/suppliers" className="text-sm underline-offset-2 hover:underline">
        {t("suppliers.back")}
      </Link>
      <SupplierForm key={existing.data?.updated_at ?? "new"} supplier={existing.data} />
      {existing.data ? <Contacts supplierId={existing.data.id} /> : null}
      {existing.data ? <SupplierContracts supplierId={existing.data.id} /> : null}
    </div>
  );
}

function SupplierForm({ supplier }: { supplier: Supplier | undefined }) {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const session = useQuery(analystSessionQuery).data;
  const types = useQuery(contractLookupQuery("supplier-types"));
  const statuses = useQuery(contractLookupQuery("supplier-statuses"));
  const companies = useQuery(companiesQuery);
  const [legalName, setLegalName] = useState(supplier?.legal_name ?? "");
  const [tradingName, setTradingName] = useState(supplier?.trading_name ?? "");
  const [registration, setRegistration] = useState(supplier?.registration_number ?? "");
  const [vat, setVat] = useState(supplier?.vat_number ?? "");
  const [typeId, setTypeId] = useState(supplier?.supplier_type_id ?? "");
  const [statusId, setStatusId] = useState(supplier?.supplier_status_id ?? "");
  const [notes, setNotes] = useState(supplier?.notes ?? "");
  const [active, setActive] = useState(supplier?.is_active ?? true);
  const [nameError, setNameError] = useState<string | undefined>();
  const canWrite = session ? can(session, "contracts.write") : false;
  const choosable = <T extends { is_active: boolean; id: string }>(
    rows: readonly T[] | undefined,
    keep: string,
  ) => (rows ?? []).filter((r) => r.is_active || r.id === keep);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!legalName.trim()) {
      setNameError(t("contracts.required"));
      return;
    }
    setNameError(undefined);
    const company = defaultCompanyId(companies.data);
    const body: Schemas["SupplierRequest"] = {
      legal_name: legalName.trim(),
      trading_name: tradingName.trim() || null,
      registration_number: registration.trim() || null,
      vat_number: vat.trim() || null,
      supplier_type_id: typeId || null,
      supplier_status_id: statusId || null,
      notes: notes.trim(),
      is_active: active,
      ...(!supplier && company ? { company_id: company } : {}),
    };
    const outcome = supplier ? await updateSupplier(supplier.id, body) : await createSupplier(body);
    if (!outcome.ok) {
      flash.fail(t("contracts.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["contracts"] });
    if (supplier) {
      flash.ok(t("users.saved"));
    } else {
      await navigate({ to: "/suppliers/$id", params: { id: outcome.data.id } });
    }
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-3" aria-labelledby="supplier-heading">
      <h1 id="supplier-heading" tabIndex={-1} className="text-2xl font-semibold outline-none">
        {supplier ? supplier.legal_name : t("suppliers.new")}
      </h1>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field
          label={t("suppliers.legalName")}
          value={legalName}
          onChange={(e) => setLegalName(e.target.value)}
          error={nameError}
          required
        />
        <Field
          label={t("suppliers.tradingName")}
          value={tradingName}
          onChange={(e) => setTradingName(e.target.value)}
        />
        <Field
          label={t("suppliers.registrationNumber")}
          value={registration}
          onChange={(e) => setRegistration(e.target.value)}
        />
        <Field
          label={t("suppliers.vatNumber")}
          value={vat}
          onChange={(e) => setVat(e.target.value)}
        />
        <SelectField
          label={t("suppliers.type")}
          value={typeId}
          onChange={(e) => setTypeId(e.target.value)}
        >
          <option value="">{t("contracts.none")}</option>
          {choosable(types.data, typeId).map((x) => (
            <option key={x.id} value={x.id}>
              {x.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("suppliers.status")}
          value={statusId}
          onChange={(e) => setStatusId(e.target.value)}
        >
          <option value="">{t("contracts.none")}</option>
          {choosable(statuses.data, statusId).map((x) => (
            <option key={x.id} value={x.id}>
              {x.name}
            </option>
          ))}
        </SelectField>
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="supplier-notes">{t("suppliers.notes")}</Label>
        <Textarea
          id="supplier-notes"
          rows={3}
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
        />
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} />
        {t("suppliers.active")}
      </label>
      {canWrite ? (
        <div className="flex items-center gap-3">
          <Button type="submit">{supplier ? t("users.save") : t("users.create")}</Button>
          {flash.element}
        </div>
      ) : null}
    </form>
  );
}

function Contacts({ supplierId }: { supplierId: string }) {
  const t = useText();
  const session = useQuery(analystSessionQuery).data;
  const contacts = useQuery(supplierContactsQuery(supplierId));
  const [editing, setEditing] = useState<string | null>(null);
  const canWrite = session ? can(session, "contracts.write") : false;
  return (
    <section aria-labelledby="supplier-contacts" className="grid gap-3">
      <div className="flex items-center gap-3">
        <h2 id="supplier-contacts" className="text-lg font-semibold">
          {t("suppliers.contacts")}
        </h2>
        {canWrite ? (
          <Button size="sm" variant="outline" className="ml-auto" onClick={() => setEditing("new")}>
            {t("suppliers.addContact")}
          </Button>
        ) : null}
      </div>
      {editing === "new" ? (
        <ContactForm supplierId={supplierId} contact={null} done={() => setEditing(null)} />
      ) : null}
      {contacts.isSuccess && contacts.data.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("suppliers.noContacts")}</p>
      ) : null}
      <ul className="grid gap-2">
        {(contacts.data ?? []).map((c) => (
          <li key={c.id} className="grid gap-2 rounded-md border border-border p-3 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium">{c.full_name}</span>
              {c.job_title ? <span className="text-muted-foreground">{c.job_title}</span> : null}
              {c.is_active ? null : <Badge tone="muted">{t("suppliers.inactive")}</Badge>}
              {canWrite ? (
                <Button
                  size="sm"
                  variant="ghost"
                  className="ml-auto"
                  aria-expanded={editing === c.id}
                  aria-label={`${t("admin.res.edit")} ${c.full_name}`}
                  onClick={() => setEditing(editing === c.id ? null : c.id)}
                >
                  {t("admin.res.edit")}
                </Button>
              ) : null}
            </div>
            <div className="flex flex-wrap gap-3 text-muted-foreground">
              {c.email ? <a href={`mailto:${c.email}`}>{c.email}</a> : null}
              {c.phone ? <span>{c.phone}</span> : null}
              {c.mobile ? <span>{c.mobile}</span> : null}
            </div>
            {editing === c.id ? (
              <ContactForm supplierId={supplierId} contact={c} done={() => setEditing(null)} />
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}

function ContactForm({
  supplierId,
  contact,
  done,
}: {
  supplierId: string;
  contact: Contact | null;
  done: () => void;
}) {
  const t = useText();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const [name, setName] = useState(contact?.full_name ?? "");
  const [jobTitle, setJobTitle] = useState(contact?.job_title ?? "");
  const [email, setEmail] = useState(contact?.email ?? "");
  const [phone, setPhone] = useState(contact?.phone ?? "");
  const [mobile, setMobile] = useState(contact?.mobile ?? "");
  const [active, setActive] = useState(contact?.is_active ?? true);
  const [nameError, setNameError] = useState<string | undefined>();

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!name.trim()) {
      setNameError(t("contracts.required"));
      return;
    }
    const body: Schemas["ContactRequest"] = {
      full_name: name.trim(),
      job_title: jobTitle.trim() || null,
      email: email.trim() || null,
      phone: phone.trim() || null,
      mobile: mobile.trim() || null,
      notes: contact?.notes ?? "",
      is_active: active,
    };
    const outcome = contact
      ? await updateContact(contact.id, body)
      : await createContact(supplierId, body);
    if (!outcome.ok) {
      flash.fail(t("contracts.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: supplierContactsQuery(supplierId).queryKey });
    done();
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-3 rounded-md border border-border p-3">
      <div className="grid gap-3 sm:grid-cols-2">
        <Field
          label={t("contacts.fullName")}
          value={name}
          onChange={(e) => setName(e.target.value)}
          error={nameError}
          required
        />
        <Field
          label={t("contacts.jobTitle")}
          value={jobTitle}
          onChange={(e) => setJobTitle(e.target.value)}
        />
        <Field
          label={t("contacts.email")}
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <Field
          label={t("contacts.phone")}
          value={phone}
          onChange={(e) => setPhone(e.target.value)}
        />
        <Field
          label={t("contacts.mobile")}
          value={mobile}
          onChange={(e) => setMobile(e.target.value)}
        />
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} />
        {t("suppliers.active")}
      </label>
      <div className="flex items-center gap-3">
        <Button type="submit" size="sm">
          {t("users.save")}
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={done}>
          {t("admin.res.cancel")}
        </Button>
        {flash.element}
      </div>
    </form>
  );
}

function SupplierContracts({ supplierId }: { supplierId: string }) {
  const t = useText();
  const contracts = useInfiniteQuery(contractListQuery({ supplierId }));
  const rows = contracts.data?.pages.flatMap((p) => p.items) ?? [];
  return (
    <section aria-labelledby="supplier-contracts" className="grid gap-2">
      <h2 id="supplier-contracts" className="text-lg font-semibold">
        {t("suppliers.contracts")}
      </h2>
      {contracts.isSuccess && rows.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("contracts.empty")}</p>
      ) : null}
      <ul className="grid gap-1 text-sm">
        {rows.map((c) => (
          <li key={c.id} className="flex flex-wrap items-center gap-2">
            <Link
              to="/contracts/$id"
              params={{ id: c.id }}
              className="font-medium underline-offset-2 hover:underline"
            >
              {c.contract_number}
            </Link>
            <span>{c.title}</span>
            <NoticeBadge contract={c} />
          </li>
        ))}
      </ul>
    </section>
  );
}
