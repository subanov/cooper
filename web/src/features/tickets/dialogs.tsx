import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { FormDialog } from "@/components/Dialog";
import { Field, Select, SelectField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import {
  analystsQuery,
  categoriesQuery,
  categoryOptions,
  teamsQuery,
  ticketLookupQuery,
  toOptions,
} from "@/features/reference/queries";
import { useText } from "@/i18n/useText";

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
  submitLabel,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  initial: Assignment;
  onSubmit: (assignment: Assignment) => void;
  busy: boolean;
  error: unknown;
  submitLabel?: string | undefined;
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
      submitLabel={submitLabel ?? t("ticket.assign")}
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
  onSubmit,
  busy,
  error,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  currentStatusId?: string | undefined;
  onSubmit: (change: StatusChange) => void;
  busy: boolean;
  error: unknown;
}) {
  const t = useText();
  const statuses = useQuery(ticketLookupQuery("ticket-statuses"));
  const codes = useQuery(ticketLookupQuery("resolution-codes"));
  const categories = useQuery(categoriesQuery);
  const [statusId, setStatusId] = useState(currentStatusId ?? "");
  const [codeId, setCodeId] = useState("");
  const [closureId, setClosureId] = useState("");
  useEffect(() => {
    if (open) {
      setStatusId(currentStatusId ?? "");
      setCodeId("");
      setClosureId("");
    }
  }, [open, currentStatusId]);
  const target = statuses.data?.find((s) => s.id === statusId);
  const closing = Boolean(target?.is_closed);
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
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
