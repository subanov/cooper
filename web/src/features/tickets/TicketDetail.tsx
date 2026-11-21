import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import {
  AlarmClockOff,
  BellOff,
  CircleDot,
  Clock,
  Copy,
  Eye,
  MoreHorizontal,
  PencilLine,
  RotateCcw,
  Trash2,
  UserCheck,
  UserPlus,
} from "lucide-react";
import { useRef, useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import type { PickerOption } from "@/components/Combobox";
import { ConfirmDialog, FormDialog } from "@/components/Dialog";
import { LoadingBlock, Notice, ProblemNotice } from "@/components/Feedback";
import { Checkbox, FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { BackLink, Panel } from "@/components/Layout";
import { DetailHeader } from "@/components/MasterDetail";
import { Menu, MenuItem, MenuSeparator } from "@/components/Menu";
import { Property, PropertyList, PropertySelect } from "@/components/PropertyList";
import { useToast } from "@/components/Toast";
import { RequesterPicker } from "@/features/reference/pickers";
import {
  analystsQuery,
  categoriesQuery,
  categoryOptions,
  companiesQuery,
  departmentsQuery,
  namesById,
  requesterOption,
  requestersByIdQuery,
  teamsQuery,
  ticketLookupQuery,
  toOptions,
} from "@/features/reference/queries";
import { useCan, useSession } from "@/features/session/session";
import { fieldError, useAction } from "@/features/shared/mutation";
import { useClaimFocus } from "@/features/shell/focus";
import { useFormat, useText } from "@/i18n/useText";
import { useHotkeys } from "@/lib/hooks";
import { useTicketActions } from "./actions";
import { type Ticket, ticketActions, ticketQuery, ticketRequest } from "./api";
import { Composer, type ComposerHandle } from "./Composer";
import { AssignDialog, SnoozeDialog, StatusDialog, TimeDialog } from "./dialogs";
import { useTicketPresence } from "./presence";
import { currentTarget, SlaBadge } from "./Sla";
import { AttachmentsPanel, RelatedPanel, TicketTasksPanel } from "./TicketPanels";
import { Timeline } from "./Timeline";

type DialogName = "assign" | "status" | "snooze" | "time" | "edit" | "delete" | null;

/**
 * One ticket in the detail pane: number, subject, status, SLA and Company at a glance, the
 * triage toolbar, the conversation, and the Properties panel where each value changes in
 * place. `a` assigns to me, `r` replies, `n` adds a Note.
 */
export function TicketDetail({ ticketId }: { ticketId: string }) {
  const t = useText();
  const query = useQuery(ticketQuery(ticketId));
  if (!query.data) {
    return (
      <article className="mx-auto grid max-w-6xl gap-4 p-4 sm:p-6">
        <DetailHeader
          back={<BackToList />}
          title={query.isError ? t("ticket.unavailable") : t("ticket.loading")}
        />
        {query.isError ? (
          <ProblemNotice error={query.error} onRetry={() => void query.refetch()} />
        ) : (
          <LoadingBlock lines={6} />
        )}
      </article>
    );
  }
  return <TicketView ticket={query.data} />;
}

function BackToList() {
  const t = useText();
  return (
    <BackLink to="/tickets" search={true}>
      {t("tickets.backToList")}
    </BackLink>
  );
}

function TicketView({ ticket }: { ticket: Ticket }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const session = useSession();
  const toast = useToast();
  const navigate = useNavigate();
  const statuses = useQuery(ticketLookupQuery("ticket-statuses"));
  const priorities = useQuery(ticketLookupQuery("ticket-priorities"));
  const companies = useQuery(companiesQuery);
  const analysts = useQuery(analystsQuery);
  const requesters = useQuery(requestersByIdQuery([ticket.requester_id]));
  const requester = ticket.requester_id ? requesters.data?.get(ticket.requester_id) : undefined;
  const [dialog, setDialog] = useState<DialogName>(null);
  const [statusTarget, setStatusTarget] = useState<string | undefined>(undefined);
  const [composing, setComposing] = useState(false);
  const composer = useRef<ComposerHandle>(null);
  const article = useRef<HTMLElement>(null);
  useClaimFocus(article);
  const viewers = useTicketPresence(ticket.id, composing);
  const close = () => setDialog(null);
  const actions = useTicketActions(ticket, close);
  const me = actions.me;
  const deleted = Boolean(ticket.deleted_at);
  const canUpdate = can("tickets.update") && !deleted;
  const canAssign = (can("tickets.assign") || can("tickets.update")) && !deleted;
  const statusName = namesById(statuses.data).get(ticket.status_id) ?? "…";
  const analystNames = namesById(analysts.data, (a) => a.full_name);

  const remove = useAction(() => ticketActions.remove(ticket.id), {
    invalidate: [["tickets"], ["home"]],
    onSuccess: async () => {
      close();
      toast({
        tone: "success",
        message: t("ticket.deletedDone", { number: ticket.number }),
        ...(session.is_admin
          ? {
              action: {
                label: t("common.undo"),
                onClick: () =>
                  void ticketActions.restore(ticket.id).then(() =>
                    navigate({
                      to: "/tickets/$ticketId",
                      params: { ticketId: ticket.id },
                      search: true,
                    }),
                  ),
              },
            }
          : {}),
      });
      await navigate({ to: "/tickets", search: true });
    },
  });

  function assignToMe() {
    if (!canAssign || ticket.analyst_id === me) return;
    actions.assignToMe();
  }

  useHotkeys({
    a: assignToMe,
    r: () => composer.current?.focus("reply"),
    n: () => composer.current?.focus("note"),
  });

  const { target } = currentTarget(ticket.sla);
  const others = viewers.filter((v) => v.analyst_id !== me);

  return (
    <article
      ref={article}
      tabIndex={-1}
      aria-label={`${ticket.number} ${ticket.subject}`}
      className="mx-auto grid max-w-6xl gap-5 p-4 outline-none sm:p-6"
    >
      <DetailHeader
        back={<BackToList />}
        eyebrow={
          <>
            <span className="font-mono font-medium text-fg">{ticket.number}</span>
            <IconButton
              size="sm"
              label={t("ticket.copyNumber")}
              icon={<Copy aria-hidden="true" />}
              tooltip="top"
              className="size-7"
              onClick={() => {
                void navigator.clipboard
                  ?.writeText(ticket.number)
                  .then(() => toast({ tone: "info", message: t("common.copied") }));
              }}
            />
            <span aria-hidden="true">·</span>
            <span>{namesById(companies.data).get(ticket.company_id) ?? ""}</span>
            <span aria-hidden="true">·</span>
            <span>
              {ticket.analyst_id
                ? (analystNames.get(ticket.analyst_id) ?? "…")
                : t("tickets.unassigned")}
            </span>
          </>
        }
        title={ticket.subject}
        badges={
          <>
            <Badge tone="accent" icon={<CircleDot aria-hidden="true" />}>
              {statusName}
            </Badge>
            <Badge tone="neutral">
              {t("ticket.priority")}: {namesById(priorities.data).get(ticket.priority_id) ?? "…"}
            </Badge>
            <SlaBadge target={target} />
            {ticket.sensitivity === "confidential" ? (
              <Badge tone="warning">{t("tickets.confidential")}</Badge>
            ) : null}
            {ticket.snoozed_until ? (
              <Badge tone="muted" icon={<BellOff aria-hidden="true" />}>
                {t("ticket.snoozedUntil", { date: f.dateTime(ticket.snoozed_until) })}
              </Badge>
            ) : null}
            {deleted ? <Badge tone="danger">{t("ticket.deleted")}</Badge> : null}
          </>
        }
        actionsLabel={t("ticket.actions")}
        actions={
          <>
            {canAssign ? (
              <Button
                variant={ticket.analyst_id ? "secondary" : "primary"}
                icon={<UserCheck aria-hidden="true" />}
                aria-keyshortcuts="a"
                disabled={ticket.analyst_id === me}
                loading={actions.assign.isPending && actions.assign.variables?.analyst_id === me}
                onClick={assignToMe}
              >
                {t("ticket.assignToMe")}
              </Button>
            ) : null}
            {canAssign ? (
              <Button icon={<UserPlus aria-hidden="true" />} onClick={() => setDialog("assign")}>
                {t("ticket.assign")}
              </Button>
            ) : null}
            {canUpdate ? (
              <Button
                icon={<CircleDot aria-hidden="true" />}
                onClick={() => {
                  setStatusTarget(undefined);
                  setDialog("status");
                }}
              >
                {t("ticket.changeStatus")}
              </Button>
            ) : null}
            {canUpdate ? (
              ticket.snoozed_until ? (
                <Button
                  icon={<AlarmClockOff aria-hidden="true" />}
                  loading={actions.wake.isPending}
                  onClick={() => actions.wake.mutate()}
                >
                  {t("ticket.wake")}
                </Button>
              ) : (
                <Button icon={<BellOff aria-hidden="true" />} onClick={() => setDialog("snooze")}>
                  {t("ticket.snooze")}
                </Button>
              )
            ) : null}
            {canUpdate ? (
              <Button icon={<Clock aria-hidden="true" />} onClick={() => setDialog("time")}>
                {t("ticket.logTime")}
              </Button>
            ) : null}
            {canUpdate || can("tickets.delete") || (deleted && session.is_admin) ? (
              <Menu
                label={t("ticket.more")}
                trigger={
                  <IconButton
                    label={t("ticket.more")}
                    variant="secondary"
                    icon={<MoreHorizontal aria-hidden="true" />}
                  />
                }
              >
                {canUpdate ? (
                  <MenuItem icon={<PencilLine />} onSelect={() => setDialog("edit")}>
                    {t("ticket.edit")}
                  </MenuItem>
                ) : null}
                {deleted && session.is_admin ? (
                  <MenuItem icon={<RotateCcw />} onSelect={() => actions.restore.mutate()}>
                    {t("ticket.restore")}
                  </MenuItem>
                ) : null}
                {!deleted && can("tickets.delete") ? (
                  <>
                    <MenuSeparator />
                    <MenuItem icon={<Trash2 />} tone="danger" onSelect={() => setDialog("delete")}>
                      {t("ticket.delete")}
                    </MenuItem>
                  </>
                ) : null}
              </Menu>
            ) : null}
          </>
        }
      />

      {others.length > 0 ? (
        <section aria-label={t("presence.label")}>
          <Notice tone="warning">
            <ul className="grid gap-0.5">
              {others.map((v) => (
                <li key={v.analyst_id} className="flex items-center gap-1.5">
                  {v.is_composing ? (
                    <PencilLine aria-hidden="true" className="size-3.5" />
                  ) : (
                    <Eye aria-hidden="true" className="size-3.5" />
                  )}
                  {t(v.is_composing ? "presence.composing" : "presence.viewing", {
                    name: analystNames.get(v.analyst_id) ?? t("common.analyst"),
                  })}
                </li>
              ))}
            </ul>
          </Notice>
        </section>
      ) : null}
      {deleted ? (
        <Notice tone="danger" live={false}>
          {t("ticket.deletedNotice")}
        </Notice>
      ) : null}
      {ticket.merged_into_id ? (
        <Notice tone="info" live={false}>
          {t("ticket.mergedNotice")}{" "}
          <Link
            to="/tickets/$ticketId"
            params={{ ticketId: ticket.merged_into_id }}
            search={true}
            className="font-medium underline"
          >
            {t("ticket.openSurvivor")}
          </Link>
        </Notice>
      ) : null}
      <ProblemNotice
        error={
          actions.assign.error ??
          actions.wake.error ??
          actions.restore.error ??
          actions.priority.error ??
          actions.update.error
        }
      />

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div className="grid min-w-0 content-start gap-5">
          <section
            aria-labelledby="ticket-description"
            className="grid gap-2 rounded-lg border border-line bg-surface p-4 shadow-raised"
          >
            <h3
              id="ticket-description"
              className="text-xs font-semibold uppercase tracking-wider text-fg-muted"
            >
              {t("ticket.description")}
            </h3>
            <p className="whitespace-pre-wrap text-sm leading-relaxed [overflow-wrap:anywhere]">
              {ticket.description || (
                <span className="text-fg-muted">{t("ticket.noDescription")}</span>
              )}
            </p>
          </section>
          {canUpdate ? (
            <Composer
              ref={composer}
              ticketId={ticket.id}
              requesterName={requester?.full_name}
              hasRequester={Boolean(ticket.requester_id)}
              onComposingChange={setComposing}
            />
          ) : null}
          <Timeline ticketId={ticket.id} requesterName={requester?.full_name} />
        </div>
        <aside className="grid content-start gap-4" aria-label={t("ticket.details")}>
          <PropertiesPanel
            ticket={ticket}
            canUpdate={canUpdate}
            canAssign={canAssign}
            actions={actions}
            onCloseStatus={(statusId) => {
              setStatusTarget(statusId);
              setDialog("status");
            }}
          />
          <Panel title={t("ticket.sla")}>
            <PropertyList>
              {(["response", "resolution"] as const).map((key) => (
                <Property key={key} label={t(`sla.${key}`)}>
                  <span className="grid justify-items-start gap-1">
                    <SlaBadge target={ticket.sla[key]} />
                    {ticket.sla[key].due_at ? (
                      <span className="text-xs text-fg-muted">
                        {t("sla.due", { date: f.dateTime(ticket.sla[key].due_at ?? "") })}
                      </span>
                    ) : null}
                  </span>
                </Property>
              ))}
            </PropertyList>
          </Panel>
          <TicketTasksPanel ticket={ticket} />
          <AttachmentsPanel ticket={ticket} canUpdate={can("tickets.update")} />
          <RelatedPanel ticket={ticket} />
        </aside>
      </div>

      <AssignDialog
        open={dialog === "assign"}
        onOpenChange={(o) => setDialog(o ? "assign" : null)}
        title={t("ticket.assignTitle", { number: ticket.number })}
        initial={{ team_id: ticket.team_id ?? null, analyst_id: ticket.analyst_id ?? null }}
        busy={actions.assign.isPending}
        error={actions.assign.error}
        onSubmit={(a) => actions.assign.mutate(a)}
      />
      <StatusDialog
        open={dialog === "status"}
        onOpenChange={(o) => setDialog(o ? "status" : null)}
        title={t("ticket.statusTitle", { number: ticket.number })}
        currentStatusId={ticket.status_id}
        initialStatusId={statusTarget}
        busy={actions.status.isPending}
        error={actions.status.error}
        onSubmit={(change) => actions.status.mutate(change)}
      />
      <SnoozeDialog
        open={dialog === "snooze"}
        onOpenChange={(o) => setDialog(o ? "snooze" : null)}
        title={t("ticket.snoozeTitle", { number: ticket.number })}
        busy={actions.snooze.isPending}
        error={actions.snooze.error}
        onSubmit={(body) => actions.snooze.mutate(body)}
      />
      <TimeDialog
        open={dialog === "time"}
        onOpenChange={(o) => setDialog(o ? "time" : null)}
        title={t("ticket.logTimeTitle", { number: ticket.number })}
        busy={actions.logTime.isPending}
        error={actions.logTime.error}
        minutesError={fieldError(actions.logTime.error, "minutes")}
        onSubmit={(body) => actions.logTime.mutate(body)}
      />
      {dialog === "edit" ? <EditDialog ticket={ticket} onClose={close} /> : null}
      <ConfirmDialog
        open={dialog === "delete"}
        onOpenChange={(o) => setDialog(o ? "delete" : null)}
        title={t("ticket.deleteTitle", { number: ticket.number })}
        confirmLabel={t("ticket.delete")}
        busy={remove.isPending}
        onConfirm={() => remove.mutate()}
      >
        <p className="text-sm">{t("ticket.deleteBody", { subject: ticket.subject })}</p>
        <ProblemNotice error={remove.error} className="mt-3" />
      </ConfirmDialog>
    </article>
  );
}

/** Who, what and how urgent, each changeable in place (master-detail: small edits here). */
function PropertiesPanel({
  ticket,
  canUpdate,
  canAssign,
  actions,
  onCloseStatus,
}: {
  ticket: Ticket;
  canUpdate: boolean;
  canAssign: boolean;
  actions: ReturnType<typeof useTicketActions>;
  onCloseStatus: (statusId: string) => void;
}) {
  const t = useText();
  const f = useFormat();
  const statuses = useQuery(ticketLookupQuery("ticket-statuses"));
  const priorities = useQuery(ticketLookupQuery("ticket-priorities"));
  const types = useQuery(ticketLookupQuery("ticket-types"));
  const origins = useQuery(ticketLookupQuery("ticket-origins"));
  const categories = useQuery(categoriesQuery);
  const departments = useQuery(departmentsQuery);
  const analysts = useQuery(analystsQuery);
  const teams = useQuery(teamsQuery);
  const requesters = useQuery(requestersByIdQuery([ticket.requester_id]));
  const requester = ticket.requester_id ? requesters.data?.get(ticket.requester_id) : undefined;
  const [saving, setSaving] = useState<string | null>(null);
  const change = (field: string, patch: Partial<ReturnType<typeof ticketRequest>>) => {
    setSaving(field);
    actions.update.mutate(
      { ...ticketRequest(ticket), ...patch },
      { onSettled: () => setSaving(null) },
    );
  };
  const team = teams.data?.find((q) => q.id === ticket.team_id);
  const members = new Set(team?.analyst_ids ?? []);
  const activeAnalysts = (analysts.data ?? []).filter(
    (a) => a.is_active || a.id === ticket.analyst_id,
  );
  const inTeam = activeAnalysts
    .filter((a) => members.has(a.id))
    .map((a) => ({ value: a.id, label: a.full_name }));
  const otherAnalysts = activeAnalysts
    .filter((a) => !members.has(a.id))
    .map((a) => ({ value: a.id, label: a.full_name }));
  return (
    <Panel title={t("ticket.details")}>
      <PropertyList>
        <Property label={t("ticket.requester")}>
          {requester ? (
            <span className="grid">
              <span>{requester.full_name}</span>
              <a
                href={`mailto:${requester.email}`}
                className="text-xs text-accent-text hover:underline"
              >
                {requester.email}
              </a>
            </span>
          ) : ticket.requester_id ? (
            "…"
          ) : (
            t("tickets.noRequester")
          )}
        </Property>
        <PropertySelect
          label={t("ticket.status")}
          value={ticket.status_id}
          options={toOptions(statuses.data, (s) => s.name, ticket.status_id)}
          disabled={!canUpdate}
          saving={actions.status.isPending}
          onChange={(statusId) => {
            const target = statuses.data?.find((s) => s.id === statusId);
            if (target?.is_closed) onCloseStatus(statusId);
            else
              actions.status.mutate({
                status_id: statusId,
                resolution_code_id: null,
                closure_category_id: null,
              });
          }}
        />
        <PropertySelect
          label={t("ticket.priority")}
          value={ticket.priority_id}
          options={toOptions(priorities.data, (p) => p.name, ticket.priority_id)}
          disabled={!canUpdate}
          saving={actions.priority.isPending}
          onChange={(id) => actions.priority.mutate(id)}
        />
        <PropertySelect
          label={t("ticket.queue")}
          value={ticket.team_id ?? ""}
          placeholder={t("ticket.noQueue")}
          options={toOptions(teams.data, (q) => q.name, ticket.team_id)}
          disabled={!canAssign}
          saving={
            actions.assign.isPending &&
            actions.assign.variables?.team_id !== (ticket.team_id ?? null)
          }
          onChange={(id) =>
            actions.assign.mutate({ team_id: id || null, analyst_id: ticket.analyst_id ?? null })
          }
        />
        <PropertySelect
          label={t("ticket.assignee")}
          value={ticket.analyst_id ?? ""}
          placeholder={t("tickets.unassigned")}
          options={[]}
          groups={[
            ...(team && inTeam.length > 0 ? [{ label: team.name, options: inTeam }] : []),
            {
              label: team && inTeam.length > 0 ? t("ticket.otherAnalysts") : t("ticket.analysts"),
              options: otherAnalysts,
            },
          ]}
          disabled={!canAssign}
          saving={
            actions.assign.isPending &&
            actions.assign.variables?.analyst_id !== (ticket.analyst_id ?? null)
          }
          onChange={(id) =>
            actions.assign.mutate({ team_id: ticket.team_id ?? null, analyst_id: id || null })
          }
        />
        <PropertySelect
          label={t("ticket.type")}
          value={ticket.type_id ?? ""}
          placeholder={t("common.none")}
          options={toOptions(types.data, (x) => x.name, ticket.type_id)}
          disabled={!canUpdate}
          saving={saving === "type"}
          onChange={(id) => change("type", { type_id: id || null })}
        />
        <PropertySelect
          label={t("ticket.category")}
          value={ticket.category_id ?? ""}
          placeholder={t("common.none")}
          options={categoryOptions(categories.data, ticket.category_id)}
          disabled={!canUpdate}
          saving={saving === "category"}
          onChange={(id) => change("category", { category_id: id || null })}
        />
        <PropertySelect
          label={t("ticket.origin")}
          value={ticket.origin_id ?? ""}
          placeholder={t("common.none")}
          options={toOptions(origins.data, (x) => x.name, ticket.origin_id)}
          disabled={!canUpdate}
          saving={saving === "origin"}
          onChange={(id) => change("origin", { origin_id: id || null })}
        />
        <PropertySelect
          label={t("ticket.department")}
          value={ticket.department_id ?? ""}
          placeholder={t("common.none")}
          options={toOptions(departments.data, (d) => d.name, ticket.department_id)}
          disabled={!canUpdate}
          saving={saving === "department"}
          onChange={(id) => change("department", { department_id: id || null })}
        />
        <PropertySelect
          label={t("ticket.sensitivity")}
          value={ticket.sensitivity}
          options={[
            { value: "normal", label: t("ticket.sensitivity.normal") },
            { value: "confidential", label: t("ticket.sensitivity.confidential") },
          ]}
          disabled={!canUpdate}
          saving={saving === "sensitivity"}
          onChange={(value) => change("sensitivity", { sensitivity: value })}
        />
        <Property label={t("ticket.created")}>
          <time dateTime={ticket.created_at}>{f.dateTime(ticket.created_at)}</time>
        </Property>
        <Property label={t("ticket.updated")}>
          <time dateTime={ticket.updated_at} title={f.dateTime(ticket.updated_at)}>
            {f.relative(ticket.updated_at)}
          </time>
        </Property>
        {ticket.closed_at ? (
          <Property label={t("ticket.closed")}>{f.dateTime(ticket.closed_at)}</Property>
        ) : null}
        <Property label={t("ticket.totalTime")}>
          {t("common.minutes", { minutes: ticket.total_minutes ?? 0 })}
        </Property>
        <Property label={t("ticket.flags")}>
          <span className="flex flex-wrap gap-1.5">
            {ticket.first_time_fix ? (
              <Badge tone="success">{t("ticket.firstTimeFix")}</Badge>
            ) : null}
            {ticket.training_provided ? (
              <Badge tone="info">{t("ticket.trainingProvided")}</Badge>
            ) : null}
            {!ticket.first_time_fix && !ticket.training_provided ? t("common.noneDash") : null}
          </span>
        </Property>
      </PropertyList>
    </Panel>
  );
}

/** Subject, description, Requester and flags: too many fields for in place (> 3). */
function EditDialog({ ticket, onClose }: { ticket: Ticket; onClose: () => void }) {
  const t = useText();
  const requesters = useQuery(requestersByIdQuery([ticket.requester_id]));
  const initialRequester = ticket.requester_id
    ? requesters.data?.get(ticket.requester_id)
    : undefined;
  const [form, setForm] = useState(() => ticketRequest(ticket));
  // `undefined` until the Analyst picks someone else: the ticket keeps its Requester.
  const [requester, setRequester] = useState<PickerOption | null | undefined>(undefined);
  const shownRequester =
    requester === undefined
      ? initialRequester
        ? requesterOption(initialRequester)
        : null
      : requester;
  const save = useAction(
    () =>
      ticketActions.update(ticket.id, {
        ...form,
        requester_id:
          requester === undefined ? (ticket.requester_id ?? null) : (requester?.id ?? null),
      }),
    { invalidate: [["tickets"]], success: "ticket.editSaved", onSuccess: onClose },
  );
  const set = <K extends keyof typeof form>(key: K, value: (typeof form)[K]) =>
    setForm((f) => ({ ...f, [key]: value }));
  const subjectError = fieldError(save.error, "subject");
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("ticket.editTitle", { number: ticket.number })}
      submitLabel={t("common.saveChanges")}
      busy={save.isPending}
      size="lg"
      submitDisabled={!form.subject.trim()}
      onSubmit={() => save.mutate()}
    >
      {subjectError ? null : <ProblemNotice error={save.error} />}
      <RequesterPicker
        label={t("ticket.requester")}
        companyId={ticket.company_id}
        value={shownRequester}
        onChange={setRequester}
      />
      <TextField
        label={t("newTicket.subject")}
        required
        value={form.subject}
        error={subjectError}
        onChange={(e) => set("subject", e.target.value)}
      />
      <TextAreaField
        label={t("newTicket.description")}
        rows={6}
        value={form.description ?? ""}
        onChange={(e) => set("description", e.target.value)}
      />
      <FormGrid>
        <SelectField
          label={t("ticket.sensitivity")}
          value={form.sensitivity ?? "normal"}
          onChange={(e) => set("sensitivity", e.target.value)}
          options={[
            { value: "normal", label: t("ticket.sensitivity.normal") },
            { value: "confidential", label: t("ticket.sensitivity.confidential") },
          ]}
        />
      </FormGrid>
      <div className="flex flex-wrap gap-6">
        <Checkbox
          label={t("ticket.firstTimeFix")}
          checked={Boolean(form.first_time_fix)}
          onChange={(e) => set("first_time_fix", e.target.checked)}
        />
        <Checkbox
          label={t("ticket.trainingProvided")}
          checked={Boolean(form.training_provided)}
          onChange={(e) => set("training_provided", e.target.checked)}
        />
      </div>
    </FormDialog>
  );
}
