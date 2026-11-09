//! `cooper-worker` binary: runs background jobs on their schedules and dispatches
//! outbox events, until SIGINT/SIGTERM.
//!
//! Any number of workers may run: each job runs on one worker at a time (advisory
//! locks) and outbox rows are claimed with `SKIP LOCKED`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use cooper_identity::domain::SessionPolicy;
use cooper_identity::infrastructure::{IdentityHousekeeping, ReencryptIdentitySecrets};
use cooper_kernel::{Clock, SystemClock};
use cooper_notifications::email::{EmailSender, MailTransport, SmtpTransport};
use cooper_platform::attachments::{AttachmentTextExtraction, BlobOrphanCleanup};
use cooper_platform::config::{self, ConfigError, DatabaseConfig};
use cooper_platform::crypto::Keyring;
use cooper_platform::db;
use cooper_platform::jobs::{JobRunner, PruneJobRuns, RunOutcome};
use cooper_platform::outbox::Dispatcher;
use cooper_platform::system_log::{self, PruneSystemLog};
use cooper_platform::telemetry::{self, LogFormat};
use cooper_server::{AppState, SecuritySettings};
use cooper_service_desk::infrastructure::{PrunePresence, ReencryptMailboxSecrets, SlaMonitor};
use tokio::sync::watch;
use tokio::task::JoinSet;

/// Everything `cooper-worker` reads at start-up (see `.env.example`).
#[derive(Clone)]
struct WorkerConfig {
    database: DatabaseConfig,
    tick: Duration,
    outbox_batch_size: u16,
    job_run_retention_days: u16,
    system_log_retention_days: u16,
    keyring: Keyring,
    smtp_url: Option<String>,
    blobs: cooper_platform::attachments::BlobStore,
    blob_orphan_grace_hours: u16,
    public_url: Option<String>,
    log_format: LogFormat,
    metrics_addr: Option<std::net::SocketAddr>,
}

impl WorkerConfig {
    fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            database: DatabaseConfig::from_env()?,
            tick: Duration::from_millis(
                config::parse_or("COOPER_WORKER_TICK_MILLIS", 1000_u64)?.max(50),
            ),
            outbox_batch_size: config::parse_or("COOPER_OUTBOX_BATCH_SIZE", 100_u16)?,
            job_run_retention_days: config::parse_or("COOPER_JOB_RUN_RETENTION_DAYS", 30_u16)?,
            system_log_retention_days: config::parse_or(
                "COOPER_SYSTEM_LOG_RETENTION_DAYS",
                30_u16,
            )?,
            log_format: config::parse_or("COOPER_LOG_FORMAT", LogFormat::default())?,
            metrics_addr: cooper_server::config::metrics_addr()?,
            smtp_url: config::var("COOPER_SMTP_URL")?,
            blobs: cooper_server::config::blob_store()?,
            blob_orphan_grace_hours: config::parse_or("COOPER_BLOB_ORPHAN_GRACE_HOURS", 24_u16)?,
            public_url: cooper_server::config::public_url()?,
            keyring: Keyring::parse(&config::required("COOPER_ENCRYPTION_KEYS")?).map_err(|e| {
                ConfigError::Invalid {
                    name: "COOPER_ENCRYPTION_KEYS",
                    reason: e.to_string(),
                }
            })?,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = WorkerConfig::from_env().context("invalid configuration")?;
    let (system_log, system_log_writer) = system_log::channel("worker");
    let _telemetry = telemetry::init(config.log_format, Some(system_log), "cooper-worker")
        .context("failed to initialise logging")?;
    let metrics_enabled = config.metrics_addr.is_some();
    if let Some(addr) = config.metrics_addr {
        let handle = cooper_platform::metrics::install().context("failed to install metrics")?;
        tokio::spawn(async move {
            let render = move || handle.render();
            if let Err(error) =
                cooper_http::metrics::serve(addr, render, std::future::pending()).await
            {
                tracing::error!(%error, %addr, "metrics listener failed");
            }
        });
    }

    let pool = db::connect(&config.database)
        .await
        .context("failed to connect to the database")?;
    if let db::MigrationStatus::Pending { versions } = db::migration_status(&pool).await? {
        anyhow::bail!("database migrations are pending: {versions:?}; run `cooper-server migrate`");
    }

    tokio::spawn(system_log_writer.run(pool.clone()));

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let worker_id = format!("{}-{}", hostname(), uuid::Uuid::now_v7());
    let transport: Option<Arc<dyn MailTransport>> = match config.smtp_url.as_deref() {
        Some(url) => Some(Arc::new(
            SmtpTransport::from_url(url)
                .map_err(anyhow::Error::msg)
                .context("invalid COOPER_SMTP_URL")?,
        )),
        None => {
            tracing::warn!("COOPER_SMTP_URL is not set: outbound email stays queued");
            None
        }
    };
    // The same wiring as cooper-server, so handlers use each context's application API.
    let state = AppState::with_settings(
        pool.clone(),
        clock.clone(),
        SecuritySettings {
            session_policy: SessionPolicy::default(),
            secure_cookies: true,
            keyring: config.keyring.clone(),
            trusted_proxies: Vec::new(),
            blobs: config.blobs.clone(),
            public_url: config.public_url.clone(),
            mailbox_oauth: cooper_service_desk::domain::OAuthEndpoints::default(),
        },
    )
    .context("failed to initialise application services")?;
    let runner = Arc::new(with_jobs(
        JobRunner::new(pool.clone(), clock.clone(), worker_id.clone()),
        &state,
        &config,
        transport,
    ));
    runner
        .register_all()
        .await
        .context("failed to register jobs")?;
    let dispatcher = state.event_handlers().into_iter().fold(
        Dispatcher::new(pool.clone(), clock.clone()).with_batch_size(config.outbox_batch_size),
        Dispatcher::with_handler,
    );
    let dispatcher = Arc::new(dispatcher);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut tasks = JoinSet::new();
    for name in runner.job_names() {
        tasks.spawn(job_loop(
            runner.clone(),
            name,
            config.tick,
            shutdown_rx.clone(),
        ));
    }
    if metrics_enabled {
        tasks.spawn(backlog_loop(
            pool.clone(),
            clock.clone(),
            shutdown_rx.clone(),
        ));
    }
    tasks.spawn(outbox_loop(dispatcher, config.tick, shutdown_rx));
    tracing::info!(%worker_id, jobs = ?runner.job_names().collect::<Vec<_>>(), "cooper-worker started");

    shutdown_signal().await;
    let _ = shutdown_tx.send(true);
    while tasks.join_next().await.is_some() {}
    tracing::info!("cooper-worker stopped");
    Ok(())
}

/// Adds every Background Job the worker runs to `runner`.
fn with_jobs(
    runner: JobRunner,
    state: &AppState,
    config: &WorkerConfig,
    transport: Option<Arc<dyn MailTransport>>,
) -> JobRunner {
    runner
        .with_job(Arc::new(PruneJobRuns::new(config.job_run_retention_days)))
        .with_job(Arc::new(PruneSystemLog::new(
            config.system_log_retention_days,
        )))
        .with_job(Arc::new(IdentityHousekeeping))
        .with_job(Arc::new(SlaMonitor))
        .with_job(Arc::new(PrunePresence))
        .with_job(Arc::new(EmailSender::new(transport)))
        .with_job(Arc::new(ReencryptIdentitySecrets::new(
            config.keyring.clone(),
        )))
        .with_job(Arc::new(ReencryptMailboxSecrets::new(
            config.keyring.clone(),
        )))
        .with_job(Arc::new(state.mailbox_poller()))
        .with_job(Arc::new(AttachmentTextExtraction::new(
            state.service_desk.blobs.clone(),
        )))
        .with_job(Arc::new(BlobOrphanCleanup::new(
            state.service_desk.blobs.clone(),
            chrono::Duration::hours(i64::from(config.blob_orphan_grace_hours)),
        )))
        .with_job(Arc::new(
            cooper_knowledge::infrastructure::KnowledgeReviewJob {
                service: state.knowledge.service.clone(),
            },
        ))
        .with_job(Arc::new(
            cooper_service_status::infrastructure::PlannedMaintenanceJob {
                service: state.service_status.service.clone(),
            },
        ))
        .with_job(Arc::new(cooper_tasks::infrastructure::TaskRecurrenceJob {
            service: state.tasks.service.clone(),
        }))
        .with_job(Arc::new(
            cooper_morning_checks::infrastructure::MorningCheckReminderJob {
                service: state.morning_checks.service.clone(),
            },
        ))
}

async fn job_loop(
    runner: Arc<JobRunner>,
    name: &'static str,
    tick: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        match runner.run_if_due(name).await {
            Ok(RunOutcome::Succeeded) => tracing::info!(job = name, "job succeeded"),
            Ok(RunOutcome::Failed(error)) => tracing::warn!(job = name, %error, "job failed"),
            Ok(RunOutcome::Locked | RunOutcome::NotDue) => {}
            Err(error) => tracing::error!(job = name, %error, "job runner error"),
        }
        if wait_or_shutdown(tick, &mut shutdown).await {
            return;
        }
    }
}

async fn outbox_loop(
    dispatcher: Arc<Dispatcher>,
    tick: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        // Keep draining while there is work; sleep only when a pass finds nothing.
        let busy = match dispatcher.dispatch_once().await {
            Ok(stats) => {
                if stats.total() > 0 {
                    tracing::debug!(?stats, "outbox pass");
                }
                stats.total() > 0
            }
            Err(error) => {
                tracing::error!(%error, "outbox dispatch failed");
                false
            }
        };
        if *shutdown.borrow() || (!busy && wait_or_shutdown(tick, &mut shutdown).await) {
            return;
        }
    }
}

/// Samples the outbox backlog into the metrics gauges every 15 seconds (ADR-0023).
async fn backlog_loop(
    pool: sqlx::PgPool,
    clock: Arc<dyn Clock>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        match cooper_platform::outbox::backlog(&pool, clock.now()).await {
            Ok(backlog) => cooper_platform::outbox::record_backlog(&backlog),
            Err(error) => tracing::warn!(%error, "cannot measure the outbox backlog"),
        }
        if wait_or_shutdown(Duration::from_secs(15), &mut shutdown).await {
            return;
        }
    }
}

/// Sleeps for `tick`; returns `true` if shutdown was requested meanwhile.
async fn wait_or_shutdown(tick: Duration, shutdown: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        () = tokio::time::sleep(tick) => *shutdown.borrow(),
        changed = shutdown.changed() => changed.is_err() || *shutdown.borrow(),
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "worker".to_owned())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> WorkerConfig {
        WorkerConfig {
            database: DatabaseConfig {
                url: config::Secret::new("postgres://localhost/unused".to_owned()),
                max_connections: 1,
            },
            tick: Duration::from_secs(1),
            outbox_batch_size: 100,
            job_run_retention_days: 30,
            system_log_retention_days: 30,
            keyring: Keyring::ephemeral().unwrap(),
            smtp_url: None,
            blobs: cooper_platform::attachments::BlobStore::filesystem("data/blobs"),
            blob_orphan_grace_hours: 24,
            public_url: None,
            log_format: LogFormat::default(),
            metrics_addr: None,
        }
    }

    /// Regression: `task-recurrence` was defined but never registered, so recurring
    /// Tasks were never made (FR-TSK-006).
    #[tokio::test]
    async fn worker_registers_every_background_job() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/unused").unwrap();
        let state = AppState::new(pool.clone()).unwrap();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let runner = with_jobs(
            JobRunner::new(pool, clock, "test"),
            &state,
            &test_config(),
            None,
        );
        let mut names: Vec<&str> = runner.job_names().collect();
        names.sort_unstable();
        let expected = [
            "attachment-orphan-cleanup",
            "attachment-text-extraction",
            "email-sender",
            "identity-housekeeping",
            "identity-secrets-reencrypt",
            "job-run-history-prune",
            "knowledge-review",
            "mailbox-poll",
            "mailbox-secrets-reencrypt",
            "morning-check-reminders",
            "planned-maintenance",
            "sla-monitor",
            "system-log-prune",
            "task-recurrence",
            "ticket-presence-prune",
        ];
        assert_eq!(names, expected);
    }
}
