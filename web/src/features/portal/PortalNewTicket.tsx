import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { CheckCircle2 } from "lucide-react";
import { useState } from "react";
import { Button, buttonClass } from "@/components/Button";
import { ProblemNotice } from "@/components/Feedback";
import { SelectField, TextAreaField, TextField } from "@/components/Field";
import { BackLink, PageHeader, Panel } from "@/components/Layout";
import { fieldError, useAction } from "@/features/shared/mutation";
import {
  applicableFields,
  type FieldValues,
  fieldPayload,
  TicketFieldInputs,
} from "@/features/tickets/TicketFieldInputs";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { useDebounced } from "@/lib/hooks";
import { problemOf } from "@/lib/problem";
import { call } from "@/lib/request";
import { catalogueQuery, type PortalTicket, portalActions, portalArticlesQuery } from "./api";

/**
 * Raise a request (FR-PRT-002): a Category from the Portal Catalogue, a summary, details
 * and any "ask requester" Ticket Fields. Matching help articles appear while typing, and
 * the end of the flow confirms the Ticket Number and what happens next (Peak-End Rule).
 */
export function PortalNewTicketPage({ category }: { category?: string | undefined }) {
  const t = useText();
  const catalogue = useQuery(catalogueQuery);
  const fields = useQuery({
    queryKey: ["portal", "ticket-fields"],
    queryFn: () => call(api.GET("/api/v1/portal/ticket-fields")),
    staleTime: 5 * 60_000,
  });
  const [categoryId, setCategoryId] = useState(category ?? "");
  const [subject, setSubject] = useState("");
  const [description, setDescription] = useState("");
  const [values, setValues] = useState<FieldValues>({});
  const [tried, setTried] = useState(false);
  const [created, setCreated] = useState<PortalTicket | null>(null);
  const debounced = useDebounced(subject.trim(), 400);
  const suggestions = useQuery({
    ...portalArticlesQuery(debounced),
    enabled: debounced.length >= 4,
  });
  const shownFields = applicableFields(
    (fields.data ?? []).filter((f) => f.ask_requester !== false),
    null,
  );
  const byId = new Map((catalogue.data ?? []).map((c) => [c.id, c]));
  const pathOf = (id: string): string => {
    const c = byId.get(id);
    if (!c) return "";
    return c.parent_id ? `${pathOf(c.parent_id)} › ${c.name}` : c.name;
  };
  const create = useAction(
    () =>
      portalActions.create({
        subject: subject.trim(),
        description,
        category_id: categoryId || null,
        ...(shownFields.length
          ? {
              fields: fieldPayload(shownFields, values) as NonNullable<
                Schemas["PortalTicketRequest"]["fields"]
              >,
            }
          : {}),
      }),
    { invalidate: [["portal", "tickets"]], onSuccess: (ticket) => setCreated(ticket) },
  );

  if (created) {
    return (
      <Panel className="mx-auto w-full max-w-xl">
        <div className="grid justify-items-center gap-3 py-6 text-center">
          <CheckCircle2 aria-hidden="true" className="size-12 text-success-text" />
          <h1
            tabIndex={-1}
            data-page-heading
            ref={(node) => node?.focus()}
            className="text-xl font-semibold"
          >
            {t("portalNew.doneTitle", { number: created.number })}
          </h1>
          <p className="max-w-md text-sm text-fg-muted">{t("portalNew.doneBody")}</p>
          <div className="flex flex-wrap justify-center gap-2 pt-2">
            <Link
              to="/portal/tickets/$id"
              params={{ id: created.id }}
              className={buttonClass({ variant: "primary", size: "lg" })}
            >
              {t("portalNew.view")}
            </Link>
            <Button
              size="lg"
              onClick={() => {
                setCreated(null);
                setSubject("");
                setDescription("");
                setValues({});
                setTried(false);
              }}
            >
              {t("portalNew.another")}
            </Button>
          </div>
        </div>
      </Panel>
    );
  }

  const missing = create.error ? problemOf(create.error).fields : [];
  return (
    <div className="mx-auto grid max-w-2xl gap-6">
      <PageHeader
        back={<BackLink to="/portal">{t("portal.nav.home")}</BackLink>}
        title={t("portalNew.title")}
        description={t("portalNew.intro")}
      />
      <Panel>
        <form
          noValidate
          className="grid gap-5"
          onSubmit={(e) => {
            e.preventDefault();
            setTried(true);
            if (!subject.trim()) {
              document.getElementById("portal-subject")?.focus();
              return;
            }
            create.mutate();
          }}
        >
          <ProblemNotice error={create.error} />
          <SelectField
            label={t("portalNew.category")}
            hint={t("portalNew.categoryHint")}
            value={categoryId}
            placeholder={t("portalHome.other")}
            onChange={(e) => setCategoryId(e.target.value)}
            options={(catalogue.data ?? []).map((c) => ({ value: c.id, label: pathOf(c.id) }))}
            className="[&_select]:h-11"
          />
          <TextField
            id="portal-subject"
            label={t("portalNew.subject")}
            hint={t("portalNew.subjectHint")}
            required
            value={subject}
            error={
              tried && !subject.trim()
                ? t("portalNew.subjectRequired")
                : fieldError(create.error, "subject")
            }
            onChange={(e) => setSubject(e.target.value)}
            className="[&_input]:h-11"
          />
          {(suggestions.data ?? []).length > 0 ? (
            <section
              aria-live="polite"
              className="grid gap-2 rounded-lg border border-accent/30 bg-accent-soft p-3"
            >
              <p className="text-sm font-medium">{t("portalNew.suggestions")}</p>
              <ul className="grid gap-1 text-sm">
                {(suggestions.data ?? []).slice(0, 4).map((a) => (
                  <li key={a.id}>
                    <Link
                      to="/portal/knowledge/$id"
                      params={{ id: a.id }}
                      target="_blank"
                      className="text-accent-text underline"
                    >
                      {a.title}
                      <span className="sr-only"> {t("common.newTab")}</span>
                    </Link>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          <TextAreaField
            label={t("portalNew.description")}
            hint={t("portalNew.descriptionHint")}
            rows={7}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
          <TicketFieldInputs
            legend={t("portalNew.fields")}
            fields={shownFields}
            values={values}
            onChange={setValues}
            missing={missing}
          />
          <div className="flex flex-wrap justify-end gap-2 border-t border-line pt-4">
            <Link to="/portal" className={buttonClass({ size: "lg" })}>
              {t("common.cancel")}
            </Link>
            <Button type="submit" variant="primary" size="lg" loading={create.isPending}>
              {t("portalNew.submit")}
            </Button>
          </div>
        </form>
      </Panel>
    </div>
  );
}
