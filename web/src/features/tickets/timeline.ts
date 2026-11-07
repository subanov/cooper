import type { MessageId } from "@/i18n/messages";

/** Who did something, as the timeline API reports it (`{kind, id}`). */
export interface TimelineActor {
  kind: "analyst" | "requester" | "api_key" | "automation" | string;
  id?: string | null;
}

/** One field of a change entry. */
export interface FieldChange {
  field: string;
  old_value: unknown;
  new_value: unknown;
}

type Lookups = Record<
  "analyst" | "status" | "priority" | "team" | "resolution",
  Map<string, string>
>;

/** Fields shown with their new value, resolved through reference data where it is an ID. */
const SHOWN: Record<string, { label: MessageId; lookup?: keyof Lookups; text?: boolean }> = {
  status_id: { label: "field.status", lookup: "status" },
  priority_id: { label: "field.priority", lookup: "priority" },
  assigned_analyst_id: { label: "field.assignee", lookup: "analyst" },
  assigned_team_id: { label: "field.queue", lookup: "team" },
  resolution_code_id: { label: "field.resolutionCode", lookup: "resolution" },
  subject: { label: "field.subject", text: true },
  sensitivity: { label: "field.sensitivity", text: true },
};

/** Fields shown only as "changed": long text, or IDs this screen cannot name yet. */
const CHANGED_ONLY: Record<string, MessageId> = {
  description: "field.description",
  requester_id: "field.requester",
  type_id: "field.type",
  category_id: "field.category",
  department_id: "field.department",
  origin_id: "field.origin",
  snoozed_until: "field.snoozedUntil",
};

/**
 * A readable line for one changed field, or `null` for bookkeeping fields (SLA due
 * dates, timestamps) that the timeline leaves out. `value` is absent for fields shown
 * only as changed.
 */
export function describeChange(
  change: FieldChange,
  lookups: Lookups,
): { field: string; label: MessageId; value?: string } | null {
  const shown = SHOWN[change.field];
  if (shown) {
    const raw = change.new_value;
    const value =
      raw === null || raw === undefined
        ? ""
        : shown.lookup
          ? (lookups[shown.lookup].get(String(raw)) ?? "…")
          : String(raw);
    return { field: change.field, label: shown.label, value };
  }
  const label = CHANGED_ONLY[change.field];
  return label ? { field: change.field, label } : null;
}
