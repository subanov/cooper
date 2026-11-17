import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import { analystSessionQuery } from "@/features/session/queries";
import { linkTask, taskLinksQuery, unlinkTask } from "@/features/tasks/api";
import { findTicketByNumber } from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";

/**
 * Tickets and Changes linked to a Task (FR-TSK-004). A ticket is linked by its Ticket
 * Number; linking and removing need `tasks.update`.
 */
export function TaskLinks({ taskId }: { taskId: string }) {
  const t = useText();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const session = useQuery(analystSessionQuery);
  const links = useQuery(taskLinksQuery(taskId));
  const [number, setNumber] = useState("");
  const canUpdate = session.data ? can(session.data, "tasks.update") : false;

  async function refresh() {
    await queryClient.invalidateQueries({ queryKey: ["tasks"] });
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const wanted = number.trim();
    if (!wanted) {
      return;
    }
    const ticket = await findTicketByNumber(wanted);
    if (!ticket) {
      flash.fail(t("tasks.ticketNotFound", { number: wanted }));
      return;
    }
    const outcome = await linkTask(taskId, { kind: "ticket", target_id: ticket.id });
    if (!outcome.ok) {
      flash.fail(t("tasks.linkFailed", { detail: problemText(outcome.problem) }));
      return;
    }
    setNumber("");
    flash.clear();
    await refresh();
  }

  async function remove(link: Schemas["TaskLinkDto"]) {
    const outcome = await unlinkTask(taskId, link.kind, link.target_id);
    if (!outcome.ok) {
      flash.fail(t("tasks.unlinkFailed", { detail: problemText(outcome.problem) }));
      return;
    }
    flash.clear();
    await refresh();
  }

  return (
    <section aria-labelledby="task-links" className="grid gap-3">
      <h2 id="task-links" className="text-lg font-semibold">
        {t("tasks.links")}
      </h2>
      {links.isSuccess && links.data.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("tasks.noLinks")}</p>
      ) : null}
      <ul className="grid gap-1">
        {(links.data ?? []).map((l) => (
          <li key={`${l.kind}-${l.target_id}`} className="flex items-center gap-3 text-sm">
            <span className="text-muted-foreground">
              {l.kind === "ticket" ? t("tasks.linkTicket") : t("tasks.linkChange")}
            </span>
            {l.kind === "ticket" ? (
              <Link
                to="/tickets/$ticketId"
                params={{ ticketId: l.target_id }}
                className="font-medium underline-offset-2 hover:underline"
              >
                {l.number}
              </Link>
            ) : (
              <span className="font-medium">{l.number}</span>
            )}
            {canUpdate ? (
              <Button
                size="sm"
                variant="ghost"
                aria-label={t("tasks.unlinkNamed", { name: l.number })}
                onClick={() => void remove(l)}
              >
                {t("tasks.unlink")}
              </Button>
            ) : null}
          </li>
        ))}
      </ul>
      {canUpdate ? (
        <form onSubmit={submit} className="flex flex-wrap items-end gap-3">
          <Field
            label={t("tasks.ticketNumber")}
            value={number}
            onChange={(e) => setNumber(e.target.value)}
          />
          <Button type="submit" size="sm" variant="outline">
            {t("tasks.link")}
          </Button>
        </form>
      ) : null}
      {flash.element}
    </section>
  );
}
