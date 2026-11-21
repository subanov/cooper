import { useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { CheckCircle2, Plus, Siren } from "lucide-react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { EmptyState, LoadingBlock, ProblemNotice } from "@/components/Feedback";
import {
  ListPane,
  PickOne,
  RecordRow,
  rowLinkClass,
  SplitView,
  useDetailOpen,
  useListKeys,
  useOpenId,
} from "@/components/MasterDetail";
import { Segmented } from "@/components/Tabs";
import { namesById } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import { incidentStatusesQuery, incidentsQuery, servicesQuery } from "./api";

/** Service Incidents beside the open one; open ones by default (FR-SST-002). */
export function IncidentsPage({ all }: { all: boolean }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const navigate = useNavigate();
  const list = useQuery(incidentsQuery(!all));
  const statuses = useQuery(incidentStatusesQuery);
  const services = useQuery(servicesQuery);
  const serviceNames = namesById(services.data);
  const rows = list.data?.items ?? [];
  const openId = useOpenId();
  const detailOpen = useDetailOpen("/service-status/incidents");
  const search = all ? { all: true as const } : {};
  useListKeys(
    rows.map((i) => i.id),
    openId,
    (id) => void navigate({ to: "/service-status/incidents/$id", params: { id }, search }),
  );
  return (
    <SplitView
      detailOpen={detailOpen}
      placeholder={<PickOne icon={<Siren />} title={t("status.pickOne")} />}
      list={
        <ListPane
          level={2}
          title={t("status.incidents")}
          label={t("status.list")}
          actions={
            can("service-status.update") ? (
              <Link
                to="/service-status/incidents/new"
                search={search}
                className={buttonClass({ variant: "primary", size: "sm" })}
              >
                <Plus aria-hidden="true" />
                {t("status.openIncident")}
              </Link>
            ) : undefined
          }
          filters={
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
          }
        >
          {list.isError ? (
            <ProblemNotice error={list.error} className="m-4" onRetry={() => void list.refetch()} />
          ) : null}
          {list.isPending ? <LoadingBlock className="p-4" lines={5} /> : null}
          {list.isSuccess && rows.length === 0 ? (
            <EmptyState
              icon={<CheckCircle2 aria-hidden="true" />}
              title={all ? t("status.noIncidentsAll") : t("status.noIncidents")}
            />
          ) : null}
          {rows.length > 0 ? (
            <ul aria-label={t("status.list")}>
              {rows.map((i) => (
                <RecordRow key={i.id} selected={i.id === openId}>
                  <div className="flex items-center justify-between gap-2">
                    <Link
                      to="/service-status/incidents/$id"
                      params={{ id: i.id }}
                      search={search}
                      aria-current={i.id === openId ? "page" : undefined}
                      className={rowLinkClass}
                    >
                      {i.title}
                    </Link>
                    <Badge tone={i.is_resolved ? "muted" : "warning"}>
                      {namesById(statuses.data).get(i.status_id) ?? "…"}
                    </Badge>
                  </div>
                  <p className="truncate text-xs text-fg-muted">
                    {i.services.map((s) => serviceNames.get(s.service_id) ?? "…").join(", ")}
                  </p>
                  <p className="text-xs text-fg-muted">
                    {t("status.started")} {f.dateTime(i.started_at)}
                    {i.resolved_at ? ` · ${t("status.resolved")} ${f.dateTime(i.resolved_at)}` : ""}
                  </p>
                </RecordRow>
              ))}
            </ul>
          ) : null}
        </ListPane>
      }
    >
      <Outlet />
    </SplitView>
  );
}
