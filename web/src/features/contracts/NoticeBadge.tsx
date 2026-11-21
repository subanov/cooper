import { Badge } from "@/components/Badge";
import { useText } from "@/i18n/useText";
import type { Contract } from "./api";

/** Where a Contract's Notice Date stands, as the API reports it. */
export function NoticeBadge({ contract }: { contract: Contract }) {
  const t = useText();
  if (!contract.is_active) return <Badge tone="muted">{t("contracts.closed")}</Badge>;
  switch (contract.notice_status) {
    case "upcoming":
      return <Badge tone="warning">{t("contracts.notice.upcoming")}</Badge>;
    case "passed":
      return <Badge tone="danger">{t("contracts.notice.passed")}</Badge>;
    default:
      return null;
  }
}
