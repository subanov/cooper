import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import {
  contractListQuery,
  contractLookupQuery,
  supplierListQuery,
} from "@/features/contracts/api";
import { can } from "@/features/navigation/navigation";
import { analystSessionQuery } from "@/features/session/queries";
import { namesById } from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { ContractsTabs } from "./ContractsTabs";

type Contract = Schemas["ContractDto"];

/** The Notice Date of a Contract with where it stands, as the API reports it. */
export function NoticeBadge({ contract }: { contract: Contract }) {
  const t = useText();
  if (!contract.is_active) {
    return <Badge tone="muted">{t("contracts.closed")}</Badge>;
  }
  if (contract.notice_status === "upcoming") {
    return <Badge tone="warning">{t("contracts.notice.upcoming")}</Badge>;
  }
  if (contract.notice_status === "passed") {
    return <Badge tone="muted">{t("contracts.notice.passed")}</Badge>;
  }
  return null;
}

/** Contracts by number, filtered by status, Supplier or upcoming Notice Dates (FR-CON-002). */
export function ContractsPage() {
  const t = useText();
  const intl = useIntl();
  const session = useQuery(analystSessionQuery).data;
  const [q, setQ] = useState("");
  const [statusId, setStatusId] = useState("");
  const [supplierId, setSupplierId] = useState("");
  const [noticeDue, setNoticeDue] = useState(false);
  const contracts = useInfiniteQuery(
    contractListQuery({
      ...(q.trim() ? { q: q.trim() } : {}),
      ...(statusId ? { statusId } : {}),
      ...(supplierId ? { supplierId } : {}),
      noticeDue,
    }),
  );
  const statuses = useQuery(contractLookupQuery("contract-statuses"));
  const suppliers = useQuery(supplierListQuery());
  const statusName = namesById(statuses.data, (s) => s.name);
  const supplierName = namesById(suppliers.data?.items, (s) => s.trading_name ?? s.legal_name);
  const rows = contracts.data?.pages.flatMap((p) => p.items) ?? [];
  const date = (d: string) => intl.formatDate(d, { dateStyle: "medium", timeZone: "UTC" });
  const canWrite = session ? can(session, "contracts.write") : false;

  return (
    <div className="grid gap-4">
      <ContractsTabs
        action={
          canWrite ? (
            <Button asChild size="sm">
              <Link to="/contracts/new">{t("contracts.new")}</Link>
            </Button>
          ) : null
        }
      />
      <div className="flex flex-wrap items-end gap-3">
        <Field
          label={t("contracts.search")}
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <SelectField
          label={t("contracts.status")}
          value={statusId}
          onChange={(e) => setStatusId(e.target.value)}
        >
          <option value="">{t("contracts.anyStatus")}</option>
          {(statuses.data ?? []).map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("contracts.supplier")}
          value={supplierId}
          onChange={(e) => setSupplierId(e.target.value)}
        >
          <option value="">{t("contracts.anySupplier")}</option>
          {(suppliers.data?.items ?? []).map((s) => (
            <option key={s.id} value={s.id}>
              {s.trading_name ?? s.legal_name}
            </option>
          ))}
        </SelectField>
        <label className="flex items-center gap-2 pb-2 text-sm">
          <input
            type="checkbox"
            checked={noticeDue}
            onChange={(e) => setNoticeDue(e.target.checked)}
          />
          {t("contracts.noticeDueOnly")}
        </label>
      </div>
      {contracts.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {contracts.isPending ? <p>{t("common.loading")}</p> : null}
      {contracts.isSuccess && rows.length === 0 ? (
        <p className="text-muted-foreground">{t("contracts.empty")}</p>
      ) : null}
      {rows.length > 0 ? (
        <table className="w-full text-left text-sm">
          <caption className="sr-only">{t("contracts.contracts")}</caption>
          <thead>
            <tr className="border-b border-border">
              <th scope="col" className="py-2 pr-3">
                {t("contracts.number")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("contracts.title")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("contracts.party")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("contracts.status")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("contracts.endDate")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("contracts.noticeDate")}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((c) => (
              <tr key={c.id} className="border-b border-border align-top">
                <td className="py-2 pr-3">
                  <Link
                    to="/contracts/$id"
                    params={{ id: c.id }}
                    className="font-medium underline-offset-2 hover:underline"
                  >
                    {c.contract_number}
                  </Link>
                </td>
                <td className="py-2 pr-3">{c.title}</td>
                <td className="py-2 pr-3">
                  {c.supplier_id
                    ? (supplierName.get(c.supplier_id) ?? "")
                    : t("contracts.partyCustomer")}
                </td>
                <td className="py-2 pr-3">{c.status_id ? statusName.get(c.status_id) : ""}</td>
                <td className="py-2 pr-3">{c.end_date ? date(c.end_date) : ""}</td>
                <td className="py-2 pr-3">
                  <span className="flex flex-wrap items-center gap-2">
                    {c.notice_date ? date(c.notice_date) : ""}
                    <NoticeBadge contract={c} />
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
      {contracts.hasNextPage ? (
        <Button
          variant="outline"
          size="sm"
          className="justify-self-start"
          onClick={() => void contracts.fetchNextPage()}
        >
          {t("contracts.loadMore")}
        </Button>
      ) : null}
    </div>
  );
}
