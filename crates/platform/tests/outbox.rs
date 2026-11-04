//! ADR-0005: events appended in a transaction are delivered at least once, retried with
//! backoff, and marked dead after the maximum number of attempts.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: failures should abort the test

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use cooper_kernel::{Actor, DomainEvent, EventEnvelope, FixedClock, TenantId, TenantScope};
use cooper_platform::outbox::{self, DispatchStats, Dispatcher, EventHandler, HandlerError};
use cooper_platform::tenancy;
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

struct Recorder {
    seen: AtomicUsize,
    fail_first: usize,
}

#[async_trait]
impl EventHandler for Recorder {
    fn name(&self) -> &'static str {
        "recorder"
    }

    fn handles(&self, event_type: &str) -> bool {
        event_type == WidgetCreated::EVENT_TYPE
    }

    async fn handle(&self, _event: &EventEnvelope) -> Result<(), HandlerError> {
        let n = self.seen.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_first {
            Err("boom".into())
        } else {
            Ok(())
        }
    }
}

fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 5, 1, 9, 0, 0).unwrap(),
    ))
}

async fn append_event(pool: &PgPool, clock: &FixedClock) -> Uuid {
    let tenant = TenantId::new();
    let event = WidgetCreated {
        widget_id: Uuid::now_v7(),
        tenant,
    };
    let envelope =
        EventEnvelope::wrap(&event, Actor::Automation, cooper_kernel::Clock::now(clock)).unwrap();
    let mut tx = tenancy::begin(pool, &TenantScope::single(tenant))
        .await
        .unwrap();
    outbox::append(&mut tx, &envelope).await.unwrap();
    tx.commit().await.unwrap();
    envelope.id.as_uuid()
}

#[sqlx::test(migrations = "../../migrations")]
async fn delivers_committed_events_once(pool: PgPool) {
    let clock = clock();
    let handler = Arc::new(Recorder {
        seen: AtomicUsize::new(0),
        fail_first: 0,
    });
    let dispatcher = Dispatcher::new(pool.clone(), clock.clone()).with_handler(handler.clone());
    let id = append_event(&pool, &clock).await;

    assert_eq!(
        dispatcher.dispatch_once().await.unwrap(),
        DispatchStats {
            published: 1,
            retried: 0,
            dead: 0
        }
    );
    assert_eq!(dispatcher.dispatch_once().await.unwrap().total(), 0);
    assert_eq!(handler.seen.load(Ordering::SeqCst), 1);

    let published = sqlx::query_scalar!("SELECT published_at FROM outbox_events WHERE id = $1", id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(published, Some(cooper_kernel::Clock::now(&*clock)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn rolled_back_events_are_never_delivered(pool: PgPool) {
    let clock = clock();
    let tenant = TenantId::new();
    let event = WidgetCreated {
        widget_id: Uuid::now_v7(),
        tenant,
    };
    let envelope = EventEnvelope::wrap(&event, Actor::Automation, Utc::now()).unwrap();
    let mut tx = tenancy::begin(&pool, &TenantScope::single(tenant))
        .await
        .unwrap();
    outbox::append(&mut tx, &envelope).await.unwrap();
    tx.rollback().await.unwrap();

    let dispatcher = Dispatcher::new(pool.clone(), clock);
    assert_eq!(dispatcher.dispatch_once().await.unwrap().total(), 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn failed_delivery_is_retried_after_backoff(pool: PgPool) {
    let clock = clock();
    let handler = Arc::new(Recorder {
        seen: AtomicUsize::new(0),
        fail_first: 1,
    });
    let dispatcher = Dispatcher::new(pool.clone(), clock.clone()).with_handler(handler.clone());
    append_event(&pool, &clock).await;

    assert_eq!(dispatcher.dispatch_once().await.unwrap().retried, 1);
    // Not due again until the backoff (2s after the first failure) has passed.
    assert_eq!(dispatcher.dispatch_once().await.unwrap().total(), 0);
    clock.advance(Duration::seconds(2));
    assert_eq!(dispatcher.dispatch_once().await.unwrap().published, 1);
    assert_eq!(handler.seen.load(Ordering::SeqCst), 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn event_is_marked_dead_after_max_attempts(pool: PgPool) {
    let clock = clock();
    let handler = Arc::new(Recorder {
        seen: AtomicUsize::new(0),
        fail_first: usize::MAX,
    });
    let dispatcher = Dispatcher::new(pool.clone(), clock.clone())
        .with_handler(handler)
        .with_max_attempts(2);
    let id = append_event(&pool, &clock).await;

    assert_eq!(dispatcher.dispatch_once().await.unwrap().retried, 1);
    clock.advance(Duration::hours(1));
    assert_eq!(dispatcher.dispatch_once().await.unwrap().dead, 1);
    clock.advance(Duration::hours(1));
    assert_eq!(dispatcher.dispatch_once().await.unwrap().total(), 0);

    let row = sqlx::query!(
        "SELECT attempts, dead_at, last_error FROM outbox_events WHERE id = $1",
        id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.attempts, 2);
    assert!(row.dead_at.is_some());
    assert_eq!(row.last_error.as_deref(), Some("recorder: boom"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn events_without_handlers_are_marked_published(pool: PgPool) {
    let clock = clock();
    append_event(&pool, &clock).await;
    let dispatcher = Dispatcher::new(pool.clone(), clock);
    assert_eq!(dispatcher.dispatch_once().await.unwrap().published, 1);
}
