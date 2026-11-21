import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { useState } from "react";
import { Button, buttonClass } from "@/components/Button";
import type { PickerOption } from "@/components/Combobox";
import { Fieldset, FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { Notice, ProblemNotice } from "@/components/Notice";
import { useToast } from "@/components/Toast";
import { RequesterPicker } from "@/features/reference/pickers";
import {
  analystsQuery,
  categoriesQuery,
  categoryOptions,
  companiesQuery,
  departmentsQuery,
  teamsQuery,
  ticketLookupQuery,
  toOptions,
} from "@/features/reference/queries";
import { useCan, useSession } from "@/features/session/session";
import { fieldError, useAction } from "@/features/shared/mutation";
import { taskActions, taskQuery } from "@/features/tasks/api";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { problemOf } from "@/lib/problem";
import { ticketActions, ticketFieldsQuery } from "./api";
import {
  applicableFields,
  type FieldValues,
  fieldPayload,
  TicketFieldInputs,
} from "./TicketFieldInputs";

/**
 * A new ticket, beside the list. Fields follow the work: who needs help, what with, how
 * urgent, who works on it (H2). The API fills in defaults (Priority, Sensitivity) and
 * validates; only the subject is checked here.
 */
export function NewTicket({ taskId }: { taskId?: string | undefined } = {}) {
  const t = useText();
  const navigate = useNavigate();
  const toast = useToast();
  const linkTo = useQuery({ ...taskQuery(taskId ?? ""), enabled: Boolean(taskId) });
  const task = taskId ? linkTo.data : undefined;
  const can = useCan();
  const session = useSession();
  const companies = useQuery(companiesQuery);
  const priorities = useQuery(ticketLookupQuery("ticket-priorities"));
  const types = useQuery(ticketLookupQuery("ticket-types"));
  const origins = useQuery(ticketLookupQuery("ticket-origins"));
  const categories = useQuery(categoriesQuery);
  const departments = useQuery(departmentsQuery);
  const teams = useQuery(teamsQuery);
  const analysts = useQuery(analystsQuery);
  const fields = useQuery({ ...ticketFieldsQuery, enabled: can("tickets.read") });
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);
  const defaultCompany = activeCompanies.find((c) => c.is_default) ?? activeCompanies[0];
  const [companyId, setCompanyId] = useState<string>("");
  // A ticket linked to a Task belongs to the Task's Company (links stay within one Company).
  const company = task?.company_id ?? (companyId || defaultCompany?.id || "");
  const [requester, setRequester] = useState<PickerOption | null>(null);
  const [subject, setSubject] = useState("");
  const [description, setDescription] = useState("");
  const [priorityId, setPriorityId] = useState("");
  const [typeId, setTypeId] = useState("");
  const [categoryId, setCategoryId] = useState("");
  const [originId, setOriginId] = useState("");
  const [departmentId, setDepartmentId] = useState("");
  const [sensitivity, setSensitivity] = useState("");
  const [teamId, setTeamId] = useState("");
  const [analystId, setAnalystId] = useState("");
  const [values, setValues] = useState<FieldValues>({});
  const [tried, setTried] = useState(false);
  const shownFields = applicableFields(fields.data, typeId || null);

  const create = useAction(
    async () => {
      const ticket = await ticketActions.create({
        subject: subject.trim(),
        description,
        company_id: company || null,
        requester_id: requester?.id ?? null,
        priority_id: priorityId || null,
        type_id: typeId || null,
        category_id: categoryId || null,
        origin_id: originId || null,
        department_id: departmentId || null,
        sensitivity: sensitivity || null,
        ...(can("tickets.assign")
          ? { team_id: teamId || null, analyst_id: analystId || null }
          : {}),
        ...(shownFields.length
          ? {
              fields: fieldPayload(shownFields, values) as NonNullable<
                Schemas["TicketRequest"]["fields"]
              >,
            }
          : {}),
      });
      if (!task) return { ticket, linked: false };
      try {
        await taskActions.link(task.id, { kind: "ticket", target_id: ticket.id });
        return { ticket, linked: true };
      } catch {
        return { ticket, linked: false };
      }
    },
    {
      invalidate: [["tickets", "list"], ["home"], ["tasks"]],
      onSuccess: ({ ticket, linked }) => {
        const open = () =>
          navigate({ to: "/tickets/$ticketId", params: { ticketId: ticket.id }, search: true });
        if (!task) {
          toast({ tone: "success", message: t("newTicket.created", { number: ticket.number }) });
          return open();
        }
        // Back to the Task the work started from; the new ticket is one click away.
        toast(
          linked
            ? {
                tone: "success",
                message: t("newTicket.createdLinked", { number: ticket.number, title: task.title }),
                action: { label: t("newTicket.openTicket"), onClick: () => void open() },
              }
            : {
                tone: "danger",
                message: t("newTicket.createdNotLinked", { number: ticket.number }),
              },
        );
        if (!linked) return open();
        return navigate({ to: "/tasks/$id", params: { id: task.id } });
      },
    },
  );
  const missing = create.error ? problemOf(create.error).fields : [];
  const subjectError =
    tried && !subject.trim() ? t("newTicket.subjectRequired") : fieldError(create.error, "subject");

  return (
    <section aria-labelledby="new-ticket-title" className="mx-auto grid max-w-3xl gap-5 p-4 sm:p-6">
      {taskId ? (
        <Link
          to="/tasks/$id"
          params={{ id: taskId }}
          className="inline-flex w-fit items-center gap-1 text-[0.8125rem] font-medium text-accent-text hover:underline"
        >
          <ArrowLeft aria-hidden="true" className="size-3.5" />
          {t("newTicket.backToTask")}
        </Link>
      ) : (
        <Link
          to="/tickets"
          search={true}
          className="inline-flex w-fit items-center gap-1 text-[0.8125rem] font-medium text-accent-text hover:underline"
        >
          <ArrowLeft aria-hidden="true" className="size-3.5" />
          {t("tickets.backToList")}
        </Link>
      )}
      <header className="grid gap-1">
        <h2 id="new-ticket-title" tabIndex={-1} data-page-heading className="text-xl font-semibold">
          {t("newTicket.title")}
        </h2>
        <p className="text-sm text-fg-muted">{t("newTicket.intro")}</p>
      </header>
      <form
        noValidate
        className="grid gap-6 rounded-lg border border-line bg-surface p-4 shadow-raised sm:p-6"
        onSubmit={(e) => {
          e.preventDefault();
          setTried(true);
          if (!subject.trim()) {
            document.getElementById("new-ticket-subject")?.focus();
            return;
          }
          create.mutate();
        }}
      >
        <ProblemNotice error={create.error ?? linkTo.error} />
        {task ? <Notice>{t("newTicket.linkedTo", { title: task.title })}</Notice> : null}
        <Fieldset legend={t("newTicket.who")}>
          <FormGrid>
            {activeCompanies.length > 1 && !taskId ? (
              <SelectField
                label={t("ticket.company")}
                value={company}
                onChange={(e) => {
                  setCompanyId(e.target.value);
                  setRequester(null);
                }}
                options={activeCompanies.map((c) => ({ value: c.id, label: c.name }))}
              />
            ) : null}
            <RequesterPicker
              label={t("ticket.requester")}
              hint={t("newTicket.requesterHint")}
              companyId={company || undefined}
              value={requester}
              onChange={setRequester}
              className={activeCompanies.length > 1 && !taskId ? undefined : "sm:col-span-2"}
            />
          </FormGrid>
        </Fieldset>
        <Fieldset legend={t("newTicket.what")}>
          <TextField
            id="new-ticket-subject"
            label={t("newTicket.subject")}
            required
            autoComplete="off"
            value={subject}
            error={subjectError}
            onChange={(e) => setSubject(e.target.value)}
          />
          <TextAreaField
            label={t("newTicket.description")}
            rows={6}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
          <FormGrid>
            <SelectField
              label={t("ticket.category")}
              value={categoryId}
              placeholder={t("common.none")}
              onChange={(e) => setCategoryId(e.target.value)}
              options={categoryOptions(categories.data)}
            />
            <SelectField
              label={t("ticket.type")}
              value={typeId}
              placeholder={t("newTicket.default")}
              onChange={(e) => setTypeId(e.target.value)}
              options={toOptions(types.data, (x) => x.name)}
            />
          </FormGrid>
        </Fieldset>
        <Fieldset legend={t("newTicket.urgency")}>
          <FormGrid>
            <SelectField
              label={t("ticket.priority")}
              value={priorityId}
              placeholder={t("newTicket.default")}
              onChange={(e) => setPriorityId(e.target.value)}
              options={toOptions(priorities.data, (x) => x.name)}
            />
            <SelectField
              label={t("ticket.sensitivity")}
              value={sensitivity}
              placeholder={t("newTicket.defaultSensitivity")}
              onChange={(e) => setSensitivity(e.target.value)}
              options={[
                { value: "normal", label: t("ticket.sensitivity.normal") },
                { value: "confidential", label: t("ticket.sensitivity.confidential") },
              ]}
            />
          </FormGrid>
        </Fieldset>
        {can("tickets.assign") ? (
          <Fieldset legend={t("newTicket.routing")}>
            <FormGrid>
              <SelectField
                label={t("ticket.queue")}
                value={teamId}
                placeholder={t("ticket.noQueue")}
                onChange={(e) => setTeamId(e.target.value)}
                options={toOptions(teams.data, (x) => x.name)}
              />
              <div className="grid content-start gap-1.5">
                <SelectField
                  label={t("ticket.assignee")}
                  value={analystId}
                  placeholder={t("tickets.unassigned")}
                  onChange={(e) => setAnalystId(e.target.value)}
                  options={toOptions(analysts.data, (x) => x.full_name)}
                />
                {analystId !== session.analyst.id ? (
                  <button
                    type="button"
                    className="w-fit text-xs font-medium text-accent-text hover:underline"
                    onClick={() => setAnalystId(session.analyst.id)}
                  >
                    {t("newTicket.assignToMe")}
                  </button>
                ) : null}
              </div>
            </FormGrid>
          </Fieldset>
        ) : null}
        <details className="group grid gap-4">
          <summary className="cursor-pointer text-sm font-semibold text-fg">
            {t("newTicket.more")}
          </summary>
          <FormGrid className="mt-4">
            <SelectField
              label={t("ticket.origin")}
              value={originId}
              placeholder={t("newTicket.default")}
              onChange={(e) => setOriginId(e.target.value)}
              options={toOptions(origins.data, (x) => x.name)}
            />
            <SelectField
              label={t("ticket.department")}
              value={departmentId}
              placeholder={t("common.none")}
              onChange={(e) => setDepartmentId(e.target.value)}
              options={toOptions(departments.data, (x) => x.name)}
            />
          </FormGrid>
        </details>
        <TicketFieldInputs
          legend={t("newTicket.fields")}
          fields={shownFields}
          values={values}
          onChange={setValues}
          missing={missing}
        />
        <div className="flex flex-wrap justify-end gap-2 border-t border-line pt-4">
          {taskId ? (
            <Link to="/tasks/$id" params={{ id: taskId }} className={buttonClass()}>
              {t("common.cancel")}
            </Link>
          ) : (
            <Link to="/tickets" search={true} className={buttonClass()}>
              {t("common.cancel")}
            </Link>
          )}
          <Button
            type="submit"
            variant="primary"
            loading={create.isPending}
            disabled={Boolean(taskId) && !task}
          >
            {taskId ? t("newTicket.submitLinked") : t("newTicket.submit")}
          </Button>
        </div>
      </form>
    </section>
  );
}
