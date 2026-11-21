import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Trash2 } from "lucide-react";
import { useState } from "react";
import { ColourLabel } from "@/components/Badge";
import { Button, buttonClass } from "@/components/Button";
import { ConfirmDialog, Dialog, FormDialog } from "@/components/Dialog";
import { Facts } from "@/components/Facts";
import { Checkbox, FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { Notice, ProblemNotice } from "@/components/Notice";
import { companiesQuery } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { fieldError, orNull, useAction } from "@/features/shared/mutation";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { fromLocalInput, localDay, toLocalInput } from "@/lib/dates";
import { call } from "@/lib/request";
import type { CalendarEvent } from "./CalendarPage";

type Category = Schemas["CalendarCategoryDto"];

/**
 * A Calendar Event: generated ones are shown read-only with a link to their Source;
 * manual ones open as a form (when the Analyst may change them).
 */
export function EventDialog({
  event,
  day,
  categories,
  onClose,
}: {
  event?: CalendarEvent | undefined;
  day?: string | undefined;
  categories: readonly Category[];
  onClose: () => void;
}) {
  const can = useCan();
  if (event && (!event.editable || event.source || !can("calendar.update"))) {
    return <EventDetails event={event} onClose={onClose} />;
  }
  return <EventForm event={event} day={day} categories={categories} onClose={onClose} />;
}

function EventDetails({ event, onClose }: { event: CalendarEvent; onClose: () => void }) {
  const t = useText();
  const f = useFormat();
  const when = event.all_day
    ? [event.start_date, event.end_date]
        .filter(Boolean)
        .map((d) => f.day(d ?? ""))
        .join(" – ")
    : [event.starts_at, event.ends_at]
        .filter(Boolean)
        .map((d) => f.dateTime(d ?? ""))
        .join(" – ");
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={event.title}
      footer={
        event.source ? (
          event.source.kind === "contract" ? (
            <Link
              to="/contracts/$id"
              params={{ id: event.source.id }}
              className={buttonClass({ variant: "primary" })}
            >
              {t("calendar.openContract")}
            </Link>
          ) : (
            <Link
              to="/assets/$id"
              params={{ id: event.source.id }}
              className={buttonClass({ variant: "primary" })}
            >
              {t("calendar.openAsset")}
            </Link>
          )
        ) : undefined
      }
    >
      <div className="grid gap-4">
        {event.source ? (
          <Notice tone="info" live={false}>
            {t("calendar.generatedHint")}
          </Notice>
        ) : null}
        <Facts
          items={[
            { key: "when", term: t("common.when"), value: when },
            {
              key: "category",
              term: t("calendar.category"),
              value: <ColourLabel colour={event.colour}>{event.category_name}</ColourLabel>,
            },
            event.location
              ? { key: "location", term: t("calendar.location"), value: event.location }
              : null,
            event.description
              ? {
                  key: "description",
                  term: t("calendar.description"),
                  value: <span className="whitespace-pre-wrap">{event.description}</span>,
                }
              : null,
          ]}
        />
      </div>
    </Dialog>
  );
}

function EventForm({
  event,
  day,
  categories,
  onClose,
}: {
  event?: CalendarEvent | undefined;
  day?: string | undefined;
  categories: readonly Category[];
  onClose: () => void;
}) {
  const t = useText();
  const can = useCan();
  const companies = useQuery(companiesQuery);
  const start = day ?? localDay();
  const [form, setForm] = useState({
    title: event?.title ?? "",
    category_id: event?.category_id ?? "",
    all_day: event?.all_day ?? true,
    start_date: event?.start_date ?? start,
    end_date: event?.end_date ?? event?.start_date ?? start,
    starts_at: toLocalInput(event?.starts_at) || `${start}T09:00`,
    ends_at: toLocalInput(event?.ends_at) || `${start}T10:00`,
    location: event?.location ?? "",
    description: event?.description ?? "",
    company_id: event?.company_id ?? "",
  });
  const [tried, setTried] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const endBeforeStart = form.all_day
    ? form.end_date < form.start_date
    : form.ends_at < form.starts_at;
  const titleError = tried && !form.title.trim() ? t("calendar.titleRequired") : undefined;
  const categoryError = tried && !form.category_id ? t("calendar.categoryRequired") : undefined;
  const body = (): Schemas["CalendarEventRequest"] => ({
    title: form.title.trim(),
    category_id: form.category_id,
    all_day: form.all_day,
    ...(form.all_day
      ? {
          start_date: form.start_date,
          end_date: form.end_date || form.start_date,
          starts_at: null,
          ends_at: null,
        }
      : {
          starts_at: fromLocalInput(form.starts_at),
          ends_at: fromLocalInput(form.ends_at),
          start_date: null,
          end_date: null,
        }),
    location: orNull(form.location),
    description: form.description,
    ...(event ? {} : { company_id: form.company_id || null }),
  });
  const save = useAction(
    () =>
      event
        ? call(
            api.PUT("/api/v1/calendar/events/{id}", {
              params: { path: { id: event.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/calendar/events", { body: body() })),
    {
      invalidate: [["calendar"]],
      success: event ? "calendar.saved" : "calendar.created",
      onSuccess: onClose,
    },
  );
  const remove = useAction(
    () =>
      call(
        api.DELETE("/api/v1/calendar/events/{id}", { params: { path: { id: event?.id ?? "" } } }),
      ),
    {
      invalidate: [["calendar"]],
      success: "calendar.deleted",
      onSuccess: onClose,
    },
  );
  const usable = categories.filter(
    (c) => !c.system_key && (c.is_active || c.id === event?.category_id),
  );
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);
  return (
    <>
      <FormDialog
        open={!confirm}
        onOpenChange={(o) => !o && onClose()}
        title={event ? t("calendar.edit") : t("calendar.new")}
        submitLabel={event ? t("common.saveChanges") : t("common.create")}
        busy={save.isPending}
        onSubmit={() => {
          setTried(true);
          if (form.title.trim() && form.category_id && !endBeforeStart) save.mutate();
        }}
      >
        <ProblemNotice error={save.error} />
        <TextField
          label={t("calendar.titleField")}
          required
          value={form.title}
          error={titleError ?? fieldError(save.error, "title")}
          onChange={set("title")}
        />
        <FormGrid>
          <SelectField
            label={t("calendar.category")}
            required
            value={form.category_id}
            placeholder={t("common.choose")}
            error={categoryError}
            onChange={set("category_id")}
            options={usable.map((c) => ({ value: c.id, label: c.name }))}
          />
          {!event && activeCompanies.length > 1 ? (
            <SelectField
              label={t("common.company")}
              value={form.company_id}
              placeholder={t("common.choose")}
              onChange={set("company_id")}
              options={activeCompanies.map((c) => ({ value: c.id, label: c.name }))}
            />
          ) : null}
        </FormGrid>
        <Checkbox
          label={t("calendar.allDay")}
          checked={form.all_day}
          onChange={(e) => setForm((x) => ({ ...x, all_day: e.target.checked }))}
        />
        <FormGrid>
          {form.all_day ? (
            <>
              <TextField
                label={t("calendar.startDate")}
                type="date"
                required
                value={form.start_date}
                onChange={set("start_date")}
              />
              <TextField
                label={t("calendar.endDate")}
                type="date"
                min={form.start_date}
                value={form.end_date}
                error={endBeforeStart ? t("calendar.endBeforeStart") : undefined}
                onChange={set("end_date")}
              />
            </>
          ) : (
            <>
              <TextField
                label={t("calendar.starts")}
                type="datetime-local"
                required
                value={form.starts_at}
                onChange={set("starts_at")}
              />
              <TextField
                label={t("calendar.ends")}
                type="datetime-local"
                min={form.starts_at}
                value={form.ends_at}
                error={endBeforeStart ? t("calendar.endBeforeStart") : undefined}
                onChange={set("ends_at")}
              />
            </>
          )}
        </FormGrid>
        <TextField
          label={t("calendar.location")}
          value={form.location}
          onChange={set("location")}
        />
        <TextAreaField
          label={t("calendar.description")}
          rows={3}
          value={form.description}
          onChange={set("description")}
        />
        {event && can("calendar.update") ? (
          <div className="border-t border-line pt-3">
            <Button
              variant="ghost"
              className="text-danger-text"
              icon={<Trash2 aria-hidden="true" className="size-4" />}
              onClick={() => setConfirm(true)}
            >
              {t("calendar.delete")}
            </Button>
          </div>
        ) : null}
      </FormDialog>
      {event ? (
        <ConfirmDialog
          open={confirm}
          onOpenChange={(o) => setConfirm(o)}
          title={t("calendar.deleteTitle", { title: event.title })}
          confirmLabel={t("calendar.delete")}
          busy={remove.isPending}
          onConfirm={() => remove.mutate()}
        >
          <p className="text-sm">{t("calendar.deleteBody")}</p>
          <ProblemNotice error={remove.error} className="mt-3" />
        </ConfirmDialog>
      ) : null}
    </>
  );
}
