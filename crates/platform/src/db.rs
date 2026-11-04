//! PostgreSQL pool and migrations.

use std::collections::BTreeSet;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres};

use crate::config::DatabaseConfig;

/// All migrations in `/migrations`, embedded at compile time. One sequence for the whole
/// database (AGENTS.md §5).
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// A PostgreSQL transaction.
pub type Tx = sqlx::Transaction<'static, Postgres>;

/// Opens a connection pool.
///
/// # Errors
///
/// Fails if the URL is invalid or the database cannot be reached.
pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(config.url.expose())
        .await
}

/// Whether the database schema matches the migrations this binary was built with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStatus {
    /// Every embedded migration has been applied successfully.
    UpToDate,
    /// Some embedded migrations have not been applied (or failed).
    Pending {
        /// Versions not yet applied, in order.
        versions: Vec<i64>,
    },
}

/// Compares applied migrations with the embedded ones.
///
/// # Errors
///
/// Fails if the database cannot be queried.
pub async fn migration_status(pool: &PgPool) -> Result<MigrationStatus, sqlx::Error> {
    let table_exists =
        sqlx::query_scalar!(r#"SELECT to_regclass('_sqlx_migrations') IS NOT NULL AS "exists!""#)
            .fetch_one(pool)
            .await?;
    let applied: BTreeSet<i64> = if table_exists {
        sqlx::query_scalar!("SELECT version FROM _sqlx_migrations WHERE success")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect()
    } else {
        BTreeSet::new()
    };
    let versions: Vec<i64> = MIGRATOR
        .iter()
        .filter(|m| !m.migration_type.is_down_migration())
        .map(|m| m.version)
        .filter(|v| !applied.contains(v))
        .collect();
    Ok(if versions.is_empty() {
        MigrationStatus::UpToDate
    } else {
        MigrationStatus::Pending { versions }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = false)]
    async fn fresh_database_reports_every_migration_pending(pool: PgPool) {
        let status = migration_status(&pool).await.unwrap();
        let expected: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
        assert_eq!(status, MigrationStatus::Pending { versions: expected });
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn migrated_database_is_up_to_date(pool: PgPool) {
        assert_eq!(
            migration_status(&pool).await.unwrap(),
            MigrationStatus::UpToDate
        );
    }
}
