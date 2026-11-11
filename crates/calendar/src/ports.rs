//! What the Calendar needs from storage.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AuditRecord, TenantId, TenantScope};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    CalendarCategory, CalendarCategoryId, CalendarEvent, CalendarEventId, DateRange, SourceKind,
};

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A uniqueness constraint was violated, or another writer got there first.
    #[error("conflict: {0}")]
    Conflict(String),
    /// A referenced row does not exist.
    #[error("missing reference: {0}")]
    MissingReference(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("storage: {0}")]
    Storage(String),
}

/// A Calendar Event list request.
#[derive(Debug, Clone)]
pub struct EventQuery {
    /// Days the events touch.
    pub range: DateRange,
    /// Only this Company.
    pub tenant_id: Option<TenantId>,
    /// Only this category.
    pub category_id: Option<CalendarCategoryId>,
    /// At most this many events (one more is fetched to tell whether there were more).
    pub limit: u32,
}

/// The last outbox event applied to a source's generated events: its time and ID, so a
/// late redelivery of an older event cannot undo a newer one.
pub type Watermark = (DateTime<Utc>, Uuid);

/// A source's generated events and the watermark they reflect.
#[derive(Debug, Clone, Default)]
pub struct GeneratedState {
    /// The last applied event, `None` before the first.
    pub watermark: Option<Watermark>,
    /// The generated events.
    pub events: Vec<CalendarEvent>,
}

/// What syncing one source writes, atomically.
#[derive(Debug, Clone)]
pub struct GeneratedWrite<'a> {
    /// Company.
    pub tenant_id: TenantId,
    /// Contract or Asset.
    pub kind: SourceKind,
    /// Its ID.
    pub source_id: Uuid,
    /// The watermark read before (compare-and-set).
    pub expected: Option<Watermark>,
    /// The new watermark.
    pub watermark: Watermark,
    /// Events to insert.
    pub inserts: &'a [CalendarEvent],
    /// Events to update.
    pub updates: &'a [CalendarEvent],
    /// Events to delete.
    pub deletes: &'a [CalendarEvent],
    /// Audit records.
    pub audits: &'a [AuditRecord],
}

/// Calendar storage. Categories are instance-wide; events belong to a Company and every
/// event read takes the caller's Tenant Scope.
#[async_trait]
pub trait CalendarRepository: Send + Sync {
    /// Every Calendar Category.
    async fn categories(&self) -> Result<Vec<CalendarCategory>, RepoError>;
    /// One Calendar Category.
    async fn category(&self, id: CalendarCategoryId)
    -> Result<Option<CalendarCategory>, RepoError>;
    /// Inserts or updates a Calendar Category.
    async fn save_category(
        &self,
        category: &CalendarCategory,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// The system category for `kind`, inserting `candidate` (with `audit`) when there is
    /// none yet.
    async fn ensure_system_category(
        &self,
        candidate: &CalendarCategory,
        audit: &AuditRecord,
    ) -> Result<CalendarCategory, RepoError>;

    /// Events touching the range, by start.
    async fn events(
        &self,
        scope: &TenantScope,
        q: &EventQuery,
    ) -> Result<Vec<CalendarEvent>, RepoError>;
    /// One event.
    async fn event(
        &self,
        scope: &TenantScope,
        id: CalendarEventId,
    ) -> Result<Option<CalendarEvent>, RepoError>;
    /// Inserts a manual event.
    async fn insert_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Updates a manual event.
    async fn update_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError>;
    /// Deletes a manual event.
    async fn delete_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError>;

    /// The generated events of one source.
    async fn generated(
        &self,
        tenant: TenantId,
        kind: SourceKind,
        source_id: Uuid,
    ) -> Result<GeneratedState, RepoError>;
    /// Applies a source's generated events if its watermark is still `expected`;
    /// [`RepoError::Conflict`] when another writer moved it first.
    async fn apply_generated(&self, w: GeneratedWrite<'_>) -> Result<(), RepoError>;
}
