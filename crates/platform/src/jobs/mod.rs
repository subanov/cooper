//! Background Jobs (FR-SYS-002).
//!
//! A [`Job`] is registered with a [`JobRunner`]. The runner stores each job's
//! [`Schedule`] in `background_jobs` (Administrators may change it at runtime), runs due
//! jobs, and writes `background_job_runs` with outcome and duration.
//!
//! **One worker at a time:** before running a job the runner takes a PostgreSQL session
//! advisory lock derived from the job name on a dedicated connection. Other workers fail
//! to take the lock and skip the job. If a worker dies, PostgreSQL releases the lock with
//! the connection; the next run marks the dead worker's run as `abandoned`.

pub mod admin;
mod prune;
mod schedule;

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, NaiveTime, Utc};
use cooper_kernel::Clock;
use sqlx::{PgConnection, PgPool};
use thiserror::Error;
use uuid::Uuid;

use crate::metrics::names;

pub use prune::PruneJobRuns;
pub use schedule::{Schedule, ScheduleError};

/// What a job hands to [`Job::run`].
#[derive(Clone)]
pub struct JobContext {
    /// Database pool. Jobs that touch tenant data open their own
    /// [`tenancy::begin`](crate::tenancy::begin) transaction with an explicit scope.
    pub pool: PgPool,
    /// Time source.
    pub clock: Arc<dyn Clock>,
}

/// A job failed.
pub type JobError = Box<dyn std::error::Error + Send + Sync>;

/// A unit of background work, for example mailbox polling or the SLA clock.
#[async_trait]
pub trait Job: Send + Sync {
    /// Unique, stable, kebab-case name, for example `mailbox-poll`.
    fn name(&self) -> &'static str;

    /// Schedule used when the job is first registered. Afterwards the stored schedule
    /// wins, so Administrators' changes survive restarts.
    fn default_schedule(&self) -> Schedule;

    /// Does the work. The returned message is stored in the run history.
    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError>;
}

/// The runner could not talk to the database.
#[derive(Debug, Error)]
pub enum RunnerError {
    /// Database error.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// A stored schedule is invalid.
    #[error("job {job}: {source}")]
    Schedule {
        /// Job name.
        job: String,
        /// What is wrong.
        source: ScheduleError,
    },
    /// The job is not registered with this runner.
    #[error("job {0} is not registered")]
    UnknownJob(String),
}

/// Outcome of trying to run one job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// Another worker holds the job's lock.
    Locked,
    /// The job is turned off or not due yet.
    NotDue,
    /// The job ran and succeeded.
    Succeeded,
    /// The job ran and failed with this message.
    Failed(String),
}

/// Runs registered jobs on schedule.
pub struct JobRunner {
    ctx: JobContext,
    worker_id: String,
    jobs: Vec<Arc<dyn Job>>,
}

impl JobRunner {
    /// A runner with no jobs. `worker_id` identifies this process in run history.
    #[must_use]
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, worker_id: impl Into<String>) -> Self {
        Self {
            ctx: JobContext { pool, clock },
            worker_id: worker_id.into(),
            jobs: Vec::new(),
        }
    }

    /// Registers a job.
    #[must_use]
    pub fn with_job(mut self, job: Arc<dyn Job>) -> Self {
        self.jobs.push(job);
        self
    }

    /// Names of the registered jobs.
    pub fn job_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.jobs.iter().map(|j| j.name())
    }

    /// Inserts a `background_jobs` row for every registered job that has none yet, with
    /// its default schedule. Existing rows (and Administrators' changes) are kept.
    ///
    /// # Errors
    ///
    /// Fails on database errors.
    pub async fn register_all(&self) -> Result<(), RunnerError> {
        let now = self.ctx.clock.now();
        for job in &self.jobs {
            let schedule = job.default_schedule();
            let (kind, interval_seconds, daily_at) = schedule.to_columns();
            sqlx::query!(
                r#"
                INSERT INTO background_jobs
                    (name, schedule_kind, interval_seconds, daily_at, next_run_at, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $6)
                ON CONFLICT (name) DO NOTHING
                "#,
                job.name(),
                kind,
                interval_seconds,
                daily_at,
                schedule.first_run(now),
                now,
            )
            .execute(&self.ctx.pool)
            .await?;
        }
        Ok(())
    }

    /// Runs the named job if it is due and no other worker is running it.
    ///
    /// # Errors
    ///
    /// Fails on database errors or an unknown job name. A failing job is not an error:
    /// it is reported as [`RunOutcome::Failed`] and stored in the run history.
    pub async fn run_if_due(&self, name: &str) -> Result<RunOutcome, RunnerError> {
        let job = self
            .jobs
            .iter()
            .find(|j| j.name() == name)
            .ok_or_else(|| RunnerError::UnknownJob(name.to_owned()))?;

        let mut conn = self.ctx.pool.acquire().await?;
        let key = lock_key(job.name());
        let locked = sqlx::query_scalar!(r#"SELECT pg_try_advisory_lock($1) AS "locked!""#, key)
            .fetch_one(&mut *conn)
            .await?;
        if !locked {
            return Ok(RunOutcome::Locked);
        }
        let result = self.run_locked(&mut conn, job.as_ref()).await;
        let unlocked = sqlx::query_scalar!("SELECT pg_advisory_unlock($1)", key)
            .fetch_one(&mut *conn)
            .await;
        if let Err(e) = unlocked {
            // The lock dies with the connection; make sure it is not returned to the pool.
            tracing::error!(job = job.name(), error = %e, "failed to release job lock");
            conn.detach();
        }
        result
    }

    async fn run_locked(
        &self,
        conn: &mut PgConnection,
        job: &dyn Job,
    ) -> Result<RunOutcome, RunnerError> {
        let now = self.ctx.clock.now();
        let row = sqlx::query!(
            "SELECT schedule_kind, interval_seconds, daily_at, next_run_at FROM background_jobs WHERE name = $1",
            job.name(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        let Some(row) = row else {
            return Ok(RunOutcome::NotDue);
        };
        let schedule =
            Schedule::from_columns(&row.schedule_kind, row.interval_seconds, row.daily_at)
                .map_err(|source| RunnerError::Schedule {
                    job: job.name().to_owned(),
                    source,
                })?;
        // An `off` job has no next run, unless an Administrator asked for one run now.
        let due = match (&schedule, row.next_run_at) {
            (_, Some(at)) => at <= now,
            (Schedule::Off, None) => false,
            (_, None) => true,
        };
        if !due {
            return Ok(RunOutcome::NotDue);
        }

        // We hold the lock, so any run still marked `running` belongs to a dead worker.
        sqlx::query!(
            r#"
            UPDATE background_job_runs
            SET outcome = 'abandoned', finished_at = $2, message = 'worker stopped before the run finished'
            WHERE job_name = $1 AND outcome = 'running'
            "#,
            job.name(),
            now,
        )
        .execute(&mut *conn)
        .await?;

        let run_id = Uuid::now_v7();
        sqlx::query!(
            r#"
            INSERT INTO background_job_runs (id, job_name, worker_id, started_at, outcome)
            VALUES ($1, $2, $3, $4, 'running')
            "#,
            run_id,
            job.name(),
            self.worker_id,
            now,
        )
        .execute(&mut *conn)
        .await?;
        sqlx::query!(
            "UPDATE background_jobs SET next_run_at = $2, last_run_at = $3, updated_at = $3 WHERE name = $1",
            job.name(),
            schedule.next_after(now),
            now,
        )
        .execute(&mut *conn)
        .await?;

        let started = Instant::now();
        let result = run_catching_panics(job, &self.ctx).await;
        let elapsed = started.elapsed();
        let duration_ms = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
        let finished_at = self.ctx.clock.now().max(now);

        let (outcome, message, run_outcome) = match result {
            Ok(message) => ("succeeded", message, RunOutcome::Succeeded),
            Err(error) => {
                tracing::warn!(job = job.name(), %error, "background job failed");
                ("failed", Some(error.clone()), RunOutcome::Failed(error))
            }
        };
        metrics::counter!(names::JOB_RUNS, "job" => job.name(), "outcome" => outcome).increment(1);
        metrics::histogram!(names::JOB_DURATION, "job" => job.name()).record(elapsed.as_secs_f64());
        if matches!(run_outcome, RunOutcome::Succeeded) {
            #[allow(clippy::cast_precision_loss)] // Unix seconds fit an f64 exactly until 2^53
            metrics::gauge!(names::JOB_LAST_SUCCESS, "job" => job.name())
                .set(finished_at.timestamp() as f64);
        }
        sqlx::query!(
            r#"
            UPDATE background_job_runs
            SET outcome = $2, finished_at = $3, duration_ms = $4, message = $5
            WHERE id = $1
            "#,
            run_id,
            outcome,
            finished_at,
            duration_ms,
            message,
        )
        .execute(&mut *conn)
        .await?;
        Ok(run_outcome)
    }
}

/// Runs the job, recording a panic as a failed run instead of unwinding through the
/// runner. Jobs must not panic (clippy denies it); this guards against dependencies that do.
async fn run_catching_panics(job: &dyn Job, ctx: &JobContext) -> Result<Option<String>, String> {
    let future = std::panic::AssertUnwindSafe(job.run(ctx));
    match futures_catch_unwind(future).await {
        Ok(Ok(message)) => Ok(message),
        Ok(Err(error)) => Err(error.to_string()),
        Err(()) => Err("job panicked".to_owned()),
    }
}

/// Minimal `catch_unwind` for futures, to avoid a dependency on the `futures` crate.
async fn futures_catch_unwind<F: std::future::Future>(
    future: std::panic::AssertUnwindSafe<F>,
) -> Result<F::Output, ()> {
    use std::pin::pin;
    use std::task::Poll;

    let mut future = pin!(future.0);
    std::future::poll_fn(|cx| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| future.as_mut().poll(cx))) {
            Ok(Poll::Ready(output)) => Poll::Ready(Ok(output)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(_) => Poll::Ready(Err(())),
        }
    })
    .await
}

/// Advisory lock key for a job: FNV-1a 64 of `cooper.job.<name>`, stable across builds.
#[must_use]
pub fn lock_key(job_name: &str) -> i64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let hash = "cooper.job."
        .bytes()
        .chain(job_name.bytes())
        .fold(OFFSET, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(PRIME)
        });
    i64::from_ne_bytes(hash.to_ne_bytes())
}

/// Columns describing a schedule: `(schedule_kind, interval_seconds, daily_at)`.
pub type ScheduleColumns = (&'static str, Option<i32>, Option<NaiveTime>);

/// When a job runs next, as stored. Exposed for tests and the admin API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobState {
    /// Job name.
    pub name: String,
    /// Stored schedule.
    pub schedule: Schedule,
    /// Next planned run.
    pub next_run_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_keys_are_stable_and_distinct() {
        assert_eq!(lock_key("mailbox-poll"), lock_key("mailbox-poll"));
        assert_ne!(lock_key("mailbox-poll"), lock_key("sla-clock"));
        // Pinned value: changing the hash would let old and new workers run a job twice
        // during a rolling deploy.
        assert_eq!(
            lock_key(""),
            i64::from_ne_bytes(0xfc27_d340_bcdb_4e7c_u64.to_ne_bytes())
        );
    }
}
