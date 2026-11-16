import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { SelectField } from "@/components/ui/select-field";
import { Textarea } from "@/components/ui/textarea";
import { problemText } from "@/features/result";
import {
  addTaskComment,
  createTask,
  taskCommentsQuery,
  taskPrioritiesQuery,
  taskQuery,
  taskStatusesQuery,
  updateTask,
} from "@/features/tasks/api";
import { analystsQuery } from "@/features/tickets/queries";
import { companiesQuery, defaultCompanyId } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";
import { TaskLinks } from "./TaskLinks";

type Task = Schemas["TaskDto"];

/** Creates a Task (`id` absent) or edits one, with its links and comments. */
export function TaskEditor({ id }: { id?: string | undefined }) {
  const t = useText();
  const existing = useQuery({ ...taskQuery(id ?? ""), enabled: Boolean(id) });
  if (id && existing.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (id && !existing.data) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  return (
    <div className="grid max-w-2xl gap-6">
      <Link to="/tasks" className="text-sm underline-offset-2 hover:underline">
        {t("tasks.back")}
      </Link>
      <TaskForm key={existing.data?.updated_at ?? "new"} task={existing.data} />
      {existing.data ? <TaskLinks taskId={existing.data.id} /> : null}
      {existing.data ? <Comments taskId={existing.data.id} /> : null}
    </div>
  );
}

function TaskForm({ task }: { task: Task | undefined }) {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const statuses = useQuery(taskStatusesQuery);
  const priorities = useQuery(taskPrioritiesQuery);
  const analysts = useQuery(analystsQuery);
  const companies = useQuery(companiesQuery);
  const [title, setTitle] = useState(task?.title ?? "");
  const [description, setDescription] = useState(task?.description ?? "");
  const [statusId, setStatusId] = useState(task?.status_id ?? "");
  const [priorityId, setPriorityId] = useState(task?.priority_id ?? "");
  const [assignee, setAssignee] = useState(task?.assigned_analyst_id ?? "");
  const [due, setDue] = useState(task?.due_date?.slice(0, 10) ?? "");
  const [tags, setTags] = useState((task?.tags ?? []).join(", "));
  const [titleError, setTitleError] = useState<string | undefined>();

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim()) {
      setTitleError(t("tasks.titleRequired"));
      return;
    }
    setTitleError(undefined);
    const company = defaultCompanyId(companies.data);
    const body: Schemas["TaskRequest"] = {
      title: title.trim(),
      description: description.trim(),
      status_id: statusId || null,
      priority_id: priorityId || null,
      assigned_analyst_id: assignee || null,
      due_date: due || null,
      tags: tags
        .split(",")
        .map((x) => x.trim())
        .filter(Boolean),
      ...(!task && company ? { company_id: company } : {}),
    };
    const outcome = task ? await updateTask(task.id, body) : await createTask(body);
    if (!outcome.ok) {
      flash.fail(t("tasks.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["tasks"] });
    if (task) {
      flash.ok(t("users.saved"));
    } else {
      await navigate({ to: "/tasks/$id", params: { id: outcome.data.id } });
    }
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-3" aria-labelledby="task-title">
      <h1 id="task-title" tabIndex={-1} className="text-2xl font-semibold outline-none">
        {task ? task.title : t("tasks.new")}
      </h1>
      <Field
        label={t("tasks.title")}
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        error={titleError}
        required
      />
      <div className="grid gap-1.5">
        <Label htmlFor="task-description">{t("tasks.description")}</Label>
        <Textarea
          id="task-description"
          rows={4}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <SelectField
          label={t("tasks.status")}
          value={statusId}
          onChange={(e) => setStatusId(e.target.value)}
        >
          <option value="">{t("newTicket.useDefault")}</option>
          {(statuses.data ?? [])
            .filter((s) => s.is_active)
            .map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
        </SelectField>
        <SelectField
          label={t("tasks.priority")}
          value={priorityId}
          onChange={(e) => setPriorityId(e.target.value)}
        >
          <option value="">{t("newTicket.useDefault")}</option>
          {(priorities.data ?? [])
            .filter((p) => p.is_active)
            .map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
        </SelectField>
        <SelectField
          label={t("newTicket.analyst")}
          value={assignee}
          onChange={(e) => setAssignee(e.target.value)}
        >
          <option value="">{t("tickets.unassigned")}</option>
          {(analysts.data ?? []).map((a) => (
            <option key={a.id} value={a.id}>
              {a.full_name}
            </option>
          ))}
        </SelectField>
        <Field
          label={t("tasks.dueDate")}
          type="date"
          value={due}
          onChange={(e) => setDue(e.target.value)}
        />
      </div>
      <Field
        label={t("tasks.tags")}
        hint={t("tasks.tagsHint")}
        value={tags}
        onChange={(e) => setTags(e.target.value)}
      />
      <div className="flex items-center gap-3">
        <Button type="submit">{task ? t("users.save") : t("users.create")}</Button>
        {flash.element}
      </div>
    </form>
  );
}

function Comments({ taskId }: { taskId: string }) {
  const t = useText();
  const intl = useIntl();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const comments = useQuery(taskCommentsQuery(taskId));
  const [body, setBody] = useState("");

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!body.trim()) {
      return;
    }
    const outcome = await addTaskComment(taskId, body.trim());
    if (!outcome.ok) {
      flash.fail(t("tasks.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    setBody("");
    flash.clear();
    await queryClient.invalidateQueries({ queryKey: taskCommentsQuery(taskId).queryKey });
  }

  return (
    <section aria-labelledby="task-comments" className="grid gap-3">
      <h2 id="task-comments" className="text-lg font-semibold">
        {t("tasks.comments")}
      </h2>
      {(comments.data ?? []).length === 0 && comments.isSuccess ? (
        <p className="text-sm text-muted-foreground">{t("tasks.noComments")}</p>
      ) : null}
      <ul className="grid gap-2">
        {(comments.data ?? []).map((c) => (
          <li key={c.id} className="rounded-md border border-border p-3 text-sm">
            <p className="whitespace-pre-wrap">{c.body}</p>
            <p className="mt-1 text-xs text-muted-foreground">
              {intl.formatDate(c.created_at, { dateStyle: "medium", timeStyle: "short" })}
            </p>
          </li>
        ))}
      </ul>
      <form onSubmit={submit} className="grid gap-2">
        <Label htmlFor="task-comment">{t("tasks.addComment")}</Label>
        <Textarea
          id="task-comment"
          rows={3}
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
        <div className="flex items-center gap-3">
          <Button type="submit" size="sm" variant="outline">
            {t("tasks.postComment")}
          </Button>
          {flash.element}
        </div>
      </form>
    </section>
  );
}
