import { useQuery } from "@tanstack/react-query";
import { Badge, ColourLabel } from "@/components/Badge";
import { LoadingBlock, QueryState } from "@/components/Feedback";
import { PageHeader, Panel } from "@/components/Layout";
import type { MessageId } from "@/i18n/messages";
import { useFormat, useText } from "@/i18n/useText";
import { portalStatusQuery } from "./api";

/** Service Status on the Portal (FR-PRT-006): services, incidents and planned maintenance. */
export function PortalStatusPage() {
  const t = useText();
  const f = useFormat();
  const status = useQuery(portalStatusQuery);
  return (
    <div className="grid gap-6">
      <PageHeader title={t("portalStatus.title")} description={t("portalStatus.intro")} />
      <QueryState query={status} loading={<LoadingBlock lines={6} />}>
        {(s) => {
          const incidents = s.incidents.filter((i) => !i.is_resolved);
          return (
            <>
              <Panel title={t("portalStatus.services")} flush>
                <ul className="divide-y divide-line">
                  {s.services.map((svc) => (
                    <li
                      key={svc.id}
                      className="flex flex-wrap items-center justify-between gap-3 px-4 py-3"
                    >
                      <span className="grid">
                        <span className="font-medium">{svc.name}</span>
                        {svc.description ? (
                          <span className="text-xs text-fg-muted">{svc.description}</span>
                        ) : null}
                      </span>
                      <ColourLabel colour={svc.status.colour} className="text-sm font-medium">
                        {svc.status.name}
                      </ColourLabel>
                    </li>
                  ))}
                </ul>
              </Panel>
              <Panel title={t("portalStatus.incidents")}>
                {incidents.length === 0 ? (
                  <p className="text-sm text-fg-muted">{t("portalStatus.noIncidents")}</p>
                ) : (
                  <ul className="grid gap-4">
                    {incidents.map((i) => (
                      <li key={i.id} className="grid gap-2">
                        <div className="flex flex-wrap items-center gap-2">
                          <h3 className="font-semibold">{i.title}</h3>
                          <Badge tone="warning">{i.status}</Badge>
                        </div>
                        <p className="text-xs text-fg-muted">
                          {i.services.map((x) => `${x.service}: ${x.impact.name}`).join(" · ")}
                        </p>
                        <ol className="grid gap-2 border-l-2 border-line pl-3">
                          {i.updates.map((u) => (
                            <li key={u.posted_at} className="grid gap-0.5 text-sm">
                              <span className="text-xs text-fg-muted">
                                {f.dateTime(u.posted_at)} · {u.status}
                              </span>
                              <span className="whitespace-pre-wrap">{u.body}</span>
                            </li>
                          ))}
                        </ol>
                      </li>
                    ))}
                  </ul>
                )}
              </Panel>
              <Panel title={t("portalStatus.maintenance")}>
                {s.planned_maintenance.length === 0 ? (
                  <p className="text-sm text-fg-muted">{t("portalStatus.noMaintenance")}</p>
                ) : (
                  <ul className="grid gap-3">
                    {s.planned_maintenance.map((m) => (
                      <li key={m.id} className="grid gap-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <h3 className="font-semibold">{m.title}</h3>
                          <Badge tone={m.state === "started" ? "warning" : "neutral"}>
                            {["scheduled", "started", "finished", "cancelled"].includes(m.state)
                              ? t(`portalStatus.state.${m.state}` as MessageId)
                              : m.state}
                          </Badge>
                        </div>
                        <p className="text-sm">
                          {t("portalStatus.window", {
                            start: f.dateTime(m.planned_start),
                            end: f.dateTime(m.planned_end),
                          })}
                        </p>
                        {m.description ? (
                          <p className="text-sm text-fg-muted">{m.description}</p>
                        ) : null}
                      </li>
                    ))}
                  </ul>
                )}
              </Panel>
            </>
          );
        }}
      </QueryState>
    </div>
  );
}
