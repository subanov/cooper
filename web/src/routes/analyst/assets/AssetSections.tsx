import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { useIntl } from "react-intl";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import {
  assetCheckoutsQuery,
  assetHistoryQuery,
  assetLinksQuery,
  assetLookupQuery,
  checkInAsset,
  checkOutAsset,
  findTicketByNumber,
  linkAsset,
  retireAsset,
  unlinkAsset,
} from "@/features/assets/api";
import { problemText } from "@/features/result";
import {
  analystsQuery,
  namesById,
  requesterNamesQuery,
  requesterSearchQuery,
} from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";

type Asset = Schemas["AssetDto"];

function useRefresh(assetId: string) {
  const queryClient = useQueryClient();
  return async () => {
    await queryClient.invalidateQueries({ queryKey: ["assets"] });
    await queryClient.invalidateQueries({ queryKey: assetHistoryQuery(assetId).queryKey });
  };
}

/** Who holds what: names of Analysts and Requesters in the Check Out log. */
function useHolderNames(holders: readonly Schemas["AssetHolderDto"][]) {
  const analysts = useQuery(analystsQuery);
  const requesters = useQuery(
    requesterNamesQuery(holders.filter((h) => h.kind === "requester").map((h) => h.id)),
  );
  const analystName = namesById(analysts.data, (a) => a.full_name);
  return (h: Schemas["AssetHolderDto"]) =>
    (h.kind === "analyst" ? analystName.get(h.id) : requesters.data?.[h.id]?.full_name) ?? h.id;
}

/** Check Out / Check In (FR-AST-004), Retire (FR-AST-005) and the Check Out log. */
export function AssetCustody({ asset, canUpdate }: { asset: Asset; canUpdate: boolean }) {
  const t = useText();
  const intl = useIntl();
  const log = useQuery(assetCheckoutsQuery(asset.id));
  const holders = [
    ...(asset.checkout ? [asset.checkout.holder] : []),
    ...(log.data ?? []).map((c) => c.holder),
  ];
  const holderName = useHolderNames(holders);
  return (
    <section aria-labelledby="asset-custody" className="grid gap-3">
      <h2 id="asset-custody" className="text-lg font-semibold">
        {t("assets.custody")}
      </h2>
      <p className="text-sm">
        {asset.checkout
          ? t("assets.heldBy", {
              name: holderName(asset.checkout.holder),
              date: intl.formatDate(asset.checkout.checked_out_at, { dateStyle: "medium" }),
            })
          : asset.is_active
            ? t("assets.available")
            : t("assets.retired")}
      </p>
      {canUpdate && asset.is_active ? (
        asset.checkout ? (
          <CheckInForm asset={asset} />
        ) : (
          <div className="grid gap-4 sm:grid-cols-2">
            <CheckOutForm asset={asset} />
            <RetireForm asset={asset} />
          </div>
        )
      ) : null}
      <h3 className="font-medium">{t("assets.checkoutLog")}</h3>
      {log.isSuccess && log.data.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("assets.noCheckouts")}</p>
      ) : null}
      <ul className="grid gap-2">
        {(log.data ?? []).map((c) => (
          <li key={c.id} className="rounded-md border border-border p-3 text-sm">
            <p className="font-medium">{holderName(c.holder)}</p>
            <p className="text-muted-foreground">
              {t("assets.checkoutPeriod", {
                from: intl.formatDate(c.checked_out_at, { dateStyle: "medium" }),
                to: c.checked_in_at
                  ? intl.formatDate(c.checked_in_at, { dateStyle: "medium" })
                  : t("assets.stillOut"),
              })}
            </p>
            {c.checkout_note ? <p>{c.checkout_note}</p> : null}
            {c.checkin_note ? <p>{c.checkin_note}</p> : null}
          </li>
        ))}
      </ul>
    </section>
  );
}

function CheckOutForm({ asset }: { asset: Asset }) {
  const t = useText();
  const flash = useFlash();
  const refresh = useRefresh(asset.id);
  const analysts = useQuery(analystsQuery);
  const [kind, setKind] = useState<"requester" | "analyst">("requester");
  const [search, setSearch] = useState("");
  const requesters = useQuery({
    ...requesterSearchQuery(search.trim(), asset.company_id),
    enabled: kind === "requester",
  });
  const [holderId, setHolderId] = useState("");
  const [note, setNote] = useState("");
  const [due, setDue] = useState("");
  const options =
    kind === "analyst"
      ? (analysts.data ?? []).map((a) => ({ id: a.id, name: a.full_name }))
      : (requesters.data ?? []).map((r) => ({ id: r.id, name: `${r.full_name} <${r.email}>` }));

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!holderId) {
      flash.fail(t("assets.chooseHolder"));
      return;
    }
    const outcome = await checkOutAsset(asset.id, {
      holder_kind: kind,
      holder_id: holderId,
      note: note.trim(),
      due_back_on: due || null,
    });
    if (!outcome.ok) {
      flash.fail(t("assets.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await refresh();
  }

  return (
    <form
      onSubmit={submit}
      noValidate
      className="grid content-start gap-2 rounded-md border border-border p-3"
    >
      <h3 className="font-medium">{t("assets.checkOut")}</h3>
      <SelectField
        label={t("assets.holderKind")}
        value={kind}
        onChange={(e) => {
          setKind(e.target.value === "analyst" ? "analyst" : "requester");
          setHolderId("");
        }}
      >
        <option value="requester">{t("users.requesters")}</option>
        <option value="analyst">{t("users.analysts")}</option>
      </SelectField>
      {kind === "requester" ? (
        <Field
          label={t("assets.findRequester")}
          type="search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      ) : null}
      <SelectField
        label={t("assets.holder")}
        value={holderId}
        onChange={(e) => setHolderId(e.target.value)}
      >
        <option value="">{t("assets.choose")}</option>
        {options.map((o) => (
          <option key={o.id} value={o.id}>
            {o.name}
          </option>
        ))}
      </SelectField>
      <Field
        label={t("assets.dueBack")}
        type="date"
        value={due}
        onChange={(e) => setDue(e.target.value)}
      />
      <Field label={t("assets.note")} value={note} onChange={(e) => setNote(e.target.value)} />
      <div className="flex items-center gap-3">
        <Button type="submit" size="sm">
          {t("assets.checkOut")}
        </Button>
        {flash.element}
      </div>
    </form>
  );
}

function CheckInForm({ asset }: { asset: Asset }) {
  const t = useText();
  const flash = useFlash();
  const refresh = useRefresh(asset.id);
  const [note, setNote] = useState("");

  async function submit(event: FormEvent) {
    event.preventDefault();
    const outcome = await checkInAsset(asset.id, note.trim());
    if (!outcome.ok) {
      flash.fail(t("assets.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await refresh();
  }

  return (
    <form onSubmit={submit} className="grid max-w-md gap-2 rounded-md border border-border p-3">
      <Field label={t("assets.note")} value={note} onChange={(e) => setNote(e.target.value)} />
      <div className="flex items-center gap-3">
        <Button type="submit" size="sm">
          {t("assets.checkIn")}
        </Button>
        {flash.element}
      </div>
    </form>
  );
}

function RetireForm({ asset }: { asset: Asset }) {
  const t = useText();
  const flash = useFlash();
  const refresh = useRefresh(asset.id);
  const statuses = useQuery(assetLookupQuery("asset-statuses"));
  const [date, setDate] = useState("");
  const [reason, setReason] = useState("");
  const [statusId, setStatusId] = useState("");

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!reason.trim()) {
      flash.fail(t("assets.reasonRequired"));
      return;
    }
    const outcome = await retireAsset(asset.id, {
      reason: reason.trim(),
      retired_on: date || null,
      asset_status_id: statusId || null,
    });
    if (!outcome.ok) {
      flash.fail(t("assets.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await refresh();
  }

  return (
    <form
      onSubmit={submit}
      noValidate
      className="grid content-start gap-2 rounded-md border border-border p-3"
    >
      <h3 className="font-medium">{t("assets.retire")}</h3>
      <Field
        label={t("assets.retiredOnLabel")}
        hint={t("assets.retiredOnHint")}
        type="date"
        value={date}
        onChange={(e) => setDate(e.target.value)}
      />
      <Field
        label={t("assets.reason")}
        value={reason}
        onChange={(e) => setReason(e.target.value)}
        required
      />
      <SelectField
        label={t("assets.retiredStatus")}
        value={statusId}
        onChange={(e) => setStatusId(e.target.value)}
      >
        <option value="">{t("assets.none")}</option>
        {(statuses.data ?? [])
          .filter((s) => s.is_retired && s.is_active)
          .map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
      </SelectField>
      <div className="flex items-center gap-3">
        <Button type="submit" size="sm" variant="outline">
          {t("assets.retire")}
        </Button>
        {flash.element}
      </div>
    </form>
  );
}

/** Linked tickets and Contracts (FR-AST-006). */
export function AssetLinks({ asset, canUpdate }: { asset: Asset; canUpdate: boolean }) {
  const t = useText();
  const flash = useFlash();
  const refresh = useRefresh(asset.id);
  const links = useQuery(assetLinksQuery(asset.id));
  const [kind, setKind] = useState<"ticket" | "contract">("ticket");
  const [reference, setReference] = useState("");

  async function submit(event: FormEvent) {
    event.preventDefault();
    const ref = reference.trim();
    if (!ref) {
      return;
    }
    let target = ref;
    if (kind === "ticket") {
      const ticket = await findTicketByNumber(ref);
      if (!ticket) {
        flash.fail(t("assets.ticketNotFound", { number: ref }));
        return;
      }
      target = ticket.id;
    }
    const outcome = await linkAsset(asset.id, { kind, target_id: target });
    if (!outcome.ok) {
      flash.fail(t("assets.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    setReference("");
    flash.clear();
    await refresh();
  }

  async function remove(link: Schemas["AssetLinkDto"]) {
    const outcome = await unlinkAsset(asset.id, link.kind, link.target_id);
    if (!outcome.ok) {
      flash.fail(t("assets.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await refresh();
  }

  return (
    <section aria-labelledby="asset-links" className="grid gap-3">
      <h2 id="asset-links" className="text-lg font-semibold">
        {t("assets.links")}
      </h2>
      {links.isSuccess && links.data.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("assets.noLinks")}</p>
      ) : null}
      <ul className="grid gap-1">
        {(links.data ?? []).map((l) => (
          <li key={`${l.kind}-${l.target_id}`} className="flex items-center gap-3 text-sm">
            <span className="text-muted-foreground">
              {l.kind === "ticket" ? t("assets.linkTicket") : t("assets.linkContract")}
            </span>
            {l.kind === "ticket" ? (
              <Link
                to="/tickets/$ticketId"
                params={{ ticketId: l.target_id }}
                className="font-medium underline-offset-2 hover:underline"
              >
                {l.number}
              </Link>
            ) : (
              <span className="font-medium">{l.number}</span>
            )}
            {canUpdate ? (
              <Button
                size="sm"
                variant="ghost"
                aria-label={t("assets.unlinkNamed", { number: l.number })}
                onClick={() => void remove(l)}
              >
                {t("assets.unlink")}
              </Button>
            ) : null}
          </li>
        ))}
      </ul>
      {canUpdate ? (
        <form onSubmit={submit} className="flex flex-wrap items-end gap-3">
          <SelectField
            label={t("assets.linkKind")}
            value={kind}
            onChange={(e) => setKind(e.target.value === "contract" ? "contract" : "ticket")}
          >
            <option value="ticket">{t("assets.linkTicket")}</option>
            <option value="contract">{t("assets.linkContract")}</option>
          </SelectField>
          <Field
            label={kind === "ticket" ? t("assets.ticketNumber") : t("assets.contractId")}
            value={reference}
            onChange={(e) => setReference(e.target.value)}
          />
          <Button type="submit" size="sm" variant="outline">
            {t("assets.link")}
          </Button>
          {flash.element}
        </form>
      ) : null}
    </section>
  );
}

/** The full history of an Asset (FR-AST-006). */
export function AssetHistory({ assetId }: { assetId: string }) {
  const t = useText();
  const intl = useIntl();
  const history = useQuery(assetHistoryQuery(assetId));
  const show = (v: unknown) =>
    v === null || v === undefined ? "–" : typeof v === "string" ? v : JSON.stringify(v);
  return (
    <section aria-labelledby="asset-history" className="grid gap-3">
      <h2 id="asset-history" className="text-lg font-semibold">
        {t("assets.history")}
      </h2>
      {history.isSuccess && history.data.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("assets.noHistory")}</p>
      ) : null}
      {(history.data ?? []).length > 0 ? (
        <table className="w-full text-left text-sm">
          <caption className="sr-only">{t("assets.history")}</caption>
          <thead>
            <tr className="border-b border-border">
              <th scope="col" className="py-2 pr-3">
                {t("assets.when")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("assets.field")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("assets.from")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("assets.to")}
              </th>
            </tr>
          </thead>
          <tbody>
            {(history.data ?? []).map((h) => (
              <tr key={h.id} className="border-b border-border">
                <td className="py-1 pr-3">
                  {intl.formatDate(h.at, { dateStyle: "medium", timeStyle: "short" })}
                </td>
                <td className="py-1 pr-3">{h.field ?? h.action}</td>
                <td className="py-1 pr-3 break-all">{show(h.old_value)}</td>
                <td className="py-1 pr-3 break-all">{show(h.new_value)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </section>
  );
}
