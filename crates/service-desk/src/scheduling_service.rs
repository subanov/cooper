//! Scheduled work and rota use cases (FR-TKT-031, FR-SLA-008; ADR-0042).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use cooper_kernel::{Actor, AnalystId, AuditAction, AuditRecord, Clock, DomainEvent, EventEnvelope, Forbidden, Principal, TenantId, TenantScope, TicketId};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::application::{AppError as DeskError, ServiceDesk};
use crate::domain::Directory;
use crate::scheduling::{RotaEntry, RotaLocation, ScheduledWork, Shift, SchedulingError, Span};

/// Capabilities of scheduling. Scheduled work uses `tickets.update`.
pub mod capabilities {
    /// Read the rota.
    pub const ROTA_READ: &str = "rota.read";
    /// Define rota shifts and locations.
    pub const ROTA_CONFIGURE: &str = "rota.configure";
    /// Schedule Analysts on the rota.
    pub const ROTA_MANAGE: &str = "rota.manage";
    /// Every Capability declared by scheduling.
    pub const ALL: &[&str] = &[ROTA_READ, ROTA_CONFIGURE, ROTA_MANAGE];
}

/// The event published when scheduled work is made or changed (FR-CAL-002).
#[derive(Debug, Clone, Serialize)]
pub struct WorkChanged {
    /// The work.
    pub work_id: Uuid,
    /// Company (envelope only).
    #[serde(skip)]
    pub tenant_id: TenantId,
    /// The ticket.
    pub ticket_id: Uuid,
    /// Its number.
    pub ticket_number: String,
    /// The Owner.
    pub owner_id: Uuid,
    /// Whole days or a time range.
    pub all_day: bool,
    /// First day.
    pub starts_on: NaiveDate,
    /// Last day.
    pub ends_on: NaiveDate,
    /// Start instant, for timed work.
    pub starts_at: Option<DateTime<Utc>>,
    /// End instant, for timed work.
    pub ends_at: Option<DateTime<Utc>>,
    /// False once the work is cancelled.
    pub active: bool,
}

impl DomainEvent for WorkChanged {
    const EVENT_TYPE: &'static str = "ticket.work_changed";
    const AGGREGATE_TYPE: &'static str = "ticket_scheduled_work";

    fn aggregate_id(&self) -> Uuid {
        self.work_id
    }

    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchedulingRepoError {
    /// A uniqueness rule was violated.
    #[error("conflict: {0}")]
    Conflict(String),
    /// A referenced row is missing.
    #[error("missing reference: {0}")]
    MissingReference(String),
    /// Not found.
    #[error("not found")]
    NotFound,
    /// Anything else.
    #[error("storage: {0}")]
    Storage(String),
}

/// A list of scheduled work.
#[derive(Debug, Clone, Default)]
pub struct WorkQuery {
    /// Only this Company.
    pub tenant_id: Option<TenantId>,
    /// Only this Owner.
    pub owner: Option<AnalystId>,
    /// Only this ticket.
    pub ticket_id: Option<Uuid>,
    /// Work that ends on or after this day.
    pub from: Option<NaiveDate>,
    /// Work that starts on or before this day.
    pub to: Option<NaiveDate>,
}

/// A rota entry with its Analyst, for the day view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotaRow {
    /// The entry.
    pub entry: RotaEntry,
    /// The Company.
    pub tenant_id: TenantId,
}

/// Storage of scheduled work and the rota.
#[async_trait]
pub trait SchedulingRepository: Send + Sync {
    /// One piece of work.
    async fn work(&self, scope: &TenantScope, id: Uuid) -> Result<Option<ScheduledWork>, SchedulingRepoError>;
    /// Work matching a query, oldest first.
    async fn list_work(&self, scope: &TenantScope, q: &WorkQuery) -> Result<Vec<ScheduledWork>, SchedulingRepoError>;
    /// Inserts or updates work with its event and audit entry, in one transaction.
    async fn save_work(
        &self,
        w: &ScheduledWork,
        event: &EventEnvelope,
        audit: &AuditRecord,
    ) -> Result<(), SchedulingRepoError>;

    /// Shifts of a Company.
    async fn shifts(&self, scope: &TenantScope, tenant: TenantId) -> Result<Vec<Shift>, SchedulingRepoError>;
    /// Inserts a shift (audited).
    async fn insert_shift(&self, tenant: TenantId, s: &Shift, audit: &AuditRecord) -> Result<(), SchedulingRepoError>;
    /// Locations of a Company.
    async fn locations(&self, scope: &TenantScope, tenant: TenantId) -> Result<Vec<RotaLocation>, SchedulingRepoError>;
    /// Inserts a location (audited).
    async fn insert_location(&self, tenant: TenantId, l: &RotaLocation, audit: &AuditRecord) -> Result<(), SchedulingRepoError>;
    /// Rota entries of a Company on days from `from` to `to` inclusive.
    async fn entries(&self, scope: &TenantScope, tenant: TenantId, from: NaiveDate, to: NaiveDate) -> Result<Vec<RotaEntry>, SchedulingRepoError>;
    /// Inserts a rota entry (audited).
    async fn insert_entry(&self, tenant: TenantId, e: &RotaEntry, audit: &AuditRecord) -> Result<(), SchedulingRepoError>;
}

/// What scheduling needs from the rest of Cooper.
#[derive(Clone)]
pub struct SchedulingPorts {
    /// Storage.
    pub store: Arc<dyn SchedulingRepository>,
    /// Tickets (visibility and numbers).
    pub desk: ServiceDesk,
    /// Analysts.
    pub directory: Arc<dyn Directory>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A scheduling use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchedulingAppError {
    /// Input is not valid.
    #[error(transparent)]
    Invalid(#[from] SchedulingError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// Ticket use case failed.
    #[error(transparent)]
    Desk(#[from] DeskError),
    /// Unexpected failure.
    #[error("internal error")]
    Internal,
}

impl From<SchedulingRepoError> for SchedulingAppError {
    fn from(e: SchedulingRepoError) -> Self {
        match e {
            SchedulingRepoError::Conflict(c) => Self::Conflict(format!("already scheduled ({c})")),
            SchedulingRepoError::MissingReference(c) => Self::Invalid(SchedulingError::Invalid {
                field: "reference",
                reason: format!("refers to a record that does not exist ({c})"),
            }),
            SchedulingRepoError::NotFound => Self::NotFound("record"),
            SchedulingRepoError::Storage(m) => {
                tracing::error!(error = %m, "scheduling storage error");
                Self::Internal
            }
        }
    }
}

fn require(principal: &Principal, capability: &str) -> Result<(), SchedulingAppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

/// Scheduled work on tickets (FR-TKT-031).
#[derive(Clone)]
pub struct ScheduledWorkService {
    ports: SchedulingPorts,
}

impl ScheduledWorkService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: SchedulingPorts) -> Self {
        Self { ports }
    }

    async fn ticket_company(&self, principal: &Principal, ticket: Uuid) -> Result<(TenantId, String), SchedulingAppError> {
        let detail = self.ports.desk.ticket(principal, TicketId::from_uuid(ticket)).await?;
        Ok((detail.ticket.tenant_id(), detail.ticket.number().to_owned()))
    }

    async fn check_owner(&self, owner: AnalystId) -> Result<(), SchedulingAppError> {
        if !self.ports.directory.analyst_is_active(owner).await.map_err(|_| SchedulingAppError::Internal)? {
            return Err(SchedulingError::Invalid { field: "owner_id", reason: "is not an active Analyst".into() }.into());
        }
        Ok(())
    }

    fn event(w: &ScheduledWork, tenant: TenantId, number: &str, actor: Actor, now: DateTime<Utc>) -> Result<EventEnvelope, SchedulingAppError> {
        let (starts_at, ends_at) = match w.span {
            Span::Timed { starts_at, ends_at } => (Some(starts_at), Some(ends_at)),
            Span::AllDay { .. } => (None, None),
        };
        let payload = WorkChanged {
            work_id: w.id,
            tenant_id: tenant,
            ticket_id: w.ticket_id,
            ticket_number: number.to_owned(),
            owner_id: w.owner.as_uuid(),
            all_day: w.span.all_day(),
            starts_on: w.span.first_day(),
            ends_on: w.span.last_day(),
            starts_at,
            ends_at,
            active: w.active,
        };
        EventEnvelope::wrap(&payload, actor, now).map_err(|_| SchedulingAppError::Internal)
    }

    /// Schedules work on a ticket for an Owner (needs `tickets.update` and visibility of the
    /// ticket). The work appears on the Calendar.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, an inactive Owner, or a bad span.
    pub async fn schedule(
        &self,
        principal: &Principal,
        ticket: Uuid,
        owner: AnalystId,
        span: Span,
        notes: &str,
    ) -> Result<ScheduledWork, SchedulingAppError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        let (tenant, number) = self.ticket_company(principal, ticket).await?;
        self.check_owner(owner).await?;
        let w = ScheduledWork::new(tenant.as_uuid(), ticket, owner, span, notes)?;
        let now = self.ports.clock.now();
        let event = Self::event(&w, tenant, &number, principal.actor(), now)?;
        let audit = AuditRecord::new(Some(tenant), "ticket_scheduled_work", w.id, AuditAction::Created, principal.actor(), now);
        self.ports.store.save_work(&w, &event, &audit).await?;
        Ok(w)
    }

    /// Moves or changes the owner or notes of scheduled work (needs `tickets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, an inactive Owner or a bad span.
    pub async fn reschedule(
        &self,
        principal: &Principal,
        id: Uuid,
        owner: AnalystId,
        span: Span,
        notes: &str,
    ) -> Result<ScheduledWork, SchedulingAppError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        let mut w = self.find(principal, id).await?;
        let (tenant, number) = self.ticket_company(principal, w.ticket_id).await?;
        self.check_owner(owner).await?;
        let fresh = ScheduledWork::new(w.tenant_id, w.ticket_id, owner, span, notes)?;
        w.owner = fresh.owner;
        w.span = fresh.span;
        w.notes = fresh.notes;
        let now = self.ports.clock.now();
        let event = Self::event(&w, tenant, &number, principal.actor(), now)?;
        let audit = AuditRecord::new(Some(tenant), "ticket_scheduled_work", w.id, AuditAction::Updated, principal.actor(), now);
        self.ports.store.save_work(&w, &event, &audit).await?;
        Ok(w)
    }

    /// Cancels scheduled work; the record stays for the history (needs `tickets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn cancel(&self, principal: &Principal, id: Uuid) -> Result<ScheduledWork, SchedulingAppError> {
        require(principal, crate::application::capabilities::UPDATE)?;
        let mut w = self.find(principal, id).await?;
        let (tenant, number) = self.ticket_company(principal, w.ticket_id).await?;
        w.active = false;
        let now = self.ports.clock.now();
        let event = Self::event(&w, tenant, &number, principal.actor(), now)?;
        let audit = AuditRecord::new(Some(tenant), "ticket_scheduled_work", w.id, AuditAction::Updated, principal.actor(), now);
        self.ports.store.save_work(&w, &event, &audit).await?;
        Ok(w)
    }

    async fn find(&self, principal: &Principal, id: Uuid) -> Result<ScheduledWork, SchedulingAppError> {
        self.ports.store.work(principal.scope(), id).await?.ok_or(SchedulingAppError::NotFound("scheduled work"))
    }

    /// The scheduled work of a ticket, oldest first (needs visibility of the ticket).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn for_ticket(&self, principal: &Principal, ticket: Uuid) -> Result<Vec<ScheduledWork>, SchedulingAppError> {
        require(principal, crate::application::capabilities::READ)?;
        self.ticket_company(principal, ticket).await?;
        Ok(self.ports.store.list_work(principal.scope(), &WorkQuery { ticket_id: Some(ticket), ..WorkQuery::default() }).await?)
    }

    /// Work of an Owner in a range of days (needs `tickets.read`). Cancelled work is kept
    /// out of the list.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn list(&self, principal: &Principal, q: WorkQuery) -> Result<Vec<ScheduledWork>, SchedulingAppError> {
        require(principal, crate::application::capabilities::READ)?;
        let mut rows = self.ports.store.list_work(principal.scope(), &q).await?;
        rows.retain(|w| w.active);
        Ok(rows)
    }
}

/// Where an Analyst is placed on the rota.
#[derive(Debug, Clone, Copy)]
pub struct RotaPlacement {
    /// The Analyst.
    pub analyst: AnalystId,
    /// The day.
    pub date: NaiveDate,
    /// The shift.
    pub shift_id: Uuid,
    /// The location.
    pub location_id: Uuid,
    /// On call.
    pub on_call: bool,
}

/// The rota (FR-SLA-008).
#[derive(Clone)]
pub struct RotaService {
    ports: SchedulingPorts,
}

impl RotaService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: SchedulingPorts) -> Self {
        Self { ports }
    }

    /// Shifts of a Company (needs `rota.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn shifts(&self, principal: &Principal, tenant: TenantId) -> Result<Vec<Shift>, SchedulingAppError> {
        require(principal, capabilities::ROTA_READ)?;
        Ok(self.ports.store.shifts(principal.scope(), tenant).await?)
    }

    /// Defines a shift (needs `rota.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_shift(&self, principal: &Principal, tenant: TenantId, name: &str, starts: NaiveTime, ends: NaiveTime) -> Result<Shift, SchedulingAppError> {
        require(principal, capabilities::ROTA_CONFIGURE)?;
        self.check_company(principal, tenant)?;
        let s = Shift::new(name, starts, ends)?;
        let now = self.ports.clock.now();
        let audit = AuditRecord::new(Some(tenant), "rota_shift", s.id, AuditAction::Created, principal.actor(), now);
        self.ports.store.insert_shift(tenant, &s, &audit).await?;
        Ok(s)
    }

    /// Locations of a Company (needs `rota.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn locations(&self, principal: &Principal, tenant: TenantId) -> Result<Vec<RotaLocation>, SchedulingAppError> {
        require(principal, capabilities::ROTA_READ)?;
        Ok(self.ports.store.locations(principal.scope(), tenant).await?)
    }

    /// Defines a location (needs `rota.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_location(&self, principal: &Principal, tenant: TenantId, name: &str) -> Result<RotaLocation, SchedulingAppError> {
        require(principal, capabilities::ROTA_CONFIGURE)?;
        self.check_company(principal, tenant)?;
        let l = RotaLocation::new(name)?;
        let now = self.ports.clock.now();
        let audit = AuditRecord::new(Some(tenant), "rota_location", l.id, AuditAction::Created, principal.actor(), now);
        self.ports.store.insert_location(tenant, &l, &audit).await?;
        Ok(l)
    }

    /// The rota of a Company for days from `from` to `to` (needs `rota.read`). The
    /// rota for today is the range of one day.
    ///
    /// # Errors
    ///
    /// Forbidden, a range that is too long or backwards, or storage failure.
    pub async fn entries(&self, principal: &Principal, tenant: TenantId, from: NaiveDate, to: NaiveDate) -> Result<Vec<RotaEntry>, SchedulingAppError> {
        require(principal, capabilities::ROTA_READ)?;
        if to < from || (to - from).num_days() > 366 {
            return Err(SchedulingError::Invalid { field: "to", reason: "must be after from, at most a year later".into() }.into());
        }
        Ok(self.ports.store.entries(principal.scope(), tenant, from, to).await?)
    }

    /// Schedules an Analyst on a day, shift and location, on call or not (needs
    /// `rota.manage`).
    ///
    /// # Errors
    ///
    /// Forbidden, an inactive Analyst, a shift or location that does not exist, or the
    /// Analyst already scheduled for that shift.
    pub async fn schedule(
        &self,
        principal: &Principal,
        tenant: TenantId,
        placement: RotaPlacement,
    ) -> Result<RotaEntry, SchedulingAppError> {
        let RotaPlacement { analyst, date, shift_id, location_id, on_call } = placement;
        require(principal, capabilities::ROTA_MANAGE)?;
        self.check_company(principal, tenant)?;
        if !self.ports.directory.analyst_is_active(analyst).await.map_err(|_| SchedulingAppError::Internal)? {
            return Err(SchedulingError::Invalid { field: "analyst_id", reason: "is not an active Analyst".into() }.into());
        }
        let shift_ok = self.ports.store.shifts(principal.scope(), tenant).await?.iter().any(|s| s.id == shift_id && s.active);
        if !shift_ok {
            return Err(SchedulingError::Invalid { field: "shift_id", reason: "is not an active shift of the Company".into() }.into());
        }
        let location_ok = self.ports.store.locations(principal.scope(), tenant).await?.iter().any(|l| l.id == location_id && l.active);
        if !location_ok {
            return Err(SchedulingError::Invalid { field: "location_id", reason: "is not an active location of the Company".into() }.into());
        }
        let e = RotaEntry { id: Uuid::now_v7(), analyst, work_date: date, shift_id, location_id, on_call };
        let now = self.ports.clock.now();
        let audit = AuditRecord::new(Some(tenant), "rota_entry", e.id, AuditAction::Created, principal.actor(), now);
        self.ports.store.insert_entry(tenant, &e, &audit).await?;
        Ok(e)
    }

    fn check_company(&self, principal: &Principal, tenant: TenantId) -> Result<(), SchedulingAppError> {
        if !principal.scope().allows(tenant) {
            return Err(SchedulingAppError::NotFound("Company"));
        }
        Ok(())
    }
}

