//! Reading and changing Background Jobs for administration (FR-SYS-002). The tables are
//! owned by the platform; the administration context calls these functions.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::Schedule;

/// A registered job with its schedule and latest run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobInfo {
    /// Job name.
    pub name: String,
    /// Current schedule.
    pub schedule: Schedule,
    /// When it runs next (`None` when off).
    pub next_run_at: Option<DateTime<Utc>>,
    /// When it last started.
    pub last_run_at: Option<DateTime<Utc>>,
    /// Outcome of the latest run.
    pub last_outcome: Option<String>,
    /// Duration of the latest finished run.
    pub last_duration_ms: Option<i64>,
}

/// One run of a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRunInfo {
    /// Run ID.
    pub id: Uuid,
    /// Worker that ran it.
    pub worker_id: String,
    /// Start.
    pub started_at: DateTime<Utc>,
    /// End, unless still running.
    pub finished_at: Option<DateTime<Utc>>,
    /// `running`, `succeeded`, `failed` or `abandoned`.
    pub outcome: String,
    /// Duration in milliseconds.
    pub duration_ms: Option<i64>,
    /// Message from the job or the error.
    pub message: Option<String>,
}

/// Errors of the administration functions.
#[derive(Debug, thiserror::Error)]
pub enum JobAdminError {
    /// No job with that name is registered.
    #[error("unknown job {0}")]
    UnknownJob(String),
    /// Stored schedule is inconsistent.
    #[error(transparent)]
    Schedule(#[from] super::ScheduleError),
    /// Database error.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Every registered job, by name.
///
/// # Errors
///
/// Database errors or an inconsistent stored schedule.
pub async fn list_jobs(pool: &PgPool) -> Result<Vec<JobInfo>, JobAdminError> {
    let rows = sqlx::query!(
        r#"SELECT j.name, j.schedule_kind, j.interval_seconds, j.daily_at, j.next_run_at, j.last_run_at,
                  r.outcome AS "last_outcome?", r.duration_ms AS "last_duration_ms?"
           FROM background_jobs j
           LEFT JOIN LATERAL (
               SELECT outcome, duration_ms FROM background_job_runs
               WHERE job_name = j.name ORDER BY started_at DESC LIMIT 1
           ) r ON true
           ORDER BY j.name"#
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(JobInfo {
                schedule: Schedule::from_columns(&r.schedule_kind, r.interval_seconds, r.daily_at)?,
                name: r.name,
                next_run_at: r.next_run_at,
                last_run_at: r.last_run_at,
                last_outcome: r.last_outcome,
                last_duration_ms: r.last_duration_ms,
            })
        })
        .collect()
}

/// Runs of a job, newest first, starting before `before` (a run ID from a previous page).
///
/// # Errors
///
/// Database errors.
pub async fn list_runs(
    pool: &PgPool,
    name: &str,
    before: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<JobRunInfo>, JobAdminError> {
    let (before_at, before_id) = before.unzip();
    let rows = sqlx::query_as!(
        JobRunInfo,
        "SELECT id, worker_id, started_at, finished_at, outcome, duration_ms, message
         FROM background_job_runs
         WHERE job_name = $1 AND ($2::timestamptz IS NULL OR (started_at, id) < ($2, $3))
         ORDER BY started_at DESC, id DESC
         LIMIT $4",
        name,
        before_at,
        before_id,
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Changes a job's schedule and recomputes its next run. Returns the previous schedule.
/// Runs on the caller's connection, so the caller can audit the change in the same
/// transaction.
///
/// # Errors
///
/// [`JobAdminError::UnknownJob`] or database errors.
pub async fn set_schedule(
    conn: &mut PgConnection,
    name: &str,
    schedule: Schedule,
    now: DateTime<Utc>,
) -> Result<Schedule, JobAdminError> {
    let row = sqlx::query!(
        "SELECT schedule_kind, interval_seconds, daily_at FROM background_jobs WHERE name = $1 FOR UPDATE",
        name
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| JobAdminError::UnknownJob(name.to_owned()))?;
    let previous = Schedule::from_columns(&row.schedule_kind, row.interval_seconds, row.daily_at)?;
    let (kind, interval_seconds, daily_at) = schedule.to_columns();
    sqlx::query!(
        "UPDATE background_jobs SET schedule_kind = $2, interval_seconds = $3, daily_at = $4,
                next_run_at = $5, updated_at = $6 WHERE name = $1",
        name,
        kind,
        interval_seconds,
        daily_at,
        schedule.first_run(now),
        now
    )
    .execute(&mut *conn)
    .await?;
    Ok(previous)
}

/// Makes a job due now, so the next worker tick runs it once, even when its schedule is
/// off.
///
/// # Errors
///
/// [`JobAdminError::UnknownJob`] or database errors.
pub async fn request_run(
    pool: &PgPool,
    name: &str,
    now: DateTime<Utc>,
) -> Result<(), JobAdminError> {
    let updated = sqlx::query!(
        "UPDATE background_jobs SET next_run_at = $2, updated_at = $2 WHERE name = $1",
        name,
        now
    )
    .execute(pool)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(JobAdminError::UnknownJob(name.to_owned()));
    }
    Ok(())
}
