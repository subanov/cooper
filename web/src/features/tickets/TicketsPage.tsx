import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { Inbox, Search, SearchX } from "lucide-react";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "@/components/Badge";
import { EmptyState } from "@/components/EmptyState";
import { Input, Select } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { LoadMore, Refreshing } from "@/components/QueryState";
import { Skeleton } from "@/components/Spinner";
import {
  analystsQuery,
  namesById,
  requestersByIdQuery,
  teamsQuery,
  ticketLookupQuery,
} from "@/features/reference/queries";
import { useSession } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { useHotkeys } from "@/lib/hooks";
import { type Ticket, ticketListQuery } from "./api";
import { BulkBar } from "./BulkBar";
import { currentTarget, SlaBadge } from "./Sla";
import { DEFAULT_VIEW, SORTS, type TicketSearch, VIEW_LABELS, VIEWS, type View } from "./views";

/**
 * The ticket list and the open ticket side by side (split pane, as in an inbox: Jakob's
 * Law). View, search and sort live in the URL. `j`/`k` move through the list, `/`
 * searches; checkboxes select tickets for bulk actions (FR-TKT-042).
 */
export function TicketsPage({ search }: { search: TicketSearch }) {
  const t = useText();
  const navigate = useNavigate();
  const session = useSession();
  const view = search.view ?? DEFAULT_VIEW;
  const list = useInfiniteQuery(ticketListQuery(search));
  const tickets = useMemo(() => list.data?.pages.flatMap((p) => p.items) ?? [], [list.data]);
  const openId = useRouterState({
    select: (s) => {
      const leaf = s.matches[s.matches.length - 1];
      return (leaf?.params as { ticketId?: string } | undefined)?.ticketId;
    },
  });
  const creating = useRouterState({ select: (s) => s.location.pathname === "/tickets/new" });
  const paneOpen = Boolean(openId) || creating;
  const [selected, setSelected] = useState<string[]>([]);
  const searchBox = useRef<HTMLInputElement>(null);

  // Keep the selection to tickets still in the list.
  useEffect(() => {
    const shown = new Set(tickets.map((tk) => tk.id));
    setSelected((current) => current.filter((id) => shown.has(id)));
  }, [tickets]);

  function step(delta: number) {
    if (tickets.length === 0) return;
    const index = tickets.findIndex((tk) => tk.id === openId);
    const next = tickets[Math.min(Math.max(index + delta, 0), tickets.length - 1)];
    if (next) {
      void navigate({ to: "/tickets/$ticketId", params: { ticketId: next.id }, search });
    }
  }

  useHotkeys({
    j: () => step(1),
    k: () => step(-1),
    "/": () => searchBox.current?.focus(),
  });

  const views = VIEWS.filter((v) => v !== "deleted" || session.is_admin);

  return (
    <div className="flex h-full min-h-0">
      <section
        aria-labelledby="tickets-title"
        className={cn(
          "flex min-h-0 w-full flex-col border-line bg-surface xl:w-[27rem] xl:shrink-0 xl:border-r",
          paneOpen && "hidden xl:flex",
        )}
      >
        <div className="grid gap-3 border-b border-line px-4 pb-3 pt-4">
          <div className="flex items-center justify-between gap-3">
            <h1
              id="tickets-title"
              tabIndex={-1}
              data-page-heading={paneOpen ? undefined : true}
              className="text-lg font-semibold"
            >
              {t("tickets.title")}
            </h1>
            <Refreshing active={list.isFetching && !list.isPending && !list.isFetchingNextPage} />
          </div>
          <nav aria-label={t("tickets.views")} className="-mx-1 overflow-x-auto">
            <ul className="flex min-w-max gap-1 px-1">
              {views.map((v) => (
                <li key={v}>
                  <Link
                    to="/tickets"
                    search={{ ...search, view: v }}
                    aria-current={v === view ? "page" : undefined}
                    className="inline-flex h-8 items-center rounded-full border border-line px-3 text-[0.8125rem] font-medium text-fg-muted hover:bg-sunken hover:text-fg aria-[current=page]:border-transparent aria-[current=page]:bg-fg aria-[current=page]:text-canvas"
                  >
                    {t(VIEW_LABELS[v])}
                  </Link>
                </li>
              ))}
            </ul>
          </nav>
          <SearchAndSort search={search} inputRef={searchBox} />
        </div>
        {selected.length > 0 ? (
          <BulkBar
            ids={selected}
            tickets={tickets.filter((tk) => selected.includes(tk.id))}
            onClear={() => setSelected([])}
          />
        ) : null}
        <div className="min-h-0 flex-1 overflow-y-auto">
          {list.isError ? (
            <div className="p-4">
              <ProblemNotice error={list.error} />
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
        </div>
      </section>
      <div
        className={cn(
          "min-h-0 min-w-0 flex-1",
          paneOpen ? "overflow-y-auto" : "hidden xl:grid xl:place-items-center",
        )}
      >
        {paneOpen ? (
          <Outlet />
        ) : (
          <EmptyState icon={<Inbox aria-hidden="true" />} title={t("tickets.pickOne")}>
            {t("tickets.pickOneBody")}
          </EmptyState>
        )}
      </div>
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
      <search className="min-w-56 flex-1">
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
  const all = tickets.length > 0 && tickets.every((tk) => chosen.has(tk.id));
  return (
    <>
      <div className="flex items-center gap-3 border-b border-line px-4 py-1.5 text-xs text-fg-muted">
        <input
          type="checkbox"
          aria-label={t("tickets.selectAll")}
          checked={all}
          onChange={(e) => onSelect(e.target.checked ? tickets.map((tk) => tk.id) : [])}
          className="size-4 cursor-pointer accent-accent"
        />
        <span>{t("tickets.count", { count: tickets.length, more: "false" })}</span>
      </div>
      <ul aria-label={t("tickets.list", { view: t(VIEW_LABELS[view]) })}>
        {tickets.map((tk) => {
          const requester = tk.requester_id ? requesters.data?.get(tk.requester_id) : undefined;
          const { target } = currentTarget(tk.sla);
          const open = tk.id === openId;
          return (
            <li
              key={tk.id}
              className={cn(
                "relative flex gap-3 border-b border-line px-4 py-3 hover:bg-canvas",
                open && "bg-accent-soft hover:bg-accent-soft",
              )}
            >
              {open ? (
                <span aria-hidden="true" className="absolute inset-y-0 left-0 w-0.5 bg-accent" />
              ) : null}
              <input
                type="checkbox"
                aria-label={t("tickets.select", { number: tk.number })}
                checked={chosen.has(tk.id)}
                onChange={(e) =>
                  onSelect(
                    e.target.checked ? [...selected, tk.id] : selected.filter((id) => id !== tk.id),
                  )
                }
                className="relative z-10 mt-0.5 size-4 shrink-0 cursor-pointer accent-accent"
              />
              <div className="grid min-w-0 flex-1 gap-1">
                <div className="flex items-center justify-between gap-2 text-xs text-fg-muted">
                  <span className="font-mono">{tk.number}</span>
                  <time dateTime={tk.updated_at} title={f.dateTime(tk.updated_at)}>
                    {f.relative(tk.updated_at)}
                  </time>
                </div>
                <Link
                  to="/tickets/$ticketId"
                  params={{ ticketId: tk.id }}
                  search={search}
                  aria-current={open ? "page" : undefined}
                  className="line-clamp-2 font-medium text-fg after:absolute after:inset-0 hover:underline [overflow-wrap:anywhere]"
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
                </div>
              </div>
            </li>
          );
        })}
      </ul>
    </>
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
