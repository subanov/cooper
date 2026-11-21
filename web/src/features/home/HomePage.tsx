import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ArrowRight, ClipboardCheck, Inbox, SquareKanban } from "lucide-react";
import { ColourLabel } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { EmptyState, QueryState } from "@/components/Feedback";
import { Page, PageHeader, Panel, Stat } from "@/components/Layout";
import { useCan, useModuleOn, useSession } from "@/features/session/session";
import { ticketListQuery } from "@/features/tickets/api";
import { currentTarget, SlaBadge } from "@/features/tickets/Sla";
import { useFormat, useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";

/**
 * The start page: the Watchtower counts (when the Analyst may read reports) and the
 * Analyst's own open tickets, so unfinished work stays visible (Zeigarnik Effect).
 */
export function HomePage() {
  const t = useText();
  const session = useSession();
  const can = useCan();
  const moduleOn = useModuleOn();
  const hour = new Date().getHours();
  const greeting =
    hour < 12
      ? "home.greeting.morning"
      : hour < 18
        ? "home.greeting.afternoon"
        : "home.greeting.evening";
  const first = session.analyst.full_name.split(" ")[0] ?? session.analyst.full_name;
  return (
    <Page>
      <PageHeader
        title={t(greeting, { name: first })}
        description={t("home.intro", { key: "Ctrl K" })}
        actions={
          <>
            {moduleOn("morning-checks") && can("morning-checks.record") ? (
              <Link to="/morning-checks/today" className={buttonClass()}>
                <ClipboardCheck aria-hidden="true" />
                {t("home.recordChecks")}
              </Link>
            ) : null}
            {moduleOn("tasks") && can("tasks.read") ? (
              <Link to="/tasks" search={{ mine: true }} className={buttonClass()}>
                <SquareKanban aria-hidden="true" />
                {t("home.myTasks")}
              </Link>
            ) : null}
          </>
        }
      />
      {can("reporting.read") ? <Watchtower /> : null}
      {can("tickets.read") && moduleOn("tickets") ? <MyTickets /> : null}
      <p className="text-xs text-fg-muted">
        {session.all_companies
          ? t("home.allCompanies")
          : t("home.companies", { count: session.company_ids.length })}
      </p>
    </Page>
  );
}

function Watchtower() {
  const t = useText();
  const query = useQuery({
    queryKey: ["home", "watchtower"],
    queryFn: () => call(api.GET("/api/v1/watchtower")),
    refetchInterval: 60_000,
  });
  return (
    <section aria-labelledby="watchtower-title" className="grid gap-3">
      <div>
        <h2 id="watchtower-title" className="text-[0.9375rem] font-semibold">
          {t("home.watchtower")}
        </h2>
        <p className="text-xs text-fg-muted">{t("home.watchtowerIntro")}</p>
      </div>
      <QueryState query={query}>
        {(w) => (
          <div className="grid gap-4 lg:grid-cols-3">
            {w.tickets ? (
              <Panel title={t("nav.tickets")} headingLevel={3}>
                <div className="grid grid-cols-3 gap-2">
                  <Stat
                    label={t("home.openTickets")}
                    value={w.tickets.total_open}
                    to="/tickets"
                    search={{ view: "open" }}
                  />
                  <Stat
                    label={t("home.unassigned")}
                    value={w.tickets.unassigned}
                    tone={w.tickets.unassigned > 0 ? "warning" : undefined}
                    to="/tickets"
                    search={{ view: "unassigned" }}
                  />
                  <Stat
                    label={t("home.highPriority")}
                    value={w.tickets.high_priority}
                    tone={w.tickets.high_priority > 0 ? "danger" : undefined}
                  />
                </div>
                {w.tickets.by_status.length > 0 ? (
                  <ul
                    aria-label={t("home.byStatus")}
                    className="mt-3 grid gap-1.5 border-t border-line pt-3 text-sm"
                  >
                    {w.tickets.by_status.map((s) => (
                      <li key={s.id} className="flex items-center justify-between gap-2">
                        <ColourLabel colour={s.colour}>{s.name}</ColourLabel>
                        <span className="font-semibold tabular-nums">{s.count}</span>
                      </li>
                    ))}
                  </ul>
                ) : null}
              </Panel>
            ) : null}
            {w.tasks ? (
              <Panel title={t("nav.tasks")} headingLevel={3}>
                <div className="grid grid-cols-3 gap-2">
                  <Stat label={t("home.openTasks")} value={w.tasks.total_open} to="/tasks" />
                  <Stat
                    label={t("home.overdue")}
                    value={w.tasks.overdue}
                    tone={w.tasks.overdue > 0 ? "danger" : undefined}
                  />
                  <Stat
                    label={t("home.dueToday")}
                    value={w.tasks.due_today}
                    tone={w.tasks.due_today > 0 ? "warning" : undefined}
                  />
                </div>
              </Panel>
            ) : null}
            {w.morning_checks ? (
              <Panel title={t("home.checks")} headingLevel={3}>
                <div className="grid gap-3">
                  <div className="grid grid-cols-2 gap-2">
                    <Stat
                      label={t("home.checksRecorded", {
                        recorded: w.morning_checks.recorded,
                        total: w.morning_checks.checks,
                      })}
                      value={`${w.morning_checks.checks ? Math.round((w.morning_checks.recorded / w.morning_checks.checks) * 100) : 0}%`}
                      to="/morning-checks/today"
                    />
                    <Stat
                      label={t("home.needAttention")}
                      value={w.morning_checks.attention}
                      tone={w.morning_checks.attention > 0 ? "danger" : undefined}
                    />
                  </div>
                  <div
                    role="progressbar"
                    aria-label={t("home.checks")}
                    aria-valuemin={0}
                    aria-valuemax={w.morning_checks.checks}
                    aria-valuenow={w.morning_checks.recorded}
                    className="h-2 overflow-hidden rounded-full bg-sunken"
                  >
                    <div
                      className="h-full rounded-full bg-accent"
                      style={{
                        width: `${w.morning_checks.checks ? (w.morning_checks.recorded / w.morning_checks.checks) * 100 : 0}%`,
                      }}
                    />
                  </div>
                </div>
              </Panel>
            ) : null}
          </div>
        )}
      </QueryState>
    </section>
  );
}

function MyTickets() {
  const t = useText();
  const f = useFormat();
  const list = useInfiniteQuery(
    ticketListQuery({ view: "mine", sort: "updated_at", order: "desc" }),
  );
  const tickets = list.data?.pages[0]?.items.slice(0, 8) ?? [];
  return (
    <Panel
      title={t("home.myTickets")}
      flush
      actions={
        <Link
          to="/tickets"
          search={{ view: "mine" }}
          className={buttonClass({ variant: "quiet", size: "sm" })}
        >
          {t("home.seeAll")}
          <ArrowRight aria-hidden="true" />
        </Link>
      }
    >
      {list.isSuccess && tickets.length === 0 ? (
        <EmptyState icon={<Inbox aria-hidden="true" />} title={t("home.myTicketsEmpty")} />
      ) : (
        <ul className="divide-y divide-line">
          {tickets.map((tk) => (
            <li
              key={tk.id}
              className="relative flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2.5 hover:bg-sunken/60"
            >
              <span className="font-mono text-xs text-fg-muted">{tk.number}</span>
              <Link
                to="/tickets/$ticketId"
                params={{ ticketId: tk.id }}
                search={{ view: "mine" }}
                className="min-w-0 flex-1 truncate font-medium after:absolute after:inset-0 hover:underline"
              >
                {tk.subject}
              </Link>
              <SlaBadge target={currentTarget(tk.sla).target} />
              <time dateTime={tk.updated_at} className="text-xs text-fg-muted">
                {f.relative(tk.updated_at)}
              </time>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}
