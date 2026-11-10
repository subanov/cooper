import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import {
  Link,
  Outlet,
  useLocation,
  useNavigate,
  useParams,
  useSearch,
} from "@tanstack/react-router";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { can } from "@/features/navigation/navigation";
import { analystSessionQuery } from "@/features/session/queries";
import {
  analystsQuery,
  namesById,
  prioritiesQuery,
  requesterNamesQuery,
  statusesQuery,
  ticketListQuery,
} from "@/features/tickets/queries";
import {
  DEFAULT_VIEW,
  SORTS,
  type Sort,
  type TicketSearch,
  VIEW_LABELS,
  VIEWS,
} from "@/features/tickets/views";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/utils";
import { useKeys } from "../useShortcuts";
import { SlaBadge } from "./SlaBadge";

const SORT_LABELS: Record<Sort, MessageId> = {
  updated_at: "tickets.sortUpdated",
  created_at: "tickets.sortCreated",
  number: "tickets.sortNumber",
};

/**
 * The ticket list (FR-TKT-040) beside the selected ticket (split pane). View, search
 * and sort live in the URL; `j`/`k` move through the list, `/` searches.
 */
export function TicketsPage() {
  const t = useText();
  const intl = useIntl();
  const navigate = useNavigate();
  // Typed here: the route tree imports this component, so inference would be circular.
  const search: TicketSearch = useSearch({ from: "/analyst/tickets" });
  const params = useParams({ strict: false }) as { ticketId?: string };
  const selected = params.ticketId;
  const creating = useLocation({ select: (l) => l.pathname === "/tickets/new" });
  // The right-hand pane shows a ticket or the New ticket form.
  const pane = Boolean(selected) || creating;
  const session = useQuery(analystSessionQuery).data;
  const canCreate = session ? can(session, "tickets.create") : false;
  const list = useInfiniteQuery(ticketListQuery(search));
  const statuses = useQuery(statusesQuery);
  const priorities = useQuery(prioritiesQuery);
  const analysts = useQuery(analystsQuery);
  const tickets = useMemo(() => list.data?.pages.flatMap((p) => p.items) ?? [], [list.data]);
  const requesters = useQuery(
    requesterNamesQuery(tickets.flatMap((tk) => (tk.requester_id ? [tk.requester_id] : []))),
  );
  const statusNames = namesById(statuses.data, (s) => s.name);
  const priorityNames = namesById(priorities.data, (p) => p.name);
  const analystNames = namesById(analysts.data, (a) => a.full_name);
  const searchInput = useRef<HTMLInputElement>(null);
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  const view = search.view ?? DEFAULT_VIEW;

  function setSearch(next: Partial<TicketSearch>) {
    void navigate({ to: ".", search: (prev: TicketSearch) => ({ ...prev, ...next }) });
  }

  function submitSearch(event: FormEvent) {
    event.preventDefault();
    setSearch({ q: q.trim() || undefined });
  }

  function open(offset: number) {
    if (tickets.length === 0) return;
    const index = tickets.findIndex((tk) => tk.id === selected);
    const next = index === -1 ? (offset > 0 ? 0 : tickets.length - 1) : index + offset;
    const ticket = tickets[Math.min(Math.max(next, 0), tickets.length - 1)];
    if (ticket && ticket.id !== selected) {
      void navigate({
        to: "/tickets/$ticketId",
        params: { ticketId: ticket.id },
        search: (prev: TicketSearch) => prev,
      });
    }
  }

  useKeys({
    j: () => open(1),
    k: () => open(-1),
    "/": () => searchInput.current?.focus(),
  });

  return (
    <div className="flex min-h-0 gap-6">
      <section
        aria-labelledby="tickets-title"
        className={cn("grid min-w-0 content-start gap-3", pane ? "w-[44%] shrink-0" : "flex-1")}
      >
        <div className="flex items-center gap-3">
          <h1 id="tickets-title" tabIndex={-1} className="text-2xl font-semibold outline-none">
            {t("nav.tickets")}
          </h1>
          {canCreate ? (
            <Button asChild size="sm" className="ml-auto">
              <Link to="/tickets/new">{t("newTicket.open")}</Link>
            </Button>
          ) : null}
        </div>
        <nav aria-label={t("views.label")}>
          <ul className="flex flex-wrap gap-1">
            {VIEWS.map((v) => (
              <li key={v}>
                <Link
                  to="/tickets"
                  search={(prev: TicketSearch) => ({ ...prev, view: v })}
                  aria-current={v === view ? "page" : undefined}
                  className="block rounded-md px-3 py-1.5 text-sm hover:bg-muted aria-[current=page]:bg-muted aria-[current=page]:font-medium"
                >
                  {t(VIEW_LABELS[v])}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
        <div className="flex flex-wrap items-end gap-2">
          <search className="flex flex-1">
            <form onSubmit={submitSearch} className="flex flex-1 gap-2">
              <label htmlFor="ticket-search" className="sr-only">
                {t("tickets.search")}
              </label>
              <Input
                ref={searchInput}
                id="ticket-search"
                type="search"
                placeholder={t("tickets.searchPlaceholder")}
                aria-keyshortcuts="/"
                value={q}
                onChange={(e) => setQ(e.target.value)}
              />
              <Button type="submit" variant="outline">
                {t("tickets.searchSubmit")}
              </Button>
            </form>
          </search>
          <div className="flex items-center gap-2 text-sm">
            <label htmlFor="ticket-sort">{t("tickets.sort")}</label>
            <Select
              id="ticket-sort"
              className="w-auto"
              value={search.sort ?? "updated_at"}
              onChange={(e) => setSearch({ sort: e.target.value as Sort })}
            >
              {SORTS.map((s) => (
                <option key={s} value={s}>
                  {t(SORT_LABELS[s])}
                </option>
              ))}
            </Select>
          </div>
        </div>

        {list.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
        {list.isPending ? (
          <p role="status" className="text-sm text-muted-foreground">
            {t("common.loading")}
          </p>
        ) : tickets.length === 0 && !list.isError ? (
          <p role="status" className="text-sm text-muted-foreground">
            {search.q ? t("tickets.noMatches") : t("tickets.empty")}
          </p>
        ) : (
          <table className="w-full table-fixed border-collapse text-sm">
            <caption className="sr-only">
              {t("tickets.caption", { view: t(VIEW_LABELS[view]) })}
            </caption>
            <thead className="text-left text-xs text-muted-foreground">
              <tr className="border-b border-border">
                <th scope="col" className="py-2 pr-2 font-medium">
                  {t("tickets.colTicket")}
                </th>
                <th scope="col" className="w-28 py-2 pr-2 font-medium">
                  {t("tickets.colStatus")}
                </th>
                {pane ? null : (
                  <th scope="col" className="w-36 py-2 pr-2 font-medium">
                    {t("tickets.colAssignee")}
                  </th>
                )}
                <th scope="col" className="w-40 py-2 font-medium">
                  {t("tickets.colSla")}
                </th>
              </tr>
            </thead>
            <tbody>
              {tickets.map((ticket) => {
                const requester = ticket.requester_id
                  ? requesters.data?.[ticket.requester_id]?.full_name
                  : undefined;
                const isSelected = ticket.id === selected;
                return (
                  <tr
                    key={ticket.id}
                    className={cn(
                      "border-b border-border align-top hover:bg-muted/60",
                      isSelected && "bg-muted",
                    )}
                  >
                    <td className="py-2 pr-2">
                      <Link
                        to="/tickets/$ticketId"
                        params={{ ticketId: ticket.id }}
                        search={(prev: TicketSearch) => prev}
                        aria-current={isSelected ? "true" : undefined}
                        className="block font-medium underline-offset-4 hover:underline"
                      >
                        <span className="mr-2 font-mono text-xs text-muted-foreground">
                          {ticket.number}
                        </span>
                        <span className="break-words">{ticket.subject}</span>
                      </Link>
                      <span className="block truncate text-xs text-muted-foreground">
                        {[
                          requester,
                          priorityNames.get(ticket.priority_id),
                          intl.formatDate(ticket.updated_at, {
                            dateStyle: "medium",
                            timeStyle: "short",
                          }),
                        ]
                          .filter(Boolean)
                          .join(" · ")}
                      </span>
                    </td>
                    <td className="py-2 pr-2">
                      <Badge>{statusNames.get(ticket.status_id) ?? "…"}</Badge>
                      {ticket.sensitivity === "confidential" ? (
                        <Badge tone="muted" className="mt-1">
                          {t("tickets.confidential")}
                        </Badge>
                      ) : null}
                    </td>
                    {pane ? null : (
                      <td className="truncate py-2 pr-2">
                        {ticket.analyst_id
                          ? (analystNames.get(ticket.analyst_id) ?? "…")
                          : t("tickets.unassigned")}
                      </td>
                    )}
                    <td className="py-2">
                      <SlaBadge sla={ticket.sla} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        {list.hasNextPage ? (
          <Button
            variant="outline"
            onClick={() => void list.fetchNextPage()}
            disabled={list.isFetchingNextPage}
          >
            {list.isFetchingNextPage ? t("common.loading") : t("tickets.more")}
          </Button>
        ) : null}
        <p className="text-xs text-muted-foreground">{t("tickets.keysHint")}</p>
      </section>
      {pane ? (
        <div className="min-w-0 flex-1 border-l border-border pl-6">
          <Outlet />
        </div>
      ) : null}
    </div>
  );
}
