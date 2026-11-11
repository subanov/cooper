import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { SelectField } from "@/components/ui/select-field";
import { calendarCategoriesQuery, calendarEventsQuery } from "@/features/calendar/api";
import {
  addDays,
  type DayKey,
  dayDate,
  eventsByDay,
  localDay,
  type MonthKey,
  monthGrid,
  shiftMonth,
} from "@/features/calendar/month";
import { can } from "@/features/navigation/navigation";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { cn } from "@/lib/utils";
import { EventDialog } from "./EventDialog";

type CalendarEvent = Schemas["CalendarEventDto"];

/** What the dialog shows: a new event on a day, or an existing event. */
type Open = { kind: "new"; day: DayKey } | { kind: "event"; event: CalendarEvent } | null;

/**
 * The IT Calendar (FR-CAL-001, FR-CAL-002): one month, a list of events per day. The
 * month and Category filter are in the URL; every event opens in a dialog.
 */
export function CalendarPage({ month, category }: { month: MonthKey; category?: string }) {
  const t = useText();
  const intl = useIntl();
  const session = useQuery(analystSessionQuery).data;
  const [open, setOpen] = useState<Open>(null);
  const weeks = monthGrid(month);
  const days = weeks.flat();
  const first = days[0] ?? `${month}-01`;
  const last = days.at(-1) ?? `${month}-28`;
  // One extra day each side: timed events are matched on UTC days, shown on local ones.
  const events = useQuery(
    calendarEventsQuery({
      from: addDays(first, -1),
      to: addDays(last, 1),
      ...(category ? { category } : {}),
    }),
  );
  const categories = useQuery(calendarCategoriesQuery);
  const byDay = eventsByDay(events.data?.items ?? [], days);
  const today = localDay(new Date());
  const canCreate = session ? can(session, "calendar.create") : false;
  const monthLabel = intl.formatDate(dayDate(`${month}-01`), { month: "long", year: "numeric" });
  const weekdays = (weeks[0] ?? []).map((d) => intl.formatDate(dayDate(d), { weekday: "short" }));

  const time = (e: CalendarEvent) =>
    e.all_day || !e.starts_at ? null : intl.formatTime(e.starts_at, { timeStyle: "short" });

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
          {t("nav.calendar")}
        </h1>
        {canCreate ? (
          <Button
            size="sm"
            className="ml-auto"
            onClick={() =>
              setOpen({ kind: "new", day: today.startsWith(month) ? today : `${month}-01` })
            }
          >
            {t("calendar.new")}
          </Button>
        ) : null}
      </div>
      <div className="flex flex-wrap items-end gap-3">
        <nav aria-label={t("calendar.months")} className="flex items-center gap-2">
          <Button asChild size="sm" variant="outline">
            <Link
              to="/calendar"
              search={{ month: shiftMonth(month, -1), ...(category ? { category } : {}) }}
            >
              {t("calendar.previous")}
            </Link>
          </Button>
          <Button asChild size="sm" variant="outline">
            <Link to="/calendar" search={category ? { category } : {}}>
              {t("calendar.today")}
            </Link>
          </Button>
          <Button asChild size="sm" variant="outline">
            <Link
              to="/calendar"
              search={{ month: shiftMonth(month, 1), ...(category ? { category } : {}) }}
            >
              {t("calendar.next")}
            </Link>
          </Button>
        </nav>
        <h2 aria-live="polite" className="text-lg font-semibold">
          {monthLabel}
        </h2>
        <CategoryFilter month={month} category={category} categories={categories.data} />
      </div>
      {events.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {events.isPending ? <p>{t("common.loading")}</p> : null}
      {events.data?.truncated ? <Alert>{t("calendar.truncated")}</Alert> : null}
      <div aria-hidden="true" className="hidden grid-cols-7 gap-1 md:grid">
        {weekdays.map((w) => (
          <div key={w} className="px-2 text-xs font-semibold text-muted-foreground">
            {w}
          </div>
        ))}
      </div>
      <ol className="grid gap-1 md:grid-cols-7" aria-label={monthLabel}>
        {days.map((day) => {
          const list = byDay.get(day) ?? [];
          const inMonth = day.startsWith(month);
          const headingId = `day-${day}`;
          return (
            <li
              key={day}
              aria-labelledby={headingId}
              aria-current={day === today ? "date" : undefined}
              className={cn(
                "grid min-h-24 content-start gap-1 rounded-md border border-border p-1.5",
                inMonth ? "bg-background" : "bg-muted/40",
                list.length === 0 && "hidden md:grid",
                day === today && "border-2 border-primary",
              )}
            >
              <h3 id={headingId} className="text-xs font-semibold">
                <span aria-hidden="true" className={cn(!inMonth && "text-muted-foreground")}>
                  <span className="md:hidden">
                    {intl.formatDate(dayDate(day), { dateStyle: "full" })}
                  </span>
                  <span className="hidden md:inline">{Number(day.slice(8))}</span>
                </span>
                <span className="sr-only">
                  {intl.formatDate(dayDate(day), { dateStyle: "full" })}
                </span>
              </h3>
              {list.length > 0 ? (
                <ul className="grid gap-1">
                  {list.map((event) => (
                    <li key={event.id}>
                      <button
                        type="button"
                        onClick={() => setOpen({ kind: "event", event })}
                        className="grid w-full rounded-sm border border-border border-l-4 bg-muted/30 px-1.5 py-1 text-left text-xs hover:bg-muted"
                        style={{ borderLeftColor: event.colour }}
                      >
                        <span className="font-medium">
                          {time(event) ? `${time(event)} ` : null}
                          {event.title}
                        </span>
                        <span className="text-muted-foreground">
                          {event.category_name}
                          {event.editable ? null : ` · ${t("calendar.generated")}`}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              ) : null}
            </li>
          );
        })}
      </ol>
      {events.data && events.data.items.length === 0 ? (
        <p className="text-muted-foreground">{t("calendar.empty")}</p>
      ) : null}
      {open ? (
        <EventDialog
          key={open.kind === "event" ? open.event.id : "new"}
          event={open.kind === "event" ? open.event : undefined}
          day={open.kind === "new" ? open.day : undefined}
          categories={categories.data ?? []}
          onClose={() => setOpen(null)}
        />
      ) : null}
    </div>
  );
}

function CategoryFilter({
  month,
  category,
  categories,
}: {
  month: MonthKey;
  category: string | undefined;
  categories: readonly Schemas["CalendarCategoryDto"][] | undefined;
}) {
  const t = useText();
  const navigate = useNavigate();
  return (
    <SelectField
      label={t("calendar.category")}
      className="ml-auto"
      value={category ?? ""}
      onChange={(e) =>
        void navigate({
          to: "/calendar",
          search: { month, ...(e.target.value ? { category: e.target.value } : {}) },
        })
      }
    >
      <option value="">{t("calendar.allCategories")}</option>
      {(categories ?? []).map((c) => (
        <option key={c.id} value={c.id}>
          {c.name}
        </option>
      ))}
    </SelectField>
  );
}
