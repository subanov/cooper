//! ADR-0023: `cooper-server migrate` applies every migration once, even when several
//! deployments start it at the same time, and a second run finds nothing to do.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: failures should abort the test

use cooper_platform::db::{self, MIGRATOR, MigrationStatus};
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn concurrent_runs_apply_each_migration_once(pool: PgPool) {
    let (a, b) = tokio::join!(db::migrate(&pool), db::migrate(&pool));
    let (mut a, mut b) = (a.unwrap(), b.unwrap());
    let mut applied = std::mem::take(&mut a);
    applied.append(&mut b);
    applied.sort_unstable();
    let expected: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert_eq!(
        applied, expected,
        "every migration applied by exactly one run"
    );

    assert_eq!(
        db::migration_status(&pool).await.unwrap(),
        MigrationStatus::UpToDate
    );
    assert!(
        db::migrate(&pool).await.unwrap().is_empty(),
        "a second run has nothing to apply"
    );
}
