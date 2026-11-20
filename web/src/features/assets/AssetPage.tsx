import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { Archive, LogIn, LogOut, Unlink } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { Card } from "@/components/Card";
import { Combobox, type PickerOption } from "@/components/Combobox";
import { FormDialog } from "@/components/Dialog";
import { Fieldset, FormGrid, SelectField, TextField } from "@/components/Field";
import { Notice, ProblemNotice } from "@/components/Notice";
import { Page, PageHeader } from "@/components/Page";
import { LoadingBlock, QueryState } from "@/components/QueryState";
import { Segmented } from "@/components/Tabs";
import { allSuppliersQuery } from "@/features/contracts/api";
import { RequesterPicker, TicketPicker } from "@/features/reference/pickers";
import {
  analystOptions,
  analystsQuery,
  companiesQuery,
  costCentresQuery,
  toOptions,
} from "@/features/reference/queries";
import { useCan, useModuleOn } from "@/features/session/session";
import { HistoryTable } from "@/features/shared/History";
import { fieldError, orNull, useAction } from "@/features/shared/mutation";
import { RecordForm } from "@/features/shared/RecordForm";
import { useFormat, useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { localDay } from "@/lib/dates";
import {
  type Asset,
  assetActions,
  assetHistoryQuery,
  assetLinksQuery,
  assetLookupQuery,
  assetName,
  assetQuery,
  checkoutsQuery,
  locationOptions,
} from "./api";
import { HolderName } from "./HolderName";

/** One Asset: details, custody, retirement, links and history (FR-AST-001…006). */
export function AssetPage({ id }: { id?: string }) {
  const query = useQuery({ ...assetQuery(id ?? ""), enabled: Boolean(id) });
  if (!id) return <AssetView />;
  return (
    <QueryState
      query={query}
      loading={
        <Page>
          <LoadingBlock lines={8} />
        </Page>
      }
    >
      {(asset) => <AssetView key={asset.updated_at} asset={asset} />}
    </QueryState>
  );
}

function toForm(a?: Asset) {
  return {
    asset_tag: a?.asset_tag ?? "",
    hostname: a?.hostname ?? "",
    service_tag: a?.service_tag ?? "",
    asset_type_id: a?.asset_type_id ?? "",
    asset_status_id: a?.asset_status_id ?? "",
    location_id: a?.location_id ?? "",
    supplier_id: a?.supplier_id ?? "",
    cost_centre_id: a?.cost_centre_id ?? "",
    purchase_date: a?.purchase_date ?? "",
    purchase_cost: a?.purchase_cost ?? "",
    purchase_currency: a?.purchase_currency ?? "",
    warranty_expires_on: a?.warranty_expires_on ?? "",
    lease_starts_on: a?.lease_starts_on ?? "",
    lease_expires_on: a?.lease_expires_on ?? "",
    company_id: a?.company_id ?? "",
  };
}

function AssetView({ asset }: { asset?: Asset }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const moduleOn = useModuleOn();
  const navigate = useNavigate();
  const types = useQuery(assetLookupQuery("asset-types"));
  const statuses = useQuery(assetLookupQuery("asset-statuses"));
  const locations = useQuery(assetLookupQuery("asset-locations"));
  const costCentres = useQuery(costCentresQuery);
  const companies = useQuery(companiesQuery);
  const suppliers = useQuery({
    ...allSuppliersQuery,
    enabled: moduleOn("contracts") && can("contracts.read"),
  });
  const [form, setForm] = useState(() => toForm(asset));
  const [tried, setTried] = useState(false);
  const [dialog, setDialog] = useState<"out" | "in" | "retire" | null>(null);
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const dirty = JSON.stringify(form) !== JSON.stringify(toForm(asset));
  const editable = asset ? can("assets.update") && !asset.retired_on : can("assets.create");
  const hasIdentity = Boolean(
    form.asset_tag.trim() || form.hostname.trim() || form.service_tag.trim(),
  );
  const body = (): Schemas["AssetRequest"] => ({
    asset_tag: orNull(form.asset_tag),
    hostname: orNull(form.hostname),
    service_tag: orNull(form.service_tag),
    asset_type_id: form.asset_type_id || null,
    asset_status_id: form.asset_status_id || null,
    location_id: form.location_id || null,
    supplier_id: form.supplier_id || null,
    cost_centre_id: form.cost_centre_id || null,
    purchase_date: form.purchase_date || null,
    purchase_cost: orNull(form.purchase_cost),
    purchase_currency: orNull(form.purchase_currency.toUpperCase()),
    warranty_expires_on: form.warranty_expires_on || null,
    lease_starts_on: form.lease_starts_on || null,
    lease_expires_on: form.lease_expires_on || null,
    ...(asset ? {} : { company_id: form.company_id || null }),
  });
  const save = useAction(
    () => (asset ? assetActions.update(asset.id, body()) : assetActions.create(body())),
    {
      invalidate: [["assets"]],
      success: asset ? "assets.saved" : "assets.created",
      onSuccess: (saved) => {
        if (!asset) void navigate({ to: "/assets/$id", params: { id: saved.id } });
      },
    },
  );
  const err = (field: string) => fieldError(save.error, field);
  const name = asset ? (assetName(asset) ?? t("assets.unnamed")) : t("assets.new");
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);

  return (
    <Page>
      <PageHeader
        back={{ to: "/assets", label: t("assets.back") }}
        title={name}
        meta={
          asset ? (
            asset.retired_on ? (
              <Badge tone="muted">{t("assets.retired")}</Badge>
            ) : asset.checkout ? (
              <Badge tone="accent">{t("assets.checkedOut")}</Badge>
            ) : (
              <Badge tone="success">{t("assets.available")}</Badge>
            )
          ) : undefined
        }
      />
      {asset?.retired_on ? (
        <Notice tone="info" live={false}>
          {t("assets.retiredNotice", {
            date: f.day(asset.retired_on),
            reason: asset.retirement_reason ?? "",
          })}
        </Notice>
      ) : null}
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
        <Card>
          <fieldset disabled={!editable} className="contents">
            <RecordForm
              busy={save.isPending}
              error={save.error}
              dirty={dirty && Boolean(asset)}
              submitLabel={asset ? t("common.saveChanges") : t("common.create")}
              onSubmit={() => {
                setTried(true);
                if (hasIdentity) save.mutate();
              }}
            >
              <Fieldset legend={t("assets.identity")} description={t("assets.identityHint")}>
                {tried && !hasIdentity ? (
                  <Notice tone="danger">{t("assets.identifierRequired")}</Notice>
                ) : null}
                <FormGrid columns={3}>
                  <TextField
                    label={t("assets.assetTag")}
                    value={form.asset_tag}
                    error={err("asset_tag")}
                    onChange={set("asset_tag")}
                  />
                  <TextField
                    label={t("assets.hostname")}
                    value={form.hostname}
                    error={err("hostname")}
                    onChange={set("hostname")}
                  />
                  <TextField
                    label={t("assets.serviceTag")}
                    value={form.service_tag}
                    error={err("service_tag")}
                    onChange={set("service_tag")}
                  />
                </FormGrid>
              </Fieldset>
              <Fieldset legend={t("assets.classification")}>
                <FormGrid>
                  {!asset && activeCompanies.length > 1 ? (
                    <SelectField
                      label={t("common.company")}
                      value={form.company_id}
                      placeholder={t("common.choose")}
                      onChange={set("company_id")}
                      options={activeCompanies.map((c) => ({ value: c.id, label: c.name }))}
                    />
                  ) : null}
                  <SelectField
                    label={t("assets.type")}
                    value={form.asset_type_id}
                    placeholder={t("common.none")}
                    onChange={set("asset_type_id")}
                    options={toOptions(types.data, (x) => x.name, asset?.asset_type_id)}
                  />
                  <SelectField
                    label={t("assets.status")}
                    value={form.asset_status_id}
                    placeholder={t("common.none")}
                    onChange={set("asset_status_id")}
                    options={toOptions(
                      (statuses.data ?? []).filter(
                        (s) => !s.is_retired || s.id === asset?.asset_status_id,
                      ),
                      (x) => x.name,
                      asset?.asset_status_id,
                    )}
                  />
                  <SelectField
                    label={t("assets.location")}
                    value={form.location_id}
                    placeholder={t("common.none")}
                    onChange={set("location_id")}
                    options={locationOptions(locations.data, asset?.location_id)}
                  />
                </FormGrid>
              </Fieldset>
              <Fieldset legend={t("assets.purchase")}>
                <FormGrid>
                  {suppliers.data ? (
                    <SelectField
                      label={t("assets.supplier")}
                      value={form.supplier_id}
                      placeholder={t("common.none")}
                      onChange={set("supplier_id")}
                      options={toOptions(suppliers.data, (s) => s.legal_name, asset?.supplier_id)}
                    />
                  ) : null}
                  <SelectField
                    label={t("assets.costCentre")}
                    value={form.cost_centre_id}
                    placeholder={t("common.none")}
                    onChange={set("cost_centre_id")}
                    options={toOptions(
                      costCentres.data,
                      (c) => `${c.code} · ${c.name}`,
                      asset?.cost_centre_id,
                    )}
                  />
                  <TextField
                    label={t("assets.purchaseDate")}
                    type="date"
                    value={form.purchase_date}
                    onChange={set("purchase_date")}
                  />
                  <div className="grid grid-cols-[1fr_7rem] gap-2">
                    <TextField
                      label={t("assets.purchaseCost")}
                      hint={t("assets.purchaseCostHint")}
                      inputMode="decimal"
                      value={form.purchase_cost}
                      error={err("purchase_cost")}
                      onChange={set("purchase_cost")}
                    />
                    <TextField
                      label={t("assets.currency")}
                      hint={t("assets.currencyHint")}
                      maxLength={3}
                      value={form.purchase_currency}
                      error={err("purchase_currency")}
                      onChange={set("purchase_currency")}
                    />
                  </div>
                  <TextField
                    label={t("assets.warrantyExpires")}
                    type="date"
                    value={form.warranty_expires_on}
                    onChange={set("warranty_expires_on")}
                  />
                  <TextField
                    label={t("assets.leaseStarts")}
                    type="date"
                    value={form.lease_starts_on}
                    onChange={set("lease_starts_on")}
                  />
                  <TextField
                    label={t("assets.leaseExpires")}
                    type="date"
                    value={form.lease_expires_on}
                    min={form.lease_starts_on || undefined}
                    onChange={set("lease_expires_on")}
                  />
                </FormGrid>
              </Fieldset>
            </RecordForm>
          </fieldset>
        </Card>
        {asset ? (
          <aside className="grid content-start gap-4">
            <Card title={t("assets.custody")}>
              <div className="grid gap-3">
                {asset.checkout ? (
                  <div className="grid gap-0.5 text-sm">
                    <span className="font-semibold">
                      <HolderName holder={asset.checkout.holder} />
                    </span>
                    <span className="text-xs text-fg-muted">
                      {t("assets.heldSince", { date: f.date(asset.checkout.checked_out_at) })}
                    </span>
                    {asset.checkout.due_back_on ? (
                      <span className="text-xs text-fg-muted">
                        {t("assets.dueBack", { date: f.day(asset.checkout.due_back_on) })}
                      </span>
                    ) : null}
                  </div>
                ) : (
                  <p className="text-sm">{t("assets.notOut")}</p>
                )}
                {can("assets.update") && !asset.retired_on ? (
                  <div className="flex flex-wrap gap-2">
                    {asset.checkout ? (
                      <Button
                        variant="primary"
                        icon={<LogIn aria-hidden="true" className="size-4" />}
                        onClick={() => setDialog("in")}
                      >
                        {t("assets.checkIn")}
                      </Button>
                    ) : (
                      <Button
                        variant="primary"
                        icon={<LogOut aria-hidden="true" className="size-4" />}
                        onClick={() => setDialog("out")}
                      >
                        {t("assets.checkOut")}
                      </Button>
                    )}
                  </div>
                ) : null}
              </div>
            </Card>
            <AssetLinks asset={asset} />
            <CheckoutLog id={asset.id} />
            {can("assets.update") && !asset.retired_on ? (
              <Card>
                <div className="grid gap-2">
                  <p className="text-xs text-fg-muted">{t("assets.retireIntro")}</p>
                  <div>
                    <Button
                      variant="danger"
                      icon={<Archive aria-hidden="true" className="size-4" />}
                      disabled={Boolean(asset.checkout)}
                      onClick={() => setDialog("retire")}
                    >
                      {t("assets.retire")}
                    </Button>
                  </div>
                </div>
              </Card>
            ) : null}
          </aside>
        ) : null}
      </div>
      {asset ? (
        <>
          <Card title={t("assets.history")} flush>
            <AssetHistory id={asset.id} />
          </Card>
          <CheckOutDialog
            asset={asset}
            name={name}
            open={dialog === "out"}
            onOpenChange={(o) => setDialog(o ? "out" : null)}
          />
          <CheckInDialog
            asset={asset}
            name={name}
            open={dialog === "in"}
            onOpenChange={(o) => setDialog(o ? "in" : null)}
          />
          <RetireDialog
            asset={asset}
            name={name}
            open={dialog === "retire"}
            onOpenChange={(o) => setDialog(o ? "retire" : null)}
          />
        </>
      ) : null}
    </Page>
  );
}

function CheckOutDialog({
  asset,
  name,
  open,
  onOpenChange,
}: {
  asset: Asset;
  name: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const t = useText();
  const analysts = useQuery(analystsQuery);
  const [kind, setKind] = useState<"requester" | "analyst">("requester");
  const [holder, setHolder] = useState<PickerOption | null>(null);
  const [due, setDue] = useState("");
  const [note, setNote] = useState("");
  const out = useAction(
    () =>
      assetActions.checkOut(asset.id, {
        holder_kind: kind,
        holder_id: holder?.id ?? "",
        due_back_on: due || null,
        note,
      }),
    {
      invalidate: [["assets"]],
      success: "assets.checkedOutDone",
      onSuccess: () => onOpenChange(false),
    },
  );
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("assets.checkOutTitle", { name })}
      submitLabel={t("assets.checkOut")}
      busy={out.isPending}
      submitDisabled={!holder}
      onSubmit={() => out.mutate()}
    >
      <ProblemNotice error={out.error} />
      <Segmented
        hideLabel={false}
        label={t("assets.holderKind")}
        value={kind}
        onChange={(k) => {
          setKind(k);
          setHolder(null);
        }}
        options={[
          { value: "requester", label: t("assets.holderRequester") },
          { value: "analyst", label: t("assets.holderAnalyst") },
        ]}
      />
      {kind === "requester" ? (
        <RequesterPicker
          label={t("assets.holder")}
          hint={t("assets.chooseHolder")}
          companyId={asset.company_id}
          required
          value={holder}
          onChange={setHolder}
        />
      ) : (
        <Combobox
          label={t("assets.holder")}
          hint={t("assets.chooseHolder")}
          required
          value={holder}
          onChange={setHolder}
          options={analystOptions(analysts.data)}
        />
      )}
      <TextField
        label={t("assets.dueBackLabel")}
        type="date"
        min={localDay()}
        value={due}
        onChange={(e) => setDue(e.target.value)}
      />
      <TextField label={t("assets.note")} value={note} onChange={(e) => setNote(e.target.value)} />
    </FormDialog>
  );
}

function CheckInDialog({
  asset,
  name,
  open,
  onOpenChange,
}: {
  asset: Asset;
  name: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const t = useText();
  const [note, setNote] = useState("");
  const back = useAction(() => assetActions.checkIn(asset.id, { note }), {
    invalidate: [["assets"]],
    success: "assets.checkedInDone",
    onSuccess: () => onOpenChange(false),
  });
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("assets.checkInTitle", { name })}
      submitLabel={t("assets.checkIn")}
      busy={back.isPending}
      size="sm"
      onSubmit={() => back.mutate()}
    >
      <ProblemNotice error={back.error} />
      <TextField label={t("assets.note")} value={note} onChange={(e) => setNote(e.target.value)} />
    </FormDialog>
  );
}

function RetireDialog({
  asset,
  name,
  open,
  onOpenChange,
}: {
  asset: Asset;
  name: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const t = useText();
  const statuses = useQuery(assetLookupQuery("asset-statuses"));
  const retiredStatuses = (statuses.data ?? []).filter((s) => s.is_retired && s.is_active);
  const [on, setOn] = useState("");
  const [reason, setReason] = useState("");
  const [statusId, setStatusId] = useState("");
  const [tried, setTried] = useState(false);
  const retire = useAction(
    () =>
      assetActions.retire(asset.id, {
        reason: reason.trim(),
        retired_on: on || null,
        asset_status_id: statusId || null,
      }),
    {
      invalidate: [["assets"]],
      success: "assets.retiredDone",
      onSuccess: () => onOpenChange(false),
    },
  );
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("assets.retireTitle", { name })}
      description={t("assets.retireIntro")}
      submitLabel={t("assets.retire")}
      busy={retire.isPending}
      onSubmit={() => {
        setTried(true);
        if (reason.trim()) retire.mutate();
      }}
    >
      <ProblemNotice error={retire.error} />
      <TextField
        label={t("assets.reason")}
        required
        value={reason}
        error={tried && !reason.trim() ? t("assets.reasonRequired") : undefined}
        onChange={(e) => setReason(e.target.value)}
      />
      <TextField
        label={t("assets.retiredOn")}
        hint={t("assets.retiredOnHint")}
        type="date"
        max={localDay()}
        value={on}
        onChange={(e) => setOn(e.target.value)}
      />
      {retiredStatuses.length > 0 ? (
        <SelectField
          label={t("assets.retiredStatus")}
          value={statusId}
          placeholder={t("newTicket.default")}
          onChange={(e) => setStatusId(e.target.value)}
          options={retiredStatuses.map((s) => ({ value: s.id, label: s.name }))}
        />
      ) : null}
    </FormDialog>
  );
}

function AssetLinks({ asset }: { asset: Asset }) {
  const t = useText();
  const can = useCan();
  const links = useQuery(assetLinksQuery(asset.id));
  const [pick, setPick] = useState<PickerOption | null>(null);
  const editable = can("assets.update") && !asset.retired_on;
  const link = useAction(
    (targetId: string) => assetActions.link(asset.id, { kind: "ticket", target_id: targetId }),
    {
      invalidate: [["assets", "detail", asset.id]],
      success: "common.linked",
      onSuccess: () => setPick(null),
    },
  );
  const unlink = useAction(
    (l: { kind: string; target_id: string }) => assetActions.unlink(asset.id, l.kind, l.target_id),
    {
      invalidate: [["assets", "detail", asset.id]],
      success: "common.unlinked",
    },
  );
  return (
    <Card title={t("assets.links")}>
      <div className="grid gap-3">
        <QueryState query={links}>
          {(list) =>
            list.length === 0 ? (
              <p className="text-sm text-fg-muted">{t("assets.noLinks")}</p>
            ) : (
              <ul className="grid gap-1 text-sm">
                {list.map((l) => (
                  <li key={`${l.kind}-${l.target_id}`} className="flex items-center gap-2">
                    <Badge tone="muted">
                      {l.kind === "contract" ? t("assets.kind.contract") : t("assets.kind.ticket")}
                    </Badge>
                    {l.kind === "contract" ? (
                      <Link
                        to="/contracts/$id"
                        params={{ id: l.target_id }}
                        className="flex-1 font-mono hover:underline"
                      >
                        {l.number}
                      </Link>
                    ) : (
                      <Link
                        to="/tickets/$ticketId"
                        params={{ ticketId: l.target_id }}
                        search={{ view: "open" }}
                        className="flex-1 font-mono hover:underline"
                      >
                        {l.number}
                      </Link>
                    )}
                    {editable ? (
                      <IconButton
                        size="sm"
                        tooltip="top"
                        label={t("common.unlinkNamed", { name: l.number })}
                        icon={<Unlink aria-hidden="true" className="size-4" />}
                        onClick={() => unlink.mutate(l)}
                      />
                    ) : null}
                  </li>
                ))}
              </ul>
            )
          }
        </QueryState>
        {editable ? (
          <form
            className="grid gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (pick) link.mutate(pick.id);
            }}
          >
            <TicketPicker
              label={t("assets.linkTicket")}
              companyId={asset.company_id}
              value={pick}
              onChange={setPick}
            />
            <div>
              <Button type="submit" size="sm" disabled={!pick} loading={link.isPending}>
                {t("common.link")}
              </Button>
            </div>
          </form>
        ) : null}
        <ProblemNotice error={link.error ?? unlink.error} />
      </div>
    </Card>
  );
}

function CheckoutLog({ id }: { id: string }) {
  const t = useText();
  const f = useFormat();
  const log = useQuery(checkoutsQuery(id));
  return (
    <Card title={t("assets.checkoutLog")}>
      <QueryState query={log}>
        {(rows) =>
          rows.length === 0 ? (
            <p className="text-sm text-fg-muted">{t("assets.noCheckouts")}</p>
          ) : (
            <ol className="grid gap-2 text-sm">
              {rows.map((c) => (
                <li key={c.id} className="grid gap-0.5">
                  <span className="font-medium">
                    <HolderName holder={c.holder} />
                  </span>
                  <span className="text-xs text-fg-muted">
                    {t("assets.period", {
                      from: f.date(c.checked_out_at),
                      to: c.checked_in_at ? f.date(c.checked_in_at) : t("assets.stillOut"),
                    })}
                  </span>
                  {c.checkout_note || c.checkin_note ? (
                    <span className="text-xs text-fg-muted">
                      {[c.checkout_note, c.checkin_note].filter(Boolean).join(" · ")}
                    </span>
                  ) : null}
                </li>
              ))}
            </ol>
          )
        }
      </QueryState>
    </Card>
  );
}

function AssetHistory({ id }: { id: string }) {
  const t = useText();
  const history = useQuery(assetHistoryQuery(id));
  return (
    <QueryState query={history}>
      {(entries) => <HistoryTable entries={entries} caption={t("assets.history")} />}
    </QueryState>
  );
}
