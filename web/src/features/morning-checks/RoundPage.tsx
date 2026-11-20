import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { ClipboardCheck, X } from "lucide-react";
import { useState } from "react";
import { Badge, ColourDot } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { Card } from "@/components/Card";
import { EmptyState } from "@/components/EmptyState";
import { Checkbox, Textarea, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { FilterBar, SectionHeader } from "@/components/Page";
import { QueryState } from "@/components/QueryState";
import { useCan } from "@/features/session/session";
import { useActorName } from "@/features/shared/History";
import { useAction } from "@/features/shared/mutation";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { cn } from "@/lib/cn";
import { call } from "@/lib/request";
import { checkStatusesQuery, roundQuery } from "./api";

type Item = Schemas["CheckDayItemDto"];

/**
 * The Round of a Check Date (FR-MCK-002): every Check grouped as configured, with a
 * progress bar so the end of the round is in sight (Goal-Gradient Effect).
 */
export function RoundPage({ date, mine }: { date?: string | undefined; mine: boolean }) {
  const t = useText();
  const f = useFormat();
  const navigate = useNavigate();
  const round = useQuery(roundQuery(date, mine));
  return (
    <div className="grid gap-4">
      <SectionHeader
        title={t("checks.round")}
        description={round.data ? f.day(round.data.check_date) : undefined}
      />
      <FilterBar label={t("common.filters")}>
        <TextField
          className="w-48"
          label={t("checks.date")}
          type="date"
          max={round.data?.today}
          value={date ?? round.data?.check_date ?? ""}
          onChange={(e) =>
            void navigate({
              to: "/morning-checks/today",
              search: {
                ...(e.target.value ? { date: e.target.value } : {}),
                ...(mine ? { mine: true } : {}),
              },
            })
          }
        />
        <Checkbox
          className="h-9 items-center"
          label={t("checks.mine")}
          checked={mine}
          onChange={(e) =>
            void navigate({
              to: "/morning-checks/today",
              search: { ...(date ? { date } : {}), ...(e.target.checked ? { mine: true } : {}) },
            })
          }
        />
      </FilterBar>
      <QueryState query={round}>
        {(day) => {
          if (day.items.length === 0) {
            return (
              <EmptyState icon={<ClipboardCheck aria-hidden="true" />} title={t("checks.noChecks")}>
                {t("checks.noChecksHint")}
              </EmptyState>
            );
          }
          const recorded = day.items.filter((i) => i.result).length;
          const groups = new Map<string, Item[]>();
          for (const item of day.items) {
            const key = item.check.check_group_name ?? "";
            groups.set(key, [...(groups.get(key) ?? []), item]);
          }
          return (
            <>
              <div className="grid gap-1.5">
                <div className="flex justify-between text-sm">
                  <span id="round-progress" className="font-medium">
                    {t("checks.progressLabel")}
                  </span>
                  <span>{t("checks.progress", { recorded, total: day.items.length })}</span>
                </div>
                <progress
                  aria-labelledby="round-progress"
                  max={day.items.length}
                  value={recorded}
                  className="h-2 w-full overflow-hidden rounded-full [&::-moz-progress-bar]:bg-accent [&::-webkit-progress-bar]:bg-sunken [&::-webkit-progress-value]:bg-accent"
                />
              </div>
              {[...groups].map(([group, items]) => (
                <Card key={group || "none"} title={group || t("checks.noGroup")} flush>
                  <ul className="divide-y divide-line">
                    {items.map((item) => (
                      <li key={item.check.id}>
                        <CheckRow item={item} date={day.check_date} />
                      </li>
                    ))}
                  </ul>
                </Card>
              ))}
            </>
          );
        }}
      </QueryState>
    </div>
  );
}

function CheckRow({ item, date }: { item: Item; date: string }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const actorName = useActorName();
  const statuses = useQuery(checkStatusesQuery);
  const result = item.result ?? undefined;
  const [statusId, setStatusId] = useState(result?.status_id ?? "");
  const [notes, setNotes] = useState(result?.notes ?? "");
  const [tried, setTried] = useState(false);
  const status = statuses.data?.find((s) => s.id === statusId);
  const needsNotes = Boolean(status?.requires_notes) && !notes.trim();
  const canRecord = can("morning-checks.record");
  const record = useAction(
    () =>
      call(
        api.POST("/api/v1/morning-checks/results", {
          body: { check_id: item.check.id, check_date: date, status_id: statusId, notes },
        }),
      ),
    {
      invalidate: [["morning-checks", "round"], ["home"]],
      success: () => t("checks.recorded", { check: item.check.name, status: status?.name ?? "" }),
    },
  );
  const clear = useAction(
    () =>
      call(
        api.DELETE("/api/v1/morning-checks/results/{id}", {
          params: { path: { id: result?.id ?? "" } },
        }),
      ),
    {
      invalidate: [["morning-checks", "round"], ["home"]],
      success: () => t("checks.cleared", { check: item.check.name }),
      onSuccess: () => {
        setStatusId("");
        setNotes("");
      },
    },
  );
  const dirty = statusId !== (result?.status_id ?? "") || notes !== (result?.notes ?? "");
  const current = statuses.data?.find((s) => s.id === result?.status_id);
  return (
    <form
      className="grid gap-3 px-4 py-3 md:grid-cols-[minmax(12rem,1fr)_auto] md:items-start"
      onSubmit={(e) => {
        e.preventDefault();
        setTried(true);
        if (statusId && !needsNotes) record.mutate();
      }}
    >
      <div className="grid gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-medium">{item.check.name}</span>
          {current ? (
            <Badge tone="neutral" icon={<ColourDot colour={current.colour} />}>
              {current.name}
            </Badge>
          ) : (
            <Badge tone="muted">{t("checks.notRecorded")}</Badge>
          )}
        </div>
        {item.check.description ? (
          <p className="text-xs text-fg-muted">{item.check.description}</p>
        ) : null}
        {result ? (
          <p className="text-xs text-fg-muted">
            {t("checks.recordedBy", {
              name: actorName(result.modified_by),
              time: f.dateTime(result.updated_at),
            })}
          </p>
        ) : null}
      </div>
      {canRecord ? (
        <div className="grid gap-2 md:w-[26rem]">
          <fieldset className="flex flex-wrap gap-1.5">
            <legend className="sr-only">{t("checks.statusFor", { check: item.check.name })}</legend>
            {(statuses.data ?? [])
              .filter((s) => s.is_active)
              .map((s) => (
                <label
                  key={s.id}
                  className={cn(
                    "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md border px-2.5 text-[0.8125rem] font-medium has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-ring",
                    statusId === s.id ? "border-fg bg-sunken" : "border-line hover:bg-canvas",
                  )}
                >
                  <input
                    type="radio"
                    name={`status-${item.check.id}`}
                    value={s.id}
                    checked={statusId === s.id}
                    onChange={() => setStatusId(s.id)}
                    className="sr-only"
                  />
                  <ColourDot colour={s.colour} />
                  {s.name}
                </label>
              ))}
          </fieldset>
          {status?.requires_notes || notes ? (
            <div className="grid gap-1">
              <label htmlFor={`notes-${item.check.id}`} className="sr-only">
                {t("checks.notesFor", { check: item.check.name })}
              </label>
              <Textarea
                id={`notes-${item.check.id}`}
                rows={2}
                placeholder={t("checks.notes")}
                value={notes}
                aria-invalid={tried && needsNotes ? true : undefined}
                aria-describedby={tried && needsNotes ? `notes-err-${item.check.id}` : undefined}
                onChange={(e) => setNotes(e.target.value)}
                className="min-h-16"
              />
              {tried && needsNotes ? (
                <p
                  id={`notes-err-${item.check.id}`}
                  className="text-xs font-medium text-danger-text"
                >
                  {t("checks.notesRequired")}
                </p>
              ) : null}
            </div>
          ) : null}
          <ProblemNotice error={record.error ?? clear.error} />
          <div className="flex gap-2">
            <Button
              type="submit"
              size="sm"
              variant={dirty ? "primary" : "secondary"}
              disabled={!statusId || !dirty}
              loading={record.isPending}
            >
              {result ? t("checks.update") : t("checks.record")}
            </Button>
            {result ? (
              <IconButton
                size="sm"
                tooltip="top"
                label={t("checks.clearNamed", { check: item.check.name })}
                icon={<X aria-hidden="true" className="size-4" />}
                loading={clear.isPending}
                onClick={() => clear.mutate()}
              />
            ) : null}
          </div>
        </div>
      ) : null}
    </form>
  );
}
