import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Pencil } from "lucide-react";
import { useState } from "react";
import { Badge, ColourLabel } from "@/components/Badge";
import { Button } from "@/components/Button";
import { Card } from "@/components/Card";
import { FormDialog } from "@/components/Dialog";
import {
  Checkbox,
  Fieldset,
  Select,
  SelectField,
  TextAreaField,
  TextField,
} from "@/components/Field";
import { Notice, ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import { LoadingBlock, QueryState } from "@/components/QueryState";
import { namesById, toOptions } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useActorName } from "@/features/shared/History";
import { useAction } from "@/features/shared/mutation";
import { RecordForm } from "@/features/shared/RecordForm";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { impactLevelsQuery, incidentQuery, incidentStatusesQuery, servicesQuery } from "./api";

type Impacts = Record<string, string>;

/** A Service Incident: open a new one, or follow and update an existing one (FR-SST-002…004). */
export function IncidentPage({ id }: { id?: string }) {
  const query = useQuery({ ...incidentQuery(id ?? ""), enabled: Boolean(id) });
  if (!id) return <NewIncident />;
  return (
    <QueryState query={query} loading={<LoadingBlock lines={8} />}>
      {(detail) => <IncidentView detail={detail} />}
    </QueryState>
  );
}

/** One impact choice per active Service; "Not affected" leaves it out. */
function ImpactEditor({
  value,
  onChange,
  keepLabel,
  legend,
}: {
  value: Impacts;
  onChange: (v: Impacts) => void;
  keepLabel?: string | undefined;
  legend: string;
}) {
  const t = useText();
  const services = useQuery(servicesQuery);
  const levels = useQuery(impactLevelsQuery);
  const active = (services.data ?? []).filter((s) => s.is_active);
  return (
    <Fieldset legend={legend}>
      <div className="grid gap-2 sm:grid-cols-2">
        {active.map((s) => (
          <div key={s.id} className="grid gap-1">
            <label htmlFor={`impact-${s.id}`} className="text-[0.8125rem] font-medium">
              {s.name}
            </label>
            <Select
              id={`impact-${s.id}`}
              value={value[s.id] ?? ""}
              onChange={(e) => onChange({ ...value, [s.id]: e.target.value })}
            >
              <option value="">{keepLabel ?? t("status.notAffected")}</option>
              {(levels.data ?? [])
                .filter((l) => l.is_active)
                .map((l) => (
                  <option key={l.id} value={l.id}>
                    {l.name}
                  </option>
                ))}
            </Select>
          </div>
        ))}
      </div>
    </Fieldset>
  );
}

const toImpacts = (impacts: Impacts): Schemas["ServiceImpactDto"][] =>
  Object.entries(impacts)
    .filter(([, level]) => level)
    .map(([service_id, impact_level_id]) => ({ service_id, impact_level_id }));

function NewIncident() {
  const t = useText();
  const navigate = useNavigate();
  const statuses = useQuery(incidentStatusesQuery);
  const [title, setTitle] = useState("");
  const [statusId, setStatusId] = useState("");
  const [impacts, setImpacts] = useState<Impacts>({});
  const [body, setBody] = useState("");
  const [isPublic, setIsPublic] = useState(true);
  const [tried, setTried] = useState(false);
  const services = toImpacts(impacts);
  const open = useAction(
    () =>
      call(
        api.POST("/api/v1/service-status/incidents", {
          body: {
            title: title.trim(),
            status_id: statusId || null,
            services,
            body,
            is_public: isPublic,
          },
        }),
      ),
    {
      invalidate: [["service-status"]],
      success: "status.opened",
      onSuccess: (d) =>
        navigate({ to: "/service-status/incidents/$id", params: { id: d.incident.id } }),
    },
  );
  return (
    <div className="grid gap-4">
      <SectionHeader
        back={{ to: "/service-status/incidents", label: t("status.back") }}
        title={t("status.newTitle")}
      />
      <Card>
        <RecordForm
          busy={open.isPending}
          error={open.error}
          dirty={false}
          submitLabel={t("status.openIncident")}
          onSubmit={() => {
            setTried(true);
            if (title.trim() && services.length > 0) open.mutate();
          }}
        >
          <TextField
            label={t("status.titleField")}
            required
            value={title}
            error={tried && !title.trim() ? t("status.titleRequired") : undefined}
            onChange={(e) => setTitle(e.target.value)}
          />
          <SelectField
            label={t("status.incidentStatus")}
            value={statusId}
            placeholder={t("newTicket.default")}
            onChange={(e) => setStatusId(e.target.value)}
            options={toOptions(statuses.data, (s) => s.name)}
          />
          {tried && services.length === 0 ? (
            <Notice tone="danger">{t("status.pickService")}</Notice>
          ) : null}
          <ImpactEditor legend={t("status.affected")} value={impacts} onChange={setImpacts} />
          <TextAreaField
            label={t("status.message")}
            hint={t("status.messageHint")}
            rows={4}
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />
          <Checkbox
            label={t("status.public")}
            checked={isPublic}
            onChange={(e) => setIsPublic(e.target.checked)}
          />
        </RecordForm>
      </Card>
    </div>
  );
}

function IncidentView({ detail }: { detail: Schemas["ServiceIncidentDetailDto"] }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const actorName = useActorName();
  const statuses = useQuery(incidentStatusesQuery);
  const services = useQuery(servicesQuery);
  const levels = useQuery(impactLevelsQuery);
  const incident = detail.incident;
  const [rename, setRename] = useState(false);
  const statusNames = namesById(statuses.data);
  const serviceNames = namesById(services.data);
  const levelById = new Map((levels.data ?? []).map((l) => [l.id, l]));
  const updates = [...detail.updates].sort((a, b) => b.posted_at.localeCompare(a.posted_at));
  return (
    <div className="grid gap-4">
      <SectionHeader
        back={{ to: "/service-status/incidents", label: t("status.back") }}
        title={incident.title}
        description={`${t("status.started")} ${f.dateTime(incident.started_at)}${incident.resolved_at ? ` · ${t("status.resolved")} ${f.dateTime(incident.resolved_at)}` : ""}`}
        actions={
          can("service-status.update") ? (
            <Button
              icon={<Pencil aria-hidden="true" className="size-4" />}
              onClick={() => setRename(true)}
            >
              {t("status.rename")}
            </Button>
          ) : undefined
        }
      />
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={incident.is_resolved ? "muted" : "warning"}>
          {statusNames.get(incident.status_id) ?? "…"}
        </Badge>
        {incident.services.map((s) => {
          const level = levelById.get(s.impact_level_id);
          return (
            <span
              key={s.service_id}
              className="inline-flex items-center gap-1.5 rounded-full border border-line px-2 py-0.5 text-xs"
            >
              {serviceNames.get(s.service_id) ?? "…"}:
              <ColourLabel colour={level?.colour}>{level?.name ?? "…"}</ColourLabel>
            </span>
          );
        })}
      </div>
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_24rem]">
        <Card title={t("status.updates")}>
          {updates.length === 0 ? (
            <p className="text-sm text-fg-muted">{t("status.noUpdates")}</p>
          ) : (
            <ol className="grid gap-3">
              {updates.map((u) => (
                <li key={u.id} className="grid gap-1 rounded-md border border-line p-3">
                  <p className="flex flex-wrap items-center gap-2 text-xs text-fg-muted">
                    <span className="font-semibold text-fg">{actorName(u.author)}</span>
                    <time dateTime={u.posted_at}>{f.dateTime(u.posted_at)}</time>
                    <Badge tone="neutral">{statusNames.get(u.status_id) ?? "…"}</Badge>
                    <Badge tone={u.is_public ? "accent" : "muted"}>
                      {u.is_public ? t("status.publicBadge") : t("status.internal")}
                    </Badge>
                    {u.edited_at ? <span>({t("status.edited")})</span> : null}
                  </p>
                  {u.body ? <p className="whitespace-pre-wrap text-sm">{u.body}</p> : null}
                </li>
              ))}
            </ol>
          )}
        </Card>
        {can("service-status.update") ? <PostUpdate incident={incident} /> : null}
      </div>
      {rename ? <RenameDialog incident={incident} onClose={() => setRename(false)} /> : null}
    </div>
  );
}

function PostUpdate({ incident }: { incident: Schemas["ServiceIncidentDto"] }) {
  const t = useText();
  const statuses = useQuery(incidentStatusesQuery);
  const [statusId, setStatusId] = useState("");
  const [impacts, setImpacts] = useState<Impacts>({});
  const [body, setBody] = useState("");
  const [isPublic, setIsPublic] = useState(true);
  const changed = toImpacts(impacts);
  const post = useAction(
    () => {
      const merged = new Map(incident.services.map((s) => [s.service_id, s.impact_level_id]));
      for (const s of changed) merged.set(s.service_id, s.impact_level_id);
      return call(
        api.POST("/api/v1/service-status/incidents/{id}/updates", {
          params: { path: { id: incident.id } },
          body: {
            body,
            is_public: isPublic,
            status_id: statusId || null,
            services: changed.length
              ? [...merged].map(([service_id, impact_level_id]) => ({
                  service_id,
                  impact_level_id,
                }))
              : null,
          },
        }),
      );
    },
    {
      invalidate: [["service-status"]],
      success: "status.posted",
      onSuccess: () => {
        setBody("");
        setStatusId("");
        setImpacts({});
      },
    },
  );
  return (
    <Card title={t("status.postUpdate")}>
      <form
        className="grid gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          post.mutate();
        }}
      >
        <SelectField
          label={t("status.incidentStatus")}
          value={statusId}
          placeholder={t("status.keepStatus")}
          onChange={(e) => setStatusId(e.target.value)}
          options={toOptions(statuses.data, (s) => s.name)}
        />
        <TextAreaField
          label={t("status.message")}
          hint={t("status.messageHint")}
          rows={4}
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
        <details>
          <summary className="cursor-pointer text-sm font-medium">{t("status.affected")}</summary>
          <div className="mt-3">
            <ImpactEditor
              legend={t("status.affected")}
              keepLabel={t("status.keepImpact")}
              value={impacts}
              onChange={setImpacts}
            />
          </div>
        </details>
        <Checkbox
          label={t("status.public")}
          checked={isPublic}
          onChange={(e) => setIsPublic(e.target.checked)}
        />
        <ProblemNotice error={post.error} />
        <div>
          <Button type="submit" variant="primary" loading={post.isPending}>
            {t("status.post")}
          </Button>
        </div>
      </form>
    </Card>
  );
}

function RenameDialog({
  incident,
  onClose,
}: {
  incident: Schemas["ServiceIncidentDto"];
  onClose: () => void;
}) {
  const t = useText();
  const [title, setTitle] = useState(incident.title);
  const save = useAction(
    () =>
      call(
        api.PUT("/api/v1/service-status/incidents/{id}", {
          params: { path: { id: incident.id } },
          body: { title: title.trim() },
        }),
      ),
    { invalidate: [["service-status"]], success: "status.renamed", onSuccess: onClose },
  );
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("status.rename")}
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!title.trim()}
      size="sm"
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <TextField
        label={t("status.titleField")}
        required
        value={title}
        onChange={(e) => setTitle(e.target.value)}
      />
    </FormDialog>
  );
}
