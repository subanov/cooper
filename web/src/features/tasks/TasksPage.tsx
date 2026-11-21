import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowRightLeft, CalendarClock, Plus, Search } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { Badge, ColourDot } from "@/components/Badge";
import { buttonClass, IconButton } from "@/components/Button";
import { Checkbox, Input } from "@/components/Field";
import { Menu, MenuItem, MenuLabel } from "@/components/Menu";
import { FilterBar, Page, PageHeader } from "@/components/Page";
import { QueryState } from "@/components/QueryState";
import { analystsQuery, namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { useFormat, useText } from "@/i18n/useText";
import { localDay } from "@/lib/dates";
import type { SearchPatch } from "@/routes/search";
import { boardQuery, type Task, type TaskSearch, taskActions, taskPrioritiesQuery } from "./api";

/**
 * The Task Board (FR-TSK-005): one column per status. Tasks move with the "Move to"
 * menu, which works with the keyboard and screen readers.
 */
export function TasksPage({ search }: { search: TaskSearch }) {
  const t = useText();
  const can = useCan();
  const navigate = useNavigate();
  const board = useQuery(boardQuery(search));
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  const setSearch = (next: SearchPatch<TaskSearch>) =>
    void navigate({ to: "/tasks", search: { ...search, ...next } });
  function submit(e: FormEvent) {
    e.preventDefault();
    setSearch({ q: q.trim() || undefined });
  }
  return (
    <Page width="full">
      <PageHeader
        title={t("tasks.title")}
        description={t("tasks.intro")}
        actions={
          can("tasks.create") ? (
            <Link to="/tasks/new" className={buttonClass({ variant: "primary" })}>
              <Plus aria-hidden="true" className="size-4" />
              {t("tasks.new")}
            </Link>
          ) : undefined
        }
      />
      <FilterBar label={t("common.filters")}>
        <form onSubmit={submit} className="relative w-full max-w-xs">
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
        <Checkbox
          label={t("tasks.mine")}
          checked={Boolean(search.mine)}
          onChange={(e) => setSearch({ mine: e.target.checked ? true : undefined })}
          className="h-9 items-center"
        />
        <Checkbox
          label={t("tasks.showClosed")}
          checked={Boolean(search.closed)}
          onChange={(e) => setSearch({ closed: e.target.checked ? true : undefined })}
          className="h-9 items-center"
        />
      </FilterBar>
      <QueryState query={board}>
        {(columns) => (
          <section
            aria-label={t("tasks.board")}
            // Scrolls sideways; focusable so keyboard users can scroll it (WCAG 2.1.1).
            // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable region must take focus
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
                  className="flex w-72 shrink-0 flex-col gap-2 rounded-lg border border-line bg-sunken/60 p-2"
                >
                  <h2 className="flex items-center gap-2 px-1.5 py-1 text-[0.8125rem] font-semibold">
                    <ColourDot colour={column.status.colour} />
                    <span className="flex-1">{column.status.name}</span>
                    <span className="text-xs font-normal text-fg-muted">{column.tasks.length}</span>
                  </h2>
                  {column.tasks.length === 0 ? (
                    <p className="px-1.5 pb-2 text-sm text-fg-muted">{t("tasks.emptyColumn")}</p>
                  ) : null}
                  <ul className="grid gap-2">
                    {column.tasks.map((task) => (
                      <li key={task.id}>
                        <TaskCard
                          task={task}
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
        )}
      </QueryState>
    </Page>
  );
}

function TaskCard({
  task,
  columns,
  canMove,
}: {
  task: Task;
  columns: { id: string; name: string }[];
  canMove: boolean;
}) {
  const t = useText();
  const f = useFormat();
  const analysts = useQuery(analystsQuery);
  const priorities = useQuery(taskPrioritiesQuery);
  const move = useAction((statusId: string) => taskActions.move(task.id, { status_id: statusId }), {
    invalidate: [["tasks"]],
    success: (_, statusId) =>
      t("tasks.moved", { status: columns.find((c) => c.id === statusId)?.name ?? "" }),
  });
  const overdue = !task.is_closed && task.due_date && task.due_date < localDay();
  return (
    <article className="relative grid gap-2 rounded-md border border-line bg-surface p-3 shadow-raised hover:border-control">
      <div className="flex items-start gap-2">
        <Link
          to="/tasks/$id"
          params={{ id: task.id }}
          className="min-w-0 flex-1 font-medium after:absolute after:inset-0 hover:underline [overflow-wrap:anywhere]"
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
                  icon={<ArrowRightLeft aria-hidden="true" className="size-3.5" />}
                  loading={move.isPending}
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
    </article>
  );
}
