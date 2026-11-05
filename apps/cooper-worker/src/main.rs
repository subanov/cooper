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
use cooper_platform::config::{self, ConfigError, DatabaseConfig};
use cooper_platform::crypto::Keyring;
use cooper_platform::db;
use cooper_platform::jobs::{JobRunner, PruneJobRuns, RunOutcome};
use cooper_platform::outbox::Dispatcher;
use cooper_platform::system_log::{self, PruneSystemLog};
use cooper_platform::telemetry::{self, LogFormat};
use cooper_server::{AppState, SecuritySettings};
use cooper_service_desk::infrastructure::SlaMonitor;
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
    blob_dir: std::path::PathBuf,
    log_format: LogFormat,
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
            smtp_url: config::var("COOPER_SMTP_URL")?,
            blob_dir: config::parse_or("COOPER_BLOB_DIR", std::path::PathBuf::from("data/blobs"))?,
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
    telemetry::init(config.log_format, Some(system_log)).context("failed to initialise logging")?;

    let pool = db::connect(&config.database)
        .await
        .context("failed to connect to the database")?;
    if let db::MigrationStatus::Pending { versions } = db::migration_status(&pool).await? {
        anyhow::bail!(
            "database migrations are pending: {versions:?}; run `cargo sqlx migrate run`"
        );
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
    let runner = Arc::new(
        JobRunner::new(pool.clone(), clock.clone(), worker_id.clone())
            .with_job(Arc::new(PruneJobRuns::new(config.job_run_retention_days)))
            .with_job(Arc::new(PruneSystemLog::new(
                config.system_log_retention_days,
            )))
            .with_job(Arc::new(IdentityHousekeeping))
            .with_job(Arc::new(SlaMonitor))
            .with_job(Arc::new(EmailSender::new(transport)))
            .with_job(Arc::new(ReencryptIdentitySecrets::new(
                config.keyring.clone(),
            ))),
    );
    runner
        .register_all()
        .await
        .context("failed to register jobs")?;
    // The same wiring as cooper-server, so handlers use each context's application API.
    let state = AppState::with_settings(
        pool.clone(),
        clock.clone(),
        SecuritySettings {
            session_policy: SessionPolicy::default(),
            secure_cookies: true,
            keyring: config.keyring.clone(),
            trusted_proxies: Vec::new(),
            blob_dir: config.blob_dir.clone(),
        },
    )
    .context("failed to initialise application services")?;
    let dispatcher = state.event_handlers().into_iter().fold(
        Dispatcher::new(pool, clock).with_batch_size(config.outbox_batch_size),
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
    tasks.spawn(outbox_loop(dispatcher, config.tick, shutdown_rx));
    tracing::info!(%worker_id, jobs = ?runner.job_names().collect::<Vec<_>>(), "cooper-worker started");

    shutdown_signal().await;
    let _ = shutdown_tx.send(true);
    while tasks.join_next().await.is_some() {}
    tracing::info!("cooper-worker stopped");
    Ok(())
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
