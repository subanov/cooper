import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { CalendarX, ChevronLeft, ChevronRight, Plus } from "lucide-react";
import { useState } from "react";
import { useIntl } from "react-intl";
import { Badge, ColourLabel } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { EmptyState, Notice, QueryState, Refreshing } from "@/components/Feedback";
import { Page, PageHeader, Panel } from "@/components/Layout";
import { FilterSelect } from "@/components/ListSearch";
import { Segmented } from "@/components/Tabs";
import { toOptions } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { cn } from "@/lib/cn";
import {
  addDays,
  type DayKey,
  dayDate,
  localDay,
  type MonthKey,
  monthGrid,
  shiftMonth,
} from "@/lib/dates";
import { call } from "@/lib/request";
import { EventDialog } from "./EventDialog";

export type CalendarEvent = Schemas["CalendarEventDto"];
export type CalendarView = "month" | "list";

export const calendarCategoriesQuery = {
  queryKey: ["reference", "calendar-categories"],
  queryFn: () => call(api.GET("/api/v1/calendar/categories")),
  staleTime: 5 * 60_000,
} as const;

/** The days an event covers in the browser's time zone. */
export function eventDays(event: CalendarEvent): { first: DayKey; last: DayKey } {
  if (event.all_day && event.start_date)
    return { first: event.start_date, last: event.end_date ?? event.start_date };
  const start = new Date(event.starts_at ?? 0);
  const end = new Date(event.ends_at ?? event.starts_at ?? 0);
  const first = localDay(start);
  let last = localDay(end);
  // An event that ends at midnight does not occupy the day it ends on.
  if (last > first && end.getHours() === 0 && end.getMinutes() === 0) last = addDays(last, -1);
  return { first, last };
}

/**
 * The IT Calendar (FR-CAL-001, FR-CAL-002): a month grid, or a list that reads well on
 * small screens and with a screen reader. Month, category and view are in the URL.
 */
export function CalendarPage({
  month,
  category,
  view: chosen,
}: {
  month: MonthKey;
  category?: string | undefined;
  view?: CalendarView | undefined;
}) {
  const t = useText();
  const intl = useIntl();
  const can = useCan();
  const navigate = useNavigate();
  const weeks = monthGrid(month);
  const from = weeks[0]?.[0] ?? `${month}-01`;
  const to = weeks[weeks.length - 1]?.[6] ?? `${month}-28`;
  const narrow = typeof window !== "undefined" && window.matchMedia?.("(max-width: 767px)").matches;
  const view: CalendarView = chosen ?? (narrow ? "list" : "month");
  const [editing, setEditing] = useState<CalendarEvent | "new" | null>(null);
  const [newDay, setNewDay] = useState<DayKey | undefined>(undefined);
  const categories = useQuery(calendarCategoriesQuery);
  const events = useQuery({
    queryKey: ["calendar", "events", from, to, category ?? null],
    queryFn: () =>
      call(
        api.GET("/api/v1/calendar/events", {
          params: { query: { from, to, ...(category ? { category_id: category } : {}) } },
        }),
      ),
  });
  const today = localDay();
  const title = intl.formatDate(dayDate(`${month}-01`), { month: "long", year: "numeric" });
  const go = (next: { month?: MonthKey; category?: string | undefined; view?: CalendarView }) =>
    void navigate({
      to: "/calendar",
      search: {
        month: next.month ?? month,
        ...(("category" in next ? next.category : category)
          ? { category: "category" in next ? next.category : category }
          : {}),
        ...((next.view ?? chosen) ? { view: next.view ?? chosen } : {}),
      },
    });
  const byDay = (items: CalendarEvent[]) => {
    const map = new Map<DayKey, CalendarEvent[]>();
    for (const event of items) {
      const { first, last } = eventDays(event);
      // Day keys are ISO strings, so string comparison orders them.
      const start = [first, from].sort()[1] ?? first;
      for (let d = start; d <= last && d <= to; d = addDays(d, 1))
        map.set(d, [...(map.get(d) ?? []), event]);
    }
    return map;
  };
  const addOn = (day: DayKey | undefined) => {
    setNewDay(day);
    setEditing("new");
  };

  return (
    <Page width="full">
      <PageHeader
        title={t("calendar.title")}
        description={t("calendar.intro")}
        actions={
          can("calendar.create") ? (
            <Button
              variant="primary"
              icon={<Plus aria-hidden="true" />}
              onClick={() => addOn(undefined)}
            >
              {t("calendar.new")}
            </Button>
          ) : undefined
        }
      />
      <div
        role="toolbar"
        aria-label={t("calendar.toolbar")}
        className="flex flex-wrap items-center gap-x-4 gap-y-2"
      >
        <div className="flex items-center gap-1">
          <IconButton
            label={t("calendar.previous")}
            variant="secondary"
            icon={<ChevronLeft aria-hidden="true" />}
            onClick={() => go({ month: shiftMonth(month, -1) })}
          />
          <Button onClick={() => go({ month: today.slice(0, 7) })}>{t("calendar.today")}</Button>
          <IconButton
            label={t("calendar.next")}
            variant="secondary"
            icon={<ChevronRight aria-hidden="true" />}
            onClick={() => go({ month: shiftMonth(month, 1) })}
          />
        </div>
        <h2 aria-live="polite" className="min-w-40 text-lg font-semibold capitalize">
          {title}
        </h2>
        <FilterSelect
          label={t("calendar.category")}
          allLabel={t("calendar.allCategories")}
          value={category}
          options={toOptions(categories.data, (c) => c.name)}
          onChange={(c) => go({ category: c })}
        />
        <Segmented
          label={t("calendar.view")}
          value={view}
          onChange={(v) => go({ view: v })}
          options={[
            { value: "month", label: t("calendar.month") },
            { value: "list", label: t("calendar.list") },
          ]}
        />
        <Refreshing active={events.isFetching && !events.isPending} />
      </div>
      <QueryState query={events}>
        {(data) => {
          const map = byDay(data.items);
          return (
            <>
              {data.truncated ? <Notice tone="warning">{t("calendar.truncated")}</Notice> : null}
              {view === "month" ? (
                <Panel flush>
                  <div className="overflow-x-auto">
                    <table className="w-full min-w-[42rem] table-fixed border-collapse text-sm">
                      <caption className="sr-only">{title}</caption>
                      <thead>
                        <tr>
                          {(weeks[0] ?? []).map((d) => (
                            <th
                              key={d}
                              scope="col"
                              className="border-b border-line py-2 text-xs font-semibold text-fg-muted"
                            >
                              <abbr
                                title={intl.formatDate(dayDate(d), { weekday: "long" })}
                                className="no-underline"
                              >
                                {intl.formatDate(dayDate(d), { weekday: "short" })}
                              </abbr>
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {weeks.map((week) => (
                          <tr key={week[0]}>
                            {week.map((day) => {
                              const items = map.get(day) ?? [];
                              const outside = day.slice(0, 7) !== month;
                              return (
                                <td
                                  key={day}
                                  className={cn(
                                    "group h-28 border border-line p-1 align-top",
                                    outside && "bg-sunken/50",
                                  )}
                                >
                                  <div className="flex items-center justify-between">
                                    <span
                                      className={cn(
                                        "grid size-6 place-items-center rounded-full text-xs",
                                        day === today
                                          ? "bg-accent font-semibold text-accent-fg"
                                          : outside
                                            ? "text-fg-muted"
                                            : "",
                                      )}
                                    >
                                      <span className="sr-only">
                                        {intl.formatDate(dayDate(day), { dateStyle: "full" })}
                                      </span>
                                      <span aria-hidden="true">{Number(day.slice(8))}</span>
                                    </span>
                                    {can("calendar.create") ? (
                                      <button
                                        type="button"
                                        aria-label={t("calendar.newOn", {
                                          date: intl.formatDate(dayDate(day), {
                                            dateStyle: "long",
                                          }),
                                        })}
                                        className="grid size-6 place-items-center rounded-sm text-fg-muted opacity-0 hover:bg-sunken focus-visible:opacity-100 group-hover:opacity-100"
                                        onClick={() => addOn(day)}
                                      >
                                        <Plus aria-hidden="true" className="size-3.5" />
                                      </button>
                                    ) : null}
                                  </div>
                                  <ul className="mt-1 grid gap-0.5">
                                    {items.slice(0, 3).map((ev) => (
                                      <li key={ev.id}>
                                        <EventChip event={ev} onOpen={() => setEditing(ev)} />
                                      </li>
                                    ))}
                                    {items.length > 3 ? (
                                      <li>
                                        <button
                                          type="button"
                                          className="px-1 text-xs text-accent-text hover:underline"
                                          onClick={() => go({ view: "list" })}
                                        >
                                          {t("calendar.more", { count: items.length - 3 })}
                                        </button>
                                      </li>
                                    ) : null}
                                  </ul>
                                </td>
                              );
                            })}
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </Panel>
              ) : (
                <Agenda month={month} map={map} onOpen={setEditing} />
              )}
            </>
          );
        }}
      </QueryState>
      {editing ? (
        <EventDialog
          event={editing === "new" ? undefined : editing}
          day={newDay}
          categories={categories.data ?? []}
          onClose={() => setEditing(null)}
        />
      ) : null}
    </Page>
  );
}

function EventChip({ event, onOpen }: { event: CalendarEvent; onOpen: () => void }) {
  const f = useFormat();
  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex w-full items-center gap-1 truncate rounded-sm px-1 py-0.5 text-left text-xs hover:bg-sunken"
    >
      <ColourLabel colour={event.colour}>
        {!event.all_day && event.starts_at ? (
          <span className="text-fg-muted">{f.time(event.starts_at)} </span>
        ) : null}
        {event.title}
      </ColourLabel>
    </button>
  );
}

function Agenda({
  month,
  map,
  onOpen,
}: {
  month: MonthKey;
  map: Map<DayKey, CalendarEvent[]>;
  onOpen: (e: CalendarEvent) => void;
}) {
  const t = useText();
  const f = useFormat();
  const days = [...map.keys()].filter((d) => d.slice(0, 7) === month).sort();
  if (days.length === 0)
    return <EmptyState icon={<CalendarX aria-hidden="true" />} title={t("calendar.empty")} />;
  return (
    <div className="grid max-w-3xl gap-4">
      {days.map((day) => (
        <section
          key={day}
          aria-label={t("calendar.eventsOn", { date: f.day(day) })}
          className="grid gap-2"
        >
          <h3 className="text-sm font-semibold">{f.day(day)}</h3>
          <ul className="grid gap-1.5">
            {(map.get(day) ?? []).map((ev) => (
              <li key={ev.id}>
                <button
                  type="button"
                  onClick={() => onOpen(ev)}
                  className="flex w-full flex-wrap items-center gap-2 rounded-md border border-line bg-surface px-3 py-2 text-left text-sm shadow-raised hover:border-control"
                >
                  <ColourLabel colour={ev.colour} className="font-medium">
                    {ev.title}
                  </ColourLabel>
                  <span className="text-xs text-fg-muted">
                    {ev.all_day ? t("calendar.allDay") : ev.starts_at ? f.time(ev.starts_at) : ""} ·{" "}
                    {ev.category_name}
                  </span>
                  {ev.source ? <Badge tone="muted">{t("calendar.generated")}</Badge> : null}
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
