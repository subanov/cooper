import { useInfiniteQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { requesterListQuery } from "@/features/users/queries";
import { useText } from "@/i18n/useText";

/** The Requester list with search and an "include deactivated" switch. */
export function RequestersPage() {
  const t = useText();
  const [q, setQ] = useState("");
  const [includeInactive, setIncludeInactive] = useState(false);
  const list = useInfiniteQuery(
    requesterListQuery({ ...(q.trim() ? { q: q.trim() } : {}), includeInactive }),
  );
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  return (
    <div className="grid gap-3">
      <div className="flex flex-wrap items-end gap-3">
        <Field
          label={t("users.search")}
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <label className="flex items-center gap-2 pb-2 text-sm">
          <input
            type="checkbox"
            checked={includeInactive}
            onChange={(e) => setIncludeInactive(e.target.checked)}
          />
          {t("users.includeInactive")}
        </label>
        <Button asChild className="ml-auto">
          <Link to="/users/requesters/new">{t("users.newRequester")}</Link>
        </Button>
      </div>
      {list.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {list.isPending ? <p>{t("common.loading")}</p> : null}
      {list.isSuccess && rows.length === 0 ? (
        <p className="text-muted-foreground">{t("users.empty")}</p>
      ) : null}
      {rows.length > 0 ? (
        <table className="w-full text-left text-sm">
          <caption className="sr-only">{t("users.requesters")}</caption>
          <thead>
            <tr className="border-b border-border">
              <th scope="col" className="py-2 pr-3">
                {t("users.name")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("users.email")}
              </th>
              <th scope="col" className="py-2 pr-3">
                {t("users.phone")}
              </th>
              <th scope="col" className="py-2">
                {t("users.status")}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((a) => (
              <tr key={a.id} className="border-b border-border">
                <td className="py-2 pr-3">
                  <Link
                    to="/users/requesters/$id"
                    params={{ id: a.id }}
                    className="font-medium underline-offset-2 hover:underline"
                  >
                    {a.full_name}
                  </Link>
                </td>
                <td className="py-2 pr-3">{a.email}</td>
                <td className="py-2 pr-3">{a.phone ?? ""}</td>
                <td className="py-2">
                  <Badge tone={a.is_active ? "ok" : "muted"}>
                    {t(a.is_active ? "users.active" : "users.deactivated")}
                  </Badge>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
      {list.hasNextPage ? (
        <Button
          variant="outline"
          className="justify-self-start"
          disabled={list.isFetchingNextPage}
          onClick={() => void list.fetchNextPage()}
        >
          {t("users.more")}
        </Button>
      ) : null}
    </div>
  );
}
