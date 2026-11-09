//! Storage of the Watchtower Items (FR-RPT-003).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code

use std::collections::BTreeSet;

use cooper_kernel::{Actor, AuditAction, AuditRecord};
use cooper_reporting::application::WATCHTOWER_AGGREGATE_ID;
use cooper_reporting::domain::WatchtowerItems;
use cooper_reporting::infrastructure::PgWatchtowerStore;
use cooper_reporting::ports::WatchtowerStore;
use sqlx::PgPool;
use uuid::Uuid;

fn audit() -> AuditRecord {
    AuditRecord::new(
        None,
        "watchtower_items",
        WATCHTOWER_AGGREGATE_ID,
        AuditAction::Updated,
        Actor::Automation,
        chrono::Utc::now(),
    )
}

#[sqlx::test(migrations = "../../migrations")]
async fn watchtower_items_are_replaced_as_a_whole(pool: PgPool) {
    let store = PgWatchtowerStore::new(pool.clone());
    assert_eq!(store.items().await.unwrap(), WatchtowerItems::default());
    let (a, b, c) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let first = WatchtowerItems {
        ticket_statuses: BTreeSet::from([a, b]),
        check_statuses: BTreeSet::from([c]),
        ..WatchtowerItems::default()
    };
    store.save(&first, &audit()).await.unwrap();
    assert_eq!(store.items().await.unwrap(), first);
    // The same value may be chosen for two kinds.
    let second = WatchtowerItems {
        ticket_priorities: BTreeSet::from([a]),
        task_statuses: BTreeSet::from([a]),
        ..WatchtowerItems::default()
    };
    store.save(&second, &audit()).await.unwrap();
    assert_eq!(store.items().await.unwrap(), second);
    store
        .save(&WatchtowerItems::default(), &audit())
        .await
        .unwrap();
    assert_eq!(store.items().await.unwrap(), WatchtowerItems::default());
}
