//! Portal Notifications and Notification Preferences (FR-PRT-007, ADR-0033).
//!
//! A Requester sees in-app Portal Notifications about their own tickets on the Portal
//! and chooses, per kind, whether they want them in-app and by email. Email means the
//! automated Email Templates sent to the Requester (FR-EML-009); Replies from Analysts
//! are the conversation itself and are always sent. A Requester only ever sees their own
//! Portal Notifications, in the Company the Portal session acts for.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AuditAction, AuditRecord, Cursor, EventId, Forbidden, Limit, Page, Principal,
    RequesterId, TenantId, TenantScope, TicketId,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppError;

/// What a Portal Notification is about (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortalNotificationKind {
    /// A ticket was logged for the Requester (by an Analyst, by email or on the Portal).
    TicketCreated,
    /// An Analyst replied, or added a public Note, on the Requester's ticket. By email:
    /// the acknowledgement of the Requester's own emailed reply.
    TicketReplied,
    /// The Requester's ticket was closed.
    TicketClosed,
}

impl PortalNotificationKind {
    /// Every kind.
    pub const ALL: [Self; 3] = [Self::TicketCreated, Self::TicketReplied, Self::TicketClosed];

    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TicketCreated => "ticket_created",
            Self::TicketReplied => "ticket_replied",
            Self::TicketClosed => "ticket_closed",
        }
    }

    /// Parses a storage name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// A Requester's choice for one kind. Without a stored choice both are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NotificationPreference {
    /// The kind.
    pub kind: PortalNotificationKind,
    /// Shown on the Portal.
    pub in_app: bool,
    /// Sent by email (the Email Template of the event).
    pub email: bool,
}

impl NotificationPreference {
    /// The default: everything on.
    #[must_use]
    pub const fn default_for(kind: PortalNotificationKind) -> Self {
        Self {
            kind,
            in_app: true,
            email: true,
        }
    }
}

/// One preference per kind, in [`PortalNotificationKind::ALL`] order: the stored choice
/// or the default. A later entry for the same kind wins.
#[must_use]
pub fn effective_preferences(stored: &[NotificationPreference]) -> Vec<NotificationPreference> {
    PortalNotificationKind::ALL
        .into_iter()
        .map(|kind| {
            stored
                .iter()
                .rev()
                .find(|p| p.kind == kind)
                .copied()
                .unwrap_or(NotificationPreference::default_for(kind))
        })
        .collect()
}

/// A Portal Notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalNotification {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// Who sees it.
    pub requester_id: RequesterId,
    /// What it is about.
    pub kind: PortalNotificationKind,
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
    /// When the Requester read it.
    pub read_at: Option<DateTime<Utc>>,
}

/// A Portal Notification to deliver.
#[derive(Debug, Clone)]
pub struct PortalDraft {
    /// What it is about.
    pub kind: PortalNotificationKind,
    /// Company.
    pub tenant_id: TenantId,
    /// Recipient.
    pub requester_id: RequesterId,
    /// Short text (truncated to 300 characters).
    pub title: String,
    /// Longer text.
    pub body: String,
    /// Linked ticket.
    pub ticket_id: Option<TicketId>,
    /// Source event (one Notification per event and Requester).
    pub source_event_id: EventId,
}

/// Storage of Portal Notifications and Notification Preferences. Every call is for one
/// Requester in one Company.
#[async_trait]
pub trait PortalNotificationStore: Send + Sync {
    /// Stores a Notification unless the same event already did; `true` when new.
    async fn deliver(&self, draft: &PortalDraft, now: DateTime<Utc>) -> Result<bool, String>;
    /// A Requester's Notifications, newest first.
    async fn list(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        unread_only: bool,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: i64,
    ) -> Result<Vec<PortalNotification>, String>;
    /// Unread count.
    async fn unread_count(&self, tenant: TenantId, requester: RequesterId) -> Result<i64, String>;
    /// Marks one (or, with `None`, every) Notification read; returns how many changed.
    async fn mark_read(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<u64, String>;
    /// Stored preferences (kinds without a choice are missing).
    async fn preferences(
        &self,
        tenant: TenantId,
        requester: RequesterId,
    ) -> Result<Vec<NotificationPreference>, String>;
    /// Replaces the stored preferences, with the audit record, in one transaction.
    async fn save_preferences(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        preferences: &[NotificationPreference],
        audit: &AuditRecord,
        now: DateTime<Utc>,
    ) -> Result<(), String>;
}

/// The Portal Notifications application service.
#[derive(Clone)]
pub struct PortalNotifications {
    store: Arc<dyn PortalNotificationStore>,
    clock: Arc<dyn cooper_kernel::Clock>,
}

fn internal(e: String) -> AppError {
    tracing::error!(error = %e, "portal notification storage error");
    AppError::Internal
}

/// The Requester and Company of a Portal session.
fn me(principal: &Principal) -> Result<(RequesterId, TenantId), AppError> {
    let forbidden = || AppError::Forbidden(Forbidden("a Portal session".into()));
    let Actor::Requester(requester) = principal.actor() else {
        return Err(forbidden());
    };
    match principal.scope() {
        TenantScope::Tenants(ids) if ids.len() == 1 => ids
            .iter()
            .next()
            .map(|t| (requester, *t))
            .ok_or_else(forbidden),
        _ => Err(forbidden()),
    }
}

impl PortalNotifications {
    /// Creates the service.
    #[must_use]
    pub fn new(
        store: Arc<dyn PortalNotificationStore>,
        clock: Arc<dyn cooper_kernel::Clock>,
    ) -> Self {
        Self { store, clock }
    }

    /// Delivers a Portal Notification (from event handlers) unless the Requester caused
    /// the event themselves or turned the kind off in-app. Returns whether one was made.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn send(&self, mut draft: PortalDraft, actor: Actor) -> Result<bool, AppError> {
        if actor == Actor::Requester(draft.requester_id) {
            return Ok(false);
        }
        if !self
            .preference(draft.tenant_id, draft.requester_id, draft.kind)
            .await?
            .in_app
        {
            return Ok(false);
        }
        draft.title = draft.title.chars().take(300).collect();
        if draft.title.trim().is_empty() {
            draft.title = draft.kind.as_str().to_owned();
        }
        self.store
            .deliver(&draft, self.clock.now())
            .await
            .map_err(internal)
    }

    async fn preference(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        kind: PortalNotificationKind,
    ) -> Result<NotificationPreference, AppError> {
        let stored = self
            .store
            .preferences(tenant, requester)
            .await
            .map_err(internal)?;
        Ok(effective_preferences(&stored)
            .into_iter()
            .find(|p| p.kind == kind)
            .unwrap_or(NotificationPreference::default_for(kind)))
    }

    /// Whether a Requester wants the email of `kind` (for Email Templates).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn wants_email(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        kind: PortalNotificationKind,
    ) -> Result<bool, AppError> {
        Ok(self.preference(tenant, requester, kind).await?.email)
    }

    /// The signed-in Requester's Portal Notifications, newest first.
    ///
    /// # Errors
    ///
    /// Not a Portal session, invalid cursor or storage failures.
    pub async fn mine(
        &self,
        principal: &Principal,
        unread_only: bool,
        limit: Limit,
        cursor: Option<&Cursor>,
    ) -> Result<Page<PortalNotification>, AppError> {
        let (requester, tenant) = me(principal)?;
        let before = cursor
            .map(Cursor::decode::<(DateTime<Utc>, Uuid)>)
            .transpose()
            .map_err(|_| AppError::InvalidCursor)?;
        let rows = self
            .store
            .list(tenant, requester, unread_only, before, limit.fetch_size())
            .await
            .map_err(internal)?;
        Page::from_rows(rows, limit, |n| Cursor::encode(&(n.created_at, n.id)))
            .map_err(|_| AppError::InvalidCursor)
    }

    /// Unread count of the signed-in Requester.
    ///
    /// # Errors
    ///
    /// Not a Portal session or storage failures.
    pub async fn unread_count(&self, principal: &Principal) -> Result<i64, AppError> {
        let (requester, tenant) = me(principal)?;
        self.store
            .unread_count(tenant, requester)
            .await
            .map_err(internal)
    }

    /// Marks one of the signed-in Requester's Portal Notifications (or all) read.
    ///
    /// # Errors
    ///
    /// Not a Portal session, not found (one that is not theirs, too) or storage failures.
    pub async fn mark_read(&self, principal: &Principal, id: Option<Uuid>) -> Result<(), AppError> {
        let (requester, tenant) = me(principal)?;
        let changed = self
            .store
            .mark_read(tenant, requester, id, self.clock.now())
            .await
            .map_err(internal)?;
        // As for the Analyst bell: not theirs (or already read) is "not found".
        if id.is_some() && changed == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// The signed-in Requester's Notification Preferences, one per kind.
    ///
    /// # Errors
    ///
    /// Not a Portal session or storage failures.
    pub async fn preferences(
        &self,
        principal: &Principal,
    ) -> Result<Vec<NotificationPreference>, AppError> {
        let (requester, tenant) = me(principal)?;
        let stored = self
            .store
            .preferences(tenant, requester)
            .await
            .map_err(internal)?;
        Ok(effective_preferences(&stored))
    }

    /// Changes the signed-in Requester's Notification Preferences; kinds not given keep
    /// their setting. Audited as a change of the Requester.
    ///
    /// # Errors
    ///
    /// Not a Portal session or storage failures.
    pub async fn set_preferences(
        &self,
        principal: &Principal,
        changes: &[NotificationPreference],
    ) -> Result<Vec<NotificationPreference>, AppError> {
        let (requester, tenant) = me(principal)?;
        let before = self.preferences(principal).await?;
        let after = effective_preferences(&[before.as_slice(), changes].concat());
        if after == before {
            return Ok(after);
        }
        let now = self.clock.now();
        let audit = AuditRecord::new(
            Some(tenant),
            "requester",
            requester.as_uuid(),
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("notification_preferences", Some(&before), Some(&after));
        self.store
            .save_preferences(tenant, requester, &after, &audit, now)
            .await
            .map_err(internal)?;
        Ok(after)
    }
}

/// PostgreSQL storage; every transaction is scoped to the Requester's Company.
#[derive(Clone)]
pub struct PgPortalNotificationStore {
    pool: PgPool,
}

impl PgPortalNotificationStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn begin(&self, tenant: TenantId) -> Result<cooper_platform::db::Tx, String> {
        cooper_platform::tenancy::begin(&self.pool, &TenantScope::single(tenant))
            .await
            .map_err(|e| e.to_string())
    }
}

fn s(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[async_trait]
impl PortalNotificationStore for PgPortalNotificationStore {
    async fn deliver(&self, d: &PortalDraft, now: DateTime<Utc>) -> Result<bool, String> {
        let mut tx = self.begin(d.tenant_id).await?;
        // Only an active Requester of that Company gets one.
        let inserted = sqlx::query!(
            "INSERT INTO portal_notifications (id, tenant_id, requester_id, kind, title, body, ticket_id, source_event_id, created_at)
             SELECT $1, $2, r.id, $4, $5, $6, $7, $8, $9 FROM requesters r
             WHERE r.id = $3 AND r.tenant_id = $2 AND r.is_active
             ON CONFLICT (source_event_id, requester_id) DO NOTHING",
            Uuid::now_v7(),
            d.tenant_id.as_uuid(),
            d.requester_id.as_uuid(),
            d.kind.as_str(),
            d.title,
            d.body,
            d.ticket_id.map(|t| t.as_uuid()),
            d.source_event_id.as_uuid(),
            now
        )
        .execute(&mut *tx)
        .await
        .map_err(s)?
        .rows_affected();
        tx.commit().await.map_err(s)?;
        Ok(inserted > 0)
    }

    async fn list(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        unread_only: bool,
        before: Option<(DateTime<Utc>, Uuid)>,
        limit: i64,
    ) -> Result<Vec<PortalNotification>, String> {
        let (before_at, before_id) = before.unzip();
        let mut tx = self.begin(tenant).await?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id, requester_id, kind, title, body, ticket_id, source_event_id, created_at, read_at
             FROM portal_notifications
             WHERE tenant_id = $1 AND requester_id = $2 AND (NOT $3 OR read_at IS NULL)
               AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
             ORDER BY created_at DESC, id DESC LIMIT $6",
            tenant.as_uuid(),
            requester.as_uuid(),
            unread_only,
            before_at,
            before_id,
            limit
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(s)?;
        tx.commit().await.map_err(s)?;
        rows.into_iter()
            .map(|r| {
                Ok(PortalNotification {
                    id: r.id,
                    tenant_id: TenantId::from_uuid(r.tenant_id),
                    requester_id: RequesterId::from_uuid(r.requester_id),
                    kind: PortalNotificationKind::parse(&r.kind)
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

    async fn unread_count(&self, tenant: TenantId, requester: RequesterId) -> Result<i64, String> {
        let mut tx = self.begin(tenant).await?;
        let n = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM portal_notifications
               WHERE tenant_id = $1 AND requester_id = $2 AND read_at IS NULL"#,
            tenant.as_uuid(),
            requester.as_uuid()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(s)?;
        tx.commit().await.map_err(s)?;
        Ok(n)
    }

    async fn mark_read(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<u64, String> {
        let mut tx = self.begin(tenant).await?;
        let n = sqlx::query!(
            "UPDATE portal_notifications SET read_at = $4
             WHERE tenant_id = $1 AND requester_id = $2 AND read_at IS NULL AND ($3::uuid IS NULL OR id = $3)",
            tenant.as_uuid(),
            requester.as_uuid(),
            id,
            now
        )
        .execute(&mut *tx)
        .await
        .map_err(s)?
        .rows_affected();
        tx.commit().await.map_err(s)?;
        Ok(n)
    }

    async fn preferences(
        &self,
        tenant: TenantId,
        requester: RequesterId,
    ) -> Result<Vec<NotificationPreference>, String> {
        let mut tx = self.begin(tenant).await?;
        let rows = sqlx::query!(
            "SELECT kind, in_app, email FROM portal_notification_preferences
             WHERE tenant_id = $1 AND requester_id = $2",
            tenant.as_uuid(),
            requester.as_uuid()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(s)?;
        tx.commit().await.map_err(s)?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                PortalNotificationKind::parse(&r.kind).map(|kind| NotificationPreference {
                    kind,
                    in_app: r.in_app,
                    email: r.email,
                })
            })
            .collect())
    }

    async fn save_preferences(
        &self,
        tenant: TenantId,
        requester: RequesterId,
        preferences: &[NotificationPreference],
        audit: &AuditRecord,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let mut tx = self.begin(tenant).await?;
        for p in preferences {
            sqlx::query!(
                "INSERT INTO portal_notification_preferences (tenant_id, requester_id, kind, in_app, email, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (requester_id, kind) DO UPDATE SET in_app = EXCLUDED.in_app, email = EXCLUDED.email,
                     updated_at = EXCLUDED.updated_at",
                tenant.as_uuid(),
                requester.as_uuid(),
                p.kind.as_str(),
                p.in_app,
                p.email,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(s)?;
        }
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(s)?;
        tx.commit().await.map_err(s)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use cooper_kernel::FixedClock;

    use super::*;

    /// In-memory store.
    #[derive(Default)]
    struct Fake {
        rows: Mutex<Vec<PortalNotification>>,
        prefs: Mutex<BTreeMap<(RequesterId, PortalNotificationKind), NotificationPreference>>,
        audits: Mutex<Vec<AuditRecord>>,
    }

    #[async_trait]
    impl PortalNotificationStore for Fake {
        async fn deliver(&self, d: &PortalDraft, now: DateTime<Utc>) -> Result<bool, String> {
            let mut rows = self.rows.lock().unwrap();
            if rows
                .iter()
                .any(|r| r.source_event_id == d.source_event_id && r.requester_id == d.requester_id)
            {
                return Ok(false);
            }
            rows.push(PortalNotification {
                id: Uuid::now_v7(),
                tenant_id: d.tenant_id,
                requester_id: d.requester_id,
                kind: d.kind,
                title: d.title.clone(),
                body: d.body.clone(),
                ticket_id: d.ticket_id,
                source_event_id: d.source_event_id,
                created_at: now,
                read_at: None,
            });
            Ok(true)
        }
        async fn list(
            &self,
            tenant: TenantId,
            requester: RequesterId,
            unread_only: bool,
            _before: Option<(DateTime<Utc>, Uuid)>,
            _limit: i64,
        ) -> Result<Vec<PortalNotification>, String> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.tenant_id == tenant && r.requester_id == requester)
                .filter(|r| !unread_only || r.read_at.is_none())
                .cloned()
                .collect())
        }
        async fn unread_count(&self, t: TenantId, r: RequesterId) -> Result<i64, String> {
            Ok(self.list(t, r, true, None, 0).await?.len() as i64)
        }
        async fn mark_read(
            &self,
            tenant: TenantId,
            requester: RequesterId,
            id: Option<Uuid>,
            now: DateTime<Utc>,
        ) -> Result<u64, String> {
            let mut n = 0;
            for r in self.rows.lock().unwrap().iter_mut() {
                if r.tenant_id == tenant
                    && r.requester_id == requester
                    && r.read_at.is_none()
                    && id.is_none_or(|id| id == r.id)
                {
                    r.read_at = Some(now);
                    n += 1;
                }
            }
            Ok(n)
        }
        async fn preferences(
            &self,
            _t: TenantId,
            requester: RequesterId,
        ) -> Result<Vec<NotificationPreference>, String> {
            Ok(self
                .prefs
                .lock()
                .unwrap()
                .iter()
                .filter(|((r, _), _)| *r == requester)
                .map(|(_, p)| *p)
                .collect())
        }
        async fn save_preferences(
            &self,
            _t: TenantId,
            requester: RequesterId,
            preferences: &[NotificationPreference],
            audit: &AuditRecord,
            _now: DateTime<Utc>,
        ) -> Result<(), String> {
            let mut prefs = self.prefs.lock().unwrap();
            for p in preferences {
                prefs.insert((requester, p.kind), *p);
            }
            self.audits.lock().unwrap().push(audit.clone());
            Ok(())
        }
    }

    fn service() -> (PortalNotifications, Arc<Fake>) {
        let fake = Arc::new(Fake::default());
        let clock = Arc::new(FixedClock::new(Utc::now()));
        (PortalNotifications::new(fake.clone(), clock), fake)
    }

    fn portal(requester: RequesterId, tenant: TenantId) -> Principal {
        Principal::new(
            Actor::Requester(requester),
            false,
            Default::default(),
            TenantScope::single(tenant),
        )
    }

    fn draft(
        requester: RequesterId,
        tenant: TenantId,
        kind: PortalNotificationKind,
    ) -> PortalDraft {
        PortalDraft {
            kind,
            tenant_id: tenant,
            requester_id: requester,
            title: "Ticket DEF-1 was closed".into(),
            body: String::new(),
            ticket_id: Some(TicketId::new()),
            source_event_id: EventId::new(),
        }
    }

    #[test]
    fn preferences_default_to_on_and_later_choices_win() {
        let all = effective_preferences(&[]);
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|p| p.in_app && p.email));
        let off = NotificationPreference {
            kind: PortalNotificationKind::TicketClosed,
            in_app: false,
            email: true,
        };
        let on = NotificationPreference {
            in_app: true,
            ..off
        };
        let chosen = effective_preferences(&[off, on]);
        assert_eq!(chosen[2], on);
        assert_eq!(
            chosen[0],
            NotificationPreference::default_for(PortalNotificationKind::TicketCreated)
        );
    }

    #[tokio::test]
    async fn sending_respects_in_app_preferences_and_skips_own_actions() {
        let (svc, fake) = service();
        let (sam, company) = (RequesterId::new(), TenantId::new());
        let closed = PortalNotificationKind::TicketClosed;
        assert!(
            svc.send(draft(sam, company, closed), Actor::Automation)
                .await
                .unwrap()
        );
        assert!(
            !svc.send(draft(sam, company, closed), Actor::Requester(sam))
                .await
                .unwrap(),
            "the Requester's own action"
        );
        svc.set_preferences(
            &portal(sam, company),
            &[NotificationPreference {
                kind: closed,
                in_app: false,
                email: false,
            }],
        )
        .await
        .unwrap();
        assert!(
            !svc.send(draft(sam, company, closed), Actor::Automation)
                .await
                .unwrap()
        );
        assert!(
            svc.send(
                draft(sam, company, PortalNotificationKind::TicketReplied),
                Actor::Automation
            )
            .await
            .unwrap(),
            "other kinds stay on"
        );
        assert!(!svc.wants_email(company, sam, closed).await.unwrap());
        assert!(
            svc.wants_email(company, sam, PortalNotificationKind::TicketCreated)
                .await
                .unwrap()
        );
        let audits = fake.audits.lock().unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].actor, Actor::Requester(sam));
    }

    #[tokio::test]
    async fn requesters_see_and_read_only_their_own() {
        let (svc, _) = service();
        let (sam, alex, company) = (RequesterId::new(), RequesterId::new(), TenantId::new());
        svc.send(
            draft(sam, company, PortalNotificationKind::TicketCreated),
            Actor::Automation,
        )
        .await
        .unwrap();
        svc.send(
            draft(alex, company, PortalNotificationKind::TicketCreated),
            Actor::Automation,
        )
        .await
        .unwrap();
        let mine = svc
            .mine(&portal(sam, company), false, Limit::DEFAULT, None)
            .await
            .unwrap();
        assert_eq!(mine.items.len(), 1);
        let alexs = svc
            .mine(&portal(alex, company), false, Limit::DEFAULT, None)
            .await
            .unwrap()
            .items[0]
            .id;
        assert_eq!(
            svc.mark_read(&portal(sam, company), Some(alexs)).await,
            Err(AppError::NotFound)
        );
        svc.mark_read(&portal(sam, company), Some(mine.items[0].id))
            .await
            .unwrap();
        assert_eq!(svc.unread_count(&portal(sam, company)).await.unwrap(), 0);
        assert_eq!(svc.unread_count(&portal(alex, company)).await.unwrap(), 1);
        assert_eq!(
            svc.unread_count(&portal(sam, TenantId::new()))
                .await
                .unwrap(),
            0,
            "another Company"
        );
        let analyst = Principal::new(
            Actor::Analyst(cooper_kernel::AnalystId::new()),
            false,
            Default::default(),
            TenantScope::All,
        );
        assert!(matches!(
            svc.preferences(&analyst).await,
            Err(AppError::Forbidden(_))
        ));
    }
}
