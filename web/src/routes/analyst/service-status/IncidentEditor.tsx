import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { SelectField } from "@/components/ui/select-field";
import { Textarea } from "@/components/ui/textarea";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import {
  impactLevelsQuery,
  incidentQuery,
  incidentStatusesQuery,
  openIncident,
  postIncidentUpdate,
  servicesQuery,
} from "@/features/service-status/api";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";

/** Opens an Incident (`id` absent) or shows one with its updates and a form to post another. */
export function IncidentEditor({ id }: { id?: string | undefined }) {
  const t = useText();
  return (
    <div className="grid max-w-3xl gap-4">
      <Link to="/service-status/incidents" className="text-sm underline-offset-2 hover:underline">
        {t("status.backToIncidents")}
      </Link>
      {id ? <IncidentDetail id={id} /> : <OpenIncidentForm />}
    </div>
  );
}

function OpenIncidentForm() {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const services = useQuery(servicesQuery);
  const levels = useQuery(impactLevelsQuery);
  const statuses = useQuery(incidentStatusesQuery);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [statusId, setStatusId] = useState("");
  const [isPublic, setIsPublic] = useState(true);
  const [impact, setImpact] = useState<Record<string, string>>({});

  async function submit(event: FormEvent) {
    event.preventDefault();
    const affected = Object.entries(impact)
      .filter(([, level]) => level)
      .map(([service_id, impact_level_id]) => ({ service_id, impact_level_id }));
    if (!title.trim() || affected.length === 0) {
      flash.fail(t("status.incidentNeeds"));
      return;
    }
    const outcome = await openIncident({
      title: title.trim(),
      body: body.trim(),
      is_public: isPublic,
      services: affected,
      ...(statusId ? { status_id: statusId } : {}),
    });
    if (!outcome.ok) {
      flash.fail(t("tasks.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["service-status"] });
    await navigate({
      to: "/service-status/incidents/$id",
      params: { id: outcome.data.incident.id },
    });
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-3" aria-labelledby="incident-form-title">
      <h2 id="incident-form-title" className="text-xl font-semibold">
        {t("status.openIncident")}
      </h2>
      <Field
        label={t("tasks.title")}
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        required
      />
      <div className="grid gap-1.5">
        <Label htmlFor="incident-body">{t("status.message")}</Label>
        <Textarea
          id="incident-body"
          rows={3}
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
      </div>
      <SelectField
        label={t("tasks.status")}
        value={statusId}
        onChange={(e) => setStatusId(e.target.value)}
      >
        <option value="">{t("newTicket.useDefault")}</option>
        {(statuses.data ?? [])
          .filter((s) => s.is_active)
          .map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
      </SelectField>
      <fieldset className="grid gap-2">
        <legend className="mb-1 text-sm font-medium">{t("status.affected")}</legend>
        {(services.data ?? [])
          .filter((s) => s.is_active)
          .map((s) => (
            <SelectField
              key={s.id}
              label={s.name}
              value={impact[s.id] ?? ""}
              onChange={(e) => setImpact({ ...impact, [s.id]: e.target.value })}
            >
              <option value="">{t("status.notAffected")}</option>
              {(levels.data ?? [])
                .filter((l) => l.is_active)
                .map((l) => (
                  <option key={l.id} value={l.id}>
                    {l.name}
                  </option>
                ))}
            </SelectField>
          ))}
      </fieldset>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={isPublic} onChange={(e) => setIsPublic(e.target.checked)} />
        {t("status.public")}
      </label>
      <div className="flex items-center gap-3">
        <Button type="submit">{t("status.openIncident")}</Button>
        {flash.element}
      </div>
    </form>
  );
}

function IncidentDetail({ id }: { id: string }) {
  const t = useText();
  const intl = useIntl();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const session = useQuery(analystSessionQuery).data;
  const canUpdate = session ? can(session, "service-status.update") : false;
  const detail = useQuery(incidentQuery(id));
  const statuses = useQuery(incidentStatusesQuery);
  const [body, setBody] = useState("");
  const [statusId, setStatusId] = useState("");
  const [isPublic, setIsPublic] = useState(true);

  if (detail.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (!detail.data) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  const { incident, updates } = detail.data;
  const statusName = new Map((statuses.data ?? []).map((s) => [s.id, s.name]));

  async function post(event: FormEvent) {
    event.preventDefault();
    const request: Schemas["PostIncidentUpdateRequest"] = {
      body: body.trim(),
      is_public: isPublic,
      ...(statusId ? { status_id: statusId } : {}),
    };
    const outcome = await postIncidentUpdate(id, request);
    if (!outcome.ok) {
      flash.fail(t("tasks.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    setBody("");
    flash.ok(t("status.updatePosted"));
    await queryClient.invalidateQueries({ queryKey: ["service-status"] });
  }

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <h2 tabIndex={-1} className="text-xl font-semibold outline-none">
          {incident.title}
        </h2>
        <Badge tone={incident.is_resolved ? "ok" : "warning"}>
          {statusName.get(incident.status_id) ?? ""}
        </Badge>
      </div>
      <ol className="grid gap-2">
        {updates.map((u) => (
          <li key={u.id} className="rounded-md border border-border p-3 text-sm">
            <p className="flex flex-wrap items-center gap-2">
              <Badge>{statusName.get(u.status_id) ?? ""}</Badge>
              {u.is_public ? null : <Badge tone="muted">{t("status.internal")}</Badge>}
              <span className="text-xs text-muted-foreground">
                {intl.formatDate(u.posted_at, { dateStyle: "medium", timeStyle: "short" })}
              </span>
            </p>
            {u.body ? <p className="mt-1 whitespace-pre-wrap">{u.body}</p> : null}
          </li>
        ))}
      </ol>
      {canUpdate && !incident.is_resolved ? (
        <form onSubmit={post} className="grid gap-2" aria-labelledby="post-update">
          <h3 id="post-update" className="font-semibold">
            {t("status.postUpdate")}
          </h3>
          <div className="grid gap-1.5">
            <Label htmlFor="update-body">{t("status.message")}</Label>
            <Textarea
              id="update-body"
              rows={3}
              value={body}
              onChange={(e) => setBody(e.target.value)}
            />
          </div>
          <SelectField
            label={t("tasks.status")}
            value={statusId}
            onChange={(e) => setStatusId(e.target.value)}
          >
            <option value="">{t("status.keepStatus")}</option>
            {(statuses.data ?? [])
              .filter((s) => s.is_active)
              .map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
          </SelectField>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={isPublic}
              onChange={(e) => setIsPublic(e.target.checked)}
            />
            {t("status.public")}
          </label>
          <div className="flex items-center gap-3">
            <Button type="submit" size="sm">
              {t("status.postUpdate")}
            </Button>
            {flash.element}
          </div>
        </form>
      ) : null}
    </div>
  );
}
