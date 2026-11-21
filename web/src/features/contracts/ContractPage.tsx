import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Trash2 } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/Button";
import type { PickerOption } from "@/components/Combobox";
import { ConfirmDialog } from "@/components/Dialog";
import { LoadingBlock, ProblemNotice, QueryState } from "@/components/Feedback";
import {
  Checkbox,
  Fieldset,
  FormGrid,
  SelectField,
  TextAreaField,
  TextField,
} from "@/components/Field";
import { BackLink, Facts, Panel } from "@/components/Layout";
import { DetailHeader } from "@/components/MasterDetail";
import { Segmented } from "@/components/Tabs";
import { RequesterPicker } from "@/features/reference/pickers";
import {
  analystsQuery,
  companiesQuery,
  costCentresQuery,
  namesById,
  requesterOption,
  requestersByIdQuery,
  toOptions,
} from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { HistoryTable } from "@/features/shared/History";
import { fieldError, orNull, useAction } from "@/features/shared/mutation";
import { RecordForm } from "@/features/shared/RecordForm";
import { useFormat, useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import {
  allSuppliersQuery,
  type Contract,
  contractActions,
  contractHistoryQuery,
  contractLookupQuery,
  contractQuery,
} from "./api";
import { NoticeBadge } from "./NoticeBadge";

/** One Contract in the detail pane (FR-CON-001…004). Cooper calculates the Notice Date. */
export function ContractPage({ id }: { id?: string | undefined }) {
  const t = useText();
  const query = useQuery({ ...contractQuery(id ?? ""), enabled: Boolean(id) });
  if (!id) return <ContractView />;
  if (!query.data) {
    return (
      <div className="mx-auto grid max-w-6xl gap-4 p-4 sm:p-6">
        <DetailHeader
          back={<BackToContracts />}
          title={query.isError ? t("contracts.unavailable") : t("common.loading")}
        />
        {query.isError ? (
          <ProblemNotice error={query.error} onRetry={() => void query.refetch()} />
        ) : (
          <LoadingBlock lines={8} />
        )}
      </div>
    );
  }
  return <ContractView key={query.data.updated_at} contract={query.data} />;
}

function BackToContracts() {
  const t = useText();
  return (
    <BackLink to="/contracts" search={true}>
      {t("contracts.back")}
    </BackLink>
  );
}

function toForm(c?: Contract) {
  return {
    contract_number: c?.contract_number ?? "",
    title: c?.title ?? "",
    description: c?.description ?? "",
    status_id: c?.status_id ?? "",
    supplier_id: c?.supplier_id ?? "",
    owner_id: c?.owner_id ?? "",
    start_date: c?.start_date ?? "",
    end_date: c?.end_date ?? "",
    notice_amount: c?.notice_period ? String(c.notice_period.amount) : "",
    notice_unit: (c?.notice_period?.unit ?? "months") as "days" | "months",
    value: c?.value ?? "",
    currency: c?.currency ?? "",
    payment_schedule_id: c?.payment_schedule_id ?? "",
    cost_centre_id: c?.cost_centre_id ?? "",
    dms_link: c?.dms_link ?? "",
    personal_data_transferred: c?.personal_data_transferred ?? false,
    dpia_required: c?.dpia_required ?? false,
    dpia_completed_on: c?.dpia_completed_on ?? "",
    dpia_link: c?.dpia_link ?? "",
    company_id: c?.company_id ?? "",
  };
}

function ContractView({ contract }: { contract?: Contract }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const navigate = useNavigate();
  const statuses = useQuery(contractLookupQuery("contract-statuses"));
  const schedules = useQuery(contractLookupQuery("payment-schedules"));
  const suppliers = useQuery(allSuppliersQuery);
  const analysts = useQuery(analystsQuery);
  const costCentres = useQuery(costCentresQuery);
  const companies = useQuery(companiesQuery);
  const customer = useQuery(requestersByIdQuery([contract?.requester_id]));
  const [form, setForm] = useState(() => toForm(contract));
  const [party, setParty] = useState<"supplier" | "customer">(
    contract?.requester_id ? "customer" : "supplier",
  );
  const [requester, setRequester] = useState<PickerOption | null | undefined>(undefined);
  const [tried, setTried] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const initialRequester = contract?.requester_id
    ? customer.data?.get(contract.requester_id)
    : undefined;
  const shownRequester =
    requester === undefined
      ? initialRequester
        ? requesterOption(initialRequester)
        : null
      : requester;
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const flag =
    (key: "personal_data_transferred" | "dpia_required") => (e: { target: { checked: boolean } }) =>
      setForm((x) => ({ ...x, [key]: e.target.checked }));
  const editable = can("contracts.write");
  const dirty =
    JSON.stringify(form) !== JSON.stringify(toForm(contract)) || requester !== undefined;
  const valid = form.contract_number.trim() !== "" && form.title.trim() !== "";
  const body = (): Schemas["ContractRequest"] => ({
    contract_number: form.contract_number.trim(),
    title: form.title.trim(),
    description: form.description,
    status_id: form.status_id || null,
    supplier_id: party === "supplier" ? form.supplier_id || null : null,
    requester_id:
      party === "customer"
        ? requester === undefined
          ? (contract?.requester_id ?? null)
          : (requester?.id ?? null)
        : null,
    owner_id: form.owner_id || null,
    start_date: form.start_date || null,
    end_date: form.end_date || null,
    notice_period: form.notice_amount
      ? { amount: Number(form.notice_amount), unit: form.notice_unit }
      : null,
    value: orNull(form.value),
    currency: orNull(form.currency.toUpperCase()),
    payment_schedule_id: form.payment_schedule_id || null,
    cost_centre_id: form.cost_centre_id || null,
    dms_link: orNull(form.dms_link),
    personal_data_transferred: form.personal_data_transferred,
    dpia_required: form.dpia_required,
    dpia_completed_on: form.dpia_completed_on || null,
    dpia_link: orNull(form.dpia_link),
    ...(contract ? {} : { company_id: form.company_id || null }),
  });
  const save = useAction(
    () => (contract ? contractActions.update(contract.id, body()) : contractActions.create(body())),
    {
      invalidate: [["contracts"]],
      success: contract ? "contracts.saved" : "contracts.created",
      onSuccess: (saved) => {
        if (!contract)
          void navigate({ to: "/contracts/$id", params: { id: saved.id }, search: true });
      },
    },
  );
  const remove = useAction(() => contractActions.remove(contract?.id ?? ""), {
    invalidate: [["contracts"]],
    success: "contracts.deleted",
    onSuccess: () => navigate({ to: "/contracts", search: true }),
  });
  const required = (field: "contract_number" | "title") =>
    fieldError(save.error, field) ??
    (tried && !form[field].trim() ? t("contracts.required") : undefined);
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);

  return (
    <article className="mx-auto grid max-w-6xl gap-5 p-4 sm:p-6">
      <DetailHeader
        back={<BackToContracts />}
        eyebrow={
          contract ? (
            <>
              <span className="font-mono font-medium text-fg">{contract.contract_number}</span>
              <span aria-hidden="true">·</span>
              <span>{namesById(companies.data).get(contract.company_id) ?? ""}</span>
              {contract.supplier_id ? (
                <>
                  <span aria-hidden="true">·</span>
                  <span>
                    {namesById(suppliers.data, (s) => s.legal_name).get(contract.supplier_id) ?? ""}
                  </span>
                </>
              ) : null}
            </>
          ) : undefined
        }
        title={contract ? contract.title : t("contracts.newTitle")}
        badges={contract ? <NoticeBadge contract={contract} /> : undefined}
      />
      <div
        className={
          contract ? "grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]" : "grid max-w-3xl gap-5"
        }
      >
        <div className="grid min-w-0 content-start gap-5">
          <Panel>
            <RecordForm
              busy={save.isPending}
              error={save.error}
              dirty={dirty && Boolean(contract)}
              disabled={!editable}
              submitLabel={contract ? t("common.saveChanges") : t("common.create")}
              onSubmit={() => {
                setTried(true);
                if (valid) save.mutate();
                else
                  document
                    .getElementById(
                      form.contract_number.trim() ? "contract-title" : "contract-number",
                    )
                    ?.focus();
              }}
            >
              <Fieldset legend={t("contracts.basics")}>
                <FormGrid>
                  <TextField
                    id="contract-number"
                    label={t("contracts.number")}
                    required
                    value={form.contract_number}
                    error={required("contract_number")}
                    onChange={set("contract_number")}
                  />
                  <TextField
                    id="contract-title"
                    label={t("contracts.titleField")}
                    required
                    value={form.title}
                    error={required("title")}
                    onChange={set("title")}
                  />
                  {!contract && activeCompanies.length > 1 ? (
                    <SelectField
                      label={t("common.company")}
                      value={form.company_id}
                      placeholder={t("common.choose")}
                      onChange={set("company_id")}
                      options={activeCompanies.map((c) => ({ value: c.id, label: c.name }))}
                    />
                  ) : null}
                  <SelectField
                    label={t("contracts.status")}
                    value={form.status_id}
                    placeholder={t("common.none")}
                    onChange={set("status_id")}
                    options={toOptions(statuses.data, (s) => s.name, contract?.status_id)}
                  />
                  <SelectField
                    label={t("contracts.owner")}
                    value={form.owner_id}
                    placeholder={t("contracts.noOwner")}
                    onChange={set("owner_id")}
                    options={toOptions(analysts.data, (a) => a.full_name, contract?.owner_id)}
                  />
                </FormGrid>
                <Segmented
                  showLabel
                  label={t("contracts.party")}
                  value={party}
                  onChange={setParty}
                  options={[
                    { value: "supplier", label: t("contracts.partySupplier") },
                    { value: "customer", label: t("contracts.partyCustomer") },
                  ]}
                />
                {party === "supplier" ? (
                  <SelectField
                    label={t("contracts.supplier")}
                    value={form.supplier_id}
                    placeholder={t("common.choose")}
                    onChange={set("supplier_id")}
                    options={toOptions(suppliers.data, (s) => s.legal_name, contract?.supplier_id)}
                  />
                ) : (
                  <RequesterPicker
                    label={t("contracts.customer")}
                    value={shownRequester}
                    onChange={setRequester}
                  />
                )}
                <TextAreaField
                  label={t("contracts.description")}
                  rows={4}
                  value={form.description}
                  onChange={set("description")}
                />
              </Fieldset>
              <Fieldset legend={t("contracts.dates")} description={t("contracts.noticeHint")}>
                <FormGrid>
                  <TextField
                    label={t("contracts.startDate")}
                    type="date"
                    value={form.start_date}
                    onChange={set("start_date")}
                  />
                  <TextField
                    label={t("contracts.endDate")}
                    type="date"
                    min={form.start_date || undefined}
                    value={form.end_date}
                    error={fieldError(save.error, "end_date")}
                    onChange={set("end_date")}
                  />
                  <TextField
                    label={t("contracts.noticePeriod")}
                    type="number"
                    min={0}
                    value={form.notice_amount}
                    onChange={set("notice_amount")}
                  />
                  <SelectField
                    label={t("contracts.noticeUnit")}
                    value={form.notice_unit}
                    onChange={set("notice_unit")}
                    options={[
                      { value: "days", label: t("contracts.days") },
                      { value: "months", label: t("contracts.months") },
                    ]}
                  />
                </FormGrid>
              </Fieldset>
              <Fieldset legend={t("contracts.money")}>
                <FormGrid>
                  <div className="grid grid-cols-[1fr_7rem] gap-2">
                    <TextField
                      label={t("contracts.value")}
                      hint={t("contracts.valueHint")}
                      inputMode="decimal"
                      value={form.value}
                      error={fieldError(save.error, "value")}
                      onChange={set("value")}
                    />
                    <TextField
                      label={t("contracts.currency")}
                      hint={t("contracts.currencyHint")}
                      maxLength={3}
                      value={form.currency}
                      error={fieldError(save.error, "currency")}
                      onChange={set("currency")}
                    />
                  </div>
                  <SelectField
                    label={t("contracts.paymentSchedule")}
                    value={form.payment_schedule_id}
                    placeholder={t("common.none")}
                    onChange={set("payment_schedule_id")}
                    options={toOptions(
                      schedules.data,
                      (s) => s.name,
                      contract?.payment_schedule_id,
                    )}
                  />
                  <SelectField
                    label={t("contracts.costCentre")}
                    value={form.cost_centre_id}
                    placeholder={t("common.none")}
                    onChange={set("cost_centre_id")}
                    options={toOptions(
                      costCentres.data,
                      (c) => `${c.code} · ${c.name}`,
                      contract?.cost_centre_id,
                    )}
                  />
                  <TextField
                    label={t("contracts.dmsLink")}
                    type="url"
                    value={form.dms_link}
                    onChange={set("dms_link")}
                  />
                </FormGrid>
              </Fieldset>
              <Fieldset legend={t("contracts.dataProtection")}>
                <div className="flex flex-wrap gap-6">
                  <Checkbox
                    label={t("contracts.personalData")}
                    checked={form.personal_data_transferred}
                    onChange={flag("personal_data_transferred")}
                  />
                  <Checkbox
                    label={t("contracts.dpiaRequired")}
                    checked={form.dpia_required}
                    onChange={flag("dpia_required")}
                  />
                </div>
                {form.dpia_required ? (
                  <FormGrid>
                    <TextField
                      label={t("contracts.dpiaCompleted")}
                      type="date"
                      value={form.dpia_completed_on}
                      onChange={set("dpia_completed_on")}
                    />
                    <TextField
                      label={t("contracts.dpiaLink")}
                      type="url"
                      value={form.dpia_link}
                      onChange={set("dpia_link")}
                    />
                  </FormGrid>
                ) : null}
              </Fieldset>
            </RecordForm>
          </Panel>
          {contract ? (
            <Panel title={t("contracts.history")} flush>
              <ContractHistory id={contract.id} />
            </Panel>
          ) : null}
        </div>
        {contract ? (
          <aside className="grid content-start gap-4" aria-label={t("contracts.noticeDate")}>
            <Panel title={t("contracts.noticeDate")}>
              <Facts
                items={[
                  {
                    key: "notice",
                    term: t("contracts.noticeDate"),
                    value: contract.notice_date
                      ? f.day(contract.notice_date)
                      : t("contracts.noNoticeDate"),
                  },
                  {
                    key: "end",
                    term: t("contracts.endDate"),
                    value: contract.end_date ? f.day(contract.end_date) : t("common.noneDash"),
                  },
                ]}
              />
            </Panel>
            {editable ? (
              <Panel title={t("contracts.delete")}>
                <Button
                  variant="danger"
                  icon={<Trash2 aria-hidden="true" />}
                  onClick={() => setConfirm(true)}
                >
                  {t("contracts.delete")}
                </Button>
              </Panel>
            ) : null}
          </aside>
        ) : null}
      </div>
      {contract ? (
        <ConfirmDialog
          open={confirm}
          onOpenChange={setConfirm}
          title={t("contracts.deleteTitle", { number: contract.contract_number })}
          confirmLabel={t("contracts.delete")}
          busy={remove.isPending}
          onConfirm={() => remove.mutate()}
        >
          <p className="text-sm">{t("contracts.deleteBody", { title: contract.title })}</p>
          <ProblemNotice error={remove.error} className="mt-3" />
        </ConfirmDialog>
      ) : null}
    </article>
  );
}

function ContractHistory({ id }: { id: string }) {
  const t = useText();
  const history = useQuery(contractHistoryQuery(id));
  return (
    <QueryState query={history}>
      {(entries) => <HistoryTable entries={entries} caption={t("contracts.history")} />}
    </QueryState>
  );
}
