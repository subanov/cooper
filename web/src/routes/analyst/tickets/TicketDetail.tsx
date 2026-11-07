import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, type Ref, useEffect, useImperativeHandle, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { analystSessionQuery } from "@/features/session/queries";
import { addNote, assignTicket, changeStatus, sendReply } from "@/features/tickets/actions";
import {
  analystsQuery,
  namesById,
  prioritiesQuery,
  requesterNamesQuery,
  resolutionCodesQuery,
  statusesQuery,
  teamsQuery,
  ticketQuery,
} from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import type { Problem } from "@/lib/problem";
import { useKeys } from "../useShortcuts";
import { SlaTargetBadge } from "./SlaBadge";
import { Timeline } from "./Timeline";

type Ticket = Schemas["TicketDto"];

/**
 * One ticket in the split pane: facts, SLA targets, triage actions (assign, status),
 * the reply/Note composer and the timeline. `a` assigns to me, `r` replies, `n` notes.
 */
export function TicketDetail({ ticketId }: { ticketId: string }) {
  const t = useText();
  const intl = useIntl();
  const queryClient = useQueryClient();
  const ticket = useQuery(ticketQuery(ticketId));
  const session = useQuery(analystSessionQuery);
  const statuses = useQuery(statusesQuery);
  const priorities = useQuery(prioritiesQuery);
  const analysts = useQuery(analystsQuery);
  const teams = useQuery(teamsQuery);
  const requesterId = ticket.data?.requester_id;
  const requesters = useQuery(requesterNamesQuery(requesterId ? [requesterId] : []));
  const [problem, setProblem] = useState<Problem | null>(null);
  const composer = useRef<ComposerHandle>(null);

  async function updated(result: { ok: true; data: Ticket } | { ok: false; problem: Problem }) {
    if (!result.ok) {
      setProblem(result.problem);
      return false;
    }
    setProblem(null);
    queryClient.setQueryData(ticketQuery(ticketId).queryKey, result.data);
    await queryClient.invalidateQueries({ queryKey: ["tickets"] });
    return true;
  }

  const me = session.data?.analyst.id;
  async function assignToMe() {
    if (!ticket.data || !me || ticket.data.analyst_id === me) return;
    await updated(
      await assignTicket(ticketId, { analyst_id: me, team_id: ticket.data.team_id ?? null }),
    );
  }

  useKeys({
    a: () => void assignToMe(),
    r: () => composer.current?.focus("reply"),
    n: () => composer.current?.focus("note"),
  });

  const tk = ticket.data;
  const date = (at: string) => intl.formatDate(at, { dateStyle: "medium", timeStyle: "short" });
  // The heading renders at once (also while loading), so focus can move to it on navigation.
  const header = (
    <header className="grid gap-2">
      <p className="font-mono text-sm text-muted-foreground">{tk?.number ?? "\u00a0"}</p>
      <h2
        id="ticket-subject"
        tabIndex={-1}
        data-page-heading
        className="text-xl font-semibold outline-none"
      >
        {tk ? tk.subject : ticket.isError ? t("ticket.unavailable") : t("common.loading")}
      </h2>
      {tk ? (
        <div className="flex flex-wrap gap-2">
          <Badge>{namesById(statuses.data, (s) => s.name).get(tk.status_id) ?? "…"}</Badge>
          <Badge tone="muted">
            {namesById(priorities.data, (p) => p.name).get(tk.priority_id) ?? "…"}
          </Badge>
          {tk.sensitivity === "confidential" ? (
            <Badge tone="warning">{t("tickets.confidential")}</Badge>
          ) : null}
          {tk.snoozed_until ? (
            <Badge tone="muted">{t("ticket.snoozedUntil", { date: date(tk.snoozed_until) })}</Badge>
          ) : null}
        </div>
      ) : null}
    </header>
  );
  if (!tk) {
    return (
      <article aria-labelledby="ticket-subject" className="grid content-start gap-5">
        {header}
        {ticket.isError ? <Alert variant="error">{t("ticket.notFound")}</Alert> : null}
      </article>
    );
  }
  const analystName = tk.analyst_id
    ? (namesById(analysts.data, (a) => a.full_name).get(tk.analyst_id) ?? "…")
    : t("tickets.unassigned");
  const teamName = tk.team_id
    ? (namesById(teams.data, (q) => q.name).get(tk.team_id) ?? "…")
    : t("ticket.noQueue");
  const requester = requesterId ? requesters.data?.[requesterId] : undefined;

  return (
    <article aria-labelledby="ticket-subject" className="grid content-start gap-5">
      {header}

      <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1.5 text-sm">
        <dt className="text-muted-foreground">{t("ticket.requester")}</dt>
        <dd>
          {requester
            ? `${requester.full_name} <${requester.email ?? ""}>`
            : tk.requester_id
              ? "…"
              : "—"}
        </dd>
        <dt className="text-muted-foreground">{t("ticket.assignee")}</dt>
        <dd>{analystName}</dd>
        <dt className="text-muted-foreground">{t("ticket.queue")}</dt>
        <dd>{teamName}</dd>
        <dt className="text-muted-foreground">{t("ticket.created")}</dt>
        <dd>{date(tk.created_at)}</dd>
        <dt className="text-muted-foreground">{t("ticket.slaResponse")}</dt>
        <dd>
          <SlaTargetBadge target={tk.sla.response} />
          {tk.sla.response.due_at ? (
            <span className="ml-2 text-muted-foreground">
              {t("ticket.due", { date: date(tk.sla.response.due_at) })}
            </span>
          ) : null}
        </dd>
        <dt className="text-muted-foreground">{t("ticket.slaResolution")}</dt>
        <dd>
          <SlaTargetBadge target={tk.sla.resolution} />
          {tk.sla.resolution.due_at ? (
            <span className="ml-2 text-muted-foreground">
              {t("ticket.due", { date: date(tk.sla.resolution.due_at) })}
            </span>
          ) : null}
        </dd>
      </dl>

      {problem ? <Alert variant="error">{problem.detail ?? t("common.error")}</Alert> : null}

      <div className="flex flex-wrap items-end gap-3">
        <Button
          variant="outline"
          onClick={() => void assignToMe()}
          disabled={!me || tk.analyst_id === me || Boolean(tk.deleted_at)}
          aria-keyshortcuts="a"
        >
          {t("ticket.assignToMe")}
        </Button>
        <StatusForm ticket={tk} onDone={updated} />
      </div>

      <section aria-labelledby="ticket-description" className="grid gap-1">
        <h3 id="ticket-description" className="text-sm font-medium text-muted-foreground">
          {t("ticket.description")}
        </h3>
        <p className="whitespace-pre-wrap text-sm">{tk.description}</p>
      </section>

      <Composer ref={composer} ticketId={ticketId} />
      <Timeline ticketId={ticketId} />
    </article>
  );
}

/** Moves the ticket to another Status; a closed Status asks for a Resolution Code. */
function StatusForm({
  ticket,
  onDone,
}: {
  ticket: Ticket;
  onDone: (
    result: { ok: true; data: Ticket } | { ok: false; problem: Problem },
  ) => Promise<boolean>;
}) {
  const t = useText();
  const statuses = useQuery(statusesQuery);
  const codes = useQuery(resolutionCodesQuery);
  const [statusId, setStatusId] = useState(ticket.status_id);
  const [codeId, setCodeId] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => setStatusId(ticket.status_id), [ticket.status_id]);
  const target = statuses.data?.find((s) => s.id === statusId);
  const closing = Boolean(target?.is_closed) && statusId !== ticket.status_id;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    await onDone(
      await changeStatus(ticket.id, {
        status_id: statusId,
        resolution_code_id: closing ? codeId || null : null,
      }),
    );
    setBusy(false);
  }

  return (
    <form onSubmit={submit} className="flex flex-wrap items-end gap-2">
      <div className="grid gap-1">
        <Label htmlFor="ticket-status">{t("ticket.status")}</Label>
        <Select id="ticket-status" value={statusId} onChange={(e) => setStatusId(e.target.value)}>
          {(statuses.data ?? [])
            .filter((s) => s.is_active || s.id === ticket.status_id)
            .map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
        </Select>
      </div>
      {closing ? (
        <div className="grid gap-1">
          <Label htmlFor="ticket-resolution">{t("ticket.resolutionCode")}</Label>
          <Select
            id="ticket-resolution"
            required
            value={codeId}
            onChange={(e) => setCodeId(e.target.value)}
          >
            <option value="">{t("ticket.choose")}</option>
            {(codes.data ?? [])
              .filter((c) => c.is_active)
              .map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
          </Select>
        </div>
      ) : null}
      <Button type="submit" disabled={busy || statusId === ticket.status_id}>
        {t("ticket.changeStatus")}
      </Button>
    </form>
  );
}

/** Lets `r` and `n` open the composer in reply or Note mode. */
export interface ComposerHandle {
  focus: (mode: "reply" | "note") => void;
}

function Composer({ ticketId, ref }: { ticketId: string; ref: Ref<ComposerHandle> }) {
  const t = useText();
  const queryClient = useQueryClient();
  const [mode, setMode] = useState<"reply" | "note">("reply");
  const [body, setBody] = useState("");
  const [isPublic, setIsPublic] = useState(false);
  const [state, setState] = useState<"idle" | "busy" | "sent" | "error">("idle");
  const [error, setError] = useState<string | undefined>();
  const textarea = useRef<HTMLTextAreaElement>(null);

  useImperativeHandle(ref, () => ({
    focus: (next) => {
      setMode(next);
      textarea.current?.focus();
    },
  }));

  async function submit(event: FormEvent) {
    event.preventDefault();
    setState("busy");
    const result =
      mode === "reply"
        ? await sendReply(ticketId, { body })
        : await addNote(ticketId, { body, is_public: isPublic });
    if (result.ok) {
      setBody("");
      setIsPublic(false);
      setState("sent");
      await queryClient.invalidateQueries({ queryKey: ["tickets"] });
    } else {
      setError(result.problem.detail);
      setState("error");
    }
  }

  return (
    <section
      aria-labelledby="composer-title"
      className="grid gap-2 rounded-md border border-border p-3"
    >
      <h3 id="composer-title" className="sr-only">
        {t("composer.title")}
      </h3>
      <fieldset className="flex gap-4 text-sm">
        <legend className="sr-only">{t("composer.mode")}</legend>
        <label className="flex items-center gap-2">
          <input
            type="radio"
            name="composer-mode"
            checked={mode === "reply"}
            onChange={() => setMode("reply")}
          />
          {t("composer.reply")}
        </label>
        <label className="flex items-center gap-2">
          <input
            type="radio"
            name="composer-mode"
            checked={mode === "note"}
            onChange={() => setMode("note")}
          />
          {t("composer.note")}
        </label>
      </fieldset>
      <form onSubmit={submit} className="grid gap-2">
        <Label htmlFor="composer-body" className="sr-only">
          {mode === "reply" ? t("composer.replyLabel") : t("composer.noteLabel")}
        </Label>
        <Textarea
          ref={textarea}
          id="composer-body"
          required
          value={body}
          placeholder={mode === "reply" ? t("composer.replyLabel") : t("composer.noteLabel")}
          onChange={(e) => {
            setBody(e.target.value);
            if (state !== "busy") setState("idle");
          }}
        />
        <div className="flex flex-wrap items-center gap-3">
          {mode === "note" ? (
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={isPublic}
                onChange={(e) => setIsPublic(e.target.checked)}
              />
              {t("composer.public")}
            </label>
          ) : (
            <span className="text-xs text-muted-foreground">{t("composer.replyHint")}</span>
          )}
          <Button type="submit" className="ml-auto" disabled={state === "busy" || !body.trim()}>
            {mode === "reply" ? t("composer.send") : t("composer.addNote")}
          </Button>
        </div>
        <p role="status" className="text-sm">
          {state === "sent" ? t("composer.done") : ""}
        </p>
        {state === "error" ? <Alert variant="error">{error ?? t("common.error")}</Alert> : null}
      </form>
    </section>
  );
}
