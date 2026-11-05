//! System log (FR-SYS-003): warnings and errors stored in `system_log_entries` so
//! Administrators can see them in Cooper without access to the servers' log output.
//!
//! [`SystemLogLayer`] is a `tracing` layer that copies `WARN` and `ERROR` events into a
//! bounded channel; [`SystemLogWriter`] drains it into the database in batches. When the
//! channel is full, entries are dropped (and counted) rather than slowing requests down.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveTime, SubsecRound as _, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use uuid::Uuid;

use crate::jobs::{Job, JobContext, JobError, Schedule};

/// Channel capacity between the layer and the writer.
const CHANNEL_CAPACITY: usize = 10_000;
/// Rows inserted per batch.
const BATCH_SIZE: usize = 200;
/// Longest message and field value stored, in characters.
const MAX_TEXT: usize = 4_000;

/// One captured warning or error.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemLogEntry {
    /// Entry ID.
    pub id: Uuid,
    /// When it happened.
    pub occurred_at: DateTime<Utc>,
    /// `warn` or `error`.
    pub level: String,
    /// Which binary wrote it, for example `server` or `worker`.
    pub source: String,
    /// The `tracing` target (module path).
    pub target: String,
    /// The message.
    pub message: String,
    /// Structured fields of the event.
    pub fields: Value,
    /// The HTTP request ID, when the event happened during a request.
    pub request_id: Option<String>,
}

/// Targets never stored: the writer's own database driver, to avoid feedback loops.
fn excluded(target: &str) -> bool {
    target.starts_with("sqlx") || target == "cooper_platform::system_log"
}

/// Now, at the microsecond precision PostgreSQL stores.
fn now_micros() -> DateTime<Utc> {
    Utc::now().trunc_subsecs(6)
}

fn truncate(mut s: String) -> String {
    if s.chars().count() > MAX_TEXT {
        s = s.chars().take(MAX_TEXT).collect();
        s.push('…');
    }
    s
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: BTreeMap<String, Value>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let text = truncate(format!("{value:?}"));
        if field.name() == "message" {
            self.message = Some(text);
        } else {
            self.fields
                .insert(field.name().to_owned(), Value::String(text));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let text = truncate(value.to_owned());
        if field.name() == "message" {
            self.message = Some(text);
        } else {
            self.fields
                .insert(field.name().to_owned(), Value::String(text));
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(field.name().to_owned(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(field.name().to_owned(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name().to_owned(), value.into());
    }
}

/// The request ID recorded on a span, kept in the span's extensions.
struct RequestId(String);

/// `tracing` layer that forwards warnings and errors to a [`SystemLogWriter`].
pub struct SystemLogLayer {
    source: &'static str,
    sender: mpsc::Sender<SystemLogEntry>,
    dropped: Arc<AtomicU64>,
}

/// Drains captured entries into `system_log_entries`.
pub struct SystemLogWriter {
    receiver: mpsc::Receiver<SystemLogEntry>,
    dropped: Arc<AtomicU64>,
}

/// Creates a connected layer and writer. `source` names the binary (`server`, `worker`).
#[must_use]
pub fn channel(source: &'static str) -> (SystemLogLayer, SystemLogWriter) {
    let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
    let dropped = Arc::new(AtomicU64::new(0));
    (
        SystemLogLayer {
            source,
            sender,
            dropped: dropped.clone(),
        },
        SystemLogWriter { receiver, dropped },
    )
}

impl<S> Layer<S> for SystemLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        if let (Some(Value::String(request_id)), Some(span)) =
            (visitor.fields.remove("request_id"), ctx.span(id))
            && !request_id.is_empty()
        {
            span.extensions_mut().insert(RequestId(request_id));
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        if level > Level::WARN || excluded(event.metadata().target()) {
            return;
        }
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let request_id = ctx.event_scope(event).and_then(|scope| {
            scope
                .from_root()
                .find_map(|span| span.extensions().get::<RequestId>().map(|r| r.0.clone()))
        });
        let entry = SystemLogEntry {
            id: Uuid::now_v7(),
            occurred_at: now_micros(),
            level: if level == Level::ERROR {
                "error"
            } else {
                "warn"
            }
            .to_owned(),
            source: self.source.to_owned(),
            target: event.metadata().target().to_owned(),
            message: visitor.message.unwrap_or_default(),
            fields: Value::Object(visitor.fields.into_iter().collect()),
            request_id,
        };
        if self.sender.try_send(entry).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl SystemLogWriter {
    /// Writes entries until every [`SystemLogLayer`] is dropped. Spawn it on the runtime
    /// once the pool exists; entries logged before that wait in the channel.
    pub async fn run(mut self, pool: PgPool) {
        let mut batch = Vec::with_capacity(BATCH_SIZE);
        loop {
            let received = self.receiver.recv_many(&mut batch, BATCH_SIZE).await;
            if received == 0 {
                return;
            }
            let dropped = self.dropped.swap(0, Ordering::Relaxed);
            if dropped > 0 {
                batch.push(dropped_entry(dropped));
            }
            if let Err(error) = insert(&pool, &batch).await {
                // Not a WARN: a database outage must not feed back into this writer.
                eprintln!(
                    "system log: failed to store {} entries: {error}",
                    batch.len()
                );
                tokio::time::sleep(StdDuration::from_secs(1)).await;
            }
            batch.clear();
        }
    }
}

fn dropped_entry(count: u64) -> SystemLogEntry {
    SystemLogEntry {
        id: Uuid::now_v7(),
        occurred_at: now_micros(),
        level: "warn".into(),
        source: "system-log".into(),
        target: "cooper_platform::system_log".into(),
        message: format!("{count} log entries were dropped because the system log was busy"),
        fields: Value::Object(serde_json::Map::new()),
        request_id: None,
    }
}

/// Stores entries in one statement.
///
/// # Errors
///
/// Database errors.
pub async fn insert(pool: &PgPool, entries: &[SystemLogEntry]) -> Result<(), sqlx::Error> {
    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
    let at: Vec<DateTime<Utc>> = entries.iter().map(|e| e.occurred_at).collect();
    let levels: Vec<String> = entries.iter().map(|e| e.level.clone()).collect();
    let sources: Vec<String> = entries.iter().map(|e| e.source.clone()).collect();
    let targets: Vec<String> = entries.iter().map(|e| e.target.clone()).collect();
    let messages: Vec<String> = entries.iter().map(|e| e.message.clone()).collect();
    let fields: Vec<Value> = entries.iter().map(|e| e.fields.clone()).collect();
    let request_ids: Vec<Option<String>> = entries.iter().map(|e| e.request_id.clone()).collect();
    sqlx::query!(
        "INSERT INTO system_log_entries (id, occurred_at, level, source, target, message, fields, request_id)
         SELECT * FROM UNNEST($1::uuid[], $2::timestamptz[], $3::text[], $4::text[], $5::text[], $6::text[], $7::jsonb[], $8::text[])",
        &ids,
        &at,
        &levels,
        &sources,
        &targets,
        &messages,
        &fields,
        &request_ids as &[Option<String>],
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Filter for [`query`].
#[derive(Debug, Clone, Default)]
pub struct SystemLogQuery {
    /// Only this level (`warn` or `error`).
    pub level: Option<String>,
    /// Only targets starting with this prefix.
    pub target_prefix: Option<String>,
    /// Case-insensitive substring of the message.
    pub search: Option<String>,
    /// Only entries before this `(occurred_at, id)` (from a previous page).
    pub before: Option<(DateTime<Utc>, Uuid)>,
    /// Rows to fetch.
    pub limit: i64,
}

/// Entries matching `filter`, newest first.
///
/// # Errors
///
/// Database errors.
pub async fn query(
    pool: &PgPool,
    filter: &SystemLogQuery,
) -> Result<Vec<SystemLogEntry>, sqlx::Error> {
    let (before_at, before_id) = filter.before.unzip();
    let search = filter.search.as_ref().map(|s| {
        format!(
            "%{}%",
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    let prefix = filter.target_prefix.as_ref().map(|p| {
        format!(
            "{}%",
            p.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    sqlx::query_as!(
        SystemLogEntry,
        "SELECT id, occurred_at, level, source, target, message, fields, request_id
         FROM system_log_entries
         WHERE ($1::text IS NULL OR level = $1)
           AND ($2::text IS NULL OR target LIKE $2)
           AND ($3::text IS NULL OR message ILIKE $3)
           AND ($4::timestamptz IS NULL OR (occurred_at, id) < ($4, $5))
         ORDER BY occurred_at DESC, id DESC
         LIMIT $6",
        filter.level,
        prefix,
        search,
        before_at,
        before_id,
        filter.limit
    )
    .fetch_all(pool)
    .await
}

/// Deletes system log entries older than the retention period. Daily at 03:15 UTC.
#[derive(Debug, Clone, Copy)]
pub struct PruneSystemLog {
    retention_days: u16,
}

impl PruneSystemLog {
    /// Keeps `retention_days` days (at least one).
    #[must_use]
    pub fn new(retention_days: u16) -> Self {
        Self {
            retention_days: retention_days.max(1),
        }
    }
}

#[async_trait]
impl Job for PruneSystemLog {
    fn name(&self) -> &'static str {
        "system-log-prune"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::Daily {
            at: NaiveTime::from_hms_opt(3, 15, 0).unwrap_or(NaiveTime::MIN),
        }
    }

    async fn run(&self, ctx: &JobContext) -> Result<Option<String>, JobError> {
        let cutoff = ctx.clock.now() - Duration::days(i64::from(self.retention_days));
        let deleted = sqlx::query!(
            "DELETE FROM system_log_entries WHERE occurred_at < $1",
            cutoff
        )
        .execute(&ctx.pool)
        .await?
        .rows_affected();
        Ok(Some(format!(
            "deleted {deleted} entries older than {} days",
            self.retention_days
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt as _;

    #[tokio::test]
    async fn captures_warnings_and_errors_with_request_id() {
        let (layer, mut writer) = channel("test");
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("http_request", request_id = "req-1");
            let _guard = span.enter();
            tracing::info!("ignored");
            tracing::warn!(
                mailbox = "it@acme.example",
                attempts = 3,
                "mailbox poll failed"
            );
            tracing::error!(target: "sqlx::query", "ignored to avoid loops");
        });
        let entry = writer.receiver.try_recv().unwrap();
        assert_eq!(entry.level, "warn");
        assert_eq!(entry.message, "mailbox poll failed");
        assert_eq!(entry.fields["attempts"], 3);
        assert_eq!(entry.fields["mailbox"], "it@acme.example");
        assert_eq!(entry.request_id.as_deref(), Some("req-1"));
        assert_eq!(entry.source, "test");
        assert!(
            writer.receiver.try_recv().is_err(),
            "info and sqlx events are skipped"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn stores_queries_and_prunes(pool: PgPool) {
        let old = SystemLogEntry {
            occurred_at: Utc::now() - Duration::days(40),
            ..dropped_entry(1)
        };
        let new = SystemLogEntry {
            level: "error".into(),
            message: "IMAP login failed".into(),
            target: "cooper_service_desk::mail".into(),
            ..dropped_entry(2)
        };
        insert(&pool, &[old, new.clone()]).await.unwrap();
        let errors = query(
            &pool,
            &SystemLogQuery {
                level: Some("error".into()),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(errors, vec![new.clone()]);
        let by_target = query(
            &pool,
            &SystemLogQuery {
                target_prefix: Some("cooper_service_desk".into()),
                search: Some("imap".into()),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(by_target.len(), 1);

        let ctx = JobContext {
            pool: pool.clone(),
            clock: Arc::new(cooper_kernel::SystemClock),
        };
        let message = PruneSystemLog::new(30).run(&ctx).await.unwrap().unwrap();
        assert!(message.starts_with("deleted 1 "), "{message}");
    }
}
