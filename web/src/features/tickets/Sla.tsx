import { AlarmClock, CheckCircle2, CircleSlash, PauseCircle, TriangleAlert } from "lucide-react";
import { Badge, type Tone } from "@/components/Badge";
import type { MessageId } from "@/i18n/messages";
import { compactDuration, useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

type Target = Schemas["SlaTargetDto"];

/** Display of an SLA state. The state and time left always come from the API (FR-SLA-007). */
const STATES: Record<string, { tone: Tone; icon: React.ReactNode }> = {
  on_track: { tone: "success", icon: <AlarmClock aria-hidden="true" /> },
  warning: { tone: "warning", icon: <TriangleAlert aria-hidden="true" /> },
  breached: { tone: "danger", icon: <TriangleAlert aria-hidden="true" /> },
  paused: { tone: "muted", icon: <PauseCircle aria-hidden="true" /> },
  met: { tone: "success", icon: <CheckCircle2 aria-hidden="true" /> },
  met_late: { tone: "danger", icon: <CheckCircle2 aria-hidden="true" /> },
  none: { tone: "muted", icon: <CircleSlash aria-hidden="true" /> },
};

/** The target that matters now: response until it is met, then resolution. */
export function currentTarget(sla: Schemas["SlaDto"]): {
  target: Target;
  which: "response" | "resolution";
} {
  const responseOpen = !["met", "met_late", "none"].includes(sla.response.state);
  return responseOpen
    ? { target: sla.response, which: "response" }
    : { target: sla.resolution, which: "resolution" };
}

/**
 * An SLA state with the time left or overdue. Breaches stand out (Von Restorff) through
 * text, icon and colour together, never colour alone.
 */
export function SlaBadge({ target, showTime = true }: { target: Target; showTime?: boolean }) {
  const t = useText();
  const state = STATES[target.state] ?? STATES.none;
  const label = t(`sla.${target.state in STATES ? target.state : "none"}` as MessageId);
  const seconds = target.remaining_seconds;
  const running = ["on_track", "warning", "breached"].includes(target.state);
  const time =
    showTime && running && seconds !== null && seconds !== undefined
      ? seconds < 0
        ? t("sla.overdue", { time: compactDuration(seconds) })
        : t("sla.left", { time: compactDuration(seconds) })
      : null;
  // A warning names itself, so the state never depends on colour (WCAG 1.4.1).
  const text = time && target.state === "warning" ? `${label} · ${time}` : (time ?? label);
  return (
    <Badge tone={state?.tone} icon={state?.icon}>
      {text}
      {time && target.state !== "warning" ? <span className="sr-only"> ({label})</span> : null}
    </Badge>
  );
}
