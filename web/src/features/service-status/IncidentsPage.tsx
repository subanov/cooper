import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { CheckCircle2, Plus } from "lucide-react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { Card } from "@/components/Card";
import { DataTable, rowLinkClass } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import { Segmented } from "@/components/Tabs";
import { namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import { incidentStatusesQuery, incidentsQuery, servicesQuery } from "./api";

/** Service Incidents, newest first; open ones by default (FR-SST-002). */
export function IncidentsPage({ all }: { all: boolean }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const navigate = useNavigate();
  const list = useQuery(incidentsQuery(!all));
  const statuses = useQuery(incidentStatusesQuery);
  const services = useQuery(servicesQuery);
  const serviceNames = namesById(services.data);
  return (
    <div className="grid gap-4">
      <SectionHeader
        title={t("status.incidents")}
        actions={
          can("service-status.update") ? (
            <Link
              to="/service-status/incidents/new"
              className={buttonClass({ variant: "primary" })}
            >
              <Plus aria-hidden="true" className="size-4" />
              {t("status.openIncident")}
            </Link>
          ) : undefined
        }
      />
      <Segmented
        label={t("status.show")}
        value={all ? "all" : "open"}
        onChange={(v) =>
          void navigate({
            to: "/service-status/incidents",
            search: v === "all" ? { all: true } : {},
          })
        }
        options={[
          { value: "open", label: t("status.openOnly") },
          { value: "all", label: t("status.all") },
        ]}
      />
      <ProblemNotice error={list.error} />
      <Card flush>
        <DataTable
          caption={t("status.list")}
          rows={list.data?.items}
          loading={list.isPending}
          rowKey={(i) => i.id}
          empty={
            <EmptyState
              icon={<CheckCircle2 aria-hidden="true" />}
              title={all ? t("status.noIncidentsAll") : t("status.noIncidents")}
            />
          }
          columns={[
            {
              id: "title",
              header: t("status.titleField"),
              rowHeader: true,
              cell: (i) => (
                <Link
                  to="/service-status/incidents/$id"
                  params={{ id: i.id }}
                  className={rowLinkClass}
                >
                  {i.title}
                </Link>
              ),
            },
            {
              id: "status",
              header: t("status.incidentStatus"),
              cell: (i) => (
                <Badge tone={i.is_resolved ? "muted" : "warning"}>
                  {namesById(statuses.data).get(i.status_id) ?? "…"}
                </Badge>
              ),
            },
            {
              id: "services",
              header: t("status.affected"),
              wide: true,
              cell: (i) => i.services.map((s) => serviceNames.get(s.service_id) ?? "…").join(", "),
            },
            { id: "started", header: t("status.started"), cell: (i) => f.dateTime(i.started_at) },
            {
              id: "resolved",
              header: t("status.resolved"),
              wide: true,
              cell: (i) => (i.resolved_at ? f.dateTime(i.resolved_at) : t("common.noneDash")),
            },
          ]}
        />
      </Card>
    </div>
  );
}
