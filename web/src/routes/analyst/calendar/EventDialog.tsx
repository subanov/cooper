import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { useIntl } from "react-intl";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { SelectField } from "@/components/ui/select-field";
import { Textarea } from "@/components/ui/textarea";
import {
  createCalendarEvent,
  deleteCalendarEvent,
  updateCalendarEvent,
} from "@/features/calendar/api";
import { type DayKey, dayDate, localDay } from "@/features/calendar/month";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import { analystSessionQuery } from "@/features/session/queries";
import { companiesQuery, defaultCompanyId } from "@/features/users/queries";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";

type CalendarEvent = Schemas["CalendarEventDto"];

const pad = (n: number) => String(n).padStart(2, "0");
const localTime = (instant: Date) => `${pad(instant.getHours())}:${pad(instant.getMinutes())}`;

/** Where the UI opens the record a generated event comes from. */
const SOURCE_LINKS: Record<string, { path: string; label: MessageId }> = {
  contract: { path: "/contracts", label: "calendar.openContract" },
  asset: { path: "/assets", label: "calendar.openAsset" },
};

/**
 * A Calendar Event in a dialog: a form for new and editable manual events, the details
 * with a link to their source for generated ones (read-only, FR-CAL-002).
 */
export function EventDialog({
  event,
  day,
  categories,
  onClose,
}: {
  event: CalendarEvent | undefined;
  day: DayKey | undefined;
  categories: readonly Schemas["CalendarCategoryDto"][];
  onClose: () => void;
}) {
  const t = useText();
  const session = useQuery(analystSessionQuery).data;
  const editable = event
    ? event.editable && (session ? can(session, "calendar.update") : false)
    : true;
  return (
    <Dialog open onOpenChange={(isOpen) => (isOpen ? null : onClose())}>
      <DialogContent className="max-h-[80vh] overflow-y-auto p-5">
        <DialogTitle>
          {event ? (editable ? t("calendar.edit") : event.title) : t("calendar.new")}
        </DialogTitle>
        {editable ? (
          <EventForm event={event} day={day} categories={categories} onClose={onClose} />
        ) : event ? (
          <EventDetails event={event} onClose={onClose} />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

function When({ event }: { event: CalendarEvent }) {
  const intl = useIntl();
  if (event.all_day && event.start_date && event.end_date) {
    const start = intl.formatDate(dayDate(event.start_date), { dateStyle: "full" });
    return (
      <>
        {event.start_date === event.end_date
          ? start
          : `${start} – ${intl.formatDate(dayDate(event.end_date), { dateStyle: "full" })}`}
      </>
    );
  }
  return (
    <>
      {`${intl.formatDate(event.starts_at ?? "", { dateStyle: "full", timeStyle: "short" })} – ${intl.formatDate(
        event.ends_at ?? "",
        { dateStyle: "full", timeStyle: "short" },
      )}`}
    </>
  );
}

function EventDetails({ event, onClose }: { event: CalendarEvent; onClose: () => void }) {
  const t = useText();
  const source = event.source ? SOURCE_LINKS[event.source.kind] : undefined;
  return (
    <div className="mt-3 grid gap-3 text-sm">
      {event.source ? (
        <DialogDescription>{t("calendar.generatedHint")}</DialogDescription>
      ) : (
        <DialogDescription className="sr-only">{event.title}</DialogDescription>
      )}
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2">
        <dt className="font-medium">{t("calendar.when")}</dt>
        <dd>
          <When event={event} />
        </dd>
        <dt className="font-medium">{t("calendar.category")}</dt>
        <dd className="flex items-center gap-2">
          <span
            aria-hidden="true"
            className="inline-block size-3 rounded-sm border border-border"
            style={{ backgroundColor: event.colour }}
          />
          {event.category_name}
        </dd>
        {event.location ? (
          <>
            <dt className="font-medium">{t("calendar.location")}</dt>
            <dd>{event.location}</dd>
          </>
        ) : null}
      </dl>
      {event.description ? <p className="whitespace-pre-wrap">{event.description}</p> : null}
      <div className="flex items-center gap-3">
        {event.source && source ? (
          <a
            href={`${source.path}/${event.source.id}`}
            className="font-medium text-primary underline-offset-2 hover:underline"
          >
            {t(source.label)}
          </a>
        ) : null}
        <Button variant="outline" size="sm" className="ml-auto" onClick={onClose}>
          {t("calendar.close")}
        </Button>
      </div>
    </div>
  );
}

function EventForm({
  event,
  day,
  categories,
  onClose,
}: {
  event: CalendarEvent | undefined;
  day: DayKey | undefined;
  categories: readonly Schemas["CalendarCategoryDto"][];
  onClose: () => void;
}) {
  const t = useText();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const companies = useQuery(companiesQuery);
  const start = event?.starts_at ? new Date(event.starts_at) : undefined;
  const end = event?.ends_at ? new Date(event.ends_at) : undefined;
  const [title, setTitle] = useState(event?.title ?? "");
  const [categoryId, setCategoryId] = useState(event?.category_id ?? "");
  const [companyId, setCompanyId] = useState("");
  const [allDay, setAllDay] = useState(event?.all_day ?? false);
  const [startDate, setStartDate] = useState(
    event?.start_date ?? (start ? localDay(start) : (day ?? "")),
  );
  const [startTime, setStartTime] = useState(start ? localTime(start) : "09:00");
  const [endDate, setEndDate] = useState(event?.end_date ?? (end ? localDay(end) : (day ?? "")));
  const [endTime, setEndTime] = useState(end ? localTime(end) : "10:00");
  const [location, setLocation] = useState(event?.location ?? "");
  const [description, setDescription] = useState(event?.description ?? "");
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [confirmDelete, setConfirmDelete] = useState(false);
  // Only active categories Analysts may use; an event keeps the one it has.
  const choices = categories.filter(
    (c) => !c.system_key && (c.is_active || c.id === event?.category_id),
  );
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);
  const company = companyId || defaultCompanyId(companies.data);

  function validate(): Record<string, string> {
    const found: Record<string, string> = {};
    if (!title.trim()) found.title = t("calendar.titleRequired");
    if (!categoryId) found.category = t("calendar.categoryRequired");
    if (!startDate) found.start = t("calendar.startRequired");
    const startKey = allDay ? startDate : `${startDate}T${startTime}`;
    const endKey = allDay ? endDate || startDate : `${endDate || startDate}T${endTime}`;
    if (startDate && endKey < startKey) found.end = t("calendar.endBeforeStart");
    return found;
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    const found = validate();
    setErrors(found);
    if (Object.keys(found).length > 0) {
      return;
    }
    const lastDay = endDate || startDate;
    const body: Schemas["CalendarEventRequest"] = {
      title: title.trim(),
      description,
      category_id: categoryId,
      all_day: allDay,
      location: location.trim() || null,
      ...(allDay
        ? { start_date: startDate, end_date: lastDay }
        : {
            starts_at: new Date(`${startDate}T${startTime}`).toISOString(),
            ends_at: new Date(`${lastDay}T${endTime}`).toISOString(),
          }),
      ...(!event && company ? { company_id: company } : {}),
    };
    const outcome = event
      ? await updateCalendarEvent(event.id, body)
      : await createCalendarEvent(body);
    if (!outcome.ok) {
      flash.fail(t("calendar.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["calendar"] });
    onClose();
  }

  async function remove() {
    if (!event) return;
    if (!confirmDelete) {
      setConfirmDelete(true);
      return;
    }
    const outcome = await deleteCalendarEvent(event.id);
    if (!outcome.ok) {
      flash.fail(t("calendar.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["calendar"] });
    onClose();
  }

  return (
    <form onSubmit={submit} noValidate className="mt-3 grid gap-3">
      <DialogDescription className="sr-only">{t("calendar.formHint")}</DialogDescription>
      <Field
        label={t("calendar.title")}
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        error={errors.title}
        required
      />
      <div className="grid gap-3 sm:grid-cols-2">
        <SelectField
          label={t("calendar.category")}
          value={categoryId}
          onChange={(e) => setCategoryId(e.target.value)}
          error={errors.category}
          required
        >
          <option value="">{t("calendar.chooseCategory")}</option>
          {choices.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </SelectField>
        {!event && activeCompanies.length > 1 ? (
          <SelectField
            label={t("newTicket.company")}
            value={company}
            onChange={(e) => setCompanyId(e.target.value)}
          >
            {activeCompanies.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </SelectField>
        ) : null}
      </div>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={allDay} onChange={(e) => setAllDay(e.target.checked)} />
        {t("calendar.allDay")}
      </label>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field
          label={t("calendar.startDate")}
          type="date"
          value={startDate}
          onChange={(e) => setStartDate(e.target.value)}
          error={errors.start}
          required
        />
        {allDay ? null : (
          <Field
            label={t("calendar.startTime")}
            type="time"
            value={startTime}
            onChange={(e) => setStartTime(e.target.value)}
          />
        )}
        <Field
          label={t("calendar.endDate")}
          type="date"
          value={endDate}
          onChange={(e) => setEndDate(e.target.value)}
          error={errors.end}
        />
        {allDay ? null : (
          <Field
            label={t("calendar.endTime")}
            type="time"
            value={endTime}
            onChange={(e) => setEndTime(e.target.value)}
          />
        )}
      </div>
      <Field
        label={t("calendar.location")}
        value={location}
        onChange={(e) => setLocation(e.target.value)}
      />
      <div className="grid gap-1.5">
        <Label htmlFor="calendar-description">{t("calendar.description")}</Label>
        <Textarea
          id="calendar-description"
          rows={3}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <Button type="submit">{event ? t("users.save") : t("users.create")}</Button>
        <Button type="button" variant="outline" onClick={onClose}>
          {t("calendar.cancel")}
        </Button>
        {event ? (
          <Button type="button" variant="destructive" className="ml-auto" onClick={remove}>
            {confirmDelete ? t("calendar.confirmDelete") : t("calendar.delete")}
          </Button>
        ) : null}
      </div>
      {flash.element}
    </form>
  );
}
