//! `cooper-server` binary: reads configuration, connects to PostgreSQL and serves HTTP
//! until SIGINT/SIGTERM.
//!
//! `cooper-server openapi` prints the OpenAPI document instead (no configuration or
//! database needed); `web/` generates its API client from it.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context as _;
use cooper_kernel::SystemClock;
use cooper_platform::{db, system_log, telemetry};
use cooper_server::AppState;
use cooper_server::config::ServerConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("openapi") {
        let mut out = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut out, &cooper_server::openapi())?;
        std::io::Write::write_all(&mut out, b"\n")?;
        return Ok(());
    }
    let config = ServerConfig::from_env().context("invalid configuration")?;
    let (system_log, system_log_writer) = system_log::channel("server");
    telemetry::init(config.log_format, Some(system_log)).context("failed to initialise logging")?;

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
    let app = cooper_server::router(state, &config.http)
        .into_make_service_with_connect_info::<SocketAddr>();
    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_addr))?;
    tracing::info!(addr = %config.bind_addr, "cooper-server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
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
