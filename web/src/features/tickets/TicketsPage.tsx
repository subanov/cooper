import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import {
  AlarmClock,
  CircleDot,
  Inbox,
  MoreHorizontal,
  Search,
  SearchX,
  UserCheck,
  UserPlus,
} from "lucide-react";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "@/components/Badge";
import { IconButton } from "@/components/Button";
import { EmptyState, LoadMore, ProblemNotice, Refreshing, Skeleton } from "@/components/Feedback";
import { Input, Select } from "@/components/Field";
import {
  ListPane,
  PickOne,
  RecordRow,
  rowLinkClass,
  SplitView,
  useDetailOpen,
  useListKeys,
  useOpenId,
} from "@/components/MasterDetail";
import { Menu, MenuItem } from "@/components/Menu";
import {
  analystsQuery,
  namesById,
  requestersByIdQuery,
  teamsQuery,
  ticketLookupQuery,
} from "@/features/reference/queries";
import { useCan, useSession } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { useHotkeys } from "@/lib/hooks";
import { useTicketActions } from "./actions";
import { type Ticket, ticketListQuery } from "./api";
import { BulkBar } from "./BulkBar";
import { AssignDialog, SnoozeDialog, StatusDialog } from "./dialogs";
import { currentTarget, SlaBadge } from "./Sla";
import { DEFAULT_VIEW, SORTS, type TicketSearch, VIEW_LABELS, VIEWS, type View } from "./views";

/**
 * The ticket inbox: Views, search and sort on the left with the open ticket beside it
 * (FR-TKT-040). Rows offer quick triage in place (assign, status, snooze) with Undo;
 * `j`/`k` move through the list, `/` searches, checkboxes select for bulk actions.
 */
export function TicketsPage({ search }: { search: TicketSearch }) {
  const t = useText();
  const navigate = useNavigate();
  const session = useSession();
  const view = search.view ?? DEFAULT_VIEW;
  const list = useInfiniteQuery(ticketListQuery(search));
  const tickets = useMemo(() => list.data?.pages.flatMap((p) => p.items) ?? [], [list.data]);
  const openId = useOpenId();
  const detailOpen = useDetailOpen("/tickets");
  const [selected, setSelected] = useState<string[]>([]);
  const searchBox = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const shown = new Set(tickets.map((tk) => tk.id));
    setSelected((current) => current.filter((id) => shown.has(id)));
  }, [tickets]);

  useListKeys(
    tickets.map((tk) => tk.id),
    openId,
    (id) => void navigate({ to: "/tickets/$ticketId", params: { ticketId: id }, search }),
  );
  useHotkeys({ "/": () => searchBox.current?.focus() });

  const views = VIEWS.filter((v) => v !== "deleted" || session.is_admin);

  return (
    <SplitView
      detailOpen={detailOpen}
      placeholder={
        <PickOne icon={<Inbox />} title={t("tickets.pickOne")}>
          {t("tickets.pickOneBody")}
        </PickOne>
      }
      list={
        <ListPane
          title={t("tickets.title")}
          label={t("tickets.title")}
          actions={
            <Refreshing active={list.isFetching && !list.isPending && !list.isFetchingNextPage} />
          }
          filters={
            <>
              <nav aria-label={t("tickets.views")} className="-mx-1 overflow-x-auto pb-0.5">
                <ul className="flex min-w-max gap-1 px-1">
                  {views.map((v) => (
                    <li key={v}>
                      <Link
                        to="/tickets"
                        search={{ ...search, view: v }}
                        aria-current={v === view ? "page" : undefined}
                        className="inline-flex h-8 items-center rounded-full border border-line px-3 text-[0.8125rem] font-medium text-fg-muted hover:bg-sunken hover:text-fg aria-[current=page]:border-transparent aria-[current=page]:bg-accent aria-[current=page]:text-accent-fg"
                      >
                        {t(VIEW_LABELS[v])}
                      </Link>
                    </li>
                  ))}
                </ul>
              </nav>
              <SearchAndSort search={search} inputRef={searchBox} />
            </>
          }
          status={
            selected.length > 0 ? (
              <BulkBar
                ids={selected}
                tickets={tickets.filter((tk) => selected.includes(tk.id))}
                onClear={() => setSelected([])}
              />
            ) : tickets.length > 0 ? (
              <SelectAllBar
                tickets={tickets}
                selected={selected}
                onSelect={setSelected}
                more={Boolean(list.hasNextPage)}
              />
            ) : null
          }
        >
          {list.isError ? (
            <div className="p-4">
              <ProblemNotice error={list.error} onRetry={() => void list.refetch()} />
            </div>
          ) : null}
          {list.isPending ? <ListSkeleton /> : null}
          {list.isSuccess && tickets.length === 0 ? (
            search.q ? (
              <EmptyState
                icon={<SearchX aria-hidden="true" />}
                title={t("tickets.noMatches", { q: search.q })}
                action={
                  <Link
                    to="/tickets"
                    search={{ ...search, q: undefined }}
                    className="text-sm font-medium text-accent-text hover:underline"
                  >
                    {t("tickets.clearSearch")}
                  </Link>
                }
              />
            ) : (
              <EmptyState icon={<Inbox aria-hidden="true" />} title={t("tickets.emptyTitle")}>
                {t(`tickets.empty.${view}`)}
              </EmptyState>
            )
          ) : null}
          {tickets.length > 0 ? (
            <TicketList
              tickets={tickets}
              view={view}
              search={search}
              openId={openId}
              selected={selected}
              onSelect={setSelected}
            />
          ) : null}
          <LoadMore query={list} />
        </ListPane>
      }
    >
      <Outlet />
    </SplitView>
  );
}

function SelectAllBar({
  tickets,
  selected,
  onSelect,
  more,
}: {
  tickets: Ticket[];
  selected: string[];
  onSelect: (ids: string[]) => void;
  more: boolean;
}) {
  const t = useText();
  const all = tickets.length > 0 && tickets.every((tk) => selected.includes(tk.id));
  return (
    <div className="flex items-center gap-3 border-b border-line px-4 py-1.5 text-xs text-fg-muted">
      <input
        type="checkbox"
        aria-label={t("tickets.selectAll")}
        checked={all}
        onChange={(e) => onSelect(e.target.checked ? tickets.map((tk) => tk.id) : [])}
        className="size-4 cursor-pointer accent-accent"
      />
      <span>{t("tickets.count", { count: tickets.length, more: more ? "true" : "false" })}</span>
    </div>
  );
}

function SearchAndSort({
  search,
  inputRef,
}: {
  search: TicketSearch;
  inputRef: React.RefObject<HTMLInputElement | null>;
}) {
  const t = useText();
  const navigate = useNavigate();
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  function submit(event: FormEvent) {
    event.preventDefault();
    void navigate({ to: "/tickets", search: { ...search, q: q.trim() || undefined } });
  }
  return (
    <div className="flex flex-wrap items-center gap-2">
      <search className="min-w-48 flex-1">
        <form onSubmit={submit} className="relative">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-fg-muted"
          />
          <Input
            ref={inputRef}
            type="search"
            aria-label={t("tickets.search")}
            aria-keyshortcuts="/"
            placeholder={t("tickets.searchPlaceholder")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            className="pl-8"
          />
        </form>
      </search>
      <Select
        aria-label={t("tickets.sort")}
        value={`${search.sort ?? "updated_at"}:${search.order ?? "desc"}`}
        onChange={(e) => {
          const [sort, order] = e.target.value.split(":") as [
            TicketSearch["sort"],
            TicketSearch["order"],
          ];
          void navigate({ to: "/tickets", search: { ...search, sort, order } });
        }}
        className="w-auto"
      >
        {SORTS.flatMap((s) =>
          (["desc", "asc"] as const).map((o) => (
            <option key={`${s}:${o}`} value={`${s}:${o}`}>
              {t(`tickets.sort.${s}`)} · {t(`tickets.order.${o}`)}
            </option>
          )),
        )}
      </Select>
    </div>
  );
}

function TicketList({
  tickets,
  view,
  search,
  openId,
  selected,
  onSelect,
}: {
  tickets: Ticket[];
  view: View;
  search: TicketSearch;
  openId: string | undefined;
  selected: string[];
  onSelect: (ids: string[]) => void;
}) {
  const t = useText();
  const f = useFormat();
  const statuses = useQuery(ticketLookupQuery("ticket-statuses"));
  const priorities = useQuery(ticketLookupQuery("ticket-priorities"));
  const analysts = useQuery(analystsQuery);
  const teams = useQuery(teamsQuery);
  const requesters = useQuery(requestersByIdQuery(tickets.map((tk) => tk.requester_id)));
  const statusNames = namesById(statuses.data);
  const priorityNames = namesById(priorities.data);
  const analystNames = namesById(analysts.data, (a) => a.full_name);
  const teamNames = namesById(teams.data);
  const chosen = new Set(selected);
  return (
    <ul aria-label={t("tickets.list", { view: t(VIEW_LABELS[view]) })}>
      {tickets.map((tk) => {
        const requester = tk.requester_id ? requesters.data?.get(tk.requester_id) : undefined;
        const { target } = currentTarget(tk.sla);
        const open = tk.id === openId;
        const breached = target.state === "breached";
        return (
          <RecordRow
            key={tk.id}
            selected={open}
            className={cn(breached && !open && "bg-danger-soft/40")}
            leading={
              <input
                type="checkbox"
                aria-label={t("tickets.select", { number: tk.number })}
                checked={chosen.has(tk.id)}
                onChange={(e) =>
                  onSelect(
                    e.target.checked ? [...selected, tk.id] : selected.filter((id) => id !== tk.id),
                  )
                }
                className="size-4 cursor-pointer accent-accent"
              />
            }
          >
            <div className="flex items-center justify-between gap-2 text-xs text-fg-muted">
              <span className="font-mono">{tk.number}</span>
              <span className="flex items-center gap-1">
                <time dateTime={tk.updated_at} title={f.dateTime(tk.updated_at)}>
                  {f.relative(tk.updated_at)}
                </time>
                <QuickActions ticket={tk} />
              </span>
            </div>
            <Link
              to="/tickets/$ticketId"
              params={{ ticketId: tk.id }}
              search={search}
              aria-current={open ? "page" : undefined}
              className={cn(rowLinkClass, "line-clamp-2")}
            >
              {tk.subject}
            </Link>
            <p className="truncate text-xs text-fg-muted">
              {requester?.full_name ?? (tk.requester_id ? "…" : t("tickets.noRequester"))}
              {" · "}
              {tk.analyst_id
                ? (analystNames.get(tk.analyst_id) ?? "…")
                : tk.team_id
                  ? (teamNames.get(tk.team_id) ?? "…")
                  : t("tickets.unassigned")}
            </p>
            <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
              <Badge tone="neutral">{statusNames.get(tk.status_id) ?? "…"}</Badge>
              <Badge tone="muted">{priorityNames.get(tk.priority_id) ?? "…"}</Badge>
              <SlaBadge target={target} />
              {tk.sensitivity === "confidential" ? (
                <Badge tone="warning">{t("tickets.confidential")}</Badge>
              ) : null}
              {tk.snoozed_until ? (
                <Badge tone="muted">
                  {t("ticket.snoozedUntil", { date: f.dateTime(tk.snoozed_until) })}
                </Badge>
              ) : null}
            </div>
          </RecordRow>
        );
      })}
    </ul>
  );
}

/** Small, frequent changes in place, without opening the ticket (master-detail rule). */
function QuickActions({ ticket }: { ticket: Ticket }) {
  const t = useText();
  const can = useCan();
  const [dialog, setDialog] = useState<"assign" | "status" | "snooze" | null>(null);
  const close = () => setDialog(null);
  const actions = useTicketActions(ticket, close);
  const deleted = Boolean(ticket.deleted_at);
  const canAssign = (can("tickets.assign") || can("tickets.update")) && !deleted;
  const canUpdate = can("tickets.update") && !deleted;
  if (!canAssign && !canUpdate) return null;
  return (
    <span className="relative z-10">
      <Menu
        label={t("tickets.quickActions", { number: ticket.number })}
        trigger={
          <IconButton
            size="sm"
            tooltip="none"
            label={t("tickets.quickActions", { number: ticket.number })}
            icon={<MoreHorizontal aria-hidden="true" />}
            className="size-7"
          />
        }
      >
        {canAssign && ticket.analyst_id !== actions.me ? (
          <MenuItem icon={<UserCheck />} onSelect={actions.assignToMe}>
            {t("ticket.assignToMe")}
          </MenuItem>
        ) : null}
        {canAssign ? (
          <MenuItem icon={<UserPlus />} onSelect={() => setDialog("assign")}>
            {t("bulk.assign")}
          </MenuItem>
        ) : null}
        {canUpdate ? (
          <MenuItem icon={<CircleDot />} onSelect={() => setDialog("status")}>
            {t("bulk.status")}
          </MenuItem>
        ) : null}
        {canUpdate ? (
          ticket.snoozed_until ? (
            <MenuItem icon={<AlarmClock />} onSelect={() => actions.wake.mutate()}>
              {t("ticket.wake")}
            </MenuItem>
          ) : (
            <MenuItem icon={<AlarmClock />} onSelect={() => setDialog("snooze")}>
              {t("tickets.snoozeMenu")}
            </MenuItem>
          )
        ) : null}
      </Menu>
      <AssignDialog
        open={dialog === "assign"}
        onOpenChange={(o) => setDialog(o ? "assign" : null)}
        title={t("ticket.assignTitle", { number: ticket.number })}
        initial={{ team_id: ticket.team_id ?? null, analyst_id: ticket.analyst_id ?? null }}
        busy={actions.assign.isPending}
        error={actions.assign.error}
        onSubmit={(a) => actions.assign.mutate(a)}
      />
      <StatusDialog
        open={dialog === "status"}
        onOpenChange={(o) => setDialog(o ? "status" : null)}
        title={t("ticket.statusTitle", { number: ticket.number })}
        currentStatusId={ticket.status_id}
        busy={actions.status.isPending}
        error={actions.status.error}
        onSubmit={(change) => actions.status.mutate(change)}
      />
      <SnoozeDialog
        open={dialog === "snooze"}
        onOpenChange={(o) => setDialog(o ? "snooze" : null)}
        title={t("ticket.snoozeTitle", { number: ticket.number })}
        busy={actions.snooze.isPending}
        error={actions.snooze.error}
        onSubmit={(body) => actions.snooze.mutate(body)}
      />
    </span>
  );
}

function ListSkeleton() {
  const t = useText();
  return (
    <div role="status" className="grid">
      <span className="sr-only">{t("common.loading")}</span>
      {[0, 1, 2, 3, 4].map((n) => (
        <div key={n} className="grid gap-2 border-b border-line px-4 py-3.5">
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-4 w-4/5" />
          <Skeleton className="h-3 w-1/2" />
        </div>
      ))}
    </div>
  );
}
