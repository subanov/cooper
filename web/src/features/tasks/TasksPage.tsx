import { useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { ArrowRightLeft, CalendarClock, Plus, Search, SquareKanban } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { Badge, ColourLabel } from "@/components/Badge";
import { buttonClass, IconButton } from "@/components/Button";
import { EmptyState, LoadingBlock, ProblemNotice, QueryState } from "@/components/Feedback";
import { Checkbox, Input } from "@/components/Field";
import { Page, PageHeader } from "@/components/Layout";
import {
  ListPane,
  RecordRow,
  rowLinkClass,
  SplitView,
  useDetailOpen,
  useListKeys,
  useOpenId,
} from "@/components/MasterDetail";
import { Menu, MenuItem, MenuLabel } from "@/components/Menu";
import { analystsQuery, namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { useFormat, useText } from "@/i18n/useText";
import { localDay } from "@/lib/dates";
import type { SearchPatch } from "@/routes/search";
import { boardQuery, type Task, type TaskSearch, taskActions, taskPrioritiesQuery } from "./api";

/**
 * Tasks (FR-TSK-005). With nothing open, the Board: a column per status, cards move with
 * the "Move to" menu (works with keyboard and screen readers). Opening a Task shows it beside
 * a compact list of the same Tasks, grouped by status (master-detail).
 */
export function TasksPage({ search }: { search: TaskSearch }) {
  const detailOpen = useDetailOpen("/tasks");
  if (!detailOpen) return <Board search={search} />;
  return (
    <SplitView detailOpen list={<TaskList search={search} />}>
      <Outlet />
    </SplitView>
  );
}

function useTaskSearch(search: TaskSearch) {
  const navigate = useNavigate();
  return (next: SearchPatch<TaskSearch>) =>
    void navigate({ to: "/tasks", search: { ...search, ...next } });
}

function Filters({ search }: { search: TaskSearch }) {
  const t = useText();
  const setSearch = useTaskSearch(search);
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  function submit(e: FormEvent) {
    e.preventDefault();
    setSearch({ q: q.trim() || undefined });
  }
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
      <search className="w-full max-w-xs">
        <form onSubmit={submit} className="relative">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-fg-muted"
          />
          <Input
            type="search"
            aria-label={t("tasks.search")}
            placeholder={t("tasks.searchPlaceholder")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            className="pl-8"
          />
        </form>
      </search>
      <Checkbox
        label={t("tasks.mine")}
        checked={Boolean(search.mine)}
        onChange={(e) => setSearch({ mine: e.target.checked ? true : undefined })}
      />
      <Checkbox
        label={t("tasks.showClosed")}
        checked={Boolean(search.closed)}
        onChange={(e) => setSearch({ closed: e.target.checked ? true : undefined })}
      />
    </div>
  );
}

function NewTaskLink({ compact }: { compact?: boolean }) {
  const t = useText();
  const can = useCan();
  if (!can("tasks.create")) return null;
  return (
    <Link
      to="/tasks/new"
      search={true}
      className={buttonClass({
        variant: compact ? "secondary" : "primary",
        size: compact ? "sm" : "md",
      })}
    >
      <Plus aria-hidden="true" />
      {t("tasks.new")}
    </Link>
  );
}

function Board({ search }: { search: TaskSearch }) {
  const t = useText();
  const can = useCan();
  const board = useQuery(boardQuery(search));
  return (
    <Page width="full">
      <PageHeader
        title={t("tasks.title")}
        description={t("tasks.intro")}
        actions={<NewTaskLink />}
      />
      <Filters search={search} />
      <QueryState query={board}>
        {(columns) =>
          columns.every((c) => c.tasks.length === 0) && !search.q ? (
            <EmptyState
              icon={<SquareKanban aria-hidden="true" />}
              title={t("tasks.emptyBoard")}
              action={<NewTaskLink />}
            >
              {t("tasks.emptyBoardHint")}
            </EmptyState>
          ) : (
            <section
              aria-label={t("tasks.board")}
              // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable region must take focus (WCAG 2.1.1)
              tabIndex={0}
              className="-mx-4 overflow-x-auto px-4 pb-2 sm:-mx-6 sm:px-6"
            >
              <div className="flex min-w-max gap-4">
                {columns.map((column) => (
                  <section
                    key={column.status.id}
                    aria-label={t("tasks.column", {
                      status: column.status.name,
                      count: column.tasks.length,
                    })}
                    className="flex w-72 shrink-0 flex-col gap-2 rounded-lg border border-line bg-sunken/70 p-2"
                  >
                    <h2 className="flex items-center gap-2 px-1.5 py-1 text-[0.8125rem] font-semibold">
                      <ColourLabel colour={column.status.colour} className="flex-1">
                        {column.status.name}
                      </ColourLabel>
                      <span className="text-xs font-normal text-fg-muted">
                        {column.tasks.length}
                      </span>
                    </h2>
                    {column.tasks.length === 0 ? (
                      <p className="px-1.5 pb-2 text-sm text-fg-muted">{t("tasks.emptyColumn")}</p>
                    ) : null}
                    <ul className="grid gap-2">
                      {column.tasks.map((task) => (
                        <li key={task.id}>
                          <TaskCard
                            task={task}
                            search={search}
                            columns={columns.map((c) => c.status)}
                            canMove={can("tasks.update")}
                          />
                        </li>
                      ))}
                    </ul>
                    {column.has_more ? (
                      <p className="px-1.5 text-xs text-fg-muted">{t("tasks.hasMore")}</p>
                    ) : null}
                  </section>
                ))}
              </div>
            </section>
          )
        }
      </QueryState>
    </Page>
  );
}

function TaskMeta({ task }: { task: Task }) {
  const t = useText();
  const f = useFormat();
  const analysts = useQuery(analystsQuery);
  const priorities = useQuery(taskPrioritiesQuery);
  const overdue = !task.is_closed && task.due_date && task.due_date < localDay();
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-xs text-fg-muted">
      <Badge tone="muted">{namesById(priorities.data).get(task.priority_id) ?? "…"}</Badge>
      {task.due_date ? (
        <Badge tone={overdue ? "danger" : "neutral"} icon={<CalendarClock aria-hidden="true" />}>
          {overdue ? `${t("tasks.overdue")} · ` : ""}
          {t("tasks.due", { date: f.day(task.due_date) })}
        </Badge>
      ) : null}
      {task.assigned_analyst_id ? (
        <span className="truncate">
          {namesById(analysts.data, (a) => a.full_name).get(task.assigned_analyst_id)}
        </span>
      ) : null}
    </div>
  );
}

function TaskCard({
  task,
  search,
  columns,
  canMove,
}: {
  task: Task;
  search: TaskSearch;
  columns: { id: string; name: string }[];
  canMove: boolean;
}) {
  const t = useText();
  const move = useAction((statusId: string) => taskActions.move(task.id, { status_id: statusId }), {
    invalidate: [["tasks"]],
    success: (_, statusId) =>
      t("tasks.moved", { status: columns.find((c) => c.id === statusId)?.name ?? "" }),
    undo: () => ({ label: t("common.undo"), run: (): void => move.mutate(task.status_id) }),
  });
  return (
    <article className="relative grid gap-2 rounded-md border border-line bg-surface p-3 shadow-raised hover:border-control">
      <div className="flex items-start gap-2">
        <Link
          to="/tasks/$id"
          params={{ id: task.id }}
          search={search}
          className={`min-w-0 flex-1 ${rowLinkClass}`}
        >
          {task.title}
        </Link>
        {canMove ? (
          <div className="relative z-10 -mr-1 -mt-1">
            <Menu
              label={t("tasks.moveNamed", { title: task.title })}
              trigger={
                <IconButton
                  size="sm"
                  label={t("tasks.moveNamed", { title: task.title })}
                  tooltip="none"
                  icon={<ArrowRightLeft aria-hidden="true" />}
                  loading={move.isPending}
                  className="size-7"
                />
              }
            >
              <MenuLabel>{t("tasks.moveTo")}</MenuLabel>
              {columns
                .filter((c) => c.id !== task.status_id)
                .map((c) => (
                  <MenuItem key={c.id} onSelect={() => move.mutate(c.id)}>
                    {c.name}
                  </MenuItem>
                ))}
            </Menu>
          </div>
        ) : null}
      </div>
      <TaskMeta task={task} />
      {task.tags.length > 0 ? (
        <div className="flex flex-wrap gap-1">
          {task.tags.map((tag) => (
            <span
              key={tag}
              className="rounded-sm bg-sunken px-1.5 py-0.5 text-[0.6875rem] text-fg-muted"
            >
              {tag}
            </span>
          ))}
        </div>
      ) : null}
      {move.error ? <ProblemNotice error={move.error} /> : null}
    </article>
  );
}

function TaskList({ search }: { search: TaskSearch }) {
  const t = useText();
  const navigate = useNavigate();
  const board = useQuery(boardQuery(search));
  const openId = useOpenId();
  const ids = (board.data ?? []).flatMap((c) => c.tasks.map((k) => k.id));
  useListKeys(ids, openId, (id) => void navigate({ to: "/tasks/$id", params: { id }, search }));
  return (
    <ListPane
      title={t("tasks.title")}
      label={t("tasks.title")}
      actions={<NewTaskLink compact />}
      filters={<Filters search={search} />}
    >
      {board.isPending ? <LoadingBlock className="p-4" lines={6} /> : null}
      {board.isError ? <ProblemNotice error={board.error} className="m-4" /> : null}
      {(board.data ?? [])
        .filter((c) => c.tasks.length > 0)
        .map((column) => (
          <section key={column.status.id} aria-label={column.status.name}>
            <h2 className="sticky top-0 z-20 flex items-center gap-2 border-b border-line bg-sunken px-4 py-1.5 text-xs font-semibold">
              <ColourLabel colour={column.status.colour} className="flex-1">
                {column.status.name}
              </ColourLabel>
              <span className="font-normal text-fg-muted">{column.tasks.length}</span>
            </h2>
            <ul>
              {column.tasks.map((task) => (
                <RecordRow key={task.id} selected={task.id === openId}>
                  <Link
                    to="/tasks/$id"
                    params={{ id: task.id }}
                    search={search}
                    aria-current={task.id === openId ? "page" : undefined}
                    className={rowLinkClass}
                  >
                    {task.title}
                  </Link>
                  <TaskMeta task={task} />
                </RecordRow>
              ))}
            </ul>
          </section>
        ))}
    </ListPane>
  );
}
