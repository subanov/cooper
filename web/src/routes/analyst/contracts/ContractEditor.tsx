import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { SelectField } from "@/components/ui/select-field";
import { Textarea } from "@/components/ui/textarea";
import {
  contractLookupQuery,
  contractQuery,
  createContract,
  deleteContract,
  supplierListQuery,
  updateContract,
} from "@/features/contracts/api";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import { analystSessionQuery } from "@/features/session/queries";
import { analystsQuery } from "@/features/tickets/queries";
import { companiesQuery, defaultCompanyId } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";
import { NoticeBadge } from "./ContractsPage";

type Contract = Schemas["ContractDto"];

/** Creates a Contract (`id` absent) or edits one (FR-CON-002 to FR-CON-004). */
export function ContractEditor({ id }: { id?: string | undefined }) {
  const t = useText();
  const existing = useQuery({ ...contractQuery(id ?? ""), enabled: Boolean(id) });
  if (id && existing.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (id && !existing.data) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  return (
    <div className="grid max-w-3xl gap-6">
      <Link to="/contracts" className="text-sm underline-offset-2 hover:underline">
        {t("contracts.back")}
      </Link>
      <ContractForm key={existing.data?.updated_at ?? "new"} contract={existing.data} />
    </div>
  );
}

function ContractForm({ contract }: { contract: Contract | undefined }) {
  const t = useText();
  const intl = useIntl();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const session = useQuery(analystSessionQuery).data;
  const statuses = useQuery(contractLookupQuery("contract-statuses"));
  const schedules = useQuery(contractLookupQuery("payment-schedules"));
  const suppliers = useQuery(supplierListQuery());
  const analysts = useQuery(analystsQuery);
  const companies = useQuery(companiesQuery);
  const [number, setNumber] = useState(contract?.contract_number ?? "");
  const [title, setTitle] = useState(contract?.title ?? "");
  const [description, setDescription] = useState(contract?.description ?? "");
  const [party, setParty] = useState(contract?.party ?? "supplier");
  const [supplierId, setSupplierId] = useState(contract?.supplier_id ?? "");
  const [ownerId, setOwnerId] = useState(contract?.owner_id ?? "");
  const [statusId, setStatusId] = useState(contract?.status_id ?? "");
  const [start, setStart] = useState(contract?.start_date ?? "");
  const [end, setEnd] = useState(contract?.end_date ?? "");
  const [noticeAmount, setNoticeAmount] = useState(
    contract?.notice_period ? String(contract.notice_period.amount) : "",
  );
  const [noticeUnit, setNoticeUnit] = useState<Schemas["NoticePeriodUnit"]>(
    contract?.notice_period?.unit ?? "months",
  );
  const [value, setValue] = useState(contract?.value ?? "");
  const [currency, setCurrency] = useState(contract?.currency ?? "");
  const [scheduleId, setScheduleId] = useState(contract?.payment_schedule_id ?? "");
  const [dmsLink, setDmsLink] = useState(contract?.dms_link ?? "");
  const [personalData, setPersonalData] = useState(contract?.personal_data_transferred ?? false);
  const [dpiaRequired, setDpiaRequired] = useState(contract?.dpia_required ?? false);
  const [dpiaCompleted, setDpiaCompleted] = useState(contract?.dpia_completed_on ?? "");
  const [dpiaLink, setDpiaLink] = useState(contract?.dpia_link ?? "");
  const [errors, setErrors] = useState<Record<string, string>>({});
  const canWrite = session ? can(session, "contracts.write") : false;
  const active = <T extends { is_active: boolean; id: string }>(
    rows: T[] | undefined,
    keep: string,
  ) => (rows ?? []).filter((r) => r.is_active || r.id === keep);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const missing: Record<string, string> = {};
    if (!number.trim()) {
      missing.number = t("contracts.required");
    }
    if (!title.trim()) {
      missing.title = t("contracts.required");
    }
    setErrors(missing);
    if (Object.keys(missing).length > 0) {
      return;
    }
    const company = defaultCompanyId(companies.data);
    const body: Schemas["ContractRequest"] = {
      contract_number: number.trim(),
      title: title.trim(),
      description: description.trim(),
      supplier_id: party === "supplier" ? supplierId || null : null,
      requester_id: party === "customer" ? (contract?.requester_id ?? null) : null,
      owner_id: ownerId || null,
      status_id: statusId || null,
      start_date: start || null,
      end_date: end || null,
      notice_period:
        noticeAmount.trim() === "" ? null : { amount: Number(noticeAmount), unit: noticeUnit },
      value: value.trim() || null,
      currency: currency.trim() || null,
      payment_schedule_id: scheduleId || null,
      cost_centre_id: contract?.cost_centre_id ?? null,
      dms_link: dmsLink.trim() || null,
      personal_data_transferred: personalData,
      dpia_required: dpiaRequired,
      dpia_completed_on: dpiaRequired ? dpiaCompleted || null : null,
      dpia_link: dpiaRequired ? dpiaLink.trim() || null : null,
      ...(!contract && company ? { company_id: company } : {}),
    };
    const outcome = contract ? await updateContract(contract.id, body) : await createContract(body);
    if (!outcome.ok) {
      flash.fail(t("contracts.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["contracts"] });
    if (contract) {
      flash.ok(t("users.saved"));
    } else {
      await navigate({ to: "/contracts/$id", params: { id: outcome.data.id } });
    }
  }

  async function remove() {
    if (!contract) {
      return;
    }
    const outcome = await deleteContract(contract.id);
    if (!outcome.ok) {
      flash.fail(t("contracts.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["contracts"] });
    await navigate({ to: "/contracts" });
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-4" aria-labelledby="contract-heading">
      <h1 id="contract-heading" tabIndex={-1} className="text-2xl font-semibold outline-none">
        {contract ? `${contract.contract_number} ${contract.title}` : t("contracts.new")}
      </h1>
      {contract ? (
        <p className="flex flex-wrap items-center gap-2 text-sm">
          <span className="font-medium">{t("contracts.noticeDate")}:</span>
          {contract.notice_date
            ? intl.formatDate(contract.notice_date, { dateStyle: "long", timeZone: "UTC" })
            : t("contracts.noNoticeDate")}
          <NoticeBadge contract={contract} />
        </p>
      ) : null}
      <div className="grid gap-3 sm:grid-cols-2">
        <Field
          label={t("contracts.number")}
          value={number}
          onChange={(e) => setNumber(e.target.value)}
          error={errors.number}
          required
        />
        <Field
          label={t("contracts.title")}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          error={errors.title}
          required
        />
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="contract-description">{t("contracts.description")}</Label>
        <Textarea
          id="contract-description"
          rows={3}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
      </div>
      <fieldset className="grid gap-2">
        <legend className="mb-1 text-sm font-medium">{t("contracts.party")}</legend>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="radio"
            name="party"
            checked={party === "supplier"}
            onChange={() => setParty("supplier")}
          />
          {t("contracts.partySupplier")}
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="radio"
            name="party"
            checked={party === "customer"}
            onChange={() => setParty("customer")}
          />
          {t("contracts.partyCustomer")}
        </label>
      </fieldset>
      <div className="grid gap-3 sm:grid-cols-2">
        {party === "supplier" ? (
          <SelectField
            label={t("contracts.supplier")}
            value={supplierId}
            onChange={(e) => setSupplierId(e.target.value)}
          >
            <option value="">{t("contracts.chooseSupplier")}</option>
            {active(suppliers.data?.items, supplierId).map((s) => (
              <option key={s.id} value={s.id}>
                {s.trading_name ?? s.legal_name}
              </option>
            ))}
          </SelectField>
        ) : null}
        <SelectField
          label={t("contracts.owner")}
          value={ownerId}
          onChange={(e) => setOwnerId(e.target.value)}
        >
          <option value="">{t("contracts.noOwner")}</option>
          {(analysts.data ?? []).map((a) => (
            <option key={a.id} value={a.id}>
              {a.full_name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("contracts.status")}
          value={statusId}
          onChange={(e) => setStatusId(e.target.value)}
        >
          <option value="">{t("contracts.none")}</option>
          {active(statuses.data, statusId).map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </SelectField>
        <Field
          label={t("contracts.startDate")}
          type="date"
          value={start}
          onChange={(e) => setStart(e.target.value)}
        />
        <Field
          label={t("contracts.endDate")}
          type="date"
          value={end}
          onChange={(e) => setEnd(e.target.value)}
        />
        <Field
          label={t("contracts.noticePeriod")}
          type="number"
          min={0}
          hint={t("contracts.noticeHint")}
          value={noticeAmount}
          onChange={(e) => setNoticeAmount(e.target.value)}
        />
        <SelectField
          label={t("contracts.noticeUnit")}
          value={noticeUnit}
          onChange={(e) => setNoticeUnit(e.target.value as Schemas["NoticePeriodUnit"])}
        >
          <option value="months">{t("contracts.months")}</option>
          <option value="days">{t("contracts.days")}</option>
        </SelectField>
        <Field
          label={t("contracts.value")}
          inputMode="decimal"
          value={value}
          onChange={(e) => setValue(e.target.value)}
        />
        <Field
          label={t("contracts.currency")}
          hint={t("contracts.currencyHint")}
          maxLength={3}
          value={currency}
          onChange={(e) => setCurrency(e.target.value)}
        />
        <SelectField
          label={t("contracts.paymentSchedule")}
          value={scheduleId}
          onChange={(e) => setScheduleId(e.target.value)}
        >
          <option value="">{t("contracts.none")}</option>
          {active(schedules.data, scheduleId).map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </SelectField>
        <Field
          label={t("contracts.dmsLink")}
          type="url"
          value={dmsLink}
          onChange={(e) => setDmsLink(e.target.value)}
        />
      </div>
      <fieldset className="grid gap-3 rounded-lg border border-border p-4">
        <legend className="px-1 text-sm font-medium">{t("contracts.dataProtection")}</legend>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={personalData}
            onChange={(e) => setPersonalData(e.target.checked)}
          />
          {t("contracts.personalData")}
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={dpiaRequired}
            onChange={(e) => setDpiaRequired(e.target.checked)}
          />
          {t("contracts.dpiaRequired")}
        </label>
        {dpiaRequired ? (
          <div className="grid gap-3 sm:grid-cols-2">
            <Field
              label={t("contracts.dpiaCompleted")}
              type="date"
              value={dpiaCompleted}
              onChange={(e) => setDpiaCompleted(e.target.value)}
            />
            <Field
              label={t("contracts.dpiaLink")}
              type="url"
              value={dpiaLink}
              onChange={(e) => setDpiaLink(e.target.value)}
            />
          </div>
        ) : null}
      </fieldset>
      {canWrite ? (
        <div className="flex flex-wrap items-center gap-3">
          <Button type="submit">{contract ? t("users.save") : t("users.create")}</Button>
          {contract ? (
            <Button type="button" variant="outline" onClick={() => void remove()}>
              {t("contracts.delete")}
            </Button>
          ) : null}
          {flash.element}
        </div>
      ) : null}
    </form>
  );
}
