import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import { checkDayQuery, checkStatusesQuery, recordResult } from "@/features/morning-checks/api";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";

type Item = Schemas["CheckDayItemDto"];
type Status = Schemas["CheckStatusDto"];

/** The Checks of one day, with a form per Check to record or change its result. */
export function TodayPage() {
  const t = useText();
  const session = useQuery(analystSessionQuery).data;
  const canRecord = session ? can(session, "morning-checks.record") : false;
  const [date, setDate] = useState("");
  const day = useQuery(checkDayQuery(date || undefined));
  const statuses = useQuery(checkStatusesQuery);
  return (
    <div className="grid gap-3">
      <Field
        label={t("checks.date")}
        type="date"
        className="max-w-48"
        value={date || (day.data?.today ?? "")}
        max={day.data?.today}
        onChange={(e) => setDate(e.target.value)}
      />
      {day.isPending ? <p>{t("common.loading")}</p> : null}
      {day.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {day.isSuccess && day.data.items.length === 0 ? (
        <p className="text-muted-foreground">{t("checks.noChecks")}</p>
      ) : null}
      <ul className="grid gap-2">
        {(day.data?.items ?? []).map((item) => (
          <CheckRow
            key={item.check.id}
            item={item}
            date={day.data?.check_date ?? ""}
            statuses={(statuses.data ?? []).filter((s) => s.is_active)}
            canRecord={canRecord}
          />
        ))}
      </ul>
    </div>
  );
}

function CheckRow({
  item,
  date,
  statuses,
  canRecord,
}: {
  item: Item;
  date: string;
  statuses: Status[];
  canRecord: boolean;
}) {
  const t = useText();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const result = item.result as Schemas["CheckResultDto"] | null;
  const [statusId, setStatusId] = useState(result?.status_id ?? "");
  const [notes, setNotes] = useState(result?.notes ?? "");
  const current = statuses.find((s) => s.id === result?.status_id);

  async function save() {
    if (!statusId) {
      flash.fail(t("checks.pickStatus"));
      return;
    }
    const outcome = await recordResult({
      check_id: item.check.id,
      status_id: statusId,
      notes,
      check_date: date,
    });
    if (!outcome.ok) {
      flash.fail(t("tasks.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    flash.ok(t("checks.recorded"));
    await queryClient.invalidateQueries({ queryKey: ["morning-checks", "day"] });
  }

  return (
    <li className="grid gap-2 rounded-lg border border-border p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-medium">{item.check.name}</span>
        {item.check.check_group_name ? (
          <Badge tone="muted">{item.check.check_group_name}</Badge>
        ) : null}
        {current ? (
          <Badge tone="neutral">{current.name}</Badge>
        ) : (
          <Badge tone="warning">{t("checks.notRecorded")}</Badge>
        )}
      </div>
      {item.check.description ? (
        <p className="text-sm text-muted-foreground">{item.check.description}</p>
      ) : null}
      {canRecord ? (
        <form
          className="flex flex-wrap items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <SelectField
            label={`${t("tasks.status")}: ${item.check.name}`}
            value={statusId}
            onChange={(e) => setStatusId(e.target.value)}
          >
            <option value="">{t("checks.pickStatus")}</option>
            {statuses.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </SelectField>
          <Field
            label={`${t("checks.notes")}: ${item.check.name}`}
            className="min-w-64 flex-1"
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
          />
          <Button type="submit" size="sm">
            {t("checks.record")}
          </Button>
        </form>
      ) : null}
      {flash.element}
    </li>
  );
}
