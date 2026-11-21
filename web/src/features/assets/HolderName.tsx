import { useQuery } from "@tanstack/react-query";
import { analystsQuery, namesById, requestersByIdQuery } from "@/features/reference/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

/** The name of whoever holds a checked-out Asset (a Requester or an Analyst). */
export function HolderName({ holder }: { holder: Schemas["AssetHolderDto"] }) {
  const t = useText();
  const analysts = useQuery({ ...analystsQuery, enabled: holder.kind === "analyst" });
  const requesters = useQuery({
    ...requestersByIdQuery([holder.id]),
    enabled: holder.kind === "requester",
  });
  const name =
    holder.kind === "analyst"
      ? namesById(analysts.data, (a) => a.full_name).get(holder.id)
      : requesters.data?.get(holder.id)?.full_name;
  return <>{name ?? (holder.kind === "analyst" ? t("common.analyst") : t("common.requester"))}</>;
}
