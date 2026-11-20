import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import {
  AlarmClockOff,
  ArrowLeft,
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
import { Card } from "@/components/Card";
import type { PickerOption } from "@/components/Combobox";
import { ConfirmDialog, FormDialog } from "@/components/Dialog";
import { Facts } from "@/components/Facts";
import { Checkbox, FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { Menu, MenuItem, MenuSeparator } from "@/components/Menu";
import { Notice, ProblemNotice } from "@/components/Notice";
import { LoadingBlock } from "@/components/QueryState";
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
import { useClaimFocus } from "@/features/shell/useFocusOnNavigate";
import { useFormat, useText } from "@/i18n/useText";
import { fromLocalInput, localDay, toLocalInput } from "@/lib/dates";
import { useHotkeys } from "@/lib/hooks";
import { type Ticket, ticketActions, ticketQuery, ticketRequest } from "./api";
import { Composer, type ComposerHandle } from "./Composer";
import { AssignDialog, StatusDialog } from "./dialogs";
import { useTicketPresence } from "./presence";
import { currentTarget, SlaBadge } from "./Sla";
import { AttachmentsCard, RelatedCard, TicketTasksCard } from "./TicketSideCards";
import { Timeline } from "./Timeline";

type DialogName = "assign" | "status" | "snooze" | "time" | "edit" | "delete" | null;

/**
 * One ticket: who, what and how urgent at a glance, the triage actions, the composer and
 * the timeline. `a` assigns to me, `r` replies, `n` adds a Note.
 */
export function TicketDetail({ ticketId }: { ticketId: string }) {
  const t = useText();
  const query = useQuery(ticketQuery(ticketId));
  const ticket = query.data;
  if (!ticket) {
    return (
      <article aria-labelledby="ticket-subject" className="grid gap-4 p-4 sm:p-6">
        <BackToList />
        <h2 id="ticket-subject" tabIndex={-1} data-page-heading className="text-xl font-semibold">
          {query.isError ? t("ticket.unavailable") : t("ticket.loading")}
        </h2>
        {query.isError ? <ProblemNotice error={query.error} /> : <LoadingBlock lines={6} />}
      </article>
    );
  }
  return <TicketView ticket={ticket} />;
}

function BackToList() {
  const t = useText();
  return (
    <Link
      to="/tickets"
      search={true}
      className="inline-flex w-fit items-center gap-1 text-[0.8125rem] font-medium text-accent-text hover:underline xl:hidden"
    >
      <ArrowLeft aria-hidden="true" className="size-3.5" />
      {t("tickets.backToList")}
    </Link>
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
  const types = useQuery(ticketLookupQuery("ticket-types"));
  const origins = useQuery(ticketLookupQuery("ticket-origins"));
  const categories = useQuery(categoriesQuery);
  const departments = useQuery(departmentsQuery);
  const companies = useQuery(companiesQuery);
  const analysts = useQuery(analystsQuery);
  const teams = useQuery(teamsQuery);
  const requesters = useQuery(requestersByIdQuery([ticket.requester_id]));
  const requester = ticket.requester_id ? requesters.data?.get(ticket.requester_id) : undefined;
  const [dialog, setDialog] = useState<DialogName>(null);
  const [composing, setComposing] = useState(false);
  const composer = useRef<ComposerHandle>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  useClaimFocus(heading);
  const viewers = useTicketPresence(ticket.id, composing);
  const me = session.analyst.id;
  const deleted = Boolean(ticket.deleted_at);
  const canUpdate = can("tickets.update") && !deleted;
  const canAssign = (can("tickets.assign") || can("tickets.update")) && !deleted;
  const statusName = namesById(statuses.data).get(ticket.status_id) ?? "…";
  const analystNames = namesById(analysts.data, (a) => a.full_name);

  const refresh = [["tickets"]] as const;
  const close = () => setDialog(null);
  const assign = useAction(
    (body: { team_id: string | null; analyst_id: string | null }) =>
      ticketActions.assign(ticket.id, body),
    {
      invalidate: refresh,
      success: (_, vars) =>
        vars.analyst_id === me ? t("ticket.assigned") : t("ticket.assignSaved"),
      onSuccess: close,
    },
  );
  const status = useAction(
    (body: Parameters<typeof ticketActions.status>[1]) => ticketActions.status(ticket.id, body),
    {
      invalidate: refresh,
      success: (updated) =>
        t("ticket.statusSaved", { status: namesById(statuses.data).get(updated.status_id) ?? "" }),
      onSuccess: close,
    },
  );
  const wake = useAction(() => ticketActions.wake(ticket.id), {
    invalidate: refresh,
    success: "ticket.woken",
  });
  const remove = useAction(() => ticketActions.remove(ticket.id), {
    invalidate: refresh,
    onSuccess: async () => {
      close();
      toast({ tone: "success", message: t("ticket.deletedDone", { number: ticket.number }) });
      await navigate({ to: "/tickets", search: true });
    },
  });
  const restore = useAction(() => ticketActions.restore(ticket.id), {
    invalidate: refresh,
    success: "ticket.restored",
  });

  function assignToMe() {
    if (!canAssign || ticket.analyst_id === me) return;
    assign.mutate({ analyst_id: me, team_id: ticket.team_id ?? null });
  }

  useHotkeys({
    a: assignToMe,
    r: () => composer.current?.focus("reply"),
    n: () => composer.current?.focus("note"),
  });

  const { target } = currentTarget(ticket.sla);
  const others = viewers.filter((v) => v.analyst_id !== me);

  return (
    <article aria-labelledby="ticket-subject" className="mx-auto grid max-w-6xl gap-5 p-4 sm:p-6">
      <BackToList />
      <header className="grid gap-3">
        <div className="flex flex-wrap items-center gap-2 text-sm text-fg-muted">
          <span className="font-mono font-medium text-fg">{ticket.number}</span>
          <IconButton
            size="sm"
            label={t("ticket.copyNumber")}
            icon={<Copy aria-hidden="true" className="size-3.5" />}
            tooltip="top"
            onClick={() => {
              void navigator.clipboard
                ?.writeText(ticket.number)
                .then(() => toast({ tone: "info", message: t("common.copied") }));
            }}
          />
          <span aria-hidden="true">·</span>
          <span>{namesById(companies.data).get(ticket.company_id) ?? ""}</span>
        </div>
        <h2
          ref={heading}
          id="ticket-subject"
          tabIndex={-1}
          data-page-heading
          className="text-xl font-semibold leading-snug tracking-tight [overflow-wrap:anywhere]"
        >
          {ticket.subject}
        </h2>
        <div className="flex flex-wrap items-center gap-1.5">
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
        </div>
      </header>

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
          <Link to="/tickets/$ticketId" params={{ ticketId: ticket.merged_into_id }}>
            {t("notifications.open")}
          </Link>
        </Notice>
      ) : null}

      <div
        role="toolbar"
        aria-label={t("ticket.actions")}
        className="flex flex-wrap items-center gap-2"
      >
        {canAssign ? (
          <Button
            variant={ticket.analyst_id ? "secondary" : "primary"}
            icon={<UserCheck aria-hidden="true" className="size-4" />}
            aria-keyshortcuts="a"
            disabled={ticket.analyst_id === me}
            loading={assign.isPending && assign.variables?.analyst_id === me}
            onClick={assignToMe}
          >
            {t("ticket.assignToMe")}
          </Button>
        ) : null}
        {canAssign ? (
          <Button
            icon={<UserPlus aria-hidden="true" className="size-4" />}
            onClick={() => setDialog("assign")}
          >
            {t("ticket.assign")}
          </Button>
        ) : null}
        {canUpdate ? (
          <Button
            icon={<CircleDot aria-hidden="true" className="size-4" />}
            onClick={() => setDialog("status")}
          >
            {t("ticket.changeStatus")}
          </Button>
        ) : null}
        {canUpdate ? (
          ticket.snoozed_until ? (
            <Button
              icon={<AlarmClockOff aria-hidden="true" className="size-4" />}
              loading={wake.isPending}
              onClick={() => wake.mutate()}
            >
              {t("ticket.wake")}
            </Button>
          ) : (
            <Button
              icon={<BellOff aria-hidden="true" className="size-4" />}
              onClick={() => setDialog("snooze")}
            >
              {t("ticket.snooze")}
            </Button>
          )
        ) : null}
        {canUpdate ? (
          <Button
            icon={<Clock aria-hidden="true" className="size-4" />}
            onClick={() => setDialog("time")}
          >
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
                icon={<MoreHorizontal aria-hidden="true" className="size-4" />}
              />
            }
          >
            {canUpdate ? (
              <MenuItem icon={<PencilLine />} onSelect={() => setDialog("edit")}>
                {t("ticket.edit")}
              </MenuItem>
            ) : null}
            {deleted && session.is_admin ? (
              <MenuItem icon={<RotateCcw />} onSelect={() => restore.mutate()}>
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
      </div>
      <ProblemNotice error={assign.error ?? wake.error ?? restore.error} />

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div className="grid min-w-0 content-start gap-5">
          <section
            aria-labelledby="ticket-description"
            className="grid gap-2 rounded-lg border border-line bg-surface p-4 shadow-raised"
          >
            <h2
              id="ticket-description"
              className="text-xs font-semibold uppercase tracking-wider text-fg-muted"
            >
              {t("ticket.description")}
            </h2>
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
          <Card title={t("ticket.details")}>
            <Facts
              layout="inline"
              items={[
                {
                  key: "requester",
                  term: t("ticket.requester"),
                  value: requester ? (
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
                  ),
                },
                {
                  key: "assignee",
                  term: t("ticket.assignee"),
                  value: ticket.analyst_id
                    ? (analystNames.get(ticket.analyst_id) ?? "…")
                    : t("tickets.unassigned"),
                },
                {
                  key: "queue",
                  term: t("ticket.queue"),
                  value: ticket.team_id
                    ? (namesById(teams.data).get(ticket.team_id) ?? "…")
                    : t("ticket.noQueue"),
                },
                {
                  key: "type",
                  term: t("ticket.type"),
                  value:
                    (ticket.type_id && namesById(types.data).get(ticket.type_id)) ||
                    t("common.noneDash"),
                },
                {
                  key: "category",
                  term: t("ticket.category"),
                  value:
                    (ticket.category_id &&
                      categoryOptions(categories.data, ticket.category_id).find(
                        (c) => c.value === ticket.category_id,
                      )?.label) ||
                    t("common.noneDash"),
                },
                {
                  key: "origin",
                  term: t("ticket.origin"),
                  value:
                    (ticket.origin_id && namesById(origins.data).get(ticket.origin_id)) ||
                    t("common.noneDash"),
                },
                {
                  key: "department",
                  term: t("ticket.department"),
                  value:
                    (ticket.department_id &&
                      namesById(departments.data).get(ticket.department_id)) ||
                    t("common.noneDash"),
                },
                {
                  key: "created",
                  term: t("ticket.created"),
                  value: <time dateTime={ticket.created_at}>{f.dateTime(ticket.created_at)}</time>,
                },
                {
                  key: "updated",
                  term: t("ticket.updated"),
                  value: <time dateTime={ticket.updated_at}>{f.relative(ticket.updated_at)}</time>,
                },
                ticket.closed_at
                  ? { key: "closed", term: t("ticket.closed"), value: f.dateTime(ticket.closed_at) }
                  : null,
                {
                  key: "time",
                  term: t("ticket.totalTime"),
                  value: t("common.minutes", { minutes: ticket.total_minutes ?? 0 }),
                },
              ]}
            />
          </Card>
          <Card title={t("ticket.sla")}>
            <Facts
              items={(["response", "resolution"] as const).map((key) => ({
                key,
                term: t(`sla.${key}`),
                value: (
                  <span className="flex flex-wrap items-center gap-2">
                    <SlaBadge target={ticket.sla[key]} />
                    {ticket.sla[key].due_at ? (
                      <span className="text-xs text-fg-muted">
                        {t("sla.due", { date: f.dateTime(ticket.sla[key].due_at ?? "") })}
                      </span>
                    ) : null}
                  </span>
                ),
              }))}
            />
          </Card>
          <TicketTasksCard ticket={ticket} />
          <AttachmentsCard ticket={ticket} canUpdate={can("tickets.update")} />
          <RelatedCard ticket={ticket} />
        </aside>
      </div>

      <AssignDialog
        open={dialog === "assign"}
        onOpenChange={(o) => setDialog(o ? "assign" : null)}
        title={t("ticket.assignTitle", { number: ticket.number })}
        initial={{ team_id: ticket.team_id ?? null, analyst_id: ticket.analyst_id ?? null }}
        busy={assign.isPending}
        error={assign.error}
        onSubmit={(a) => assign.mutate(a)}
      />
      <StatusDialog
        open={dialog === "status"}
        onOpenChange={(o) => setDialog(o ? "status" : null)}
        title={t("ticket.statusTitle", { number: ticket.number })}
        currentStatusId={ticket.status_id}
        busy={status.isPending}
        error={status.error}
        onSubmit={(change) => status.mutate(change)}
      />
      <SnoozeDialog
        ticket={ticket}
        open={dialog === "snooze"}
        onOpenChange={(o) => setDialog(o ? "snooze" : null)}
      />
      <TimeDialog
        ticket={ticket}
        open={dialog === "time"}
        onOpenChange={(o) => setDialog(o ? "time" : null)}
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

function SnoozeDialog({
  ticket,
  open,
  onOpenChange,
}: {
  ticket: Ticket;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const t = useText();
  const f = useFormat();
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(9, 0, 0, 0);
  const [until, setUntil] = useState(toLocalInput(tomorrow));
  const [reason, setReason] = useState("");
  const snooze = useAction(
    () => ticketActions.snooze(ticket.id, { until: fromLocalInput(until) ?? "", reason }),
    {
      invalidate: [["tickets"]],
      success: () => t("ticket.snoozed", { date: f.dateTime(fromLocalInput(until) ?? until) }),
      onSuccess: () => onOpenChange(false),
    },
  );
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("ticket.snoozeTitle", { number: ticket.number })}
      description={t("ticket.snoozeIntro")}
      submitLabel={t("ticket.snooze")}
      busy={snooze.isPending}
      submitDisabled={!fromLocalInput(until)}
      onSubmit={() => snooze.mutate()}
    >
      <ProblemNotice error={snooze.error} />
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

function TimeDialog({
  ticket,
  open,
  onOpenChange,
}: {
  ticket: Ticket;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const t = useText();
  const [minutes, setMinutes] = useState("");
  const [date, setDate] = useState("");
  const [note, setNote] = useState("");
  const log = useAction(
    () =>
      ticketActions.logTime(ticket.id, { minutes: Number(minutes), note, work_date: date || null }),
    {
      invalidate: [["tickets", "detail", ticket.id]],
      success: () => t("ticket.timeLogged", { minutes: Number(minutes) }),
      onSuccess: () => {
        onOpenChange(false);
        setMinutes("");
        setNote("");
        setDate("");
      },
    },
  );
  const valid = Number.isInteger(Number(minutes)) && Number(minutes) > 0;
  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("ticket.logTimeTitle", { number: ticket.number })}
      submitLabel={t("ticket.logTime")}
      busy={log.isPending}
      submitDisabled={!valid}
      size="sm"
      onSubmit={() => log.mutate()}
    >
      <ProblemNotice error={log.error} />
      <TextField
        label={t("ticket.minutes")}
        hint={t("ticket.minutesHint")}
        type="number"
        inputMode="numeric"
        min={1}
        step={1}
        required
        value={minutes}
        error={fieldError(log.error, "minutes")}
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

function EditDialog({ ticket, onClose }: { ticket: Ticket; onClose: () => void }) {
  const t = useText();
  const priorities = useQuery(ticketLookupQuery("ticket-priorities"));
  const types = useQuery(ticketLookupQuery("ticket-types"));
  const origins = useQuery(ticketLookupQuery("ticket-origins"));
  const categories = useQuery(categoriesQuery);
  const departments = useQuery(departmentsQuery);
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
    {
      invalidate: [["tickets"]],
      success: "ticket.editSaved",
      onSuccess: onClose,
    },
  );
  const set = <K extends keyof typeof form>(key: K, value: (typeof form)[K]) =>
    setForm((f) => ({ ...f, [key]: value }));
  const err = (field: string) => fieldError(save.error, field);
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
      <ProblemNotice error={save.error} />
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
        error={err("subject")}
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
          label={t("ticket.priority")}
          value={form.priority_id ?? ""}
          onChange={(e) => set("priority_id", e.target.value)}
          options={toOptions(priorities.data, (p) => p.name, ticket.priority_id)}
        />
        <SelectField
          label={t("ticket.type")}
          value={form.type_id ?? ""}
          placeholder={t("common.none")}
          onChange={(e) => set("type_id", e.target.value || null)}
          options={toOptions(types.data, (p) => p.name, ticket.type_id)}
        />
        <SelectField
          label={t("ticket.category")}
          value={form.category_id ?? ""}
          placeholder={t("common.none")}
          onChange={(e) => set("category_id", e.target.value || null)}
          options={categoryOptions(categories.data, ticket.category_id)}
        />
        <SelectField
          label={t("ticket.origin")}
          value={form.origin_id ?? ""}
          placeholder={t("common.none")}
          onChange={(e) => set("origin_id", e.target.value || null)}
          options={toOptions(origins.data, (p) => p.name, ticket.origin_id)}
        />
        <SelectField
          label={t("ticket.department")}
          value={form.department_id ?? ""}
          placeholder={t("common.none")}
          onChange={(e) => set("department_id", e.target.value || null)}
          options={toOptions(departments.data, (d) => d.name, ticket.department_id)}
        />
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
