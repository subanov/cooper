import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { runJob, setJobSchedule } from "@/features/administration/actions";
import { jobRunsQuery, jobsQuery } from "@/features/administration/queries";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

type Job = Schemas["JobDto"];

interface Schedule {
  kind?: string;
  interval_seconds?: number;
  daily_at?: string;
}

/** Background Jobs: schedules, latest runs and history (FR-SYS-002). */
export function JobsPage() {
  const t = useText();
  const jobs = useQuery(jobsQuery);
  if (jobs.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (jobs.isError) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  return (
    <div className="grid gap-3">
      <p className="text-muted-foreground">{t("admin.jobs.intro")}</p>
      {jobs.data.length === 0 ? (
        <p className="text-muted-foreground">{t("admin.jobs.empty")}</p>
      ) : (
        <ul className="grid gap-3">
          {jobs.data.map((job) => (
            <JobCard key={job.name} job={job} />
          ))}
        </ul>
      )}
    </div>
  );
}

function JobCard({ job }: { job: Job }) {
  const t = useText();
  const intl = useIntl();
  const queryClient = useQueryClient();
  const [showHistory, setShowHistory] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const when = (at: string | null | undefined) =>
    at ? intl.formatDate(at, { dateStyle: "medium", timeStyle: "short" }) : t("admin.jobs.never");

  async function run() {
    const outcome = await runJob(job.name);
    setNotice(
      outcome.ok
        ? t("admin.jobs.queued", { name: job.name })
        : t("admin.invalid", { detail: outcome.problem.detail ?? outcome.problem.kind }),
    );
    await queryClient.invalidateQueries({ queryKey: jobsQuery.queryKey });
  }

  return (
    <li className="grid gap-3 rounded-lg border border-border p-4">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="font-mono text-base font-semibold">{job.name}</h2>
        {job.last_outcome ? <OutcomeBadge outcome={job.last_outcome} /> : null}
        <Button className="ml-auto" size="sm" variant="outline" onClick={() => void run()}>
          {t("admin.jobs.runNow")}
        </Button>
      </div>
      <dl className="grid gap-1 text-sm sm:grid-cols-3">
        <div>
          <dt className="text-muted-foreground">{t("admin.jobs.schedule")}</dt>
          <dd>{scheduleSummary(job.schedule as Schedule, t)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t("admin.jobs.last")}</dt>
          <dd>{when(job.last_run_at)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t("admin.jobs.next")}</dt>
          <dd>{when(job.next_run_at)}</dd>
        </div>
      </dl>
      {notice ? (
        <p role="status" className="text-sm">
          {notice}
        </p>
      ) : null}
      <ScheduleForm job={job} />
      <div>
        <Button
          size="sm"
          variant="ghost"
          aria-expanded={showHistory}
          onClick={() => setShowHistory(!showHistory)}
        >
          {t("admin.jobs.history")}
        </Button>
        {showHistory ? <History name={job.name} /> : null}
      </div>
    </li>
  );
}

function ScheduleForm({ job }: { job: Job }) {
  const t = useText();
  const queryClient = useQueryClient();
  const current = job.schedule as Schedule;
  const [kind, setKind] = useState(current.kind ?? "off");
  const [seconds, setSeconds] = useState(String(current.interval_seconds ?? 300));
  const [dailyAt, setDailyAt] = useState(current.daily_at ?? "02:00");
  const [message, setMessage] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  async function save() {
    const body: Schemas["ScheduleDto"] =
      kind === "interval"
        ? { kind, interval_seconds: Number(seconds) }
        : kind === "daily"
          ? { kind, daily_at: dailyAt }
          : { kind };
    const outcome = await setJobSchedule(job.name, body);
    setFailed(!outcome.ok);
    setMessage(
      outcome.ok
        ? t("admin.saved")
        : t("admin.invalid", { detail: outcome.problem.detail ?? outcome.problem.kind }),
    );
    if (outcome.ok) {
      await queryClient.invalidateQueries({ queryKey: jobsQuery.queryKey });
    }
  }

  const kindId = `kind-${job.name}`;
  return (
    <form
      className="flex flex-wrap items-end gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <div className="grid gap-1.5">
        <Label htmlFor={kindId}>{t("admin.jobs.kind")}</Label>
        <Select id={kindId} value={kind} onChange={(e) => setKind(e.target.value)}>
          <option value="interval">{t("admin.jobs.kind.interval")}</option>
          <option value="daily">{t("admin.jobs.kind.daily")}</option>
          <option value="off">{t("admin.jobs.kind.off")}</option>
        </Select>
      </div>
      {kind === "interval" ? (
        <Field
          label={t("admin.jobs.intervalSeconds")}
          type="number"
          min={10}
          max={604800}
          value={seconds}
          onChange={(e) => setSeconds(e.target.value)}
        />
      ) : null}
      {kind === "daily" ? (
        <Field
          label={t("admin.jobs.dailyAt")}
          type="time"
          value={dailyAt}
          onChange={(e) => setDailyAt(e.target.value)}
        />
      ) : null}
      <Button type="submit" size="sm">
        {t("admin.save")}
      </Button>
      {message ? (
        <span role={failed ? "alert" : "status"} className="text-sm">
          {message}
        </span>
      ) : null}
    </form>
  );
}

function History({ name }: { name: string }) {
  const t = useText();
  const intl = useIntl();
  const runs = useQuery(jobRunsQuery(name));
  if (runs.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (runs.isError) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  if (runs.data.items.length === 0) {
    return <p className="text-sm text-muted-foreground">{t("admin.jobs.noRuns")}</p>;
  }
  return (
    <ul className="mt-2 grid gap-1 text-sm">
      {runs.data.items.map((run) => (
        <li key={run.id} className="flex flex-wrap items-center gap-2">
          <OutcomeBadge outcome={run.outcome} />
          <span>
            {intl.formatDate(run.started_at, { dateStyle: "medium", timeStyle: "medium" })}
          </span>
          {run.duration_ms != null ? (
            <span className="text-muted-foreground">
              {t("admin.jobs.duration", { ms: run.duration_ms })}
            </span>
          ) : null}
          {run.message ? <span className="text-muted-foreground">{run.message}</span> : null}
        </li>
      ))}
    </ul>
  );
}

const OUTCOMES = {
  running: ["neutral", "admin.jobs.outcome.running"],
  succeeded: ["ok", "admin.jobs.outcome.succeeded"],
  failed: ["breached", "admin.jobs.outcome.failed"],
  abandoned: ["warning", "admin.jobs.outcome.abandoned"],
} as const;

function OutcomeBadge({ outcome }: { outcome: string }) {
  const t = useText();
  const known = OUTCOMES[outcome as keyof typeof OUTCOMES];
  return known ? <Badge tone={known[0]}>{t(known[1])}</Badge> : <Badge>{outcome}</Badge>;
}

function scheduleSummary(schedule: Schedule, t: ReturnType<typeof useText>): string {
  const id = `admin.jobs.scheduleSummary.${schedule.kind ?? "off"}` as MessageId;
  return t(id, {
    seconds: schedule.interval_seconds ?? 0,
    time: schedule.daily_at ?? "",
  });
}
