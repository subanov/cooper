import { useQuery } from "@tanstack/react-query";
import { History, Pencil, Play } from "lucide-react";
import { useState } from "react";
import { Badge, type Tone } from "@/components/Badge";
import { IconButton } from "@/components/Button";
import { Card } from "@/components/Card";
import { DataTable } from "@/components/DataTable";
import { Dialog, FormDialog } from "@/components/Dialog";
import { EmptyState } from "@/components/EmptyState";
import { SelectField, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import { QueryState } from "@/components/QueryState";
import { useAction } from "@/features/shared/mutation";
import type { MessageId } from "@/i18n/messages";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

type Job = Schemas["JobDto"];
type Schedule = { kind?: string; interval_seconds?: number | null; daily_at?: string | null };

const OUTCOME_TONE: Record<string, Tone> = {
  running: "accent",
  succeeded: "success",
  failed: "danger",
  abandoned: "warning",
};

function OutcomeBadge({ outcome }: { outcome: string }) {
  const t = useText();
  const known = ["running", "succeeded", "failed", "abandoned"].includes(outcome);
  return (
    <Badge tone={OUTCOME_TONE[outcome] ?? "muted"}>
      {known ? t(`admin.jobs.outcome.${outcome}` as MessageId) : outcome}
    </Badge>
  );
}

/** Background Jobs: schedules, latest runs, Run now and history (FR-SYS-006). */
export function JobsPage() {
  const t = useText();
  const f = useFormat();
  const jobs = useQuery({
    queryKey: ["administration", "jobs"],
    queryFn: () => call(api.GET("/api/v1/background-jobs")),
    refetchInterval: 15_000,
  });
  const [editing, setEditing] = useState<Job | null>(null);
  const [history, setHistory] = useState<Job | null>(null);
  const run = useAction(
    (name: string) =>
      call(api.POST("/api/v1/background-jobs/{name}/run", { params: { path: { name } } })),
    {
      invalidate: [["administration", "jobs"]],
      success: (_, name) => t("admin.jobs.queued", { name }),
    },
  );
  const summary = (s: Schedule) =>
    s.kind === "interval"
      ? t("admin.jobs.summary.interval", { seconds: s.interval_seconds ?? 0 })
      : s.kind === "daily"
        ? t("admin.jobs.summary.daily", { time: s.daily_at ?? "" })
        : t("admin.jobs.summary.off");
  return (
    <>
      <SectionHeader title={t("admin.jobs")} description={t("admin.jobs.intro")} />
      <ProblemNotice error={run.error} />
      <QueryState query={jobs}>
        {(list) => (
          <Card flush>
            <DataTable
              caption={t("admin.jobs")}
              rows={list}
              rowKey={(j) => j.name}
              empty={<EmptyState title={t("admin.jobs.empty")} />}
              columns={[
                {
                  id: "name",
                  header: t("admin.jobs.name"),
                  rowHeader: true,
                  cell: (j) => <span className="font-mono text-[0.8125rem]">{j.name}</span>,
                },
                {
                  id: "schedule",
                  header: t("admin.jobs.schedule"),
                  cell: (j) => summary(j.schedule as Schedule),
                },
                {
                  id: "last",
                  header: t("admin.jobs.last"),
                  cell: (j) =>
                    j.last_run_at ? (
                      <span className="flex flex-wrap items-center gap-1.5">
                        {j.last_outcome ? <OutcomeBadge outcome={j.last_outcome} /> : null}
                        <span className="text-xs text-fg-muted">
                          {f.relative(j.last_run_at)}
                          {j.last_duration_ms != null
                            ? ` · ${t("admin.jobs.duration", { ms: j.last_duration_ms })}`
                            : ""}
                        </span>
                      </span>
                    ) : (
                      t("admin.jobs.never")
                    ),
                },
                {
                  id: "next",
                  header: t("admin.jobs.next"),
                  wide: true,
                  cell: (j) => (j.next_run_at ? f.dateTime(j.next_run_at) : t("common.noneDash")),
                },
                {
                  id: "actions",
                  header: <span className="sr-only">{t("common.actions")}</span>,
                  className: "whitespace-nowrap text-right",
                  cell: (j) => (
                    <span className="inline-flex gap-1">
                      <IconButton
                        size="sm"
                        tooltip="top"
                        label={t("admin.jobs.runNamed", { name: j.name })}
                        icon={<Play aria-hidden="true" className="size-3.5" />}
                        loading={run.isPending && run.variables === j.name}
                        onClick={() => run.mutate(j.name)}
                      />
                      <IconButton
                        size="sm"
                        tooltip="top"
                        label={t("admin.jobs.editNamed", { name: j.name })}
                        icon={<Pencil aria-hidden="true" className="size-3.5" />}
                        onClick={() => setEditing(j)}
                      />
                      <IconButton
                        size="sm"
                        tooltip="top"
                        label={t("admin.jobs.historyNamed", { name: j.name })}
                        icon={<History aria-hidden="true" className="size-3.5" />}
                        onClick={() => setHistory(j)}
                      />
                    </span>
                  ),
                },
              ]}
            />
          </Card>
        )}
      </QueryState>
      {editing ? <ScheduleDialog job={editing} onClose={() => setEditing(null)} /> : null}
      {history ? <RunsDialog job={history} onClose={() => setHistory(null)} /> : null}
    </>
  );
}

function ScheduleDialog({ job, onClose }: { job: Job; onClose: () => void }) {
  const t = useText();
  const current = job.schedule as Schedule;
  const [kind, setKind] = useState(current.kind ?? "off");
  const [seconds, setSeconds] = useState(String(current.interval_seconds ?? 300));
  const [time, setTime] = useState(current.daily_at ?? "02:00");
  const save = useAction(
    () =>
      call(
        api.PUT("/api/v1/background-jobs/{name}/schedule", {
          params: { path: { name: job.name } },
          body: {
            kind,
            interval_seconds: kind === "interval" ? Number(seconds) : null,
            daily_at: kind === "daily" ? time : null,
          },
        }),
      ),
    { invalidate: [["administration", "jobs"]], success: "admin.jobs.saved", onSuccess: onClose },
  );
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("admin.jobs.editNamed", { name: job.name })}
      submitLabel={t("common.save")}
      busy={save.isPending}
      size="sm"
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <SelectField
        label={t("admin.jobs.kind")}
        value={kind}
        onChange={(e) => setKind(e.target.value)}
        options={(["interval", "daily", "off"] as const).map((k) => ({
          value: k,
          label: t(`admin.jobs.kind.${k}`),
        }))}
      />
      {kind === "interval" ? (
        <TextField
          label={t("admin.jobs.intervalSeconds")}
          type="number"
          min={1}
          value={seconds}
          onChange={(e) => setSeconds(e.target.value)}
        />
      ) : null}
      {kind === "daily" ? (
        <TextField
          label={t("admin.jobs.dailyAt")}
          type="time"
          value={time}
          onChange={(e) => setTime(e.target.value)}
        />
      ) : null}
    </FormDialog>
  );
}

function RunsDialog({ job, onClose }: { job: Job; onClose: () => void }) {
  const t = useText();
  const f = useFormat();
  const runs = useQuery({
    queryKey: ["administration", "jobs", job.name, "runs"],
    queryFn: () =>
      call(
        api.GET("/api/v1/background-jobs/{name}/runs", {
          params: { path: { name: job.name }, query: { limit: 50 } },
        }),
      ),
  });
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("admin.jobs.historyNamed", { name: job.name })}
      size="lg"
    >
      <QueryState query={runs}>
        {(page) => (
          <DataTable
            caption={t("admin.jobs.history")}
            rows={page.items}
            rowKey={(r) => r.id}
            empty={<p className="py-4 text-sm text-fg-muted">{t("admin.jobs.noRuns")}</p>}
            columns={[
              {
                id: "started",
                header: t("admin.jobs.started"),
                cell: (r) => f.dateTime(r.started_at),
              },
              {
                id: "outcome",
                header: t("admin.jobs.outcome"),
                cell: (r) => <OutcomeBadge outcome={r.outcome} />,
              },
              {
                id: "duration",
                header: t("admin.jobs.durationHeader"),
                cell: (r) =>
                  r.duration_ms != null
                    ? t("admin.jobs.duration", { ms: r.duration_ms })
                    : t("common.noneDash"),
              },
              {
                id: "message",
                header: t("admin.jobs.message"),
                cell: (r) => <span className="[overflow-wrap:anywhere]">{r.message ?? ""}</span>,
              },
            ]}
          />
        )}
      </QueryState>
    </Dialog>
  );
}
