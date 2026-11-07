//! ADR-0022: Live Updates are announced with `NOTIFY` in the writing transaction, so they
//! arrive only after commit, once per transaction, and every outbox event announces its
//! aggregate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: failures should abort the test

use std::time::Duration;

use chrono::Utc;
use cooper_kernel::{Actor, DomainEvent, EventEnvelope, TenantId, TenantScope};
use cooper_platform::live::{self, LiveChange, LiveFeed, LiveSignal, LiveSubscription};
use cooper_platform::{outbox, tenancy};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Serialize)]
struct WidgetCreated {
    widget_id: Uuid,
    tenant: TenantId,
}

impl DomainEvent for WidgetCreated {
    const EVENT_TYPE: &'static str = "widget.created";
    const AGGREGATE_TYPE: &'static str = "widget";

    fn aggregate_id(&self) -> Uuid {
        self.widget_id
    }

    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant)
    }
}

async fn next(subscription: &mut LiveSubscription) -> LiveSignal {
    tokio::time::timeout(Duration::from_secs(10), subscription.recv())
        .await
        .expect("a live update within 10 s")
        .expect("the feed is open")
}

#[sqlx::test(migrations = "../../migrations")]
async fn changes_arrive_after_commit_once_per_transaction(pool: PgPool) {
    let feed = LiveFeed::new(pool.clone());
    let mut subscription = feed.subscribe();
    feed.listening().wait_for(|l| *l).await.unwrap();

    let tenant = TenantId::new();
    let scope = TenantScope::single(tenant);
    let rolled_back = LiveChange::new(tenant, "widget", Uuid::now_v7());
    let committed = LiveChange::new(tenant, "widget", Uuid::now_v7());

    let mut tx = tenancy::begin(&pool, &scope).await.unwrap();
    live::publish(&mut tx, &rolled_back).await.unwrap();
    tx.rollback().await.unwrap();

    let mut tx = tenancy::begin(&pool, &scope).await.unwrap();
    live::publish(&mut tx, &committed).await.unwrap();
    live::publish(&mut tx, &committed).await.unwrap();
    assert!(
        subscription.try_recv().is_none(),
        "nothing before the commit"
    );
    tx.commit().await.unwrap();

    let widget = WidgetCreated {
        widget_id: Uuid::now_v7(),
        tenant,
    };
    let envelope = EventEnvelope::wrap(&widget, Actor::Automation, Utc::now()).unwrap();
    let mut tx = tenancy::begin(&pool, &scope).await.unwrap();
    outbox::append(&mut tx, &envelope).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(
        next(&mut subscription).await,
        LiveSignal::Changed(committed),
        "the rolled-back change is never announced"
    );
    assert_eq!(
        next(&mut subscription).await,
        LiveSignal::Changed(LiveChange::new(tenant, "widget", widget.widget_id)),
        "the duplicate in one transaction is delivered once; outbox events announce their aggregate"
    );

    feed.shutdown();
    tokio::time::timeout(Duration::from_secs(5), feed.listening().wait_for(|l| !*l))
        .await
        .expect("the listener stops on shutdown")
        .unwrap();
}
