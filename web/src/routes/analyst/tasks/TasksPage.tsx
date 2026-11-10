import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { can } from "@/features/navigation/navigation";
import { analystSessionQuery } from "@/features/session/queries";
import {
  moveTask,
  taskBoardQuery,
  taskPrioritiesQuery,
  taskStatusesQuery,
} from "@/features/tasks/api";
import { namesById } from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

type Task = Schemas["TaskDto"];

/** The Task board: a column per Status. Each card has a "Move to" list, so no dragging is needed. */
export function TasksPage() {
  const t = useText();
  const intl = useIntl();
  const queryClient = useQueryClient();
  const session = useQuery(analystSessionQuery).data;
  const [q, setQ] = useState("");
  const [mine, setMine] = useState(false);
  const [showClosed, setShowClosed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const board = useQuery(
    taskBoardQuery({
      ...(q.trim() ? { q: q.trim() } : {}),
      ...(mine ? { mine: "me" } : {}),
      open: !showClosed,
    }),
  );
  const priorities = useQuery(taskPrioritiesQuery);
  const statuses = useQuery(taskStatusesQuery);
  const priorityName = namesById(priorities.data, (p) => p.name);
  const canCreate = session ? can(session, "tasks.create") : false;
  const canUpdate = session ? can(session, "tasks.update") : false;

  async function move(task: Task, statusId: string) {
    const outcome = await moveTask(task.id, statusId);
    if (outcome.ok) {
      setError(null);
      await queryClient.invalidateQueries({ queryKey: ["tasks"] });
    } else {
      setError(t("tasks.moveFailed", { detail: outcome.problem.detail ?? outcome.problem.kind }));
    }
  }

  return (
    <div className="grid gap-4">
      <div className="flex items-center gap-3">
        <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
          {t("nav.tasks")}
        </h1>
        {canCreate ? (
          <Button asChild size="sm" className="ml-auto">
            <Link to="/tasks/new">{t("tasks.new")}</Link>
          </Button>
        ) : null}
      </div>
      <div className="flex flex-wrap items-end gap-3">
        <Field
          label={t("tasks.search")}
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <label className="flex items-center gap-2 pb-2 text-sm">
          <input type="checkbox" checked={mine} onChange={(e) => setMine(e.target.checked)} />
          {t("tasks.mine")}
        </label>
        <label className="flex items-center gap-2 pb-2 text-sm">
          <input
            type="checkbox"
            checked={showClosed}
            onChange={(e) => setShowClosed(e.target.checked)}
          />
          {t("tasks.showClosed")}
        </label>
      </div>
      {error ? <Alert variant="error">{error}</Alert> : null}
      {board.isPending ? <p>{t("common.loading")}</p> : null}
      {board.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {board.data ? (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          {board.data.map((column) => (
            <section
              key={column.status.id}
              aria-labelledby={`col-${column.status.id}`}
              className="grid content-start gap-2 rounded-lg border border-border bg-muted/30 p-3"
            >
              <h2 id={`col-${column.status.id}`} className="flex items-center gap-2 font-semibold">
                {column.status.name}
                <Badge tone="muted">{column.tasks.length}</Badge>
              </h2>
              {column.tasks.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t("tasks.emptyColumn")}</p>
              ) : null}
              <ul className="grid gap-2">
                {column.tasks.map((task) => (
                  <li
                    key={task.id}
                    className="grid gap-2 rounded-md border border-border bg-background p-3"
                  >
                    <Link
                      to="/tasks/$id"
                      params={{ id: task.id }}
                      className="font-medium underline-offset-2 hover:underline"
                    >
                      {task.title}
                    </Link>
                    <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                      <Badge>{priorityName.get(task.priority_id) ?? ""}</Badge>
                      {task.due_date ? (
                        <span>
                          {t("tasks.due", {
                            date: intl.formatDate(task.due_date, { dateStyle: "medium" }),
                          })}
                        </span>
                      ) : null}
                      {task.tags.map((tag) => (
                        <Badge key={tag} tone="muted">
                          {tag}
                        </Badge>
                      ))}
                    </div>
                    {canUpdate ? (
                      <label className="grid gap-1 text-xs">
                        <span className="text-muted-foreground">{t("tasks.moveTo")}</span>
                        <select
                          aria-label={`${t("tasks.moveTo")}: ${task.title}`}
                          className="h-8 rounded-md border border-input bg-background px-2 text-sm"
                          value={task.status_id}
                          onChange={(e) => void move(task, e.target.value)}
                        >
                          {(statuses.data ?? [])
                            .filter((s) => s.is_active)
                            .map((s) => (
                              <option key={s.id} value={s.id}>
                                {s.name}
                              </option>
                            ))}
                        </select>
                      </label>
                    ) : null}
                  </li>
                ))}
              </ul>
              {column.has_more ? (
                <p className="text-xs text-muted-foreground">{t("tasks.hasMore")}</p>
              ) : null}
            </section>
          ))}
        </div>
      ) : null}
    </div>
  );
}
