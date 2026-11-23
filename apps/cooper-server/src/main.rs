//! `cooper-server` binary: reads configuration, connects to PostgreSQL and serves HTTP
//! until SIGINT/SIGTERM.
//!
//! Subcommands:
//!
//! - `cooper-server openapi` prints the OpenAPI document (no configuration or database
//!   needed); `web/` generates its API client from it.
//! - `cooper-server migrate` applies pending migrations as the owner role
//!   (`DATABASE_URL`) and exits. Run it once per deployment, before new servers start
//!   (ADR-0023, `docs/runbooks/deployment.md`).
//! - `cooper-server healthcheck` exits 0 if `GET /health/live` on `COOPER_BIND_ADDR`
//!   answers 200: the container `HEALTHCHECK`.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context as _;
use cooper_kernel::SystemClock;
use cooper_platform::config::DatabaseConfig;
use cooper_platform::{db, system_log, telemetry};
use cooper_server::AppState;
use cooper_server::config::ServerConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("openapi") => {
            let mut out = std::io::stdout().lock();
            serde_json::to_writer_pretty(&mut out, &cooper_server::openapi())?;
            std::io::Write::write_all(&mut out, b"\n")?;
            return Ok(());
        }
        Some("migrate") => return migrate().await,
        Some("healthcheck") => {
            let default_addr = SocketAddr::from(([0, 0, 0, 0], 8080));
            let addr = cooper_platform::config::parse_or("COOPER_BIND_ADDR", default_addr)?;
            std::process::exit(if cooper_server::healthcheck::live(addr) {
                0
            } else {
                1
            });
        }
        Some(other) => anyhow::bail!(
            "unknown command {other:?}; expected no command, `openapi`, `migrate` or `healthcheck`"
        ),
        None => {}
    }
    let config = ServerConfig::from_env().context("invalid configuration")?;
    let (system_log, system_log_writer) = system_log::channel("server");
    let _telemetry = telemetry::init(config.log_format, Some(system_log), "cooper-server")
        .context("failed to initialise logging")?;
    if let Some(suggester) = &config.security.reply_suggester {
        tracing::info!(?suggester, "Suggested Replies enabled");
    }
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
    tokio::spawn(system_log_writer.run(pool.clone()));
    if let db::MigrationStatus::Pending { versions } = db::migration_status(&pool).await? {
        tracing::warn!(
            ?versions,
            "database migrations are pending; /health/ready reports 503"
        );
    }

    let state = AppState::with_settings(pool, Arc::new(SystemClock), config.security)
        .context("failed to initialise application state")?;
    let live = state.live.clone();
    let app = cooper_server::router(state, &config.http)
        .into_make_service_with_connect_info::<SocketAddr>();
    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_addr))?;
    tracing::info!(addr = %config.bind_addr, "cooper-server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            // End open Live Updates streams, or graceful shutdown waits for them forever.
            live.shutdown();
        })
        .await?;
    tracing::info!("cooper-server stopped");
    Ok(())
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

/// `cooper-server migrate`: applies pending migrations as the owner role.
async fn migrate() -> anyhow::Result<()> {
    let format = cooper_platform::config::parse_or("COOPER_LOG_FORMAT", Default::default())?;
    let _telemetry =
        telemetry::init(format, None, "cooper-migrate").context("failed to initialise logging")?;
    let config = DatabaseConfig::owner_from_env().context("invalid configuration")?;
    let pool = db::connect(&config)
        .await
        .context("failed to connect to the database")?;
    let applied = db::migrate(&pool).await.context("migration failed")?;
    if applied.is_empty() {
        tracing::info!("database schema is up to date; nothing to apply");
    } else {
        tracing::info!(?applied, "applied migrations");
    }
    pool.close().await;
    Ok(())
}
