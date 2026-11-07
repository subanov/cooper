//! Live Updates (ADR-0022, FR-GEN-010).
//!
//! Writers call [`publish`] inside the transaction that changes a record. It sends a
//! PostgreSQL `NOTIFY` on [`CHANNEL`], which is delivered only when that transaction
//! commits. Every `cooper-server` process holds one [`LiveFeed`]: a single `LISTEN`
//! connection that re-broadcasts each [`LiveChange`] to the open Server-Sent Events
//! streams, which decide per Analyst whether to pass it on.
//!
//! A [`LiveChange`] says only *which* record changed (Company, topic, ID). It never
//! carries the record's data: browsers re-read it through the API, which applies the
//! usual access checks.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use cooper_kernel::TenantId;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgListener;
use sqlx::{PgConnection, PgPool};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

/// The PostgreSQL notification channel.
pub const CHANNEL: &str = "cooper_live";

/// How many signals a slow subscriber may fall behind before it gets
/// [`LiveSignal::Resync`] instead.
const BUFFER: usize = 1024;

/// Longest wait between attempts to (re)connect the listener.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// One record changed. The payload of a `NOTIFY` on [`CHANNEL`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiveChange {
    /// The record's Company.
    pub tenant_id: TenantId,
    /// What kind of record: the aggregate type, for example `ticket`.
    pub topic: String,
    /// The record's ID.
    pub id: Uuid,
}

impl LiveChange {
    /// A change of record `id` of kind `topic` in Company `tenant_id`.
    #[must_use]
    pub fn new(tenant_id: TenantId, topic: impl Into<String>, id: Uuid) -> Self {
        Self {
            tenant_id,
            topic: topic.into(),
            id,
        }
    }
}

/// Announces `change` when the caller's transaction commits. Identical announcements in
/// one transaction are delivered once (PostgreSQL de-duplicates them).
///
/// # Errors
///
/// Fails on database errors.
pub async fn publish(conn: &mut PgConnection, change: &LiveChange) -> Result<(), sqlx::Error> {
    let payload = serde_json::to_string(change).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
    sqlx::query!("SELECT pg_notify($1, $2)", CHANNEL, payload)
        .execute(conn)
        .await?;
    Ok(())
}

/// What a subscriber receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveSignal {
    /// A record changed.
    Changed(LiveChange),
    /// Changes may have been missed (the listener reconnected or the subscriber fell
    /// behind): everything live data covers should be read again.
    Resync,
}

/// Fans `NOTIFY`s on [`CHANNEL`] out to subscribers. Cheap to clone.
///
/// The `LISTEN` connection is opened on the first [`LiveFeed::subscribe`], so processes
/// that never subscribe (`cooper-worker`) never hold one.
#[derive(Clone)]
pub struct LiveFeed {
    inner: Arc<Inner>,
}

struct Inner {
    pool: PgPool,
    signals: broadcast::Sender<LiveSignal>,
    listening: watch::Sender<bool>,
    shutdown: watch::Sender<bool>,
    started: OnceLock<()>,
}

impl LiveFeed {
    /// A feed reading notifications through a connection from `pool`.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            inner: Arc::new(Inner {
                pool,
                signals: broadcast::channel(BUFFER).0,
                listening: watch::channel(false).0,
                shutdown: watch::channel(false).0,
                started: OnceLock::new(),
            }),
        }
    }

    /// Subscribes to changes committed from now on. Starts the listener if needed; use
    /// [`LiveFeed::listening`] to wait until it is connected.
    ///
    /// Must be called within a Tokio runtime.
    #[must_use]
    pub fn subscribe(&self) -> LiveSubscription {
        let rx = self.inner.signals.subscribe();
        self.inner.started.get_or_init(|| {
            tokio::spawn(listen(self.inner.clone()));
        });
        LiveSubscription { rx }
    }

    /// Whether the listener is connected, as a watch: `wait_for(|l| *l)` resolves once
    /// notifications are being received.
    #[must_use]
    pub fn listening(&self) -> watch::Receiver<bool> {
        self.inner.listening.subscribe()
    }

    /// Stops the listener and tells every stream to end (server shutdown).
    pub fn shutdown(&self) {
        self.inner.shutdown.send_replace(true);
    }

    /// Whether [`LiveFeed::shutdown`] was called, as a watch.
    #[must_use]
    pub fn shutting_down(&self) -> watch::Receiver<bool> {
        self.inner.shutdown.subscribe()
    }
}

/// One subscriber's view of a [`LiveFeed`].
pub struct LiveSubscription {
    rx: broadcast::Receiver<LiveSignal>,
}

impl LiveSubscription {
    /// The next signal; `None` once the feed is gone. Falling behind yields
    /// [`LiveSignal::Resync`].
    pub async fn recv(&mut self) -> Option<LiveSignal> {
        match self.rx.recv().await {
            Ok(signal) => Some(signal),
            Err(broadcast::error::RecvError::Lagged(_)) => Some(LiveSignal::Resync),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }

    /// A signal that is already waiting, without blocking.
    pub fn try_recv(&mut self) -> Option<LiveSignal> {
        match self.rx.try_recv() {
            Ok(signal) => Some(signal),
            Err(broadcast::error::TryRecvError::Lagged(_)) => Some(LiveSignal::Resync),
            Err(_) => None,
        }
    }
}

/// The listener task: (re)connects, `LISTEN`s and re-broadcasts until shutdown.
async fn listen(inner: Arc<Inner>) {
    let mut shutdown = inner.shutdown.subscribe();
    let mut connected_before = false;
    let mut backoff = Duration::from_secs(1);
    loop {
        if *shutdown.borrow() || inner.pool.is_closed() {
            break;
        }
        match connect(&inner.pool).await {
            Ok(mut listener) => {
                backoff = Duration::from_secs(1);
                inner.listening.send_replace(true);
                if connected_before {
                    // Notifications sent while disconnected are lost.
                    let _ = inner.signals.send(LiveSignal::Resync);
                }
                connected_before = true;
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => break,
                        received = listener.try_recv() => match received {
                            Ok(Some(notification)) => {
                                match serde_json::from_str::<LiveChange>(notification.payload()) {
                                    Ok(change) => {
                                        let _ = inner.signals.send(LiveSignal::Changed(change));
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, "ignoring malformed live update");
                                    }
                                }
                            }
                            Ok(None) => {
                                tracing::warn!("live updates listener reconnected; changes may have been missed");
                                let _ = inner.signals.send(LiveSignal::Resync);
                            }
                            Err(error) => {
                                tracing::warn!(%error, "live updates listener disconnected");
                                break;
                            }
                        }
                    }
                }
                inner.listening.send_replace(false);
            }
            Err(error) => {
                tracing::warn!(%error, "live updates listener cannot connect");
            }
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            () = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
    inner.listening.send_replace(false);
}

async fn connect(pool: &PgPool) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(CHANNEL).await?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_names_only_company_topic_and_id() {
        let change = LiveChange::new(TenantId::new(), "ticket", Uuid::now_v7());
        let json: serde_json::Value = serde_json::to_value(&change).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["id", "tenant_id", "topic"]);
        let back: LiveChange = serde_json::from_value(json).unwrap();
        assert_eq!(back, change);
    }
}
