import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { SelectField } from "@/components/ui/select-field";
import { Textarea } from "@/components/ui/textarea";
import { problemText } from "@/features/result";
import { createTicket } from "@/features/tickets/actions";
import {
  analystsQuery,
  categoriesQuery,
  originsQuery,
  prioritiesQuery,
  requesterSearchQuery,
  teamsQuery,
  typesQuery,
} from "@/features/tickets/queries";
import { companiesQuery, defaultCompanyId } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

const NONE = "";

/** The New ticket form (FR-TKT-001). The server applies defaults for anything left empty. */
export function NewTicket() {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const companies = useQuery(companiesQuery);
  const priorities = useQuery(prioritiesQuery);
  const types = useQuery(typesQuery);
  const origins = useQuery(originsQuery);
  const categories = useQuery(categoriesQuery);
  const teams = useQuery(teamsQuery);
  const analysts = useQuery(analystsQuery);

  const [subject, setSubject] = useState("");
  const [description, setDescription] = useState("");
  const [companyId, setCompanyId] = useState(NONE);
  const [requesterSearch, setRequesterSearch] = useState("");
  const [requesterId, setRequesterId] = useState(NONE);
  const [priorityId, setPriorityId] = useState(NONE);
  const [typeId, setTypeId] = useState(NONE);
  const [originId, setOriginId] = useState(NONE);
  const [categoryId, setCategoryId] = useState(NONE);
  const [teamId, setTeamId] = useState(NONE);
  const [analystId, setAnalystId] = useState(NONE);
  const [sensitivity, setSensitivity] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [subjectError, setSubjectError] = useState<string | undefined>();
  const [busy, setBusy] = useState(false);

  // The API needs a Company: the default one unless the Analyst picks another.
  const company = companyId || defaultCompanyId(companies.data);
  const requesters = useQuery(requesterSearchQuery(requesterSearch.trim(), company || undefined));

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!subject.trim()) {
      setSubjectError(t("newTicket.subjectRequired"));
      return;
    }
    setSubjectError(undefined);
    setBusy(true);
    setError(null);
    const body: Schemas["TicketRequest"] = {
      subject: subject.trim(),
      ...(description.trim() ? { description: description.trim() } : {}),
      ...(company ? { company_id: company } : {}),
      ...(requesterId ? { requester_id: requesterId } : {}),
      ...(priorityId ? { priority_id: priorityId } : {}),
      ...(typeId ? { type_id: typeId } : {}),
      ...(originId ? { origin_id: originId } : {}),
      ...(categoryId ? { category_id: categoryId } : {}),
      ...(teamId ? { team_id: teamId } : {}),
      ...(analystId ? { analyst_id: analystId } : {}),
      ...(sensitivity ? { sensitivity } : {}),
    };
    const outcome = await createTicket(body);
    setBusy(false);
    if (!outcome.ok) {
      setError(t("newTicket.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["tickets"] });
    await navigate({ to: "/tickets/$ticketId", params: { ticketId: outcome.data.id } });
  }

  const none = <option value={NONE}>{t("newTicket.useDefault")}</option>;
  return (
    <form onSubmit={submit} noValidate className="grid gap-4" aria-labelledby="new-ticket-title">
      <h2 id="new-ticket-title" tabIndex={-1} className="text-xl font-semibold outline-none">
        {t("newTicket.title")}
      </h2>
      {error ? <Alert variant="error">{error}</Alert> : null}
      <Field
        label={t("newTicket.subject")}
        value={subject}
        onChange={(e) => setSubject(e.target.value)}
        error={subjectError}
        required
      />
      <div className="grid gap-1.5">
        <Label htmlFor="new-ticket-description">{t("newTicket.description")}</Label>
        <Textarea
          id="new-ticket-description"
          rows={5}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <SelectField
          label={t("newTicket.company")}
          value={company}
          onChange={(e) => {
            setCompanyId(e.target.value);
            setRequesterId(NONE);
          }}
        >
          {(companies.data ?? []).map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </SelectField>
        <div className="grid gap-1.5">
          <Field
            label={t("newTicket.requesterSearch")}
            type="search"
            value={requesterSearch}
            onChange={(e) => setRequesterSearch(e.target.value)}
          />
          <SelectField
            label={t("newTicket.requester")}
            value={requesterId}
            onChange={(e) => setRequesterId(e.target.value)}
          >
            <option value={NONE}>{t("newTicket.noRequester")}</option>
            {(requesters.data ?? []).map((r) => (
              <option key={r.id} value={r.id}>
                {r.full_name} ({r.email})
              </option>
            ))}
          </SelectField>
        </div>
        <SelectField
          label={t("newTicket.priority")}
          value={priorityId}
          onChange={(e) => setPriorityId(e.target.value)}
        >
          {none}
          {(priorities.data ?? [])
            .filter((p) => p.is_active)
            .map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
        </SelectField>
        <SelectField
          label={t("newTicket.type")}
          value={typeId}
          onChange={(e) => setTypeId(e.target.value)}
        >
          {none}
          {(types.data ?? [])
            .filter((x) => x.is_active)
            .map((x) => (
              <option key={x.id} value={x.id}>
                {x.name}
              </option>
            ))}
        </SelectField>
        <SelectField
          label={t("newTicket.origin")}
          value={originId}
          onChange={(e) => setOriginId(e.target.value)}
        >
          {none}
          {(origins.data ?? [])
            .filter((x) => x.is_active)
            .map((x) => (
              <option key={x.id} value={x.id}>
                {x.name}
              </option>
            ))}
        </SelectField>
        <SelectField
          label={t("newTicket.category")}
          value={categoryId}
          onChange={(e) => setCategoryId(e.target.value)}
        >
          {none}
          {(categories.data ?? [])
            .filter((x) => x.is_active)
            .map((x) => (
              <option key={x.id} value={x.id}>
                {`${"– ".repeat(x.depth)}${x.name}`}
              </option>
            ))}
        </SelectField>
        <SelectField
          label={t("newTicket.team")}
          value={teamId}
          onChange={(e) => setTeamId(e.target.value)}
        >
          <option value={NONE}>{t("tickets.unassigned")}</option>
          {(teams.data ?? []).map((x) => (
            <option key={x.id} value={x.id}>
              {x.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("newTicket.analyst")}
          value={analystId}
          onChange={(e) => setAnalystId(e.target.value)}
        >
          <option value={NONE}>{t("tickets.unassigned")}</option>
          {(analysts.data ?? []).map((x) => (
            <option key={x.id} value={x.id}>
              {x.full_name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("newTicket.sensitivity")}
          value={sensitivity}
          onChange={(e) => setSensitivity(e.target.value)}
        >
          {none}
          <option value="normal">{t("newTicket.normal")}</option>
          <option value="confidential">{t("tickets.confidential")}</option>
        </SelectField>
      </div>
      <div className="flex gap-2">
        <Button type="submit" disabled={busy}>
          {t("newTicket.submit")}
        </Button>
        <Button type="button" variant="outline" onClick={() => void navigate({ to: "/tickets" })}>
          {t("newTicket.cancel")}
        </Button>
      </div>
    </form>
  );
}
