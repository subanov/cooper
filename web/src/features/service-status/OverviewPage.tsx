import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Activity, CheckCircle2 } from "lucide-react";
import { ColourLabel } from "@/components/Badge";
import { Card } from "@/components/Card";
import { EmptyState } from "@/components/EmptyState";
import { Notice } from "@/components/Notice";
import { QueryState } from "@/components/QueryState";
import { useFormat, useText } from "@/i18n/useText";
import { boardQuery, incidentsQuery } from "./api";

/** The board (FR-SST-006): each active Service with its Current Status, worst first. */
export function OverviewPage() {
  const t = useText();
  const f = useFormat();
  const board = useQuery(boardQuery);
  const open = useQuery(incidentsQuery(true));
  return (
    <div className="grid gap-5">
      <QueryState query={board}>
        {(rows) =>
          rows.length === 0 ? (
            <EmptyState icon={<Activity aria-hidden="true" />} title={t("status.noServices")}>
              {t("status.noServicesHint")}
            </EmptyState>
          ) : (
            <>
              {rows.every((r) => r.status.is_default) ? (
                <Notice tone="success" live={false}>
                  {t("status.allGood")}
                </Notice>
              ) : null}
              <Card flush>
                <ul className="divide-y divide-line">
                  {[...rows]
                    .sort(
                      (a, b) =>
                        a.status.severity_order - b.status.severity_order ||
                        a.service.sort_order - b.service.sort_order,
                    )
                    .map((row) => (
                      <li
                        key={row.service.id}
                        className="flex flex-wrap items-center justify-between gap-3 px-4 py-3"
                      >
                        <div className="grid min-w-0">
                          <span className="font-medium">{row.service.name}</span>
                          {row.service.description ? (
                            <span className="text-xs text-fg-muted">{row.service.description}</span>
                          ) : null}
                        </div>
                        <ColourLabel colour={row.status.colour} className="text-sm font-medium">
                          {row.status.name}
                        </ColourLabel>
                      </li>
                    ))}
                </ul>
              </Card>
            </>
          )
        }
      </QueryState>
      {(open.data?.items ?? []).length > 0 ? (
        <Card title={t("status.openIncidents")} flush>
          <ul className="divide-y divide-line">
            {(open.data?.items ?? []).map((i) => (
              <li key={i.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <Link
                  to="/service-status/incidents/$id"
                  params={{ id: i.id }}
                  className="flex-1 font-medium hover:underline"
                >
                  {i.title}
                </Link>
                <span className="text-xs text-fg-muted">
                  {t("status.started")} {f.dateTime(i.started_at)}
                </span>
              </li>
            ))}
          </ul>
        </Card>
      ) : open.isSuccess ? (
        <p className="flex items-center gap-2 text-sm text-fg-muted">
          <CheckCircle2 aria-hidden="true" className="size-4 text-success-text" />
          {t("status.noIncidents")}
        </p>
      ) : null}
    </div>
  );
}
