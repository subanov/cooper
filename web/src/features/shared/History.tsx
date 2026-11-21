import { useQuery } from "@tanstack/react-query";
import { History as HistoryIcon } from "lucide-react";
import { DataTable } from "@/components/DataTable";
import { EmptyState } from "@/components/Feedback";
import { analystsQuery, namesById } from "@/features/reference/queries";
import { type Text, useFormat, useText } from "@/i18n/useText";

export interface HistoryEntry {
  action: string;
  actor: unknown;
  at: string;
  field?: string | null;
  old_value: unknown;
  new_value: unknown;
  id?: string;
}

/** Who did something, from an `actor` object (`{kind, id}`). */
export function useActorName(): (actor: unknown) => string {
  const t = useText();
  const analysts = useQuery(analystsQuery);
  const names = namesById(analysts.data, (a) => a.full_name);
  return (actor) => {
    const a = (actor ?? {}) as { kind?: string; id?: string | null };
    switch (a.kind) {
      case "analyst":
        return (a.id && names.get(a.id)) || t("common.analyst");
      case "requester":
        return t("common.requester");
      case "api_key":
        return t("common.apiKey");
      case "system":
      case "automation":
        return t("common.automation");
      default:
        return t("common.someone");
    }
  };
}

/** A value from an audit entry as text; objects are shown compactly. */
export function auditValue(value: unknown, t: Text): string {
  if (value === null || value === undefined || value === "") return t("common.noneDash");
  if (typeof value === "boolean") return value ? t("common.yes") : t("common.no");
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

/** The audit trail of a record (FR-GEN-001): when, who, and what changed, newest first. */
export function HistoryTable({
  entries,
  caption,
  fieldLabel,
}: {
  entries: readonly HistoryEntry[];
  caption: string;
  /** Plain-language names of fields (falls back to the field key). */
  fieldLabel?: ((field: string) => string | undefined) | undefined;
}) {
  const t = useText();
  const f = useFormat();
  const actorName = useActorName();
  const rows = [...entries].sort((a, b) => b.at.localeCompare(a.at));
  if (rows.length === 0)
    return <EmptyState icon={<HistoryIcon aria-hidden="true" />} title={t("common.noHistory")} />;
  return (
    <DataTable
      caption={caption}
      rows={rows}
      rowKey={(r) => r.id ?? `${r.at}-${r.field ?? r.action}`}
      columns={[
        {
          key: "when",
          header: t("common.when"),
          cell: (r) => <time dateTime={r.at}>{f.dateTime(r.at)}</time>,
          className: "whitespace-nowrap",
        },
        { key: "who", header: t("common.who"), cell: (r) => actorName(r.actor) },
        {
          key: "what",
          header: t("common.change"),
          cell: (r) =>
            r.field ? (
              <span className="grid gap-0.5">
                <span className="font-medium">
                  {fieldLabel?.(r.field) ?? r.field.replaceAll("_", " ")}
                </span>
                <span className="text-xs text-fg-muted [overflow-wrap:anywhere]">
                  {auditValue(r.old_value, t)} → {auditValue(r.new_value, t)}
                </span>
              </span>
            ) : (
              r.action.replaceAll("_", " ")
            ),
        },
      ]}
    />
  );
}
