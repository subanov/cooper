-- Background Jobs with schedules and run history (FR-SYS-002).
-- Instance-wide: not tenant-scoped.
CREATE TABLE background_jobs (
    name             text PRIMARY KEY,
    schedule_kind    text        NOT NULL CHECK (schedule_kind IN ('interval', 'daily', 'off')),
    interval_seconds integer     CHECK (interval_seconds > 0),
    -- Time of day in UTC for daily jobs.
    daily_at         time,
    next_run_at      timestamptz,
    last_run_at      timestamptz,
    created_at       timestamptz NOT NULL,
    updated_at       timestamptz NOT NULL,
    CONSTRAINT background_jobs_interval_set CHECK ((schedule_kind = 'interval') = (interval_seconds IS NOT NULL)),
    CONSTRAINT background_jobs_daily_set CHECK ((schedule_kind = 'daily') = (daily_at IS NOT NULL))
);

COMMENT ON TABLE background_jobs IS
    'Registered background jobs and their schedules. Administrators change schedules at runtime.';

CREATE TABLE background_job_runs (
    id          uuid PRIMARY KEY,
    job_name    text        NOT NULL REFERENCES background_jobs (name) ON DELETE CASCADE,
    worker_id   text        NOT NULL,
    started_at  timestamptz NOT NULL,
    finished_at timestamptz,
    -- `abandoned`: the worker stopped (crash, kill) before the run finished.
    outcome     text        NOT NULL CHECK (outcome IN ('running', 'succeeded', 'failed', 'abandoned')),
    duration_ms bigint      CHECK (duration_ms >= 0),
    message     text,
    CONSTRAINT background_job_runs_finished_when_done CHECK ((outcome = 'running') = (finished_at IS NULL))
);

COMMENT ON TABLE background_job_runs IS 'Run history of background jobs: outcome and duration.';

CREATE INDEX background_job_runs_job_started_idx ON background_job_runs (job_name, started_at DESC);
