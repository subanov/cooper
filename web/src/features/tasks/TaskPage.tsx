import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { Plus, Unlink } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, buttonClass, IconButton } from "@/components/Button";
import { Card } from "@/components/Card";
import type { PickerOption } from "@/components/Combobox";
import { FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { Page, PageHeader } from "@/components/Page";
import { LoadingBlock, LoadMore, QueryState } from "@/components/QueryState";
import { TicketPicker } from "@/features/reference/pickers";
import { analystsQuery, companiesQuery, teamsQuery, toOptions } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { HistoryTable } from "@/features/shared/History";
import { fieldError, useAction } from "@/features/shared/mutation";
import { parseTags, RecordForm } from "@/features/shared/RecordForm";
import { useFormat, useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import {
  subtasksQuery,
  type Task,
  taskActions,
  taskCommentsQuery,
  taskHistoryQuery,
  taskLinksQuery,
  taskPrioritiesQuery,
  taskQuery,
  taskStatusesQuery,
} from "./api";

/** A Task (FR-TSK-001…004): details, comments, links, subtasks and history. */
export function TaskPage({ id }: { id?: string }) {
  const query = useQuery({ ...taskQuery(id ?? ""), enabled: Boolean(id) });
  if (!id) return <TaskEditor />;
  return (
    <QueryState
      query={query}
      loading={
        <Page>
          <LoadingBlock lines={8} />
        </Page>
      }
    >
      {(task) => <TaskEditor key={task.updated_at} task={task} />}
    </QueryState>
  );
}

function toForm(task?: Task) {
  return {
    title: task?.title ?? "",
    description: task?.description ?? "",
    status_id: task?.status_id ?? "",
    priority_id: task?.priority_id ?? "",
    assigned_analyst_id: task?.assigned_analyst_id ?? "",
    assigned_team_id: task?.assigned_team_id ?? "",
    start_date: task?.start_date ?? "",
    due_date: task?.due_date ?? "",
    estimate_minutes: task?.estimate_minutes != null ? String(task.estimate_minutes) : "",
    tags: task?.tags.join(", ") ?? "",
    company_id: task?.company_id ?? "",
  };
}

function TaskEditor({ task }: { task?: Task }) {
  const t = useText();
  const can = useCan();
  const navigate = useNavigate();
  const statuses = useQuery(taskStatusesQuery);
  const priorities = useQuery(taskPrioritiesQuery);
  const analysts = useQuery(analystsQuery);
  const teams = useQuery(teamsQuery);
  const companies = useQuery(companiesQuery);
  const [form, setForm] = useState(() => toForm(task));
  const [tried, setTried] = useState(false);
  const initial = toForm(task);
  const dirty = JSON.stringify(form) !== JSON.stringify(initial);
  const editable = task ? can("tasks.update") : can("tasks.create");
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((f) => ({ ...f, [key]: e.target.value }));
  const body = (): Schemas["TaskRequest"] => ({
    title: form.title.trim(),
    description: form.description,
    status_id: form.status_id || null,
    priority_id: form.priority_id || null,
    assigned_analyst_id: form.assigned_analyst_id || null,
    assigned_team_id: form.assigned_team_id || null,
    start_date: form.start_date || null,
    due_date: form.due_date || null,
    estimate_minutes: form.estimate_minutes ? Number(form.estimate_minutes) : null,
    tags: parseTags(form.tags),
    ...(task
      ? { parent_task_id: task.parent_task_id ?? null }
      : { company_id: form.company_id || null }),
  });
  const save = useAction(
    () => (task ? taskActions.update(task.id, body()) : taskActions.create(body())),
    {
      invalidate: [["tasks"]],
      success: task ? "tasks.saved" : "tasks.created",
      onSuccess: (saved) => {
        if (!task) void navigate({ to: "/tasks/$id", params: { id: saved.id } });
      },
    },
  );
  const titleError =
    tried && !form.title.trim() ? t("tasks.titleRequired") : fieldError(save.error, "title");
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);

  return (
    <Page width="wide">
      <PageHeader
        back={{ to: "/tasks", label: t("tasks.back") }}
        title={task ? task.title : t("tasks.newTitle")}
        meta={
          task ? (
            <Badge tone={task.is_closed ? "muted" : "accent"}>
              {task.is_closed ? t("tasks.closed") : t("tasks.open")}
            </Badge>
          ) : undefined
        }
      />
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
        <div className="grid content-start gap-5">
          <Card title={t("tasks.details")}>
            <fieldset disabled={!editable} className="contents">
              <RecordForm
                busy={save.isPending}
                error={titleError ? null : save.error}
                dirty={dirty && Boolean(task)}
                submitLabel={task ? t("common.saveChanges") : t("common.create")}
                onSubmit={() => {
                  setTried(true);
                  if (form.title.trim()) save.mutate();
                }}
              >
                <TextField
                  label={t("tasks.titleField")}
                  required
                  value={form.title}
                  error={titleError}
                  onChange={set("title")}
                />
                <TextAreaField
                  label={t("tasks.description")}
                  rows={5}
                  value={form.description}
                  onChange={set("description")}
                />
                <FormGrid>
                  {!task && activeCompanies.length > 1 ? (
                    <SelectField
                      label={t("common.company")}
                      value={form.company_id}
                      placeholder={t("common.choose")}
                      onChange={set("company_id")}
                      options={activeCompanies.map((c) => ({ value: c.id, label: c.name }))}
                    />
                  ) : null}
                  <SelectField
                    label={t("tasks.status")}
                    value={form.status_id}
                    placeholder={task ? undefined : t("newTicket.default")}
                    onChange={set("status_id")}
                    options={toOptions(statuses.data, (s) => s.name, task?.status_id)}
                  />
                  <SelectField
                    label={t("tasks.priority")}
                    value={form.priority_id}
                    placeholder={task ? undefined : t("newTicket.default")}
                    onChange={set("priority_id")}
                    options={toOptions(priorities.data, (s) => s.name, task?.priority_id)}
                  />
                  <SelectField
                    label={t("tasks.assignee")}
                    value={form.assigned_analyst_id}
                    placeholder={t("tickets.unassigned")}
                    onChange={set("assigned_analyst_id")}
                    options={toOptions(
                      analysts.data,
                      (a) => a.full_name,
                      task?.assigned_analyst_id,
                    )}
                  />
                  <SelectField
                    label={t("tasks.team")}
                    value={form.assigned_team_id}
                    placeholder={t("common.none")}
                    onChange={set("assigned_team_id")}
                    options={toOptions(teams.data, (q) => q.name, task?.assigned_team_id)}
                  />
                  <TextField
                    label={t("tasks.startDate")}
                    type="date"
                    value={form.start_date}
                    onChange={set("start_date")}
                  />
                  <TextField
                    label={t("tasks.dueDate")}
                    type="date"
                    value={form.due_date}
                    min={form.start_date || undefined}
                    error={fieldError(save.error, "due_date")}
                    onChange={set("due_date")}
                  />
                  <TextField
                    label={t("tasks.estimate")}
                    type="number"
                    min={0}
                    step={5}
                    value={form.estimate_minutes}
                    onChange={set("estimate_minutes")}
                  />
                  <TextField
                    label={t("tasks.tags")}
                    hint={t("tasks.tagsHint")}
                    value={form.tags}
                    onChange={set("tags")}
                  />
                </FormGrid>
              </RecordForm>
            </fieldset>
          </Card>
          {task ? <Comments task={task} /> : null}
        </div>
        {task ? (
          <aside className="grid content-start gap-4">
            <Links task={task} />
            <Subtasks task={task} />
            <Card title={t("tasks.history")} flush>
              <TaskHistory id={task.id} />
            </Card>
          </aside>
        ) : null}
      </div>
    </Page>
  );
}

function Comments({ task }: { task: Task }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const comments = useQuery(taskCommentsQuery(task.id));
  const analysts = useQuery(analystsQuery);
  const [body, setBody] = useState("");
  const post = useAction(() => taskActions.comment(task.id, body), {
    invalidate: [["tasks", "detail", task.id]],
    success: "tasks.posted",
    onSuccess: () => setBody(""),
  });
  const names = new Map((analysts.data ?? []).map((a) => [a.id, a.full_name]));
  return (
    <Card title={t("tasks.comments")}>
      <div className="grid gap-4">
        <QueryState query={comments}>
          {(list) =>
            list.length === 0 ? (
              <p className="text-sm text-fg-muted">{t("tasks.noComments")}</p>
            ) : (
              <ol className="grid gap-3">
                {list.map((c) => (
                  <li key={c.id} className="rounded-md border border-line p-3">
                    <p className="mb-1 flex flex-wrap gap-2 text-xs text-fg-muted">
                      <span className="font-semibold text-fg">
                        {(c.author_id && names.get(c.author_id)) || t("common.someone")}
                      </span>
                      <time dateTime={c.created_at}>{f.dateTime(c.created_at)}</time>
                    </p>
                    <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">{c.body}</p>
                  </li>
                ))}
              </ol>
            )
          }
        </QueryState>
        {can("tasks.update") ? (
          <form
            className="grid gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (body.trim()) post.mutate();
            }}
          >
            <TextAreaField
              label={t("tasks.addComment")}
              rows={3}
              value={body}
              onChange={(e) => setBody(e.target.value)}
            />
            <ProblemNotice error={post.error} />
            <div>
              <Button type="submit" loading={post.isPending} disabled={!body.trim()}>
                {t("tasks.post")}
              </Button>
            </div>
          </form>
        ) : null}
      </div>
    </Card>
  );
}

function Links({ task }: { task: Task }) {
  const t = useText();
  const can = useCan();
  const links = useQuery(taskLinksQuery(task.id));
  const linkedTickets = new Set(
    (links.data ?? []).filter((l) => l.kind === "ticket").map((l) => l.target_id),
  );
  const [pick, setPick] = useState<PickerOption | null>(null);
  const link = useAction(
    (targetId: string) => taskActions.link(task.id, { kind: "ticket", target_id: targetId }),
    {
      invalidate: [["tasks", "detail", task.id]],
      success: "common.linked",
      onSuccess: () => setPick(null),
    },
  );
  const unlink = useAction(
    (l: { kind: string; target_id: string }) => taskActions.unlink(task.id, l.kind, l.target_id),
    {
      invalidate: [["tasks", "detail", task.id]],
      success: "common.unlinked",
    },
  );
  return (
    <Card title={t("tasks.links")}>
      <div className="grid gap-3">
        <QueryState query={links}>
          {(list) =>
            list.length === 0 ? (
              <p className="text-sm text-fg-muted">{t("tasks.noLinks")}</p>
            ) : (
              <ul className="grid gap-1 text-sm">
                {list.map((l) => (
                  <li key={`${l.kind}-${l.target_id}`} className="flex items-center gap-2">
                    <Badge tone="muted">
                      {l.kind === "change" ? t("tasks.kind.change") : t("tasks.kind.ticket")}
                    </Badge>
                    {l.kind === "ticket" ? (
                      <Link
                        to="/tickets/$ticketId"
                        params={{ ticketId: l.target_id }}
                        search={{ view: "open" }}
                        className="flex-1 font-mono hover:underline"
                      >
                        {l.number}
                      </Link>
                    ) : (
                      <span className="flex-1 font-mono">{l.number}</span>
                    )}
                    {can("tasks.update") ? (
                      <IconButton
                        size="sm"
                        tooltip="top"
                        label={t("common.unlinkNamed", { name: l.number })}
                        icon={<Unlink aria-hidden="true" className="size-4" />}
                        onClick={() => unlink.mutate(l)}
                      />
                    ) : null}
                  </li>
                ))}
              </ul>
            )
          }
        </QueryState>
        {can("tasks.update") ? (
          <form
            className="grid gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (pick) link.mutate(pick.id);
            }}
          >
            <TicketPicker
              label={t("tasks.linkTicket")}
              hint={t("tasks.linkTicketHint")}
              companyId={task.company_id}
              exclude={linkedTickets}
              suggestMine
              value={pick}
              onChange={setPick}
            />
            <div className="flex flex-wrap gap-2">
              <Button type="submit" size="sm" disabled={!pick} loading={link.isPending}>
                {t("common.link")}
              </Button>
              {can("tickets.create") ? (
                <Link
                  to="/tickets/new"
                  search={{ task: task.id }}
                  className={buttonClass({ variant: "ghost", size: "sm" })}
                >
                  <Plus aria-hidden="true" className="size-4" />
                  {t("tasks.newTicket")}
                </Link>
              ) : null}
            </div>
          </form>
        ) : null}
        <ProblemNotice error={link.error ?? unlink.error} />
      </div>
    </Card>
  );
}

function Subtasks({ task }: { task: Task }) {
  const t = useText();
  const can = useCan();
  const list = useInfiniteQuery(subtasksQuery(task.id));
  const items = list.data?.pages.flatMap((p) => p.items) ?? [];
  const [title, setTitle] = useState("");
  const add = useAction(
    () =>
      taskActions.create({
        title: title.trim(),
        parent_task_id: task.id,
        company_id: task.company_id,
      }),
    {
      invalidate: [["tasks"]],
      success: "tasks.created",
      onSuccess: () => setTitle(""),
    },
  );
  if (task.parent_task_id) return null;
  return (
    <Card title={t("tasks.subtasks")}>
      <div className="grid gap-3">
        {list.isSuccess && items.length === 0 ? (
          <p className="text-sm text-fg-muted">{t("tasks.noSubtasks")}</p>
        ) : null}
        <ul className="grid gap-1 text-sm">
          {items.map((s) => (
            <li key={s.id} className="flex items-center gap-2">
              <Link
                to="/tasks/$id"
                params={{ id: s.id }}
                className="min-w-0 flex-1 truncate hover:underline"
              >
                {s.title}
              </Link>
              <Badge tone={s.is_closed ? "muted" : "accent"}>
                {s.is_closed ? t("tasks.closed") : t("tasks.open")}
              </Badge>
            </li>
          ))}
        </ul>
        <LoadMore query={list} />
        {can("tasks.create") ? (
          <form
            className="flex items-end gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (title.trim()) add.mutate();
            }}
          >
            <TextField
              className="flex-1"
              label={t("tasks.addSubtask")}
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
            <Button type="submit" size="md" loading={add.isPending} disabled={!title.trim()}>
              {t("common.add")}
            </Button>
          </form>
        ) : null}
        <ProblemNotice error={add.error} />
      </div>
    </Card>
  );
}

function TaskHistory({ id }: { id: string }) {
  const t = useText();
  const history = useQuery(taskHistoryQuery(id));
  return (
    <QueryState query={history}>
      {(entries) => <HistoryTable entries={entries} caption={t("tasks.history")} />}
    </QueryState>
  );
}
