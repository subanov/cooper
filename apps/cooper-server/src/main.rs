//! `cooper-server` binary: reads configuration, connects to PostgreSQL and serves HTTP
//! until SIGINT/SIGTERM.

use anyhow::Context as _;
use cooper_platform::{db, telemetry};
use cooper_server::AppState;
use cooper_server::config::ServerConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ServerConfig::from_env().context("invalid configuration")?;
    telemetry::init(config.log_format).context("failed to initialise logging")?;

    let pool = db::connect(&config.database)
        .await
        .context("failed to connect to the database")?;
    if let db::MigrationStatus::Pending { versions } = db::migration_status(&pool).await? {
        tracing::warn!(
            ?versions,
            "database migrations are pending; /health/ready reports 503"
        );
    }

    let app = cooper_server::router(AppState::new(pool), &config.http);
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
