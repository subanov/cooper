import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

/**
 * Who else has this ticket open (FR-TKT-025): a warning against two Analysts answering
 * the same Requester, never a lock.
 */
export function PresenceNotice({
  viewers,
  names,
}: {
  viewers: readonly Schemas["ViewerDto"][];
  names: ReadonlyMap<string, string>;
}) {
  const t = useText();
  if (viewers.length === 0) {
    return null;
  }
  return (
    <ul
      aria-label={t("presence.label")}
      aria-live="polite"
      className="grid gap-1 rounded-md border border-border bg-muted px-3 py-2 text-sm"
    >
      {viewers.map((v) => {
        const name = names.get(v.analyst_id) ?? t("presence.someone");
        return (
          <li key={v.analyst_id}>
            {v.is_composing ? t("presence.composing", { name }) : t("presence.viewing", { name })}
          </li>
        );
      })}
    </ul>
  );
}
