import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { modulesQuery } from "@/features/navigation/modules";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import { analystSessionQuery } from "@/features/session/queries";
import { linkTask, taskSearchQuery, ticketTasksQuery, unlinkTask } from "@/features/tasks/api";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";

type Task = Schemas["TaskDto"];

/**
 * Tasks linked to a ticket, with linking and unlinking (FR-TSK-004, FR-TKT-030). Shown
 * when the Tasks Module is on and the Analyst may read Tasks; linking needs
 * `tasks.update`. The API checks every request; hiding only tidies the screen.
 */
export function TicketTasks({ ticket }: { ticket: Schemas["TicketDto"] }) {
  const session = useQuery(analystSessionQuery);
  const modules = useQuery(modulesQuery);
  const s = session.data;
  const tasksOn = modules.data?.some((m) => m.key === "tasks" && m.is_enabled) ?? false;
  if (!s || !tasksOn || !can(s, "tasks.read")) {
    return null;
  }
  return <TicketTasksSection ticket={ticket} canUpdate={can(s, "tasks.update")} />;
}

function TicketTasksSection({
  ticket,
  canUpdate,
}: {
  ticket: Schemas["TicketDto"];
  canUpdate: boolean;
}) {
  const t = useText();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const linked = useQuery(ticketTasksQuery(ticket.id));
  const editable = canUpdate && !ticket.deleted_at;

  async function refresh() {
    await queryClient.invalidateQueries({ queryKey: ["tasks"] });
  }

  async function link(task: Task) {
    const outcome = await linkTask(task.id, { kind: "ticket", target_id: ticket.id });
    if (!outcome.ok) {
      flash.fail(t("tasks.linkFailed", { detail: problemText(outcome.problem) }));
      return;
    }
    flash.clear();
    await refresh();
  }

  async function unlink(task: Task) {
    const outcome = await unlinkTask(task.id, "ticket", ticket.id);
    if (!outcome.ok) {
      flash.fail(t("tasks.unlinkFailed", { detail: problemText(outcome.problem) }));
      return;
    }
    flash.clear();
    await refresh();
  }

  const linkedIds = new Set((linked.data ?? []).map((k) => k.id));
  return (
    <section aria-labelledby="ticket-tasks" className="grid gap-2">
      <h3 id="ticket-tasks" className="text-sm font-medium text-muted-foreground">
        {t("ticketTasks.title")}
      </h3>
      {linked.isSuccess && linked.data.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("ticketTasks.none")}</p>
      ) : null}
      <ul className="grid gap-1">
        {(linked.data ?? []).map((task) => (
          <li key={task.id} className="flex flex-wrap items-center gap-2 text-sm">
            <Link
              to="/tasks/$id"
              params={{ id: task.id }}
              className="font-medium underline-offset-2 hover:underline"
            >
              {task.title}
            </Link>
            <Badge tone={task.is_closed ? "muted" : "neutral"}>
              {task.is_closed ? t("ticketTasks.closed") : t("ticketTasks.open")}
            </Badge>
            {editable ? (
              <Button
                size="sm"
                variant="ghost"
                aria-label={t("tasks.unlinkNamed", { name: task.title })}
                onClick={() => void unlink(task)}
              >
                {t("tasks.unlink")}
              </Button>
            ) : null}
          </li>
        ))}
      </ul>
      {editable ? (
        <TaskFinder companyId={ticket.company_id} exclude={linkedIds} onPick={link} />
      ) : null}
      {flash.element}
    </section>
  );
}

/** Searches open Tasks by title and offers those of the ticket's Company to link. */
function TaskFinder({
  companyId,
  exclude,
  onPick,
}: {
  companyId: string;
  exclude: ReadonlySet<string>;
  onPick: (task: Task) => Promise<void>;
}) {
  const t = useText();
  const [text, setText] = useState("");
  const [q, setQ] = useState("");
  const found = useQuery(taskSearchQuery(q));
  // Links are only allowed within a Company; the server enforces it, this narrows the list.
  const matches = (found.data ?? []).filter(
    (k) => k.company_id === companyId && !exclude.has(k.id),
  );

  function submit(event: FormEvent) {
    event.preventDefault();
    setQ(text.trim());
  }

  return (
    <div className="grid gap-2">
      <form onSubmit={submit} className="flex flex-wrap items-end gap-2">
        <Field
          label={t("ticketTasks.find")}
          hint={t("ticketTasks.findHint")}
          type="search"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <Button type="submit" size="sm" variant="outline">
          {t("ticketTasks.search")}
        </Button>
      </form>
      {q && found.isSuccess ? (
        matches.length === 0 ? (
          <p role="status" className="text-sm text-muted-foreground">
            {t("ticketTasks.noMatches")}
          </p>
        ) : (
          <ul aria-label={t("ticketTasks.results")} className="grid gap-1">
            {matches.map((task) => (
              <li key={task.id} className="flex items-center gap-2 text-sm">
                <span>{task.title}</span>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={t("ticketTasks.linkNamed", { name: task.title })}
                  onClick={() => void onPick(task)}
                >
                  {t("tasks.link")}
                </Button>
              </li>
            ))}
          </ul>
        )
      ) : null}
    </div>
  );
}
