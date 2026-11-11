import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import {
  calendarCategoriesQuery,
  createCalendarCategory,
  updateCalendarCategory,
} from "@/features/calendar/api";
import { problemText } from "@/features/result";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { CheckField, FormActions, ResourceAdmin, useFlash, useInvalidateAdmin } from "./shared";

type Category = Schemas["CalendarCategoryDto"];

/** Calendar Categories (FR-CAL-001): name, colour and whether new events may use them. */
export function CalendarCategoriesPage() {
  const t = useText();
  const rows = useQuery(calendarCategoriesQuery);
  return (
    <ResourceAdmin
      title={t("admin.calendarCategories")}
      intro={t("admin.calendarCategories.intro")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Category) => r.id}
      getName={(r) => r.name}
      addLabel={t("admin.calendarCategories.add")}
      columns={[
        {
          header: t("admin.res.name"),
          cell: (r) => (
            <span className="flex items-center gap-2 font-medium">
              <span
                aria-hidden="true"
                className="inline-block size-3 rounded-sm border border-border"
                style={{ backgroundColor: r.colour }}
              />
              {r.name}
              {r.system_key ? (
                <Badge tone="muted">{t("admin.calendarCategories.system")}</Badge>
              ) : null}
            </span>
          ),
        },
        {
          header: t("admin.calendarCategories.colour"),
          cell: (r) => <code>{r.colour}</code>,
        },
        {
          header: t("admin.res.status"),
          cell: (r) => (
            <Badge tone={r.is_active ? "ok" : "muted"}>
              {t(r.is_active ? "admin.res.active" : "admin.res.inactive")}
            </Badge>
          ),
        },
      ]}
      renderForm={(row, done) => <CategoryForm row={row} done={done} />}
    />
  );
}

function CategoryForm({ row, done }: { row: Category | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const [name, setName] = useState(row?.name ?? "");
  const [colour, setColour] = useState(row?.colour ?? "#2563eb");
  const [active, setActive] = useState(row?.is_active ?? true);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["CalendarCategoryRequest"] = {
      name: name.trim(),
      colour,
      is_active: active,
    };
    const outcome = row
      ? await updateCalendarCategory(row.id, body)
      : await createCalendarCategory(body);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await invalidate();
    done();
  }

  return (
    <form onSubmit={submit} noValidate className="grid max-w-xl gap-3">
      <Field
        label={t("admin.res.name")}
        value={name}
        onChange={(e) => setName(e.target.value)}
        required
      />
      <Field
        label={t("admin.calendarCategories.colour")}
        type="color"
        className="w-32"
        value={colour}
        onChange={(e) => setColour(e.target.value)}
      />
      {row?.system_key ? null : (
        <CheckField label={t("admin.res.active")} checked={active} onChange={setActive} />
      )}
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
    </form>
  );
}
