import { useQuery } from "@tanstack/react-query";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import {
  analystsQuery,
  namesById,
  prioritiesQuery,
  resolutionCodesQuery,
  statusesQuery,
  teamsQuery,
  timelineQuery,
} from "@/features/tickets/queries";
import { describeChange, type FieldChange, type TimelineActor } from "@/features/tickets/timeline";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";

/**
 * The ticket timeline (FR-TKT-024): emails, Notes, Time Entries and changes, oldest
 * first, as the API returns them.
 */
export function Timeline({ ticketId }: { ticketId: string }) {
  const t = useText();
  const intl = useIntl();
  const items = useQuery(timelineQuery(ticketId));
  const analysts = useQuery(analystsQuery);
  const statuses = useQuery(statusesQuery);
  const priorities = useQuery(prioritiesQuery);
  const teams = useQuery(teamsQuery);
  const codes = useQuery(resolutionCodesQuery);
  const lookups = {
    analyst: namesById(analysts.data, (a) => a.full_name),
    status: namesById(statuses.data, (s) => s.name),
    priority: namesById(priorities.data, (p) => p.name),
    team: namesById(teams.data, (q) => q.name),
    resolution: namesById(codes.data, (c) => c.name),
  };

  function who(actor: TimelineActor): string {
    switch (actor.kind) {
      case "analyst":
        return (actor.id && lookups.analyst.get(actor.id)) || t("timeline.anAnalyst");
      case "requester":
        return t("timeline.requester");
      case "api_key":
        return t("timeline.apiKey");
      default:
        return t("timeline.automation");
    }
  }

  return (
    <section aria-labelledby="timeline-title" className="grid gap-3">
      <h3 id="timeline-title" className="text-base font-semibold">
        {t("timeline.title")}
      </h3>
      {items.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {items.isPending ? (
        <p role="status" className="text-sm text-muted-foreground">
          {t("common.loading")}
        </p>
      ) : (
        <ol className="grid gap-3">
          {(items.data ?? []).map((item, index) => {
            const actor = item.actor as TimelineActor;
            const data = item.data as Record<string, unknown>;
            const when = intl.formatDate(item.at, { dateStyle: "medium", timeStyle: "short" });
            return (
              // Items have no common ID; the API's order is stable.
              // biome-ignore lint/suspicious/noArrayIndexKey: stable server order
              <li key={index} className="rounded-md border border-border p-3 text-sm">
                <div className="mb-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                  <span className="font-medium text-foreground">{who(actor)}</span>
                  <Kind kind={item.kind} data={data} />
                  <time dateTime={item.at}>{when}</time>
                </div>
                <Body kind={item.kind} data={data} lookups={lookups} />
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}

const KIND_LABELS: Record<string, MessageId> = {
  email: "timeline.email",
  note: "timeline.note",
  time_entry: "timeline.time",
  change: "timeline.change",
};

function Kind({ kind, data }: { kind: string; data: Record<string, unknown> }) {
  const t = useText();
  if (kind === "email") {
    return (
      <Badge tone="neutral">
        {data.direction === "outbound" ? t("timeline.emailOut") : t("timeline.emailIn")}
      </Badge>
    );
  }
  if (kind === "note") {
    return (
      <Badge tone={data.is_public ? "neutral" : "muted"}>
        {data.is_public ? t("timeline.publicNote") : t("timeline.internalNote")}
      </Badge>
    );
  }
  return <Badge tone="muted">{t(KIND_LABELS[kind] ?? "timeline.change")}</Badge>;
}

function Body({
  kind,
  data,
  lookups,
}: {
  kind: string;
  data: Record<string, unknown>;
  lookups: Parameters<typeof describeChange>[1];
}) {
  const t = useText();
  switch (kind) {
    case "email":
      return (
        <div className="grid gap-1">
          <p className="text-xs text-muted-foreground">
            {t("timeline.fromTo", {
              from: String(data.from ?? ""),
              to: Array.isArray(data.to) ? data.to.join(", ") : "",
            })}
          </p>
          <p className="font-medium">{String(data.subject ?? "")}</p>
          <p className="whitespace-pre-wrap">{String(data.text_body ?? "")}</p>
        </div>
      );
    case "note":
      return <p className="whitespace-pre-wrap">{String(data.body ?? "")}</p>;
    case "time_entry":
      return (
        <p>
          {t("timeline.minutes", { minutes: Number(data.minutes ?? 0) })}
          {data.note ? ` — ${String(data.note)}` : ""}
        </p>
      );
    default: {
      const changes = (Array.isArray(data.changes) ? data.changes : []) as FieldChange[];
      const lines = changes.flatMap((c) => {
        const line = describeChange(c, lookups);
        return line ? [line] : [];
      });
      if (data.action === "created") {
        return <p>{t("timeline.created")}</p>;
      }
      if (lines.length === 0) {
        return <p>{t(ACTION_LABELS[String(data.action)] ?? "timeline.updated")}</p>;
      }
      return (
        <ul className="grid gap-0.5">
          {lines.map((l) => (
            <li key={l.field}>
              {l.value === undefined
                ? t("timeline.fieldChanged", { field: t(l.label) })
                : t("timeline.fieldSet", {
                    field: t(l.label),
                    value: l.value || t("timeline.empty"),
                  })}
            </li>
          ))}
        </ul>
      );
    }
  }
}

const ACTION_LABELS: Record<string, MessageId> = {
  updated: "timeline.updated",
  deleted: "timeline.deleted",
  restored: "timeline.restored",
};
