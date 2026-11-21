import { useQuery } from "@tanstack/react-query";
import { namesById, ticketLookupQuery } from "@/features/reference/queries";
import { useSession } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { useFormat, useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { type Ticket, ticketActions, ticketRequest } from "./api";

const REFRESH = [["tickets"], ["home"]] as const;

/**
 * The triage actions on one ticket, shared by the list's quick actions and the ticket
 * screen, so both behave and read the same (H4). Reversible changes offer Undo (H3).
 */
export function useTicketActions(ticket: Ticket, onDone?: () => void) {
  const t = useText();
  const f = useFormat();
  const session = useSession();
  const statuses = useQuery(ticketLookupQuery("ticket-statuses"));
  const priorities = useQuery(ticketLookupQuery("ticket-priorities"));
  const statusNames = namesById(statuses.data);
  const me = session.analyst.id;
  const before = { team_id: ticket.team_id ?? null, analyst_id: ticket.analyst_id ?? null };

  const assign = useAction(
    (body: { team_id: string | null; analyst_id: string | null }) =>
      ticketActions.assign(ticket.id, body),
    {
      invalidate: REFRESH,
      success: (_, vars) =>
        vars.analyst_id === me ? t("ticket.assigned") : t("ticket.assignSaved"),
      undo: () => ({ label: t("common.undo"), run: (): void => assign.mutate(before) }),
      onSuccess: onDone,
    },
  );

  const previousStatus = statuses.data?.find((s) => s.id === ticket.status_id);
  const status = useAction(
    (body: Schemas["StatusRequest"]) => ticketActions.status(ticket.id, body),
    {
      invalidate: REFRESH,
      success: (updated) =>
        t("ticket.statusSaved", { status: statusNames.get(updated.status_id) ?? "" }),
      undo: (_, vars) =>
        previousStatus && !previousStatus.is_closed && vars.status_id !== ticket.status_id
          ? {
              label: t("common.undo"),
              run: (): void =>
                status.mutate({
                  status_id: ticket.status_id,
                  resolution_code_id: null,
                  closure_category_id: null,
                }),
            }
          : undefined,
      onSuccess: onDone,
    },
  );

  const update = useAction(
    (body: Schemas["TicketRequest"]) => ticketActions.update(ticket.id, body),
    {
      invalidate: REFRESH,
      success: "ticket.editSaved",
      onSuccess: onDone,
    },
  );

  const priority = useAction(
    (priorityId: string) =>
      ticketActions.update(ticket.id, { ...ticketRequest(ticket), priority_id: priorityId }),
    {
      invalidate: REFRESH,
      success: (_, id) =>
        t("ticket.prioritySaved", { priority: namesById(priorities.data).get(id) ?? "" }),
      undo: () => ({
        label: t("common.undo"),
        run: (): void => priority.mutate(ticket.priority_id),
      }),
      onSuccess: onDone,
    },
  );

  const snooze = useAction(
    (body: { until: string; reason: string }) => ticketActions.snooze(ticket.id, body),
    {
      invalidate: REFRESH,
      success: (_, vars) => t("ticket.snoozed", { date: f.dateTime(vars.until) }),
      undo: () => ({ label: t("common.undo"), run: (): void => wake.mutate() }),
      onSuccess: onDone,
    },
  );

  const wake = useAction(() => ticketActions.wake(ticket.id), {
    invalidate: REFRESH,
    success: "ticket.woken",
  });

  const logTime = useAction(
    (body: { minutes: number; note: string; work_date: string | null }) =>
      ticketActions.logTime(ticket.id, body),
    {
      invalidate: [["tickets", "detail", ticket.id]],
      success: (_, vars) => t("ticket.timeLogged", { minutes: vars.minutes }),
      onSuccess: onDone,
    },
  );

  const restore = useAction(() => ticketActions.restore(ticket.id), {
    invalidate: REFRESH,
    success: "ticket.restored",
  });

  return {
    me,
    assign,
    assignToMe: () => assign.mutate({ analyst_id: me, team_id: ticket.team_id ?? null }),
    status,
    update,
    priority,
    snooze,
    wake,
    logTime,
    restore,
  };
}
