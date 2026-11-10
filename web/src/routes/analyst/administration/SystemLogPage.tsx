import { useInfiniteQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { type LogFilter, systemLogQuery } from "@/features/administration/queries";
import { useText } from "@/i18n/useText";

/** The system log viewer: warnings and errors of the server and worker (FR-SYS-003). */
export function SystemLogPage() {
  const t = useText();
  const intl = useIntl();
  const [level, setLevel] = useState("");
  const [q, setQ] = useState("");
  const filter: LogFilter = {
    ...(level === "warn" || level === "error" ? { level } : {}),
    ...(q.trim() ? { q: q.trim() } : {}),
  };
  const log = useInfiniteQuery(systemLogQuery(filter));
  const entries = log.data?.pages.flatMap((page) => page.items) ?? [];

  return (
    <div className="grid gap-3">
      <p className="text-muted-foreground">{t("admin.log.intro")}</p>
      <div className="flex flex-wrap items-end gap-3">
        <div className="grid gap-1.5">
          <Label htmlFor="log-level">{t("admin.log.level")}</Label>
          <Select id="log-level" value={level} onChange={(e) => setLevel(e.target.value)}>
            <option value="">{t("admin.log.levelAll")}</option>
            <option value="warn">{t("admin.log.warn")}</option>
            <option value="error">{t("admin.log.error")}</option>
          </Select>
        </div>
        <Field
          label={t("admin.log.search")}
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
      </div>
      {log.isPending ? <p>{t("common.loading")}</p> : null}
      {log.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {log.isSuccess && entries.length === 0 ? (
        <p className="text-muted-foreground">{t("admin.log.empty")}</p>
      ) : null}
      {entries.length > 0 ? (
        <table className="w-full text-left text-sm">
          <thead>
            <tr className="border-b border-border">
              <th className="py-2 pr-3">{t("admin.log.when")}</th>
              <th className="py-2 pr-3">{t("admin.log.level")}</th>
              <th className="py-2 pr-3">{t("admin.log.source")}</th>
              <th className="py-2">{t("admin.log.message")}</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr key={entry.id} className="border-b border-border align-top">
                <td className="whitespace-nowrap py-2 pr-3">
                  {intl.formatDate(entry.occurred_at, { dateStyle: "short", timeStyle: "medium" })}
                </td>
                <td className="py-2 pr-3">
                  <Badge tone={entry.level === "error" ? "breached" : "warning"}>
                    {t(entry.level === "error" ? "admin.log.error" : "admin.log.warn")}
                  </Badge>
                </td>
                <td className="py-2 pr-3">
                  {entry.source}
                  <div className="font-mono text-xs text-muted-foreground">{entry.target}</div>
                </td>
                <td className="py-2">{entry.message}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
      {log.hasNextPage ? (
        <Button
          variant="outline"
          className="justify-self-start"
          disabled={log.isFetchingNextPage}
          onClick={() => void log.fetchNextPage()}
        >
          {t("admin.log.more")}
        </Button>
      ) : null}
    </div>
  );
}
