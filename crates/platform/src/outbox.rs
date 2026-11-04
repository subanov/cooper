//! Transactional outbox (ADR-0005).
//!
//! Producers call [`append`] inside the transaction that changes the aggregate. The
//! [`Dispatcher`] (run by cooper-worker) delivers each pending event to every
//! [`EventHandler`] that handles its type, **at least once**: handlers must be
//! idempotent (use [`EventEnvelope::id`] to de-duplicate). Failed deliveries are retried
//! with exponential backoff; after [`Dispatcher::max_attempts`] the event is marked dead.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use cooper_kernel::{Actor, Clock, EventEnvelope, EventId, TenantId, TenantScope};
use sqlx::{PgConnection, PgPool};
use thiserror::Error;

use crate::tenancy;

/// Error returned by an event handler.
pub type HandlerError = Box<dyn std::error::Error + Send + Sync>;

/// Reacts to domain events.
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Stable name, used in logs and error messages.
    fn name(&self) -> &'static str;

    /// Whether this handler wants events of `event_type`.
    fn handles(&self, event_type: &str) -> bool;

    /// Handles one event. May be called more than once for the same event.
    async fn handle(&self, event: &EventEnvelope) -> Result<(), HandlerError>;
}

/// Appends an event to the outbox using the caller's transaction.
///
/// # Errors
///
/// Fails on database errors, including a Row-Level Security violation when the event's
/// Company is outside the transaction's tenant scope.
pub async fn append(conn: &mut PgConnection, event: &EventEnvelope) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO outbox_events (
            id, tenant_id, event_type, aggregate_type, aggregate_id, actor_kind, actor_id,
            occurred_at, payload, next_attempt_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $8)
        "#,
        event.id.as_uuid(),
        event.tenant_id.map(|t| t.as_uuid()),
        event.event_type,
        event.aggregate_type,
        event.aggregate_id,
        event.actor.kind().as_str(),
        event.actor.id(),
        event.occurred_at,
        event.payload,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// Dispatching failed at the database level (handler failures are recorded per event).
#[derive(Debug, Error)]
pub enum DispatchError {
    /// Database error.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Result of one dispatch pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatchStats {
    /// Events delivered to all their handlers.
    pub published: usize,
    /// Events that failed and will be retried.
    pub retried: usize,
    /// Events that failed for the last time.
    pub dead: usize,
}

impl DispatchStats {
    /// Number of events processed.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.published + self.retried + self.dead
    }
}

/// Delivers pending outbox events to handlers.
pub struct Dispatcher {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    handlers: Vec<Arc<dyn EventHandler>>,
    batch_size: i64,
    max_attempts: i32,
}

impl Dispatcher {
    /// Default number of delivery attempts before an event is marked dead.
    pub const DEFAULT_MAX_ATTEMPTS: i32 = 10;

    /// A dispatcher with no handlers, batch size 100 and
    /// [`Dispatcher::DEFAULT_MAX_ATTEMPTS`].
    #[must_use]
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self {
            pool,
            clock,
            handlers: Vec::new(),
            batch_size: 100,
            max_attempts: Self::DEFAULT_MAX_ATTEMPTS,
        }
    }

    /// Registers a handler.
    #[must_use]
    pub fn with_handler(mut self, handler: Arc<dyn EventHandler>) -> Self {
        self.handlers.push(handler);
        self
    }

    /// Sets how many events one pass processes.
    #[must_use]
    pub fn with_batch_size(mut self, batch_size: u16) -> Self {
        self.batch_size = i64::from(batch_size.max(1));
        self
    }

    /// Sets the number of attempts before an event is marked dead.
    #[must_use]
    pub fn with_max_attempts(mut self, max_attempts: u16) -> Self {
        self.max_attempts = i32::from(max_attempts.max(1));
        self
    }

    /// The configured maximum number of attempts.
    #[must_use]
    pub const fn max_attempts(&self) -> i32 {
        self.max_attempts
    }

    /// Processes one batch of due events. Rows are locked with `SKIP LOCKED`, so several
    /// workers can dispatch concurrently without delivering the same event twice at once.
    ///
    /// # Errors
    ///
    /// Fails on database errors. Handler errors do not fail the pass.
    pub async fn dispatch_once(&self) -> Result<DispatchStats, DispatchError> {
        let now = self.clock.now();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All).await?;
        let rows = sqlx::query!(
            r#"
            SELECT id, tenant_id, event_type, aggregate_type, aggregate_id, actor_kind, actor_id,
                   occurred_at, payload, attempts
            FROM outbox_events
            WHERE published_at IS NULL AND dead_at IS NULL AND next_attempt_at <= $1
            ORDER BY next_attempt_at, id
            LIMIT $2
            FOR UPDATE SKIP LOCKED
            "#,
            now,
            self.batch_size,
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut stats = DispatchStats::default();
        for row in rows {
            let result = match Actor::from_parts(&row.actor_kind, row.actor_id) {
                Ok(actor) => {
                    let event = EventEnvelope {
                        id: EventId::from_uuid(row.id),
                        event_type: row.event_type,
                        aggregate_type: row.aggregate_type,
                        aggregate_id: row.aggregate_id,
                        tenant_id: row.tenant_id.map(TenantId::from_uuid),
                        actor,
                        occurred_at: row.occurred_at,
                        payload: row.payload,
                    };
                    self.deliver(&event).await
                }
                Err(e) => Err(format!("corrupt actor: {e}")),
            };
            let attempts = row.attempts.saturating_add(1);
            match result {
                Ok(()) => {
                    sqlx::query!(
                        "UPDATE outbox_events SET published_at = $2, attempts = $3, last_error = NULL WHERE id = $1",
                        row.id,
                        now,
                        attempts,
                    )
                    .execute(&mut *tx)
                    .await?;
                    stats.published += 1;
                }
                Err(error) => {
                    let dead = attempts >= self.max_attempts;
                    tracing::warn!(event_id = %row.id, attempts, dead, %error, "outbox delivery failed");
                    sqlx::query!(
                        r#"
                        UPDATE outbox_events
                        SET attempts = $2, last_error = $3, next_attempt_at = $4,
                            dead_at = CASE WHEN $5 THEN $6::timestamptz END
                        WHERE id = $1
                        "#,
                        row.id,
                        attempts,
                        error,
                        retry_at(now, attempts),
                        dead,
                        now,
                    )
                    .execute(&mut *tx)
                    .await?;
                    if dead {
                        stats.dead += 1;
                    } else {
                        stats.retried += 1;
                    }
                }
            }
        }
        tx.commit().await?;
        Ok(stats)
    }

    async fn deliver(&self, event: &EventEnvelope) -> Result<(), String> {
        let mut errors = Vec::new();
        for handler in self
            .handlers
            .iter()
            .filter(|h| h.handles(&event.event_type))
        {
            if let Err(e) = handler.handle(event).await {
                errors.push(format!("{}: {e}", handler.name()));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

/// When to retry after `attempts` failed attempts: 2^attempts seconds, capped at one hour.
#[must_use]
pub fn retry_at(now: DateTime<Utc>, attempts: i32) -> DateTime<Utc> {
    let exponent = u32::try_from(attempts.clamp(0, 12)).unwrap_or(12);
    let seconds = 2_i64.pow(exponent).min(3600);
    now + Duration::seconds(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use proptest::prelude::*;

    #[test]
    fn backoff_doubles_then_caps_at_one_hour() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(retry_at(now, 1), now + Duration::seconds(2));
        assert_eq!(retry_at(now, 5), now + Duration::seconds(32));
        assert_eq!(retry_at(now, 12), now + Duration::seconds(3600));
        assert_eq!(retry_at(now, 50), now + Duration::seconds(3600));
    }

    proptest! {
        #[test]
        fn backoff_is_always_in_the_future_and_bounded(attempts in any::<i32>()) {
            let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
            let at = retry_at(now, attempts);
            prop_assert!(at > now);
            prop_assert!(at <= now + Duration::hours(1));
        }
    }
}
