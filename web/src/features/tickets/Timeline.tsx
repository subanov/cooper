import { useQuery } from "@tanstack/react-query";
import { ArrowLeftRight, Clock, Inbox, Lock, MessageSquare, Send } from "lucide-react";
import { useState } from "react";
import { Badge, type Tone } from "@/components/Badge";
import { EmptyState, QueryState } from "@/components/Feedback";
import { Segmented } from "@/components/Tabs";
import {
  analystsQuery,
  namesById,
  teamsQuery,
  ticketLookupQuery,
} from "@/features/reference/queries";
import type { MessageId } from "@/i18n/messages";
import { useFormat, useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { timelineQuery } from "./api";

interface Actor {
  kind?: string;
  id?: string | null;
}
interface FieldChange {
  field: string;
  old_value: unknown;
  new_value: unknown;
}
type Lookup = "status" | "priority" | "analyst" | "team" | "code";

/** Fields shown with their new value; IDs are named through reference data. */
const SHOWN: Record<string, { label: MessageId; lookup?: Lookup }> = {
  status_id: { label: "ticket.status", lookup: "status" },
  priority_id: { label: "ticket.priority", lookup: "priority" },
  assigned_analyst_id: { label: "ticket.assignee", lookup: "analyst" },
  analyst_id: { label: "ticket.assignee", lookup: "analyst" },
  assigned_team_id: { label: "ticket.queue", lookup: "team" },
  team_id: { label: "ticket.queue", lookup: "team" },
  resolution_code_id: { label: "ticket.resolutionCode", lookup: "code" },
  subject: { label: "newTicket.subject" },
  sensitivity: { label: "ticket.sensitivity" },
};

/** Fields shown only as "changed": long text, or values without a name here. */
const CHANGED_ONLY: Record<string, MessageId> = {
  description: "ticket.description",
  requester_id: "ticket.requester",
  type_id: "ticket.type",
  category_id: "ticket.category",
  department_id: "ticket.department",
  origin_id: "ticket.origin",
  snoozed_until: "ticket.snooze",
};

/**
 * The ticket timeline (FR-TKT-024): emails, Notes, time and changes, oldest first as the API
 * returns them; Analysts can flip the order. Internal Notes look different from what the
 * Requester sees.
 */
export function Timeline({
  ticketId,
  requesterName,
}: {
  ticketId: string;
  requesterName?: string | undefined;
}) {
  const t = useText();
  const f = useFormat();
  const query = useQuery(timelineQuery(ticketId));
  const [order, setOrder] = useState<"oldest" | "newest">("oldest");
  const analysts = useQuery(analystsQuery);
  const statuses = useQuery(ticketLookupQuery("ticket-statuses"));
  const priorities = useQuery(ticketLookupQuery("ticket-priorities"));
  const codes = useQuery(ticketLookupQuery("resolution-codes"));
  const teams = useQuery(teamsQuery);
  const names: Record<Lookup, Map<string, string>> = {
    analyst: namesById(analysts.data, (a) => a.full_name),
    status: namesById(statuses.data),
    priority: namesById(priorities.data),
    team: namesById(teams.data),
    code: namesById(codes.data),
  };

  function who(actor: Actor): string {
    switch (actor.kind) {
      case "analyst":
        return (actor.id && names.analyst.get(actor.id)) || t("common.analyst");
      case "requester":
        return requesterName ?? t("common.requester");
      case "api_key":
        return t("common.apiKey");
      default:
        return t("common.automation");
    }
  }

  return (
    <section aria-labelledby="timeline-title" className="grid gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id="timeline-title" className="text-[0.9375rem] font-semibold">
          {t("timeline.title")}
        </h3>
        <Segmented
          label={t("timeline.order")}
          value={order}
          onChange={setOrder}
          options={[
            { value: "oldest", label: t("timeline.oldest") },
            { value: "newest", label: t("timeline.newest") },
          ]}
        />
      </div>
      <QueryState query={query}>
        {(items) =>
          items.length === 0 ? (
            <EmptyState icon={<Inbox aria-hidden="true" />} title={t("timeline.empty")} />
          ) : (
            <ol className="relative grid gap-3 before:absolute before:inset-y-2 before:left-[0.9375rem] before:w-px before:bg-line">
              {(order === "oldest" ? items : [...items].reverse()).map((item, index) => {
                const data = item.data as Record<string, unknown>;
                const kind = kindStyle(item.kind, data);
                return (
                  <li
                    key={`${item.at}-${item.kind}-${String(data.id ?? data.message_id ?? index)}`}
                    className="relative grid grid-cols-[2rem_1fr] gap-3"
                  >
                    <span
                      aria-hidden="true"
                      className={cn(
                        "z-10 mt-1 grid size-8 place-items-center rounded-full border border-line bg-surface [&_svg]:size-4",
                        kind.iconTone,
                      )}
                    >
                      {kind.icon}
                    </span>
                    <article className={cn("min-w-0 rounded-lg border p-3", kind.card)}>
                      <header className="mb-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-fg-muted">
                        <span className="font-semibold text-fg">{who(item.actor as Actor)}</span>
                        <Badge tone={kind.badge}>{t(kind.label)}</Badge>
                        <time dateTime={item.at} className="ml-auto">
                          {f.dateTime(item.at)}
                        </time>
                      </header>
                      <Body kind={item.kind} data={data} names={names} />
                    </article>
                  </li>
                );
              })}
            </ol>
          )
        }
      </QueryState>
    </section>
  );
}

function kindStyle(kind: string, data: Record<string, unknown>) {
  if (kind === "email") {
    const out = data.direction === "outbound";
    return {
      label: (out ? "timeline.emailOut" : "timeline.emailIn") as MessageId,
      icon: out ? <Send /> : <Inbox />,
      iconTone: "text-accent-text",
      badge: "accent" as Tone,
      card: "border-line bg-surface",
    };
  }
  if (kind === "note") {
    const isPublic = Boolean(data.is_public);
    return {
      label: (isPublic ? "timeline.publicNote" : "timeline.internalNote") as MessageId,
      icon: isPublic ? <MessageSquare /> : <Lock />,
      iconTone: isPublic ? "text-accent-text" : "text-warning-text",
      badge: (isPublic ? "accent" : "warning") as Tone,
      card: isPublic ? "border-line bg-surface" : "border-warning-text/30 bg-warning-soft/60",
    };
  }
  if (kind === "time_entry") {
    return {
      label: "timeline.time" as MessageId,
      icon: <Clock />,
      iconTone: "text-fg-muted",
      badge: "muted" as Tone,
      card: "border-line bg-sunken/50",
    };
  }
  return {
    label: "timeline.change" as MessageId,
    icon: <ArrowLeftRight />,
    iconTone: "text-fg-muted",
    badge: "muted" as Tone,
    card: "border-line bg-sunken/50",
  };
}

function Body({
  kind,
  data,
  names,
}: {
  kind: string;
  data: Record<string, unknown>;
  names: Record<Lookup, Map<string, string>>;
}) {
  const t = useText();
  if (kind === "email") {
    return (
      <div className="grid gap-1">
        <p className="text-xs text-fg-muted">
          {t("timeline.fromTo", {
            from: String(data.from ?? ""),
            to: Array.isArray(data.to) ? data.to.join(", ") : "",
          })}
        </p>
        <p className="font-medium">{String(data.subject ?? "")}</p>
        <p className="whitespace-pre-wrap leading-relaxed [overflow-wrap:anywhere]">
          {String(data.text_body ?? "")}
        </p>
      </div>
    );
  }
  if (kind === "note") {
    return (
      <p className="whitespace-pre-wrap leading-relaxed [overflow-wrap:anywhere]">
        {String(data.body ?? "")}
      </p>
    );
  }
  if (kind === "time_entry") {
    return (
      <p>
        {t("timeline.minutes", { minutes: Number(data.minutes ?? 0) })}
        {data.note ? ` · ${String(data.note)}` : ""}
      </p>
    );
  }
  if (data.action === "created") return <p>{t("timeline.created")}</p>;
  const changes = (Array.isArray(data.changes) ? data.changes : []) as FieldChange[];
  const lines = changes.flatMap((change) => {
    const shown = SHOWN[change.field];
    if (shown) {
      const raw = change.new_value;
      const value =
        raw === null || raw === undefined || raw === ""
          ? t("timeline.noValue")
          : shown.lookup
            ? (names[shown.lookup].get(String(raw)) ?? "…")
            : String(raw);
      return [
        { key: change.field, text: t("timeline.fieldSet", { field: t(shown.label), value }) },
      ];
    }
    const label = CHANGED_ONLY[change.field];
    return label
      ? [{ key: change.field, text: t("timeline.fieldChanged", { field: t(label) }) }]
      : [];
  });
  if (lines.length === 0) {
    const action = String(data.action ?? "updated");
    return (
      <p>
        {t(
          action === "deleted"
            ? "timeline.deleted"
            : action === "restored"
              ? "timeline.restored"
              : "timeline.updated",
        )}
      </p>
    );
  }
  return (
    <ul className="grid gap-0.5">
      {lines.map((line) => (
        <li key={line.key}>{line.text}</li>
      ))}
    </ul>
  );
}
