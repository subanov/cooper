import { Badge } from "@/components/ui/badge";
import { currentTarget, formatDuration, slaState } from "@/features/tickets/sla";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

/**
 * SLA state and time remaining of one target (FR-SLA-007). The API decides the state;
 * this only shows it, always as text as well as colour.
 */
export function SlaTargetBadge({ target }: { target: Schemas["SlaTargetDto"] }) {
  const t = useText();
  const { label, tone } = slaState(target);
  const remaining = target.remaining_seconds;
  const time =
    remaining === null || remaining === undefined
      ? null
      : remaining < 0
        ? t("sla.overdueBy", { time: formatDuration(remaining) })
        : t("sla.left", { time: formatDuration(remaining) });
  return (
    <Badge tone={tone}>
      {t(label)}
      {time ? <span className="font-normal">· {time}</span> : null}
    </Badge>
  );
}

/** The SLA target that matters now: response until it is met, then resolution. */
export function SlaBadge({ sla }: { sla: Schemas["SlaDto"] }) {
  return <SlaTargetBadge target={currentTarget(sla)} />;
}
