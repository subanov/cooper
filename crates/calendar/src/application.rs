//! Calendar use cases (ADR-0034).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AuditAction, AuditRecord, Clock, Forbidden, Principal, TenantId, TenantScope,
};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    CalendarCategory, CalendarCategoryId, CalendarError, CalendarEvent, CalendarEventId,
    CategoryInput, DateRange, EventDetails, SourceFacts,
};
use crate::ports::{CalendarRepository, EventQuery, GeneratedWrite, RepoError};

/// Capabilities the Calendar checks (FR-IAM-010).
pub mod capabilities {
    /// See the Calendar and its Calendar Categories.
    pub const READ: &str = "calendar.read";
    /// Add manual Calendar Events.
    pub const CREATE: &str = "calendar.create";
    /// Edit and delete manual Calendar Events.
    pub const UPDATE: &str = "calendar.update";
    /// Manage Calendar Categories.
    pub const CONFIGURE: &str = "calendar.configure";

    /// Every Capability declared by the Calendar.
    pub const ALL: &[&str] = &[READ, CREATE, UPDATE, CONFIGURE];
}

/// Most events one list returns.
pub const MAX_EVENTS: u32 = 2000;

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] CalendarError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated, or another writer got there first.
    #[error("{0}")]
    Conflict(String),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Conflict(c) if c.ends_with("_name_key") => {
                Self::Conflict("a Calendar Category with this name already exists".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(CalendarError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "calendar storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct CalendarPorts {
    /// Storage.
    pub store: Arc<dyn CalendarRepository>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// An event with its category.
#[derive(Debug, Clone)]
pub struct EventView {
    /// The event.
    pub event: CalendarEvent,
    /// Its category.
    pub category: CalendarCategory,
}

/// Events of a range.
#[derive(Debug, Clone)]
pub struct EventList {
    /// Events, by start.
    pub items: Vec<EventView>,
    /// More events matched than [`MAX_EVENTS`]; narrow the range.
    pub truncated: bool,
}

/// A list request.
#[derive(Debug, Clone)]
pub struct ListEvents {
    /// Days.
    pub range: DateRange,
    /// Only this Company.
    pub tenant_id: Option<TenantId>,
    /// Only this category.
    pub category_id: Option<CalendarCategoryId>,
}

/// What syncing a source did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncOutcome {
    /// The event was older than one already applied (or the same one again).
    pub stale: bool,
    /// Events inserted.
    pub inserted: usize,
    /// Events changed.
    pub updated: usize,
    /// Events removed.
    pub deleted: usize,
}

/// Calendar use cases.
#[derive(Clone)]
pub struct CalendarService {
    ports: CalendarPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

/// Adds the changed fields of `before` → `after` to `record` (either side may be empty).
fn diff(
    record: AuditRecord,
    before: &[(&'static str, Value)],
    after: &[(&'static str, Value)],
) -> AuditRecord {
    let fields = if after.is_empty() { before } else { after };
    fields.iter().fold(record, |r, (f, _)| {
        let old = before.iter().find(|(n, _)| n == f).map(|(_, v)| v);
        let new = after.iter().find(|(n, _)| n == f).map(|(_, v)| v);
        r.change(*f, old, new)
    })
}

fn event_audit(
    before: Option<&CalendarEvent>,
    after: Option<&CalendarEvent>,
    subject: &CalendarEvent,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = match (before, after) {
        (None, _) => AuditAction::Created,
        (Some(_), None) => AuditAction::Deleted,
        (Some(_), Some(_)) => AuditAction::Updated,
    };
    diff(
        AuditRecord::new(
            Some(subject.tenant_id()),
            "calendar_event",
            subject.id().as_uuid(),
            action,
            actor,
            now,
        ),
        &before.map(CalendarEvent::audit_fields).unwrap_or_default(),
        &after.map(CalendarEvent::audit_fields).unwrap_or_default(),
    )
}

fn created(e: &CalendarEvent, actor: Actor, now: DateTime<Utc>) -> AuditRecord {
    event_audit(None, Some(e), e, actor, now)
}

fn updated(
    before: &CalendarEvent,
    after: &CalendarEvent,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    event_audit(Some(before), Some(after), after, actor, now)
}

fn deleted(e: &CalendarEvent, actor: Actor, now: DateTime<Utc>) -> AuditRecord {
    event_audit(Some(e), None, e, actor, now)
}

fn category_audit(
    before: Option<&CalendarCategory>,
    after: &CalendarCategory,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = if before.is_some() {
        AuditAction::Updated
    } else {
        AuditAction::Created
    };
    diff(
        AuditRecord::new(
            None,
            "calendar_category",
            after.id().as_uuid(),
            action,
            actor,
            now,
        ),
        &before
            .map(CalendarCategory::audit_fields)
            .unwrap_or_default(),
        &after.audit_fields(),
    )
}

impl CalendarService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: CalendarPorts) -> Self {
        Self { ports }
    }

    fn now(&self) -> DateTime<Utc> {
        self.ports.clock.now()
    }

    // --- Calendar Categories ------------------------------------------------------------

    /// Every Calendar Category, by name (needs `calendar.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn categories(&self, p: &Principal) -> Result<Vec<CalendarCategory>, AppError> {
        require(p, capabilities::READ)?;
        self.sorted_categories().await
    }

    async fn sorted_categories(&self) -> Result<Vec<CalendarCategory>, AppError> {
        let mut rows = self.ports.store.categories().await?;
        rows.sort_by_key(|c| (c.is_system(), c.name().to_lowercase()));
        Ok(rows)
    }

    /// Adds a Calendar Category (needs `calendar.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_category(
        &self,
        p: &Principal,
        input: CategoryInput,
    ) -> Result<CalendarCategory, AppError> {
        require(p, capabilities::CONFIGURE)?;
        let now = self.now();
        let c = CalendarCategory::create(input, now)?;
        self.ports
            .store
            .save_category(&c, &category_audit(None, &c, p.actor(), now))
            .await?;
        Ok(c)
    }

    /// Changes a Calendar Category (needs `calendar.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_category(
        &self,
        p: &Principal,
        id: CalendarCategoryId,
        input: CategoryInput,
    ) -> Result<CalendarCategory, AppError> {
        require(p, capabilities::CONFIGURE)?;
        let now = self.now();
        let before = self
            .ports
            .store
            .category(id)
            .await?
            .ok_or(AppError::NotFound("Calendar Category"))?;
        let mut c = before.clone();
        c.edit(input, now)?;
        self.ports
            .store
            .save_category(&c, &category_audit(Some(&before), &c, p.actor(), now))
            .await?;
        Ok(c)
    }

    // --- Calendar Events ----------------------------------------------------------------

    fn scope(p: &Principal, tenant: Option<TenantId>) -> Result<TenantScope, AppError> {
        match tenant {
            None => Ok(p.scope().clone()),
            Some(t) if p.scope().allows(t) => Ok(TenantScope::single(t)),
            Some(_) => Err(AppError::NotFound("Company")),
        }
    }

    async fn with_categories(
        &self,
        events: Vec<CalendarEvent>,
    ) -> Result<Vec<EventView>, AppError> {
        let categories = self.ports.store.categories().await?;
        events
            .into_iter()
            .map(|event| {
                let category = categories
                    .iter()
                    .find(|c| c.id() == event.details().category_id)
                    .cloned()
                    .ok_or_else(|| {
                        tracing::error!(event = %event.id(), "calendar event without category");
                        AppError::Internal
                    })?;
                Ok(EventView { event, category })
            })
            .collect()
    }

    /// Events of the visible Companies touching a range of days (needs `calendar.read`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company or storage failure.
    pub async fn events(&self, p: &Principal, q: ListEvents) -> Result<EventList, AppError> {
        require(p, capabilities::READ)?;
        let scope = Self::scope(p, q.tenant_id)?;
        let mut rows = self
            .ports
            .store
            .events(
                &scope,
                &EventQuery {
                    range: q.range,
                    tenant_id: q.tenant_id,
                    category_id: q.category_id,
                    limit: MAX_EVENTS + 1,
                },
            )
            .await?;
        let truncated = rows.len() > MAX_EVENTS as usize;
        rows.truncate(MAX_EVENTS as usize);
        Ok(EventList {
            items: self.with_categories(rows).await?,
            truncated,
        })
    }

    async fn load(&self, p: &Principal, id: CalendarEventId) -> Result<CalendarEvent, AppError> {
        self.ports
            .store
            .event(p.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Calendar Event"))
    }

    async fn view(&self, event: CalendarEvent) -> Result<EventView, AppError> {
        self.with_categories(vec![event])
            .await?
            .pop()
            .ok_or(AppError::Internal)
    }

    /// One event (needs `calendar.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn event(&self, p: &Principal, id: CalendarEventId) -> Result<EventView, AppError> {
        require(p, capabilities::READ)?;
        let e = self.load(p, id).await?;
        self.view(e).await
    }

    async fn manual_category(
        &self,
        id: CalendarCategoryId,
        current: Option<CalendarCategoryId>,
    ) -> Result<(), AppError> {
        let c =
            self.ports.store.category(id).await?.ok_or_else(|| {
                CalendarError::invalid("category_id", "is not a Calendar Category")
            })?;
        if current == Some(id) && !c.is_system() {
            // An event may keep a category that was deactivated after it was made.
            return Ok(());
        }
        Ok(c.accepts_manual_events()?)
    }

    /// Adds a manual event to a Company's Calendar (needs `calendar.create`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company or invalid details.
    pub async fn create_event(
        &self,
        p: &Principal,
        tenant: TenantId,
        details: EventDetails,
    ) -> Result<EventView, AppError> {
        require(p, capabilities::CREATE)?;
        if !p.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        self.manual_category(details.category_id, None).await?;
        let now = self.now();
        let e = CalendarEvent::create(tenant, details, p.actor(), now)?;
        self.ports
            .store
            .insert_event(&e, &created(&e, p.actor(), now))
            .await?;
        self.view(e).await
    }

    /// Changes a manual event (needs `calendar.update`). Generated events are read-only.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, generated, or invalid details.
    pub async fn edit_event(
        &self,
        p: &Principal,
        id: CalendarEventId,
        details: EventDetails,
    ) -> Result<EventView, AppError> {
        require(p, capabilities::UPDATE)?;
        let before = self.load(p, id).await?;
        before.ensure_manual()?;
        self.manual_category(details.category_id, Some(before.details().category_id))
            .await?;
        let now = self.now();
        let mut e = before.clone();
        e.edit(details, now)?;
        self.ports
            .store
            .update_event(&e, &updated(&before, &e, p.actor(), now))
            .await?;
        self.view(e).await
    }

    /// Deletes a manual event (needs `calendar.update`). Generated events are read-only.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or generated.
    pub async fn delete_event(&self, p: &Principal, id: CalendarEventId) -> Result<(), AppError> {
        require(p, capabilities::UPDATE)?;
        let e = self.load(p, id).await?;
        e.ensure_manual()?;
        let now = self.now();
        self.ports
            .store
            .delete_event(&e, &deleted(&e, p.actor(), now))
            .await?;
        Ok(())
    }

    // --- Generated events (FR-CAL-002, FR-CON-003, FR-AST-012) -------------------------

    /// Brings a source's generated events in line with the dates another context
    /// reported in outbox event `event_id` at `occurred_at`. Idempotent: the same or an
    /// older event changes nothing. Written by the System (Automation).
    ///
    /// # Errors
    ///
    /// Storage failure, or [`AppError::Conflict`] when another worker synced the same
    /// source at the same time (the outbox retries).
    pub async fn sync_generated(
        &self,
        tenant: TenantId,
        event_id: Uuid,
        occurred_at: DateTime<Utc>,
        facts: &SourceFacts,
    ) -> Result<SyncOutcome, AppError> {
        let store = &self.ports.store;
        let state = store.generated(tenant, facts.kind, facts.id).await?;
        let watermark = (occurred_at, event_id);
        if state.watermark.is_some_and(|w| watermark <= w) {
            return Ok(SyncOutcome {
                stale: true,
                ..SyncOutcome::default()
            });
        }
        let now = self.now();
        let actor = Actor::Automation;
        let wanted = facts.wanted();
        let category = if wanted.is_empty() {
            None
        } else {
            let candidate = CalendarCategory::system(facts.kind, now);
            Some(
                store
                    .ensure_system_category(
                        &candidate,
                        &category_audit(None, &candidate, actor, now),
                    )
                    .await?,
            )
        };
        let (mut inserts, mut updates, mut deletes, mut audits) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for existing in &state.events {
            let kind = existing.source().map(|s| s.date);
            if !wanted.iter().any(|w| Some(w.date_kind) == kind) {
                audits.push(deleted(existing, actor, now));
                deletes.push(existing.clone());
            }
        }
        if let Some(category) = &category {
            for w in &wanted {
                let found = state
                    .events
                    .iter()
                    .find(|e| e.source().map(|s| s.date) == Some(w.date_kind));
                if let Some(existing) = found {
                    let mut e = existing.clone();
                    if e.refresh_generated(w, category.id(), now) {
                        audits.push(updated(existing, &e, actor, now));
                        updates.push(e);
                    }
                } else {
                    let e = CalendarEvent::generated(tenant, facts.id, w, category.id(), now);
                    audits.push(created(&e, actor, now));
                    inserts.push(e);
                }
            }
        }
        store
            .apply_generated(GeneratedWrite {
                tenant_id: tenant,
                kind: facts.kind,
                source_id: facts.id,
                expected: state.watermark,
                watermark,
                inserts: &inserts,
                updates: &updates,
                deletes: &deletes,
                audits: &audits,
            })
            .await?;
        Ok(SyncOutcome {
            stale: false,
            inserted: inserts.len(),
            updated: updates.len(),
            deleted: deletes.len(),
        })
    }
}

#[cfg(test)]
mod tests;
