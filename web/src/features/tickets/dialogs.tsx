import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { FormDialog } from "@/components/Dialog";
import { ProblemNotice } from "@/components/Feedback";
import { Field, Select, SelectField, TextField } from "@/components/Field";
import {
  analystsQuery,
  categoriesQuery,
  categoryOptions,
  teamsQuery,
  ticketLookupQuery,
  toOptions,
} from "@/features/reference/queries";
import { useText } from "@/i18n/useText";
import { fromLocalInput, localDay, toLocalInput } from "@/lib/dates";

export interface Assignment {
  team_id: string | null;
  analyst_id: string | null;
}

/** Chooses a Queue and an Assignee. Members of the chosen Queue are listed first. */
export function AssignDialog({
  open,
  onOpenChange,
  title,
  initial,
  onSubmit,
  busy,
  error,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  initial: Assignment;
  onSubmit: (assignment: Assignment) => void;
  busy: boolean;
  error: unknown;
}) {
  const t = useText();
  const teams = useQuery(teamsQuery);
  const analysts = useQuery(analystsQuery);
  const [teamId, setTeamId] = useState(initial.team_id ?? "");
  const [analystId, setAnalystId] = useState(initial.analyst_id ?? "");
  useEffect(() => {
    if (open) {
      setTeamId(initial.team_id ?? "");
      setAnalystId(initial.analyst_id ?? "");
    }
  }, [open, initial.team_id, initial.analyst_id]);
  const team = teams.data?.find((q) => q.id === teamId);
  const members = new Set(team?.analyst_ids ?? []);
  const active = (analysts.data ?? []).filter((a) => a.is_active || a.id === analystId);
  const inTeam = active.filter((a) => members.has(a.id));
  const others = active.filter((a) => !members.has(a.id));
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      size="sm"
      submitLabel={t("ticket.assign")}
      busy={busy}
      onSubmit={() => onSubmit({ team_id: teamId || null, analyst_id: analystId || null })}
    >
      <ProblemNotice error={error} />
      <SelectField
        label={t("ticket.queue")}
        value={teamId}
        onChange={(e) => setTeamId(e.target.value)}
        placeholder={t("ticket.noQueue")}
        options={toOptions(teams.data, (q) => q.name, initial.team_id)}
      />
      <Field label={t("ticket.assignee")}>
        {(control) => (
          <Select {...control} value={analystId} onChange={(e) => setAnalystId(e.target.value)}>
            <option value="">{t("tickets.unassigned")}</option>
            {team && inTeam.length > 0 ? (
              <optgroup label={team.name}>
                {inTeam.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.full_name}
                  </option>
                ))}
              </optgroup>
            ) : null}
            <optgroup
              label={team && inTeam.length > 0 ? t("ticket.otherAnalysts") : t("ticket.analysts")}
            >
              {others.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.full_name}
                </option>
              ))}
            </optgroup>
          </Select>
        )}
      </Field>
    </FormDialog>
  );
}

export interface StatusChange {
  status_id: string;
  resolution_code_id: string | null;
  closure_category_id: string | null;
}

/**
 * Moves tickets to another status. A closed status asks for a Resolution Code; the API
 * decides what is required and refuses what is not allowed.
 */
export function StatusDialog({
  open,
  onOpenChange,
  title,
  currentStatusId,
  initialStatusId,
  onSubmit,
  busy,
  error,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  currentStatusId?: string | undefined;
  /** Preselects a target status (chosen in a list or the Properties panel). */
  initialStatusId?: string | undefined;
  onSubmit: (change: StatusChange) => void;
  busy: boolean;
  error: unknown;
}) {
  const t = useText();
  const statuses = useQuery(ticketLookupQuery("ticket-statuses"));
  const codes = useQuery(ticketLookupQuery("resolution-codes"));
  const categories = useQuery(categoriesQuery);
  const [statusId, setStatusId] = useState(initialStatusId ?? currentStatusId ?? "");
  const [codeId, setCodeId] = useState("");
  const [closureId, setClosureId] = useState("");
  useEffect(() => {
    if (open) {
      setStatusId(initialStatusId ?? currentStatusId ?? "");
      setCodeId("");
      setClosureId("");
    }
  }, [open, currentStatusId, initialStatusId]);
  const target = statuses.data?.find((s) => s.id === statusId);
  const closing = Boolean(target?.is_closed);
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      size="sm"
      submitLabel={t("ticket.changeStatus")}
      busy={busy}
      submitDisabled={!statusId || statusId === currentStatusId || (closing && !codeId)}
      onSubmit={() =>
        onSubmit({
          status_id: statusId,
          resolution_code_id: closing ? codeId || null : null,
          closure_category_id: closing ? closureId || null : null,
        })
      }
    >
      <ProblemNotice error={error} />
      <SelectField
        label={t("ticket.status")}
        required
        value={statusId}
        onChange={(e) => setStatusId(e.target.value)}
        placeholder={currentStatusId ? undefined : t("common.choose")}
        options={toOptions(statuses.data, (s) => s.name, currentStatusId)}
      />
      {closing ? (
        <>
          <SelectField
            label={t("ticket.resolutionCode")}
            hint={t("ticket.resolutionHint")}
            required
            value={codeId}
            onChange={(e) => setCodeId(e.target.value)}
            placeholder={t("common.choose")}
            options={toOptions(codes.data, (c) => c.name)}
          />
          <SelectField
            label={t("ticket.closureCategory")}
            hint={t("ticket.closureCategoryHint")}
            value={closureId}
            onChange={(e) => setClosureId(e.target.value)}
            placeholder={t("common.none")}
            options={categoryOptions(categories.data)}
          />
        </>
      ) : null}
    </FormDialog>
  );
}

/** Chooses a Priority. */
export function PriorityDialog({
  open,
  onOpenChange,
  title,
  onSubmit,
  busy,
  error,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  onSubmit: (priorityId: string) => void;
  busy: boolean;
  error: unknown;
}) {
  const t = useText();
  const priorities = useQuery(ticketLookupQuery("ticket-priorities"));
  const [priorityId, setPriorityId] = useState("");
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      size="sm"
      submitLabel={t("common.save")}
      busy={busy}
      submitDisabled={!priorityId}
      onSubmit={() => onSubmit(priorityId)}
    >
      <ProblemNotice error={error} />
      <SelectField
        label={t("ticket.priority")}
        required
        value={priorityId}
        onChange={(e) => setPriorityId(e.target.value)}
        placeholder={t("common.choose")}
        options={toOptions(priorities.data, (p) => p.name)}
      />
    </FormDialog>
  );
}

/** Tomorrow at 09:00, the first snooze time offered. */
function tomorrow(): string {
  const d = new Date();
  d.setDate(d.getDate() + 1);
  d.setHours(9, 0, 0, 0);
  return toLocalInput(d);
}

/** Snoozes a ticket until a time, with a reason. Tomorrow 09:00 is offered first. */
export function SnoozeDialog({
  open,
  onOpenChange,
  title,
  onSubmit,
  busy,
  error,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  onSubmit: (body: { until: string; reason: string }) => void;
  busy: boolean;
  error: unknown;
}) {
  const t = useText();
  const [until, setUntil] = useState(tomorrow);
  const [reason, setReason] = useState("");
  useEffect(() => {
    if (open) {
      setUntil(tomorrow());
      setReason("");
    }
  }, [open]);
  const instant = fromLocalInput(until);
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      description={t("ticket.snoozeIntro")}
      size="sm"
      submitLabel={t("ticket.snooze")}
      busy={busy}
      submitDisabled={!instant}
      onSubmit={() => instant && onSubmit({ until: instant, reason })}
    >
      <ProblemNotice error={error} />
      <TextField
        label={t("ticket.snoozeUntil")}
        type="datetime-local"
        required
        value={until}
        min={toLocalInput(new Date())}
        onChange={(e) => setUntil(e.target.value)}
      />
      <TextField
        label={t("ticket.snoozeReason")}
        value={reason}
        onChange={(e) => setReason(e.target.value)}
      />
    </FormDialog>
  );
}

/** Logs whole minutes of work, optionally on an earlier day. */
export function TimeDialog({
  open,
  onOpenChange,
  title,
  onSubmit,
  busy,
  error,
  minutesError,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  onSubmit: (body: { minutes: number; note: string; work_date: string | null }) => void;
  busy: boolean;
  error: unknown;
  minutesError?: string | undefined;
}) {
  const t = useText();
  const [minutes, setMinutes] = useState("");
  const [date, setDate] = useState("");
  const [note, setNote] = useState("");
  useEffect(() => {
    if (open) {
      setMinutes("");
      setDate("");
      setNote("");
    }
  }, [open]);
  const value = Number(minutes.trim());
  const valid = Number.isInteger(value) && value > 0;
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      size="sm"
      submitLabel={t("ticket.logTime")}
      busy={busy}
      submitDisabled={!valid}
      onSubmit={() => onSubmit({ minutes: value, note, work_date: date || null })}
    >
      <ProblemNotice error={minutesError ? undefined : error} />
      <TextField
        label={t("ticket.minutes")}
        hint={t("ticket.minutesHint")}
        type="number"
        inputMode="numeric"
        min={1}
        step={1}
        required
        autoFocus
        value={minutes}
        error={minutesError}
        onChange={(e) => setMinutes(e.target.value)}
      />
      <TextField
        label={t("ticket.workDate")}
        hint={t("ticket.workDateHint")}
        type="date"
        max={localDay()}
        value={date}
        onChange={(e) => setDate(e.target.value)}
      />
      <TextField
        label={t("ticket.timeNote")}
        value={note}
        onChange={(e) => setNote(e.target.value)}
      />
    </FormDialog>
  );
}
