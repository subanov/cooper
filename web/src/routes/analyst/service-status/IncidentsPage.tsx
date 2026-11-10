import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { can } from "@/features/navigation/navigation";
import { incidentStatusesQuery, incidentsQuery } from "@/features/service-status/api";
import { analystSessionQuery } from "@/features/session/queries";
import { namesById } from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";

/** Incidents, open ones by default. */
export function IncidentsPage() {
  const t = useText();
  const intl = useIntl();
  const session = useQuery(analystSessionQuery).data;
  const canOpen = session ? can(session, "service-status.update") : false;
  const [open, setOpen] = useState(true);
  const incidents = useQuery(incidentsQuery(open));
  const statuses = useQuery(incidentStatusesQuery);
  const statusName = namesById(statuses.data, (s) => s.name);
  return (
    <div className="grid gap-3">
      <div className="flex items-center gap-3">
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={open} onChange={(e) => setOpen(e.target.checked)} />
          {t("status.openOnly")}
        </label>
        {canOpen ? (
          <Button asChild size="sm" className="ml-auto">
            <Link to="/service-status/incidents/new">{t("status.openIncident")}</Link>
          </Button>
        ) : null}
      </div>
      {incidents.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {incidents.isPending ? <p>{t("common.loading")}</p> : null}
      {incidents.isSuccess && incidents.data.items.length === 0 ? (
        <p className="text-muted-foreground">{t("status.noIncidents")}</p>
      ) : null}
      <ul className="grid gap-2">
        {(incidents.data?.items ?? []).map((i) => (
          <li
            key={i.id}
            className="flex flex-wrap items-center gap-2 rounded-lg border border-border p-3"
          >
            <Link
              to="/service-status/incidents/$id"
              params={{ id: i.id }}
              className="font-medium underline-offset-2 hover:underline"
            >
              {i.title}
            </Link>
            <Badge tone={i.is_resolved ? "ok" : "warning"}>
              {statusName.get(i.status_id) ?? ""}
            </Badge>
            <span className="ml-auto text-xs text-muted-foreground">
              {intl.formatDate(i.started_at, { dateStyle: "medium", timeStyle: "short" })}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
