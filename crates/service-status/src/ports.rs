//! What Service Status needs from storage and from the rest of Cooper.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AuditRecord, EventEnvelope, Limit, TenantScope};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    ImpactLevel, MaintenanceState, PlannedMaintenance, PlannedMaintenanceId, ServiceIncidentStatus,
    StatusIncident, StatusIncidentId, StatusIncidentUpdate, StatusService, StatusServiceId,
};

/// Storage failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepoError {
    /// A uniqueness constraint was violated.
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

/// A Service Incident list request.
#[derive(Debug, Clone, Default)]
pub struct IncidentQuery {
    /// Unresolved (`true`) or resolved (`false`).
    pub open: Option<bool>,
    /// Affecting this Service.
    pub service_id: Option<StatusServiceId>,
    /// Page size.
    pub limit: Limit,
    /// After `(started_at, id)`, newest first.
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

/// A Planned Maintenance list request.
#[derive(Debug, Clone, Default)]
pub struct MaintenanceQuery {
    /// In these states (empty: any).
    pub states: Vec<MaintenanceState>,
    /// Page size.
    pub limit: Limit,
    /// After `(planned_start, id)`, soonest first.
    pub after: Option<(DateTime<Utc>, Uuid)>,
}

/// Service Status storage. Every read takes the caller's Tenant Scope; shared records
/// (no Company) are visible in every scope, the empty one included.
#[async_trait]
pub trait ServiceStatusRepository: Send + Sync {
    /// Every Impact Level.
    async fn impact_levels(&self) -> Result<Vec<ImpactLevel>, RepoError>;
    /// Inserts or updates an Impact Level (a new default clears the old one).
    async fn save_impact_level(
        &self,
        l: &ImpactLevel,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Every Incident Status.
    async fn incident_statuses(&self) -> Result<Vec<ServiceIncidentStatus>, RepoError>;
    /// Inserts or updates an Incident Status (a new default clears the old one).
    async fn save_incident_status(
        &self,
        s: &ServiceIncidentStatus,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;

    /// Services visible in `scope` (active or not).
    async fn services(&self, scope: &TenantScope) -> Result<Vec<StatusService>, RepoError>;
    /// One Service.
    async fn service(
        &self,
        scope: &TenantScope,
        id: StatusServiceId,
    ) -> Result<Option<StatusService>, RepoError>;
    /// Inserts or updates a Service.
    async fn save_service(&self, s: &StatusService, audit: &AuditRecord) -> Result<(), RepoError>;
    /// `(Service, Impact Level)` of every unresolved Incident visible in `scope`.
    async fn unresolved_impacts(
        &self,
        scope: &TenantScope,
    ) -> Result<Vec<(StatusServiceId, Uuid)>, RepoError>;

    /// Inserts an Incident with its first update, atomically, with events.
    async fn insert_incident(
        &self,
        i: &StatusIncident,
        first: &StatusIncidentUpdate,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// Updates an Incident and appends `update`, atomically, with events.
    async fn update_incident(
        &self,
        i: &StatusIncident,
        update: Option<&StatusIncidentUpdate>,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError>;
    /// One Incident.
    async fn incident(
        &self,
        scope: &TenantScope,
        id: StatusIncidentId,
    ) -> Result<Option<StatusIncident>, RepoError>;
    /// Incidents, newest start first.
    async fn incidents(
        &self,
        scope: &TenantScope,
        q: &IncidentQuery,
    ) -> Result<Vec<StatusIncident>, RepoError>;
    /// Incidents with at least one public update, unresolved or resolved since `since`,
    /// newest start first.
    async fn published_incidents(
        &self,
        scope: &TenantScope,
        since: DateTime<Utc>,
        limit: Limit,
    ) -> Result<Vec<StatusIncident>, RepoError>;
    /// The updates of Incidents, oldest first (public ones only when `public_only`).
    async fn updates(
        &self,
        scope: &TenantScope,
        incidents: &[StatusIncidentId],
        public_only: bool,
    ) -> Result<Vec<StatusIncidentUpdate>, RepoError>;
    /// Saves a corrected update.
    async fn correct_update(
        &self,
        u: &StatusIncidentUpdate,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;

    /// Inserts Planned Maintenance.
    async fn insert_maintenance(
        &self,
        m: &PlannedMaintenance,
        audit: &AuditRecord,
    ) -> Result<(), RepoError>;
    /// Saves Planned Maintenance if it is still in `from` (compare-and-set), with the
    /// Incident it opened (`opened`) or changed (`changed`), atomically. `false` when
    /// another writer moved it first.
    async fn transition_maintenance(
        &self,
        m: &PlannedMaintenance,
        from: MaintenanceState,
        change: IncidentWrite<'_>,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<bool, RepoError>;
    /// One Planned Maintenance.
    async fn maintenance(
        &self,
        scope: &TenantScope,
        id: PlannedMaintenanceId,
    ) -> Result<Option<PlannedMaintenance>, RepoError>;
    /// Planned Maintenance, soonest start first.
    async fn maintenances(
        &self,
        scope: &TenantScope,
        q: &MaintenanceQuery,
    ) -> Result<Vec<PlannedMaintenance>, RepoError>;
    /// Maintenance due to start or finish at `now`, in every Company (Automation).
    async fn due_maintenance(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<PlannedMaintenance>, RepoError>;
}

/// What a maintenance transition writes to its Incident.
#[derive(Debug, Clone, Copy)]
pub enum IncidentWrite<'a> {
    /// Nothing (edits and cancellation).
    None,
    /// Opens the Incident with its first update (start).
    Open(&'a StatusIncident, &'a StatusIncidentUpdate),
    /// Changes the Incident, maybe with an update (finish).
    Change(&'a StatusIncident, Option<&'a StatusIncidentUpdate>),
}

/// How Service Status is published (System Settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicationSettings {
    /// The Public Status Page is on.
    pub public_page: bool,
    /// Incidents with public updates are shown on the Portal and the public page.
    pub show_incidents: bool,
    /// Resolved Incidents stay shown for this many days.
    pub recent_days: u32,
}

impl Default for PublicationSettings {
    fn default() -> Self {
        Self {
            public_page: false,
            show_incidents: false,
            recent_days: 7,
        }
    }
}

/// Reads [`PublicationSettings`].
#[async_trait]
pub trait PublicationSettingsProvider: Send + Sync {
    /// The current settings.
    async fn settings(&self) -> PublicationSettings;
}
