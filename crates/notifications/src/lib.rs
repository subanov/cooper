//! Cooper notifications context (ADR-0016): in-app Notifications, the bell
//! (FR-GEN-006).
//!
//! Notifications are created by event handlers reacting to other contexts' domain events
//! (assignments, `@mentions`, SLA Warnings and Breaches) and read by Analysts through the
//! API. Delivery is idempotent: one Notification per source event and recipient.

pub mod api;
pub mod handlers;

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, Cursor, EventId, Forbidden, Limit, Page, Principal, TenantId, TicketId,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

/// What a Notification is about (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// A ticket was assigned to the recipient.
    TicketAssigned,
    /// The recipient was mentioned in a Note.
    Mentioned,
    /// An SLA target is about to be missed.
    SlaWarning,
    /// An SLA target was missed.
    SlaBreached,
    /// A Requester replied on a ticket the recipient is assigned to.
    ReplyReceived,
}

impl NotificationKind {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TicketAssigned => "ticket_assigned",
            Self::Mentioned => "mentioned",
            Self::SlaWarning => "sla_warning",
            Self::SlaBreached => "sla_breached",
            Self::ReplyReceived => "reply_received",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        [
            Self::TicketAssigned,
            Self::Mentioned,
            Self::SlaWarning,
            Self::SlaBreached,
            Self::ReplyReceived,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

/// One entry in an Analyst's bell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// ID.
    pub id: Uuid,
    /// Who sees it.
    pub recipient_id: AnalystId,
    /// Company of the record it is about.
    pub tenant_id: Option<TenantId>,
    /// What it is about.
    pub kind: NotificationKind,
    /// Short text.
    pub title: String,
    /// Longer text.
    pub body: String,
    /// The ticket it links to.
    pub ticket_id: Option<TicketId>,
    /// The domain event that caused it.
    pub source_event_id: EventId,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// When the recipient read it.
    pub read_at: Option<DateTime<Utc>>,
}

/// A Notification to send to several Analysts.
#[derive(Debug, Clone)]
pub struct Draft {
    /// What it is about.
    pub kind: NotificationKind,
    /// Company.
    pub tenant_id: Option<TenantId>,
    /// Short text (truncated to 300 characters).
    pub title: String,
    /// Longer text.
    pub body: String,
    /// Linked ticket.
    pub ticket_id: Option<TicketId>,
    /// Source event.
    pub source_event_id: EventId,
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// Only Analysts have a bell.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not the caller's.
    #[error("Notification not found")]
    NotFound,
    /// Invalid cursor.
    #[error("invalid cursor")]
    InvalidCursor,
    /// Storage failure; logged.
    #[error("internal error")]
    Internal,
}

/// Storage of Notifications.
#[async_trait]
pub trait NotificationStore: Send + Sync {
    /// Stores one Notification per recipient; repeats of the same source event and
    /// recipient are ignored. Returns how many were new.
    async fn deliver(
        &self,
        draft: &Draft,
        recipients: &BTreeSet<AnalystId>,
        now: DateTime<Utc>,
    ) -> Result<u64, String>;
    /// A recipient's Notifications, newest first.
    async fn list(
        &self,
        recipient: AnalystId,
        unread_only: bool,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: i64,
    ) -> Result<Vec<Notification>, String>;
    /// Number of unread Notifications.
    async fn unread_count(&self, recipient: AnalystId) -> Result<i64, String>;
    /// Marks Notifications read (all of the recipient's when `id` is `None`); returns
    /// how many changed.
    async fn mark_read(
        &self,
        recipient: AnalystId,
        id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<u64, String>;
}

/// The notifications application service.
#[derive(Clone)]
pub struct Notifications {
    store: Arc<dyn NotificationStore>,
    clock: Arc<dyn cooper_kernel::Clock>,
}

fn internal(e: String) -> AppError {
    tracing::error!(error = %e, "notification storage error");
    AppError::Internal
}

fn me(principal: &Principal) -> Result<AnalystId, AppError> {
    match principal.actor() {
        Actor::Analyst(id) => Ok(id),
        _ => Err(Forbidden("an Analyst session".into()).into()),
    }
}

impl Notifications {
    /// Creates the service.
    #[must_use]
    pub fn new(store: Arc<dyn NotificationStore>, clock: Arc<dyn cooper_kernel::Clock>) -> Self {
        Self { store, clock }
    }

    /// Sends a Notification to recipients (from event handlers). The actor who caused
    /// the event is never notified about their own action.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn send(
        &self,
        mut draft: Draft,
        mut recipients: BTreeSet<AnalystId>,
        actor: Actor,
    ) -> Result<u64, AppError> {
        if let Actor::Analyst(a) = actor {
            recipients.remove(&a);
        }
        if recipients.is_empty() {
            return Ok(0);
        }
        draft.title = draft.title.chars().take(300).collect();
        self.store
            .deliver(&draft, &recipients, self.clock.now())
            .await
            .map_err(internal)
    }

    /// The caller's Notifications, newest first.
    ///
    /// # Errors
    ///
    /// Not an Analyst, invalid cursor or storage failures.
    pub async fn mine(
        &self,
        principal: &Principal,
        unread_only: bool,
        limit: Limit,
        cursor: Option<&Cursor>,
    ) -> Result<Page<Notification>, AppError> {
        let me = me(principal)?;
        let before = cursor
            .map(Cursor::decode::<(DateTime<Utc>, Uuid)>)
            .transpose()
            .map_err(|_| AppError::InvalidCursor)?;
        let rows = self
            .store
            .list(me, unread_only, before, limit.fetch_size())
            .await
            .map_err(internal)?;
        Page::from_rows(rows, limit, |n| Cursor::encode(&(n.created_at, n.id)))
            .map_err(|_| AppError::InvalidCursor)
    }

    /// Number of the caller's unread Notifications.
    ///
    /// # Errors
    ///
    /// Not an Analyst or storage failures.
    pub async fn unread_count(&self, principal: &Principal) -> Result<i64, AppError> {
        self.store
            .unread_count(me(principal)?)
            .await
            .map_err(internal)
    }

    /// Marks one of the caller's Notifications (or all, with `None`) read.
    ///
    /// # Errors
    ///
    /// Not an Analyst, not found, or storage failures.
    pub async fn mark_read(
        &self,
        principal: &Principal,
        id: Option<Uuid>,
    ) -> Result<u64, AppError> {
        let changed = self
            .store
            .mark_read(me(principal)?, id, self.clock.now())
            .await
            .map_err(internal)?;
        Ok(changed)
    }
}

/// PostgreSQL storage of Notifications. Notifications are per Analyst, so queries filter
/// by recipient; the tenant scope is `All` because an Analyst's bell spans every Company
/// they could see when it was created.
#[derive(Clone)]
pub struct PgNotificationStore {
    pool: PgPool,
}

impl PgNotificationStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl NotificationStore for PgNotificationStore {
    async fn deliver(
        &self,
        d: &Draft,
        recipients: &BTreeSet<AnalystId>,
        now: DateTime<Utc>,
    ) -> Result<u64, String> {
        let ids: Vec<Uuid> = recipients.iter().map(|_| Uuid::now_v7()).collect();
        let to: Vec<Uuid> = recipients.iter().map(|a| a.as_uuid()).collect();
        let mut tx = cooper_platform::tenancy::begin(&self.pool, &cooper_kernel::TenantScope::All)
            .await
            .map_err(|e| e.to_string())?;
        let inserted = sqlx::query!(
            "INSERT INTO notifications (id, recipient_id, tenant_id, kind, title, body, ticket_id, source_event_id, created_at)
             SELECT n.id, n.recipient, $3, $4, $5, $6, $7, $8, $9 FROM UNNEST($1::uuid[], $2::uuid[]) AS n(id, recipient)
             JOIN analysts a ON a.id = n.recipient AND a.is_active
             ON CONFLICT (source_event_id, recipient_id) DO NOTHING",
            &ids,
            &to,
            d.tenant_id.map(|t| t.as_uuid()),
            d.kind.as_str(),
            d.title,
            d.body,
            d.ticket_id.map(|t| t.as_uuid()),
            d.source_event_id.as_uuid(),
            now
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(inserted)
    }

    async fn list(
        &self,
        recipient: AnalystId,
        unread_only: bool,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: i64,
    ) -> Result<Vec<Notification>, String> {
        let (before_at, before_id) = before.unzip();
        let mut tx = cooper_platform::tenancy::begin(&self.pool, &cooper_kernel::TenantScope::All)
            .await
            .map_err(|e| e.to_string())?;
        let rows = sqlx::query!(
            "SELECT id, recipient_id, tenant_id, kind, title, body, ticket_id, source_event_id, created_at, read_at
             FROM notifications
             WHERE recipient_id = $1 AND (NOT $2 OR read_at IS NULL)
               AND ($3::timestamptz IS NULL OR (created_at, id) < ($3, $4))
             ORDER BY created_at DESC, id DESC LIMIT $5",
            recipient.as_uuid(),
            unread_only,
            before_at,
            before_id,
            limit
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|r| {
                Ok(Notification {
                    id: r.id,
                    recipient_id: AnalystId::from_uuid(r.recipient_id),
                    tenant_id: r.tenant_id.map(TenantId::from_uuid),
                    kind: NotificationKind::parse(&r.kind)
                        .ok_or_else(|| format!("unknown kind {}", r.kind))?,
                    title: r.title,
                    body: r.body,
                    ticket_id: r.ticket_id.map(TicketId::from_uuid),
                    source_event_id: EventId::from_uuid(r.source_event_id),
                    created_at: r.created_at,
                    read_at: r.read_at,
                })
            })
            .collect()
    }

    async fn unread_count(&self, recipient: AnalystId) -> Result<i64, String> {
        let mut tx = cooper_platform::tenancy::begin(&self.pool, &cooper_kernel::TenantScope::All)
            .await
            .map_err(|e| e.to_string())?;
        let n = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM notifications WHERE recipient_id = $1 AND read_at IS NULL"#,
            recipient.as_uuid()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }

    async fn mark_read(
        &self,
        recipient: AnalystId,
        id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<u64, String> {
        let mut tx = cooper_platform::tenancy::begin(&self.pool, &cooper_kernel::TenantScope::All)
            .await
            .map_err(|e| e.to_string())?;
        let n = sqlx::query!(
            "UPDATE notifications SET read_at = $3 WHERE recipient_id = $1 AND read_at IS NULL AND ($2::uuid IS NULL OR id = $2)",
            recipient.as_uuid(),
            id,
            now
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(n)
    }
}
