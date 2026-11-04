//! FR-SYS-002: schedules, single-worker locking and run history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: failures should abort the test

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::{Duration, NaiveTime, TimeZone, Utc};
use cooper_kernel::FixedClock;
use cooper_platform::jobs::{
    Job, JobContext, JobError, JobRunner, PruneJobRuns, RunOutcome, Schedule, lock_key,
};
use sqlx::PgPool;

struct Counter {
    runs: AtomicUsize,
    fail: bool,
}

#[async_trait]
impl Job for Counter {
    fn name(&self) -> &'static str {
        "counter"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(60)
    }

    async fn run(&self, _ctx: &JobContext) -> Result<Option<String>, JobError> {
        let n = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail {
            Err("mailbox unreachable".into())
        } else {
            Ok(Some(format!("run {n}")))
        }
    }
}

fn setup(pool: &PgPool, fail: bool) -> (Arc<FixedClock>, Arc<Counter>, JobRunner) {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 6, 1, 8, 0, 0).unwrap(),
    ));
    let job = Arc::new(Counter {
        runs: AtomicUsize::new(0),
        fail,
    });
    let runner = JobRunner::new(pool.clone(), clock.clone(), "worker-1").with_job(job.clone());
    (clock, job, runner)
}

#[sqlx::test(migrations = "../../migrations")]
async fn interval_job_runs_immediately_then_waits_for_its_interval(pool: PgPool) {
    let (clock, job, runner) = setup(&pool, false);
    runner.register_all().await.unwrap();

    assert_eq!(
        runner.run_if_due("counter").await.unwrap(),
        RunOutcome::Succeeded
    );
    assert_eq!(
        runner.run_if_due("counter").await.unwrap(),
        RunOutcome::NotDue
    );
    clock.advance(Duration::seconds(60));
    assert_eq!(
        runner.run_if_due("counter").await.unwrap(),
        RunOutcome::Succeeded
    );
    assert_eq!(job.runs.load(Ordering::SeqCst), 2);

    let runs = sqlx::query!(
        "SELECT outcome, message, duration_ms, worker_id FROM background_job_runs ORDER BY started_at"
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(runs.len(), 2);
    assert!(
        runs.iter().all(|r| r.outcome == "succeeded"
            && r.duration_ms.is_some()
            && r.worker_id == "worker-1")
    );
    assert_eq!(runs[1].message.as_deref(), Some("run 2"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn failure_is_recorded_and_does_not_stop_the_schedule(pool: PgPool) {
    let (_clock, _job, runner) = setup(&pool, true);
    runner.register_all().await.unwrap();

    assert_eq!(
        runner.run_if_due("counter").await.unwrap(),
        RunOutcome::Failed("mailbox unreachable".to_owned())
    );
    let run = sqlx::query!("SELECT outcome, message FROM background_job_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(run.outcome, "failed");
    assert_eq!(run.message.as_deref(), Some("mailbox unreachable"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn job_does_not_run_while_another_worker_holds_its_lock(pool: PgPool) {
    let (_clock, job, runner) = setup(&pool, false);
    runner.register_all().await.unwrap();

    let mut other_worker = pool.acquire().await.unwrap();
    sqlx::query!("SELECT pg_advisory_lock($1)", lock_key("counter"))
        .fetch_one(&mut *other_worker)
        .await
        .unwrap();

    assert_eq!(
        runner.run_if_due("counter").await.unwrap(),
        RunOutcome::Locked
    );
    assert_eq!(job.runs.load(Ordering::SeqCst), 0);

    sqlx::query!("SELECT pg_advisory_unlock($1)", lock_key("counter"))
        .fetch_one(&mut *other_worker)
        .await
        .unwrap();
    assert_eq!(
        runner.run_if_due("counter").await.unwrap(),
        RunOutcome::Succeeded
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn stale_running_run_is_marked_abandoned(pool: PgPool) {
    let (_clock, _job, runner) = setup(&pool, false);
    runner.register_all().await.unwrap();
    sqlx::query!(
        "INSERT INTO background_job_runs (id, job_name, worker_id, started_at, outcome) VALUES ($1, 'counter', 'dead-worker', now(), 'running')",
        uuid::Uuid::now_v7(),
    )
    .execute(&pool)
    .await
    .unwrap();

    runner.run_if_due("counter").await.unwrap();

    let outcomes: Vec<String> =
        sqlx::query_scalar!("SELECT outcome FROM background_job_runs ORDER BY worker_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(outcomes, vec!["abandoned", "succeeded"]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn registration_keeps_an_administrators_schedule(pool: PgPool) {
    let (_clock, job, runner) = setup(&pool, false);
    runner.register_all().await.unwrap();
    sqlx::query!("UPDATE background_jobs SET schedule_kind = 'off', interval_seconds = NULL WHERE name = 'counter'")
        .execute(&pool)
        .await
        .unwrap();

    runner.register_all().await.unwrap();
    assert_eq!(
        runner.run_if_due("counter").await.unwrap(),
        RunOutcome::NotDue
    );
    assert_eq!(job.runs.load(Ordering::SeqCst), 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn prune_deletes_only_old_finished_runs(pool: PgPool) {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 6, 1, 3, 0, 0).unwrap(),
    ));
    let prune = Arc::new(PruneJobRuns::new(30));
    let runner = JobRunner::new(pool.clone(), clock.clone(), "w").with_job(prune);
    runner.register_all().await.unwrap();
    let now = cooper_kernel::Clock::now(&*clock);
    for (days_ago, outcome) in [(40, "succeeded"), (40, "running"), (5, "failed")] {
        let started = now - Duration::days(days_ago);
        let finished = (outcome != "running").then_some(started);
        sqlx::query!(
            "INSERT INTO background_job_runs (id, job_name, worker_id, started_at, finished_at, outcome) VALUES ($1, 'job-run-history-prune', 'w', $2, $3, $4)",
            uuid::Uuid::now_v7(),
            started,
            finished,
            outcome,
        )
        .execute(&pool)
        .await
        .unwrap();
    }

    // Registered daily at 03:00, so first due tomorrow.
    assert_eq!(
        runner.run_if_due("job-run-history-prune").await.unwrap(),
        RunOutcome::NotDue
    );
    clock.advance(Duration::days(1));
    assert_eq!(
        runner.run_if_due("job-run-history-prune").await.unwrap(),
        RunOutcome::Succeeded
    );

    let left: Vec<String> = sqlx::query_scalar!(
        "SELECT outcome FROM background_job_runs WHERE outcome <> 'succeeded' OR started_at > $1 ORDER BY outcome",
        now - Duration::days(10),
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    // The 40-day-old succeeded run is gone. The stale 40-day-old `running` run was first
    // marked abandoned (so finished) and then pruned. The 5-day-old failure and the prune
    // run itself stay.
    assert_eq!(left, vec!["failed", "succeeded"]);
    let daily_at = sqlx::query_scalar!("SELECT daily_at FROM background_jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(daily_at, NaiveTime::from_hms_opt(3, 0, 0));
}
