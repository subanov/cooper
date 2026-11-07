import type { MessageId } from "@/i18n/messages";
import type { Schemas } from "@/lib/api";

type Target = Schemas["SlaTargetDto"];

/** Display tone of an SLA state. The state itself always comes from the API (FR-SLA-007). */
export type SlaTone = "ok" | "warning" | "breached" | "muted";

const STATES: Record<string, { label: MessageId; tone: SlaTone }> = {
  on_track: { label: "sla.onTrack", tone: "ok" },
  warning: { label: "sla.warning", tone: "warning" },
  breached: { label: "sla.breached", tone: "breached" },
  paused: { label: "sla.paused", tone: "muted" },
  met: { label: "sla.met", tone: "ok" },
  met_late: { label: "sla.metLate", tone: "breached" },
  none: { label: "sla.none", tone: "muted" },
};

/** Label and tone of an SLA target's state. */
export function slaState(target: Target): { label: MessageId; tone: SlaTone } {
  return STATES[target.state] ?? { label: "sla.none", tone: "muted" };
}

/** `2d 3h`, `3h 20m` or `45m`: a compact duration for the remaining time. */
export function formatDuration(seconds: number): string {
  const minutes = Math.max(0, Math.round(Math.abs(seconds) / 60));
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  const mins = minutes % 60;
  if (days > 0) return hours ? `${days}d ${hours}h` : `${days}d`;
  if (hours > 0) return mins ? `${hours}h ${mins}m` : `${hours}h`;
  return `${mins}m`;
}

/**
 * The target that matters now: the response target until it is met, then the resolution
 * target.
 */
export function currentTarget(sla: Schemas["SlaDto"]): Target {
  const responseOpen = !["met", "met_late", "none"].includes(sla.response.state);
  return responseOpen ? sla.response : sla.resolution;
}
