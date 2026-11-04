//! Built-in job that keeps the run history from growing forever.

use async_trait::async_trait;
use chrono::{Duration, NaiveTime};

use super::{Job, JobContext, JobError, Schedule};

/// Deletes finished `background_job_runs` older than the retention period. Runs daily
/// at 03:00 UTC by default.
#[derive(Debug, Clone, Copy)]
pub struct PruneJobRuns {
    retention_days: u16,
}

impl PruneJobRuns {
    /// Keeps `retention_days` days of history (at least one).
    #[must_use]
    pub fn new(retention_days: u16) -> Self {
        Self {
            retention_days: retention_days.max(1),
        }
    }
}

#[async_trait]
impl Job for PruneJobRuns {
    fn name(&self) -> &'static str {
        "job-run-history-prune"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::Daily {
            at: NaiveTime::from_hms_opt(3, 0, 0).unwrap_or(NaiveTime::MIN),
        }
    }

    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError> {
        let cutoff = ctx.clock.now() - Duration::days(i64::from(self.retention_days));
        let deleted = sqlx::query!(
            "DELETE FROM background_job_runs WHERE outcome <> 'running' AND started_at < $1",
            cutoff,
        )
        .execute(&ctx.pool)
        .await?
        .rows_affected();
        Ok(Some(format!(
            "deleted {deleted} runs older than {} days",
            self.retention_days
        )))
    }
}
