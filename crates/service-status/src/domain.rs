//! Service Status domain (ADR-0030). Pure Rust: no I/O.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, DomainEvent, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

cooper_kernel::define_id!(
    /// Identifies a Service.
    StatusServiceId
);
cooper_kernel::define_id!(
    /// Identifies a Service Incident.
    StatusIncidentId
);
cooper_kernel::define_id!(
    /// Identifies a Planned Maintenance.
    PlannedMaintenanceId
);

/// A Service Status rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServiceStatusError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Not allowed in the current state.
    #[error("{0}")]
    InvalidState(String),
}

impl ServiceStatusError {
    /// A validation error.
    #[must_use]
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }

    /// A state error.
    #[must_use]
    pub fn state(reason: impl Into<String>) -> Self {
        Self::InvalidState(reason.into())
    }
}

type Result<T> = std::result::Result<T, ServiceStatusError>;

/// Longest Incident Update or maintenance text.
pub const MAX_TEXT: usize = 20_000;
/// Most Services one Incident or maintenance affects.
pub const MAX_SERVICES: usize = 100;

fn name(field: &'static str, value: &str, max: usize) -> Result<String> {
    let v = value.trim();
    if v.is_empty() || v.chars().count() > max {
        return Err(ServiceStatusError::invalid(
            field,
            format!("must be 1 to {max} characters"),
        ));
    }
    Ok(v.to_owned())
}

fn text(field: &'static str, value: &str) -> Result<String> {
    let v = value.trim();
    if v.chars().count() > MAX_TEXT {
        return Err(ServiceStatusError::invalid(
            field,
            format!("at most {MAX_TEXT} characters"),
        ));
    }
    Ok(v.to_owned())
}

fn colour(c: Option<String>) -> Result<Option<String>> {
    match c {
        Some(c)
            if !(c.len() == 7
                && c.starts_with('#')
                && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())) =>
        {
            Err(ServiceStatusError::invalid("colour", "must be #rrggbb"))
        }
        c => Ok(c),
    }
}

fn keep_default(was: bool, now: bool) -> Result<()> {
    if was && !now {
        return Err(ServiceStatusError::invalid(
            "is_default",
            "make another row the default instead",
        ));
    }
    Ok(())
}

// ----- Reference data ---------------------------------------------------------------------

/// An Impact Level (FR-SST-002): the effect of an Incident on a Service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactLevel {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// 1 is the worst; unique.
    pub severity_order: i32,
    /// Counts towards downtime in availability reporting.
    pub counts_as_downtime: bool,
    /// The all-clear shown when nothing is wrong.
    pub is_default: bool,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// Fields an Administrator edits on an Impact Level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImpactLevelInput {
    /// Name.
    pub name: String,
    /// Colour.
    pub colour: Option<String>,
    /// Severity order (1 is the worst).
    pub severity_order: i32,
    /// Counts as downtime.
    pub counts_as_downtime: bool,
    /// Default.
    pub is_default: bool,
    /// Active.
    pub is_active: bool,
}

impl ImpactLevel {
    /// A new Impact Level.
    ///
    /// # Errors
    ///
    /// Invalid input.
    pub fn create(input: ImpactLevelInput, now: DateTime<Utc>) -> Result<Self> {
        let mut l = Self {
            id: Uuid::now_v7(),
            key: None,
            name: String::new(),
            colour: None,
            severity_order: 0,
            counts_as_downtime: false,
            is_default: false,
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        l.apply(input, now)?;
        Ok(l)
    }

    /// Changes the Impact Level.
    ///
    /// # Errors
    ///
    /// A bad name, colour or order; a default that counts as downtime or is inactive;
    /// unsetting the default.
    pub fn apply(&mut self, input: ImpactLevelInput, now: DateTime<Utc>) -> Result<()> {
        let name = name("name", &input.name, 50)?;
        let colour = colour(input.colour)?;
        if !(1..=1000).contains(&input.severity_order) {
            return Err(ServiceStatusError::invalid(
                "severity_order",
                "must be 1 to 1000",
            ));
        }
        if input.is_default && (input.counts_as_downtime || !input.is_active) {
            return Err(ServiceStatusError::invalid(
                "is_default",
                "the default is the all-clear: active and not downtime",
            ));
        }
        keep_default(self.is_default, input.is_default)?;
        self.name = name;
        self.colour = colour;
        self.severity_order = input.severity_order;
        self.counts_as_downtime = input.counts_as_downtime;
        self.is_default = input.is_default;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }
}

/// An Incident Status (FR-SST-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIncidentStatus {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: Option<String>,
    /// The Incident is over.
    pub is_resolved: bool,
    /// Status of new Incidents.
    pub is_default: bool,
    /// Display order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// Fields an Administrator edits on an Incident Status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncidentStatusInput {
    /// Name.
    pub name: String,
    /// Colour.
    pub colour: Option<String>,
    /// Resolved.
    pub is_resolved: bool,
    /// Default.
    pub is_default: bool,
    /// Order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl ServiceIncidentStatus {
    /// A new Incident Status.
    ///
    /// # Errors
    ///
    /// Invalid input.
    pub fn create(input: IncidentStatusInput, now: DateTime<Utc>) -> Result<Self> {
        let mut s = Self {
            id: Uuid::now_v7(),
            key: None,
            name: String::new(),
            colour: None,
            is_resolved: false,
            is_default: false,
            sort_order: 0,
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        s.apply(input, now)?;
        Ok(s)
    }

    /// Changes the Incident Status.
    ///
    /// # Errors
    ///
    /// A bad name or colour, a resolved or inactive default, unsetting the default.
    pub fn apply(&mut self, input: IncidentStatusInput, now: DateTime<Utc>) -> Result<()> {
        let name = name("name", &input.name, 50)?;
        let colour = colour(input.colour)?;
        if input.is_default && (input.is_resolved || !input.is_active) {
            return Err(ServiceStatusError::invalid(
                "is_default",
                "the default must be active and not resolved",
            ));
        }
        keep_default(self.is_default, input.is_default)?;
        self.name = name;
        self.colour = colour;
        self.is_resolved = input.is_resolved;
        self.is_default = input.is_default;
        self.sort_order = input.sort_order;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }
}

/// The status a started Planned Maintenance takes: the first active, open, non-default
/// status in order (the default says nobody knows what is happening yet), else the
/// default.
#[must_use]
pub fn maintenance_status(statuses: &[ServiceIncidentStatus]) -> Option<&ServiceIncidentStatus> {
    let mut open: Vec<&ServiceIncidentStatus> = statuses
        .iter()
        .filter(|s| s.is_active && !s.is_resolved)
        .collect();
    open.sort_by_key(|s| (s.sort_order, s.name.clone()));
    open.iter()
        .find(|s| !s.is_default)
        .or_else(|| open.iter().find(|s| s.is_default))
        .copied()
}

/// The status a finished Planned Maintenance resolves its Incident with: the first
/// active resolved status in order.
#[must_use]
pub fn resolution_status(statuses: &[ServiceIncidentStatus]) -> Option<&ServiceIncidentStatus> {
    statuses
        .iter()
        .filter(|s| s.is_active && s.is_resolved)
        .min_by_key(|s| (s.sort_order, s.name.clone()))
}

// ----- Services ---------------------------------------------------------------------------

/// What an Administrator edits on a Service.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceDetails {
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Display order.
    pub sort_order: i32,
    /// Shown and selectable.
    pub is_active: bool,
}

impl ServiceDetails {
    fn validate(mut self) -> Result<Self> {
        self.name = name("name", &self.name, 100)?;
        let d = self.description.trim();
        if d.chars().count() > 2000 {
            return Err(ServiceStatusError::invalid(
                "description",
                "at most 2000 characters",
            ));
        }
        self.description = d.to_owned();
        Ok(self)
    }
}

/// A Service shown on the status page (FR-SST-001). Belongs to one Company or is shared
/// (no Company).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusService {
    id: StatusServiceId,
    tenant_id: Option<TenantId>,
    details: ServiceDetails,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl StatusService {
    /// A new Service.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn create(
        tenant_id: Option<TenantId>,
        details: ServiceDetails,
        now: DateTime<Utc>,
    ) -> Result<Self> {
        Ok(Self {
            id: StatusServiceId::new(),
            tenant_id,
            details: details.validate()?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a stored Service.
    #[must_use]
    pub const fn restore(
        id: StatusServiceId,
        tenant_id: Option<TenantId>,
        details: ServiceDetails,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            details,
            created_at,
            updated_at,
        }
    }

    /// Changes the details (deactivating instead of deleting).
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn edit(&mut self, details: ServiceDetails, now: DateTime<Utc>) -> Result<()> {
        self.details = details.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> StatusServiceId {
        self.id
    }
    /// Company (`None`: shared).
    #[must_use]
    pub const fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &ServiceDetails {
        &self.details
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.details.name)),
            ("description", json!(self.details.description)),
            ("sort_order", json!(self.details.sort_order)),
            ("is_active", json!(self.details.is_active)),
        ]
    }
}

/// The Impact Level on each affected Service.
pub type Impacts = BTreeMap<StatusServiceId, Uuid>;

/// The Company of an Incident or maintenance affecting `services`, with impacts naming
/// exactly these Services. They must all be of one Company or all shared; Services that
/// were not affected before (`already`) must be active.
///
/// # Errors
///
/// No Services, too many, mixed Companies or an inactive new Service.
pub fn affected_company(
    services: &[&StatusService],
    already: &Impacts,
) -> Result<Option<TenantId>> {
    let Some(first) = services.first() else {
        return Err(ServiceStatusError::invalid(
            "services",
            "at least one Service is affected",
        ));
    };
    if services.len() > MAX_SERVICES {
        return Err(ServiceStatusError::invalid(
            "services",
            format!("at most {MAX_SERVICES} Services"),
        ));
    }
    if services.iter().any(|s| s.tenant_id != first.tenant_id) {
        return Err(ServiceStatusError::invalid(
            "services",
            "must all be of one Company, or all shared",
        ));
    }
    if services
        .iter()
        .any(|s| !s.details.is_active && !already.contains_key(&s.id))
    {
        return Err(ServiceStatusError::invalid(
            "services",
            "a Service that is not active cannot be added",
        ));
    }
    Ok(first.tenant_id)
}

/// The Current Status of a Service (FR-SST-006): the worst (lowest severity order)
/// Impact Level among `unresolved` (the impacts of its unresolved Incidents), or the
/// default Impact Level when there are none. `None` only when `levels` has no default.
#[must_use]
pub fn current_status(
    levels: &[ImpactLevel],
    unresolved: impl IntoIterator<Item = Uuid>,
) -> Option<&ImpactLevel> {
    unresolved
        .into_iter()
        .filter_map(|id| levels.iter().find(|l| l.id == id))
        .min_by_key(|l| l.severity_order)
        .or_else(|| levels.iter().find(|l| l.is_default))
}

// ----- Service Incidents ------------------------------------------------------------------

/// A timestamped update on a Service Incident (FR-SST-005), with a snapshot of the
/// status and the impacts at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusIncidentUpdate {
    /// ID.
    pub id: Uuid,
    /// The Incident.
    pub incident_id: StatusIncidentId,
    /// Company (`None`: shared).
    pub tenant_id: Option<TenantId>,
    /// Incident Status at the time.
    pub status_id: Uuid,
    /// Impacts at the time.
    pub impacts: Impacts,
    /// What was said (may be empty when only the status or impacts moved).
    pub body: String,
    /// Shown to Requesters and on the Public Status Page.
    pub is_public: bool,
    /// Who posted it.
    pub author: Actor,
    /// When it took effect.
    pub posted_at: DateTime<Utc>,
    /// When its wording or visibility was last corrected.
    pub edited_at: Option<DateTime<Utc>>,
}

impl StatusIncidentUpdate {
    /// Corrects the wording and visibility; the snapshot stays what was true then.
    ///
    /// # Errors
    ///
    /// Text too long.
    pub fn correct(&mut self, body: &str, is_public: bool, now: DateTime<Utc>) -> Result<()> {
        self.body = text("body", body)?;
        self.is_public = is_public;
        self.edited_at = Some(now);
        Ok(())
    }
}

/// What an Analyst posts on an Incident.
#[derive(Debug, Clone, Default)]
pub struct IncidentChange {
    /// New status (`None`: unchanged).
    pub status: Option<ServiceIncidentStatus>,
    /// New impacts, replacing all (`None`: unchanged).
    pub impacts: Option<Impacts>,
    /// Text.
    pub body: String,
    /// Public.
    pub is_public: bool,
}

/// How an update moved the Incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Still open, or still resolved.
    None,
    /// Entered a resolved status.
    Resolved,
    /// Left a resolved status.
    Reopened,
}

/// A Service Incident (FR-SST-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusIncident {
    id: StatusIncidentId,
    tenant_id: Option<TenantId>,
    title: String,
    status_id: Uuid,
    is_resolved: bool,
    impacts: Impacts,
    started_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
    planned_maintenance_id: Option<PlannedMaintenanceId>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Stored state, for [`StatusIncident::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusIncidentRecord {
    /// ID.
    pub id: StatusIncidentId,
    /// Company.
    pub tenant_id: Option<TenantId>,
    /// Title.
    pub title: String,
    /// Status.
    pub status_id: Uuid,
    /// Whether the status is resolved.
    pub is_resolved: bool,
    /// Impacts.
    pub impacts: Impacts,
    /// Started.
    pub started_at: DateTime<Utc>,
    /// Resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// The Planned Maintenance it came from.
    pub planned_maintenance_id: Option<PlannedMaintenanceId>,
    /// Creator.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// A new Service Incident.
#[derive(Debug, Clone)]
pub struct NewIncident {
    /// Company of its Services (see [`affected_company`]).
    pub tenant_id: Option<TenantId>,
    /// Title.
    pub title: String,
    /// Status.
    pub status: ServiceIncidentStatus,
    /// Impacts.
    pub impacts: Impacts,
    /// Text of the opening update.
    pub body: String,
    /// Whether the opening update is public.
    pub is_public: bool,
    /// When the disruption began (`None`: now). Never in the future.
    pub started_at: Option<DateTime<Utc>>,
}

fn check_impacts(impacts: &Impacts) -> Result<()> {
    if impacts.is_empty() {
        return Err(ServiceStatusError::invalid(
            "services",
            "at least one Service is affected",
        ));
    }
    if impacts.len() > MAX_SERVICES {
        return Err(ServiceStatusError::invalid(
            "services",
            format!("at most {MAX_SERVICES} Services"),
        ));
    }
    Ok(())
}

impl StatusIncident {
    /// Opens an Incident and its first update (the opening snapshot).
    ///
    /// # Errors
    ///
    /// A bad title or text, an inactive status, no Services, or a start in the future.
    pub fn open(
        new: NewIncident,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<(Self, StatusIncidentUpdate)> {
        let title = name("title", &new.title, 255)?;
        let body = text("body", &new.body)?;
        check_impacts(&new.impacts)?;
        if !new.status.is_active {
            return Err(ServiceStatusError::invalid("status_id", "is not active"));
        }
        let started_at = new.started_at.unwrap_or(now);
        if started_at > now {
            return Err(ServiceStatusError::invalid(
                "started_at",
                "must not be in the future",
            ));
        }
        let incident = Self {
            id: StatusIncidentId::new(),
            tenant_id: new.tenant_id,
            title,
            status_id: new.status.id,
            is_resolved: new.status.is_resolved,
            impacts: new.impacts,
            started_at,
            resolved_at: new.status.is_resolved.then_some(now),
            planned_maintenance_id: None,
            created_by: by,
            created_at: now,
            updated_at: now,
        };
        let first = incident.snapshot(body, new.is_public, by, started_at);
        Ok((incident, first))
    }

    /// Rebuilds a stored Incident.
    #[must_use]
    pub fn restore(r: StatusIncidentRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            title: r.title,
            status_id: r.status_id,
            is_resolved: r.is_resolved,
            impacts: r.impacts,
            started_at: r.started_at,
            resolved_at: r.resolved_at,
            planned_maintenance_id: r.planned_maintenance_id,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    fn snapshot(
        &self,
        body: String,
        is_public: bool,
        by: Actor,
        at: DateTime<Utc>,
    ) -> StatusIncidentUpdate {
        StatusIncidentUpdate {
            id: Uuid::now_v7(),
            incident_id: self.id,
            tenant_id: self.tenant_id,
            status_id: self.status_id,
            impacts: self.impacts.clone(),
            body,
            is_public,
            author: by,
            posted_at: at,
            edited_at: None,
        }
    }

    /// Posts an update that took effect at `at` (now, or earlier when backdated, never
    /// before the start). Recorded only when it says something new: a status change, an
    /// impact change or text.
    ///
    /// # Errors
    ///
    /// Nothing new, an inactive status, no Services, text too long or a bad time.
    pub fn post_update(
        &mut self,
        change: IncidentChange,
        by: Actor,
        at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(StatusIncidentUpdate, Transition)> {
        let body = text("body", &change.body)?;
        if at > now || at < self.started_at {
            return Err(ServiceStatusError::invalid(
                "posted_at",
                "must be between the start and now",
            ));
        }
        let status = change.status.filter(|s| s.id != self.status_id);
        if let Some(s) = &status
            && !s.is_active
        {
            return Err(ServiceStatusError::invalid("status_id", "is not active"));
        }
        let impacts = change.impacts.filter(|i| *i != self.impacts);
        if let Some(i) = &impacts {
            check_impacts(i)?;
        }
        if status.is_none() && impacts.is_none() && body.is_empty() {
            return Err(ServiceStatusError::invalid(
                "body",
                "an update needs new text, a new status or new impacts",
            ));
        }
        let mut transition = Transition::None;
        if let Some(s) = status {
            transition = match (self.is_resolved, s.is_resolved) {
                (false, true) => {
                    self.resolved_at = Some(at);
                    Transition::Resolved
                }
                (true, false) => {
                    self.resolved_at = None;
                    Transition::Reopened
                }
                _ => Transition::None,
            };
            self.status_id = s.id;
            self.is_resolved = s.is_resolved;
        }
        if let Some(i) = impacts {
            self.impacts = i;
        }
        self.updated_at = now;
        Ok((self.snapshot(body, change.is_public, by, at), transition))
    }

    /// Changes the title.
    ///
    /// # Errors
    ///
    /// A bad title.
    pub fn retitle(&mut self, title: &str, now: DateTime<Utc>) -> Result<()> {
        self.title = name("title", title, 255)?;
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> StatusIncidentId {
        self.id
    }
    /// Company (`None`: shared).
    #[must_use]
    pub const fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
    /// Title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Status.
    #[must_use]
    pub const fn status_id(&self) -> Uuid {
        self.status_id
    }
    /// Whether the status is resolved.
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        self.is_resolved
    }
    /// Impacts.
    #[must_use]
    pub const fn impacts(&self) -> &Impacts {
        &self.impacts
    }
    /// Started.
    #[must_use]
    pub const fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }
    /// Resolved.
    #[must_use]
    pub const fn resolved_at(&self) -> Option<DateTime<Utc>> {
        self.resolved_at
    }
    /// The Planned Maintenance it came from.
    #[must_use]
    pub const fn planned_maintenance_id(&self) -> Option<PlannedMaintenanceId> {
        self.planned_maintenance_id
    }
    /// Creator.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("title", json!(self.title)),
            ("status_id", json!(self.status_id)),
            ("impacts", impacts_json(&self.impacts)),
            ("started_at", json!(self.started_at)),
            ("resolved_at", json!(self.resolved_at)),
        ]
    }
}

/// Impacts as JSON (`{service_id: impact_level_id}`), for audit entries.
#[must_use]
pub fn impacts_json(impacts: &Impacts) -> Value {
    Value::Object(
        impacts
            .iter()
            .map(|(s, l)| (s.to_string(), json!(l)))
            .collect(),
    )
}

// ----- Planned Maintenance ----------------------------------------------------------------

/// State of a Planned Maintenance (fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceState {
    /// Announced, not started.
    Scheduled,
    /// Under way: its Service Incident is open (or was resolved by hand).
    Started,
    /// Over.
    Finished,
    /// Called off before it started.
    Cancelled,
}

impl MaintenanceState {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Started => "started",
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for MaintenanceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MaintenanceState {
    type Err = ServiceStatusError;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "scheduled" => Ok(Self::Scheduled),
            "started" => Ok(Self::Started),
            "finished" => Ok(Self::Finished),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ServiceStatusError::invalid(
                "state",
                "must be scheduled, started, finished or cancelled",
            )),
        }
    }
}

/// What an Analyst plans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceDetails {
    /// Title.
    pub title: String,
    /// Public description, posted when it starts.
    pub description: String,
    /// Planned start.
    pub planned_start: DateTime<Utc>,
    /// Planned end (after the start).
    pub planned_end: DateTime<Utc>,
    /// Affected Services and their Impact Levels.
    pub impacts: Impacts,
}

impl MaintenanceDetails {
    fn validate(mut self, now: DateTime<Utc>) -> Result<Self> {
        self.title = name("title", &self.title, 255)?;
        self.description = text("description", &self.description)?;
        if self.planned_end <= self.planned_start {
            return Err(ServiceStatusError::invalid(
                "planned_end",
                "must be after the planned start",
            ));
        }
        if self.planned_end <= now {
            return Err(ServiceStatusError::invalid(
                "planned_end",
                "must be in the future",
            ));
        }
        check_impacts(&self.impacts)?;
        Ok(self)
    }
}

/// A Planned Maintenance (FR-SST-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMaintenance {
    id: PlannedMaintenanceId,
    tenant_id: Option<TenantId>,
    details: MaintenanceDetails,
    state: MaintenanceState,
    incident_id: Option<StatusIncidentId>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Stored state, for [`PlannedMaintenance::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMaintenanceRecord {
    /// ID.
    pub id: PlannedMaintenanceId,
    /// Company.
    pub tenant_id: Option<TenantId>,
    /// Details.
    pub details: MaintenanceDetails,
    /// State.
    pub state: MaintenanceState,
    /// Its Incident once started.
    pub incident_id: Option<StatusIncidentId>,
    /// Who scheduled it.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl PlannedMaintenance {
    /// Schedules maintenance.
    ///
    /// # Errors
    ///
    /// Invalid details or an end in the past.
    pub fn schedule(
        tenant_id: Option<TenantId>,
        details: MaintenanceDetails,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self> {
        Ok(Self {
            id: PlannedMaintenanceId::new(),
            tenant_id,
            details: details.validate(now)?,
            state: MaintenanceState::Scheduled,
            incident_id: None,
            created_by: by,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds stored maintenance.
    #[must_use]
    pub fn restore(r: PlannedMaintenanceRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            details: r.details,
            state: r.state,
            incident_id: r.incident_id,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    fn require(&self, state: MaintenanceState, action: &str) -> Result<()> {
        if self.state != state {
            return Err(ServiceStatusError::state(format!(
                "only {state} maintenance can be {action} (this one is {})",
                self.state
            )));
        }
        Ok(())
    }

    /// Replaces the plan (while scheduled).
    ///
    /// # Errors
    ///
    /// Not scheduled, or invalid details.
    pub fn edit(&mut self, details: MaintenanceDetails, now: DateTime<Utc>) -> Result<()> {
        self.require(MaintenanceState::Scheduled, "edited")?;
        self.details = details.validate(now)?;
        self.updated_at = now;
        Ok(())
    }

    /// Calls it off (while scheduled).
    ///
    /// # Errors
    ///
    /// Not scheduled.
    pub fn cancel(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.require(MaintenanceState::Scheduled, "cancelled")?;
        self.state = MaintenanceState::Cancelled;
        self.updated_at = now;
        Ok(())
    }

    /// Whether it should start (scheduled and its start has come).
    #[must_use]
    pub fn due_to_start(&self, now: DateTime<Utc>) -> bool {
        self.state == MaintenanceState::Scheduled && self.details.planned_start <= now
    }

    /// Whether it should finish (started and its end has come).
    #[must_use]
    pub fn due_to_finish(&self, now: DateTime<Utc>) -> bool {
        self.state == MaintenanceState::Started && self.details.planned_end <= now
    }

    /// Starts it: it becomes a Service Incident in `status` on its Services, as the
    /// Analyst who scheduled it, backdated to the planned start (an early start by hand
    /// starts now). The first update is public and carries the description.
    ///
    /// # Errors
    ///
    /// Not scheduled, or a resolved or inactive status.
    pub fn start(
        &mut self,
        status: &ServiceIncidentStatus,
        now: DateTime<Utc>,
    ) -> Result<(StatusIncident, StatusIncidentUpdate)> {
        self.require(MaintenanceState::Scheduled, "started")?;
        if status.is_resolved {
            return Err(ServiceStatusError::invalid(
                "status_id",
                "a started maintenance is not resolved",
            ));
        }
        let (mut incident, first) = StatusIncident::open(
            NewIncident {
                tenant_id: self.tenant_id,
                title: self.details.title.clone(),
                status: status.clone(),
                impacts: self.details.impacts.clone(),
                body: self.details.description.clone(),
                is_public: true,
                started_at: Some(self.details.planned_start.min(now)),
            },
            self.created_by,
            now,
        )?;
        incident.planned_maintenance_id = Some(self.id);
        self.state = MaintenanceState::Started;
        self.incident_id = Some(incident.id);
        self.updated_at = now;
        Ok((incident, first))
    }

    /// Finishes it, resolving its Incident with `resolved` at the planned end (or now,
    /// when finished early by hand), unless the Incident is already resolved: a
    /// resolution by hand stands. Returns the resolving update, if any.
    ///
    /// # Errors
    ///
    /// Not started, or `incident` is not its Incident.
    pub fn finish(
        &mut self,
        incident: &mut StatusIncident,
        resolved: Option<&ServiceIncidentStatus>,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Option<StatusIncidentUpdate>> {
        self.require(MaintenanceState::Started, "finished")?;
        if self.incident_id != Some(incident.id) {
            return Err(ServiceStatusError::state(
                "not the Incident of this maintenance",
            ));
        }
        let update = match resolved {
            Some(s) if !incident.is_resolved && s.is_resolved => {
                let at = self.details.planned_end.min(now).max(incident.started_at);
                let change = IncidentChange {
                    status: Some(s.clone()),
                    impacts: None,
                    body: String::new(),
                    is_public: true,
                };
                Some(incident.post_update(change, by, at, now)?.0)
            }
            _ => None,
        };
        self.state = MaintenanceState::Finished;
        self.updated_at = now;
        Ok(update)
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> PlannedMaintenanceId {
        self.id
    }
    /// Company (`None`: shared).
    #[must_use]
    pub const fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &MaintenanceDetails {
        &self.details
    }
    /// State.
    #[must_use]
    pub const fn state(&self) -> MaintenanceState {
        self.state
    }
    /// Its Incident once started.
    #[must_use]
    pub const fn incident_id(&self) -> Option<StatusIncidentId> {
        self.incident_id
    }
    /// Who scheduled it.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        vec![
            ("title", json!(d.title)),
            ("description", json!(d.description)),
            ("planned_start", json!(d.planned_start)),
            ("planned_end", json!(d.planned_end)),
            ("impacts", impacts_json(&d.impacts)),
            ("state", json!(self.state.as_str())),
            ("incident_id", json!(self.incident_id.map(|i| i.as_uuid()))),
        ]
    }
}

// ----- Domain Events ----------------------------------------------------------------------

/// A Service Incident was opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusIncidentOpened {
    /// The Incident.
    pub incident_id: StatusIncidentId,
    /// Its Company (`None`: shared).
    pub tenant_id: Option<TenantId>,
    /// The Planned Maintenance it came from.
    pub planned_maintenance_id: Option<PlannedMaintenanceId>,
}

impl DomainEvent for StatusIncidentOpened {
    const EVENT_TYPE: &'static str = "status_incident.opened";
    const AGGREGATE_TYPE: &'static str = "status_incident";
    fn aggregate_id(&self) -> Uuid {
        self.incident_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
}

/// An Incident Update was posted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusIncidentUpdated {
    /// The Incident.
    pub incident_id: StatusIncidentId,
    /// Its Company (`None`: shared).
    pub tenant_id: Option<TenantId>,
    /// The update.
    pub update_id: Uuid,
    /// Whether the update is public.
    pub is_public: bool,
}

impl DomainEvent for StatusIncidentUpdated {
    const EVENT_TYPE: &'static str = "status_incident.updated";
    const AGGREGATE_TYPE: &'static str = "status_incident";
    fn aggregate_id(&self) -> Uuid {
        self.incident_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
}

/// A Service Incident entered a resolved status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusIncidentResolved {
    /// The Incident.
    pub incident_id: StatusIncidentId,
    /// Its Company (`None`: shared).
    pub tenant_id: Option<TenantId>,
    /// When.
    pub resolved_at: DateTime<Utc>,
}

impl DomainEvent for StatusIncidentResolved {
    const EVENT_TYPE: &'static str = "status_incident.resolved";
    const AGGREGATE_TYPE: &'static str = "status_incident";
    fn aggregate_id(&self) -> Uuid {
        self.incident_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        self.tenant_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn now() -> DateTime<Utc> {
        at("2026-11-09T10:00:00Z")
    }

    fn level(order: i32, default: bool) -> ImpactLevel {
        ImpactLevel::create(
            ImpactLevelInput {
                name: format!("L{order}"),
                severity_order: order,
                is_default: default,
                is_active: true,
                ..ImpactLevelInput::default()
            },
            now(),
        )
        .unwrap()
    }

    fn status(resolved: bool, default: bool, order: i32) -> ServiceIncidentStatus {
        ServiceIncidentStatus::create(
            IncidentStatusInput {
                name: format!("S{order}"),
                is_resolved: resolved,
                is_default: default,
                sort_order: order,
                is_active: true,
                ..IncidentStatusInput::default()
            },
            now(),
        )
        .unwrap()
    }

    fn service(tenant: Option<TenantId>, active: bool) -> StatusService {
        StatusService::create(
            tenant,
            ServiceDetails {
                name: "Email".into(),
                is_active: active,
                ..ServiceDetails::default()
            },
            now(),
        )
        .unwrap()
    }

    fn impacts(services: &[&StatusService], level: &ImpactLevel) -> Impacts {
        services.iter().map(|s| (s.id(), level.id)).collect()
    }

    fn incident(st: &ServiceIncidentStatus, i: Impacts) -> (StatusIncident, StatusIncidentUpdate) {
        StatusIncident::open(
            NewIncident {
                tenant_id: None,
                title: "Email is slow".into(),
                status: st.clone(),
                impacts: i,
                body: String::new(),
                is_public: false,
                started_at: None,
            },
            Actor::Automation,
            now(),
        )
        .unwrap()
    }

    #[test]
    fn impact_levels_keep_the_default_as_an_active_all_clear() {
        let input = |downtime, default, active| ImpactLevelInput {
            name: "X".into(),
            severity_order: 3,
            counts_as_downtime: downtime,
            is_default: default,
            is_active: active,
            ..ImpactLevelInput::default()
        };
        assert!(ImpactLevel::create(input(true, true, true), now()).is_err());
        assert!(ImpactLevel::create(input(false, true, false), now()).is_err());
        assert!(
            ImpactLevel::create(
                ImpactLevelInput {
                    severity_order: 0,
                    ..input(false, false, true)
                },
                now()
            )
            .is_err()
        );
        let mut d = ImpactLevel::create(input(false, true, true), now()).unwrap();
        assert!(
            d.apply(input(false, false, true), now()).is_err(),
            "unset by choosing another default"
        );
        assert!(
            ImpactLevel::create(
                ImpactLevelInput {
                    colour: Some("red".into()),
                    ..input(false, false, true)
                },
                now()
            )
            .is_err()
        );
    }

    #[test]
    fn incident_statuses_keep_the_default_open_and_active() {
        let input = |resolved, default, active| IncidentStatusInput {
            name: "X".into(),
            is_resolved: resolved,
            is_default: default,
            is_active: active,
            ..IncidentStatusInput::default()
        };
        assert!(ServiceIncidentStatus::create(input(true, true, true), now()).is_err());
        assert!(ServiceIncidentStatus::create(input(false, true, false), now()).is_err());
        assert!(ServiceIncidentStatus::create(input(true, false, true), now()).is_ok());
    }

    #[test]
    fn current_status_is_the_worst_unresolved_impact_or_the_default() {
        let levels = vec![
            level(1, false),
            level(3, false),
            level(5, true),
            level(6, false),
        ];
        assert_eq!(
            current_status(&levels, []).unwrap().severity_order,
            5,
            "nothing unresolved: the default"
        );
        let ids = [levels[1].id, levels[0].id, levels[3].id];
        assert_eq!(current_status(&levels, ids).unwrap().severity_order, 1);
        assert_eq!(
            current_status(&levels, [levels[3].id])
                .unwrap()
                .severity_order,
            6,
            "an informational impact is still the worst present"
        );
        assert_eq!(
            current_status(&levels, [Uuid::now_v7()])
                .unwrap()
                .severity_order,
            5,
            "unknown levels are ignored"
        );
        assert!(current_status(&levels[..2], []).is_none());
    }

    proptest! {
        #[test]
        fn current_status_never_beats_an_unresolved_impact(orders in proptest::collection::btree_set(1i32..100, 1..8),
                                                           pick in proptest::collection::vec(any::<prop::sample::Index>(), 0..6)) {
            let mut levels: Vec<ImpactLevel> = orders.iter().map(|o| level(*o, false)).collect();
            levels.push(level(500, true));
            let chosen: Vec<Uuid> = pick.iter().map(|i| levels[i.index(levels.len() - 1)].id).collect();
            let s = current_status(&levels, chosen.clone()).unwrap();
            if chosen.is_empty() {
                prop_assert!(s.is_default);
            } else {
                prop_assert!(chosen.contains(&s.id));
                for c in &chosen {
                    let l = levels.iter().find(|l| l.id == *c).unwrap();
                    prop_assert!(s.severity_order <= l.severity_order);
                }
            }
        }
    }

    #[test]
    fn affected_services_share_one_company_and_new_ones_are_active() {
        let t = TenantId::new();
        let (a, b, shared, off) = (
            service(Some(t), true),
            service(Some(t), true),
            service(None, true),
            service(Some(t), false),
        );
        assert_eq!(affected_company(&[&a, &b], &Impacts::new()), Ok(Some(t)));
        assert_eq!(affected_company(&[&shared], &Impacts::new()), Ok(None));
        assert!(affected_company(&[&a, &shared], &Impacts::new()).is_err());
        assert!(affected_company(&[], &Impacts::new()).is_err());
        assert!(affected_company(&[&a, &off], &Impacts::new()).is_err());
        let already = impacts(&[&off], &level(1, false));
        assert!(
            affected_company(&[&a, &off], &already).is_ok(),
            "an inactive Service already affected may stay"
        );
    }

    #[test]
    fn opening_an_incident_records_the_opening_snapshot() {
        let (open, resolved) = (status(false, true, 1), status(true, false, 9));
        let s = service(None, true);
        let l = level(2, false);
        let (i, first) = incident(&open, impacts(&[&s], &l));
        assert_eq!(first.status_id, open.id);
        assert_eq!(first.impacts, *i.impacts());
        assert!(!first.is_public, "internal by default");
        assert!(i.resolved_at().is_none());
        let bad = |n: NewIncident| StatusIncident::open(n, Actor::Automation, now()).is_err();
        let base = NewIncident {
            tenant_id: None,
            title: "t".into(),
            status: open.clone(),
            impacts: impacts(&[&s], &l),
            body: String::new(),
            is_public: false,
            started_at: None,
        };
        assert!(bad(NewIncident {
            impacts: Impacts::new(),
            ..base.clone()
        }));
        assert!(bad(NewIncident {
            title: " ".into(),
            ..base.clone()
        }));
        assert!(bad(NewIncident {
            started_at: Some(at("2026-11-10T00:00:00Z")),
            ..base.clone()
        }));
        let (r, _) = StatusIncident::open(
            NewIncident {
                status: resolved,
                started_at: Some(at("2026-11-09T08:00:00Z")),
                ..base
            },
            Actor::Automation,
            now(),
        )
        .unwrap();
        assert_eq!(r.resolved_at(), Some(now()), "logged after the fact");
    }

    #[test]
    fn updates_need_something_new_and_stamp_resolution() {
        let (open, other, resolved) = (
            status(false, true, 1),
            status(false, false, 2),
            status(true, false, 9),
        );
        let s = service(None, true);
        let (l1, l2) = (level(1, false), level(3, false));
        let (mut i, _) = incident(&open, impacts(&[&s], &l1));
        let post = |i: &mut StatusIncident, change: IncidentChange| {
            i.post_update(change, Actor::Automation, now(), now())
        };
        assert!(
            post(
                &mut i,
                IncidentChange {
                    status: Some(open.clone()),
                    impacts: Some(impacts(&[&s], &l1)),
                    body: "  ".into(),
                    is_public: true,
                }
            )
            .is_err(),
            "nothing new"
        );
        let (u, t) = post(
            &mut i,
            IncidentChange {
                impacts: Some(impacts(&[&s], &l2)),
                ..IncidentChange::default()
            },
        )
        .unwrap();
        assert_eq!(t, Transition::None);
        assert_eq!(u.impacts[&s.id()], l2.id, "the snapshot has the new impact");
        assert_eq!(u.status_id, open.id);
        let (_, t) = post(
            &mut i,
            IncidentChange {
                status: Some(other.clone()),
                body: "Found it".into(),
                is_public: true,
                ..IncidentChange::default()
            },
        )
        .unwrap();
        assert_eq!(t, Transition::None);
        let (u, t) = post(
            &mut i,
            IncidentChange {
                status: Some(resolved.clone()),
                ..IncidentChange::default()
            },
        )
        .unwrap();
        assert_eq!(t, Transition::Resolved);
        assert_eq!(i.resolved_at(), Some(now()));
        assert!(u.body.is_empty(), "a status change alone repeats nothing");
        let (_, t) = post(
            &mut i,
            IncidentChange {
                status: Some(other),
                ..IncidentChange::default()
            },
        )
        .unwrap();
        assert_eq!(t, Transition::Reopened);
        assert!(i.resolved_at().is_none());
        assert!(
            post(
                &mut i,
                IncidentChange {
                    impacts: Some(Impacts::new()),
                    ..IncidentChange::default()
                }
            )
            .is_err(),
            "an Incident keeps a Service"
        );
        assert!(
            i.post_update(
                IncidentChange {
                    body: "x".into(),
                    ..IncidentChange::default()
                },
                Actor::Automation,
                at("2026-11-09T09:00:00Z"),
                now()
            )
            .is_err(),
            "not before the start"
        );
    }

    #[test]
    fn corrections_change_wording_and_visibility_only() {
        let open = status(false, true, 1);
        let s = service(None, true);
        let (_, mut u) = incident(&open, impacts(&[&s], &level(1, false)));
        let snapshot = (u.status_id, u.impacts.clone(), u.posted_at);
        u.correct(" Fixed typo ", true, now()).unwrap();
        assert_eq!(u.body, "Fixed typo");
        assert!(u.is_public);
        assert_eq!(u.edited_at, Some(now()));
        assert_eq!((u.status_id, u.impacts.clone(), u.posted_at), snapshot);
    }

    fn plan(start: &str, end: &str, s: &StatusService) -> MaintenanceDetails {
        MaintenanceDetails {
            title: "Move day".into(),
            description: "Phones down".into(),
            planned_start: at(start),
            planned_end: at(end),
            impacts: impacts(&[s], &level(4, false)),
        }
    }

    #[test]
    fn maintenance_plans_are_validated() {
        let s = service(None, true);
        let by = Actor::Automation;
        assert!(
            PlannedMaintenance::schedule(
                None,
                plan("2026-11-14T10:00:00Z", "2026-11-14T08:00:00Z", &s),
                by,
                now()
            )
            .is_err(),
            "end after start"
        );
        assert!(
            PlannedMaintenance::schedule(
                None,
                plan("2026-11-01T10:00:00Z", "2026-11-02T08:00:00Z", &s),
                by,
                now()
            )
            .is_err(),
            "already over"
        );
        let mut m = PlannedMaintenance::schedule(
            None,
            plan("2026-11-14T08:00:00Z", "2026-11-14T14:00:00Z", &s),
            by,
            now(),
        )
        .unwrap();
        assert_eq!(m.state(), MaintenanceState::Scheduled);
        assert!(!m.due_to_start(now()));
        assert!(m.due_to_start(at("2026-11-14T08:00:00Z")));
        m.cancel(now()).unwrap();
        assert!(m.cancel(now()).is_err());
        assert!(
            m.edit(
                plan("2026-11-15T08:00:00Z", "2026-11-15T14:00:00Z", &s),
                now()
            )
            .is_err(),
            "only scheduled maintenance is edited"
        );
        assert!(m.start(&status(false, false, 2), now()).is_err());
    }

    #[test]
    fn maintenance_moves_scheduled_started_finished_and_becomes_an_incident() {
        let s = service(None, true);
        let analyst = Actor::Analyst(cooper_kernel::AnalystId::new());
        let mut m = PlannedMaintenance::schedule(
            None,
            plan("2026-11-14T08:00:00Z", "2026-11-14T14:00:00Z", &s),
            analyst,
            now(),
        )
        .unwrap();
        let identified = status(false, false, 2);
        assert!(m.start(&status(true, false, 9), now()).is_err());
        let late = at("2026-11-14T08:47:00Z");
        let (mut i, first) = m.start(&identified, late).unwrap();
        assert_eq!(m.state(), MaintenanceState::Started);
        assert_eq!(m.incident_id(), Some(i.id()));
        assert_eq!(i.planned_maintenance_id(), Some(m.id()));
        assert_eq!(i.started_at(), at("2026-11-14T08:00:00Z"), "backdated");
        assert_eq!(i.created_by(), analyst, "as the person who planned it");
        assert!(first.is_public, "announced in advance");
        assert_eq!(first.body, "Phones down");
        assert_eq!(*i.impacts(), m.details().impacts);
        assert!(m.start(&identified, late).is_err(), "starts once");
        assert!(!m.due_to_finish(at("2026-11-14T13:59:00Z")));
        let end = at("2026-11-14T14:05:00Z");
        assert!(m.due_to_finish(end));
        let resolved = status(true, false, 9);
        let u = m
            .finish(&mut i, Some(&resolved), Actor::Automation, end)
            .unwrap()
            .unwrap();
        assert_eq!(m.state(), MaintenanceState::Finished);
        assert_eq!(
            i.resolved_at(),
            Some(at("2026-11-14T14:00:00Z")),
            "backdated"
        );
        assert_eq!(u.posted_at, at("2026-11-14T14:00:00Z"));
        assert!(
            m.finish(&mut i, Some(&resolved), Actor::Automation, end)
                .is_err()
        );
    }

    #[test]
    fn an_early_start_starts_now_and_a_resolution_by_hand_stands() {
        let s = service(None, true);
        let mut m = PlannedMaintenance::schedule(
            None,
            plan("2026-11-14T08:00:00Z", "2026-11-14T14:00:00Z", &s),
            Actor::Automation,
            now(),
        )
        .unwrap();
        let (mut i, _) = m.start(&status(false, false, 2), now()).unwrap();
        assert_eq!(i.started_at(), now(), "never forward");
        let by_hand = at("2026-11-14T12:00:00Z");
        i.post_update(
            IncidentChange {
                status: Some(status(true, false, 8)),
                ..IncidentChange::default()
            },
            Actor::Automation,
            by_hand,
            by_hand,
        )
        .unwrap();
        let other = status(true, false, 9);
        let u = m
            .finish(
                &mut i,
                Some(&other),
                Actor::Automation,
                at("2026-11-14T15:00:00Z"),
            )
            .unwrap();
        assert!(u.is_none());
        assert_eq!(i.resolved_at(), Some(by_hand));
        let mut wrong = PlannedMaintenance::schedule(
            None,
            plan("2026-11-14T08:00:00Z", "2026-11-14T14:00:00Z", &s),
            Actor::Automation,
            now(),
        )
        .unwrap();
        wrong.start(&status(false, false, 2), now()).unwrap();
        assert!(
            wrong
                .finish(&mut i, None, Actor::Automation, now())
                .is_err()
        );
    }

    #[test]
    fn maintenance_and_resolution_statuses_come_from_flags_and_order() {
        let statuses = vec![
            status(false, true, 10),
            status(false, false, 30),
            status(false, false, 20),
            status(true, false, 50),
            status(true, false, 40),
        ];
        assert_eq!(maintenance_status(&statuses).unwrap().sort_order, 20);
        assert_eq!(resolution_status(&statuses).unwrap().sort_order, 40);
        assert_eq!(
            maintenance_status(&statuses[..1]).unwrap().sort_order,
            10,
            "the default when nothing else is open"
        );
        assert!(resolution_status(&statuses[..3]).is_none());
    }
}
