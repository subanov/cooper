import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { contractLookupQuery, supplierListQuery } from "@/features/contracts/api";
import { can } from "@/features/navigation/navigation";
import { analystSessionQuery } from "@/features/session/queries";
import { namesById } from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";
import { ContractsTabs } from "./ContractsTabs";

/** Suppliers by legal name (FR-CON-001). */
export function SuppliersPage() {
  const t = useText();
  const session = useQuery(analystSessionQuery).data;
  const [q, setQ] = useState("");
  const suppliers = useQuery(supplierListQuery(q.trim() ? { q: q.trim() } : {}));
  const types = useQuery(contractLookupQuery("supplier-types"));
  const statuses = useQuery(contractLookupQuery("supplier-statuses"));
  const typeName = namesById(types.data, (x) => x.name);
  const statusName = namesById(statuses.data, (x) => x.name);
  const rows = suppliers.data?.items ?? [];
  const canWrite = session ? can(session, "contracts.write") : false;

  return (
    <div className="grid gap-4">
      <ContractsTabs
        action={
          canWrite ? (
            <Button asChild size="sm">
              <Link to="/suppliers/new">{t("suppliers.new")}</Link>
            </Button>
          ) : null
        }
      />
      <Field
        label={t("suppliers.search")}
        type="search"
        className="max-w-sm"
        value={q}
        onChange={(e) => setQ(e.target.value)}
      />
      {suppliers.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {suppliers.isPending ? <p>{t("common.loading")}</p> : null}
      {suppliers.isSuccess && rows.length === 0 ? (
        <p className="text-muted-foreground">{t("suppliers.empty")}</p>
      ) : null}
      {rows.length > 0 ? (
        <table className="w-full text-left text-sm">
          <caption className="sr-only">{t("contracts.suppliers")}</caption>
          <thead>
            <tr className="border-b border-border">
              <th scope="col" className="py-2 pr-3">
                {t("suppliers.legalName")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("suppliers.tradingName")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("suppliers.type")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("suppliers.status")}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((s) => (
              <tr key={s.id} className="border-b border-border align-top">
                <td className="py-2 pr-3">
                  <Link
                    to="/suppliers/$id"
                    params={{ id: s.id }}
                    className="font-medium underline-offset-2 hover:underline"
                  >
                    {s.legal_name}
                  </Link>
                  {s.is_active ? null : (
                    <Badge tone="muted" className="ml-2">
                      {t("suppliers.inactive")}
                    </Badge>
                  )}
                </td>
                <td className="py-2 pr-3">{s.trading_name ?? ""}</td>
                <td className="py-2 pr-3">
                  {s.supplier_type_id ? typeName.get(s.supplier_type_id) : ""}
                </td>
                <td className="py-2 pr-3">
                  {s.supplier_status_id ? statusName.get(s.supplier_status_id) : ""}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}
