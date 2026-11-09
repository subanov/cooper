//! Service Status use cases (ADR-0030).

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use cooper_kernel::{
    Actor, AuditAction, AuditRecord, Clock, DomainEvent, EventEnvelope, Forbidden, Limit,
    Principal, TenantId, TenantScope,
};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    ImpactLevel, ImpactLevelInput, Impacts, IncidentChange, IncidentStatusInput,
    MaintenanceDetails, MaintenanceState, NewIncident, PlannedMaintenance, PlannedMaintenanceId,
    ServiceDetails, ServiceIncidentStatus, ServiceStatusError, StatusIncident, StatusIncidentId,
    StatusIncidentOpened, StatusIncidentResolved, StatusIncidentUpdate, StatusIncidentUpdated,
    StatusService, StatusServiceId, Transition, affected_company, current_status,
    maintenance_status, resolution_status,
};
use crate::ports::{
    IncidentQuery, IncidentWrite, MaintenanceQuery, PublicationSettingsProvider, RepoError,
    ServiceStatusRepository,
};

/// Capabilities Service Status checks (FR-IAM-010).
pub mod capabilities {
    /// See Services, Incidents and Planned Maintenance.
    pub const READ: &str = "service-status.read";
    /// Open Incidents, post Incident Updates, plan Maintenance.
    pub const UPDATE: &str = "service-status.update";
    /// Manage Services, Impact Levels and Incident Statuses.
    pub const CONFIGURE: &str = "service-status.configure";

    /// Every Capability declared by Service Status.
    pub const ALL: &[&str] = &[READ, UPDATE, CONFIGURE];
}

/// System Settings Service Status reads (FR-SST-009, FR-PRT-006).
pub mod settings {
    use cooper_kernel::{SettingDefinition, SettingKind};

    /// The Public Status Page is on.
    pub const PUBLIC_PAGE: SettingDefinition = SettingDefinition {
        key: "service_status.public_page",
        description: "Publish the Public Status Page: shared Services, their status and Planned Maintenance, readable without signing in.",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// Incidents are shown to Requesters and the public.
    pub const SHOW_INCIDENTS: SettingDefinition = SettingDefinition {
        key: "service_status.show_incidents",
        description: "Show Service Incidents that have a public Incident Update, with their public updates, on the Portal and the Public Status Page.",
        kind: SettingKind::Bool,
        default: "false",
    };

    /// How long resolved Incidents stay shown.
    pub const RECENT_DAYS: SettingDefinition = SettingDefinition {
        key: "service_status.recent_days",
        description: "Days a resolved Service Incident stays on the Portal and the Public Status Page.",
        kind: SettingKind::Integer { min: 0, max: 90 },
        default: "7",
    };

    /// Every setting declared by Service Status.
    pub const ALL: &[SettingDefinition] = &[PUBLIC_PAGE, SHOW_INCIDENTS, RECENT_DAYS];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] ServiceStatusError),
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
                Self::Conflict("a row with this name already exists".into())
            }
            RepoError::Conflict(c) if c.contains("severity_order") => {
                Self::Conflict("another Impact Level has this severity order".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(ServiceStatusError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "service status storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct ServiceStatusPorts {
    /// Storage.
    pub store: Arc<dyn ServiceStatusRepository>,
    /// Publication settings.
    pub settings: Arc<dyn PublicationSettingsProvider>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A Service with its Current Status.
#[derive(Debug, Clone)]
pub struct ServiceWithStatus {
    /// The Service.
    pub service: StatusService,
    /// Its Current Status (FR-SST-006).
    pub status: ImpactLevel,
}

/// An Incident with its updates.
#[derive(Debug, Clone)]
pub struct IncidentView {
    /// The Incident.
    pub incident: StatusIncident,
    /// Its updates, oldest first.
    pub updates: Vec<StatusIncidentUpdate>,
}

/// What a status page shows (Portal and public).
#[derive(Debug, Clone)]
pub struct StatusPage {
    /// Impact Levels (to name impacts).
    pub levels: Vec<ImpactLevel>,
    /// Incident Statuses (to name them).
    pub statuses: Vec<ServiceIncidentStatus>,
    /// Names of every visible Service, active or not.
    pub service_names: BTreeMap<StatusServiceId, String>,
    /// Active Services with their Current Status, in order.
    pub services: Vec<ServiceWithStatus>,
    /// Incidents with public updates (empty unless shown), with public updates only.
    pub incidents: Vec<IncidentView>,
    /// Scheduled and started Planned Maintenance, soonest first.
    pub maintenance: Vec<PlannedMaintenance>,
}

/// A new Incident from an Analyst.
#[derive(Debug, Clone, Default)]
pub struct OpenIncident {
    /// Title.
    pub title: String,
    /// Status (default when absent).
    pub status_id: Option<Uuid>,
    /// Affected Services with Impact Levels.
    pub services: Vec<(StatusServiceId, Uuid)>,
    /// Text of the opening update.
    pub body: String,
    /// Whether the opening update is public.
    pub is_public: bool,
    /// When it began (now when absent).
    pub started_at: Option<DateTime<Utc>>,
}

/// An Incident Update from an Analyst.
#[derive(Debug, Clone, Default)]
pub struct PostUpdate {
    /// New status.
    pub status_id: Option<Uuid>,
    /// New impacts (replace all).
    pub services: Option<Vec<(StatusServiceId, Uuid)>>,
    /// Text.
    pub body: String,
    /// Public.
    pub is_public: bool,
}

/// A Planned Maintenance from an Analyst.
#[derive(Debug, Clone)]
pub struct PlanMaintenance {
    /// Title.
    pub title: String,
    /// Public description.
    pub description: String,
    /// Planned start.
    pub planned_start: DateTime<Utc>,
    /// Planned end.
    pub planned_end: DateTime<Utc>,
    /// Affected Services with Impact Levels.
    pub services: Vec<(StatusServiceId, Uuid)>,
}

/// Service Status use cases.
#[derive(Clone)]
pub struct ServiceStatusService {
    ports: ServiceStatusPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

/// Whether the caller may change records of `tenant` (none: shared with every Company,
/// which needs sight of every Company).
fn check_company(
    principal: &Principal,
    tenant: Option<TenantId>,
    field: &'static str,
) -> Result<(), AppError> {
    match (tenant, principal.scope()) {
        (None, TenantScope::All) => Ok(()),
        (None, _) => Err(ServiceStatusError::invalid(
            field,
            "shared records need access to every Company",
        )
        .into()),
        (Some(t), scope) if scope.allows(t) => Ok(()),
        (Some(_), _) => Err(AppError::NotFound("Company")),
    }
}

fn event<E: DomainEvent>(
    e: &E,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<EventEnvelope, AppError> {
    EventEnvelope::wrap(e, actor, now).map_err(|err| {
        tracing::error!(error = %err, "cannot serialize domain event");
        AppError::Internal
    })
}

fn audit(
    tenant: Option<TenantId>,
    kind: &str,
    id: Uuid,
    before: Option<Vec<(&'static str, Value)>>,
    after: Vec<(&'static str, Value)>,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = if before.is_some() {
        AuditAction::Updated
    } else {
        AuditAction::Created
    };
    let old = before.unwrap_or_default();
    after.into_iter().fold(
        AuditRecord::new(tenant, kind, id, action, actor, now),
        |r, (f, v)| {
            r.change(
                f,
                old.iter().find(|(n, _)| *n == f).map(|(_, v)| v),
                Some(&v),
            )
        },
    )
}

fn incident_audit(
    before: Option<&StatusIncident>,
    after: &StatusIncident,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    audit(
        after.tenant_id(),
        "status_incident",
        after.id().as_uuid(),
        before.map(StatusIncident::audit_fields),
        after.audit_fields(),
        actor,
        now,
    )
}

fn maintenance_audit(
    before: Option<&PlannedMaintenance>,
    after: &PlannedMaintenance,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    audit(
        after.tenant_id(),
        "planned_maintenance",
        after.id().as_uuid(),
        before.map(PlannedMaintenance::audit_fields),
        after.audit_fields(),
        actor,
        now,
    )
}

fn update_events(
    incident: &StatusIncident,
    update: &StatusIncidentUpdate,
    transition: Transition,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Vec<EventEnvelope>, AppError> {
    let mut events = vec![event(
        &StatusIncidentUpdated {
            incident_id: incident.id(),
            tenant_id: incident.tenant_id(),
            update_id: update.id,
            is_public: update.is_public,
        },
        actor,
        now,
    )?];
    if let (Transition::Resolved, Some(at)) = (transition, incident.resolved_at()) {
        events.push(event(
            &StatusIncidentResolved {
                incident_id: incident.id(),
                tenant_id: incident.tenant_id(),
                resolved_at: at,
            },
            actor,
            now,
        )?);
    }
    Ok(events)
}

fn opened_events(
    incident: &StatusIncident,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Vec<EventEnvelope>, AppError> {
    let mut events = vec![event(
        &StatusIncidentOpened {
            incident_id: incident.id(),
            tenant_id: incident.tenant_id(),
            planned_maintenance_id: incident.planned_maintenance_id(),
        },
        actor,
        now,
    )?];
    if let Some(at) = incident.resolved_at() {
        events.push(event(
            &StatusIncidentResolved {
                incident_id: incident.id(),
                tenant_id: incident.tenant_id(),
                resolved_at: at,
            },
            actor,
            now,
        )?);
    }
    Ok(events)
}

impl ServiceStatusService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: ServiceStatusPorts) -> Self {
        Self { ports }
    }

    fn now(&self) -> DateTime<Utc> {
        self.ports.clock.now()
    }

    // --- Reference data (FR-SST-002, -003) ----------------------------------------------

    async fn levels(&self) -> Result<Vec<ImpactLevel>, AppError> {
        let mut rows = self.ports.store.impact_levels().await?;
        rows.sort_by_key(|l| l.severity_order);
        Ok(rows)
    }

    async fn statuses(&self) -> Result<Vec<ServiceIncidentStatus>, AppError> {
        let mut rows = self.ports.store.incident_statuses().await?;
        rows.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
        Ok(rows)
    }

    /// Impact Levels, worst first.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn impact_levels(&self, principal: &Principal) -> Result<Vec<ImpactLevel>, AppError> {
        require(principal, capabilities::READ)?;
        self.levels().await
    }

    /// Creates an Impact Level (needs `service-status.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name or order.
    pub async fn create_impact_level(
        &self,
        principal: &Principal,
        input: ImpactLevelInput,
    ) -> Result<ImpactLevel, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.now();
        let l = ImpactLevel::create(input, now)?;
        let a = AuditRecord::new(
            None,
            "service_status_lookup",
            l.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("name", None, Some(&l.name));
        self.ports.store.save_impact_level(&l, &a).await?;
        Ok(l)
    }

    /// Changes an Impact Level (needs `service-status.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name or order.
    pub async fn update_impact_level(
        &self,
        principal: &Principal,
        id: Uuid,
        input: ImpactLevelInput,
    ) -> Result<ImpactLevel, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut l = self
            .levels()
            .await?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(AppError::NotFound("Impact Level"))?;
        let before = l.clone();
        let now = self.now();
        l.apply(input, now)?;
        let a = AuditRecord::new(
            None,
            "service_status_lookup",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("name", Some(&before.name), Some(&l.name))
        .change(
            "severity_order",
            Some(&before.severity_order),
            Some(&l.severity_order),
        )
        .change(
            "counts_as_downtime",
            Some(&before.counts_as_downtime),
            Some(&l.counts_as_downtime),
        )
        .change("is_default", Some(&before.is_default), Some(&l.is_default))
        .change("is_active", Some(&before.is_active), Some(&l.is_active));
        self.ports.store.save_impact_level(&l, &a).await?;
        Ok(l)
    }

    /// Incident Statuses in order.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn incident_statuses(
        &self,
        principal: &Principal,
    ) -> Result<Vec<ServiceIncidentStatus>, AppError> {
        require(principal, capabilities::READ)?;
        self.statuses().await
    }

    /// Creates an Incident Status (needs `service-status.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_incident_status(
        &self,
        principal: &Principal,
        input: IncidentStatusInput,
    ) -> Result<ServiceIncidentStatus, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.now();
        let s = ServiceIncidentStatus::create(input, now)?;
        let a = AuditRecord::new(
            None,
            "service_status_lookup",
            s.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("name", None, Some(&s.name));
        self.ports.store.save_incident_status(&s, &a).await?;
        Ok(s)
    }

    /// Changes an Incident Status (needs `service-status.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_incident_status(
        &self,
        principal: &Principal,
        id: Uuid,
        input: IncidentStatusInput,
    ) -> Result<ServiceIncidentStatus, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut s = self
            .statuses()
            .await?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or(AppError::NotFound("Incident Status"))?;
        let before = s.clone();
        let now = self.now();
        s.apply(input, now)?;
        let a = AuditRecord::new(
            None,
            "service_status_lookup",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("name", Some(&before.name), Some(&s.name))
        .change(
            "is_resolved",
            Some(&before.is_resolved),
            Some(&s.is_resolved),
        )
        .change("is_default", Some(&before.is_default), Some(&s.is_default))
        .change("sort_order", Some(&before.sort_order), Some(&s.sort_order))
        .change("is_active", Some(&before.is_active), Some(&s.is_active));
        self.ports.store.save_incident_status(&s, &a).await?;
        Ok(s)
    }

    // --- Services (FR-SST-001, -006) ----------------------------------------------------

    /// Services the caller can see, active or not, in order.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn services(&self, principal: &Principal) -> Result<Vec<StatusService>, AppError> {
        require(principal, capabilities::READ)?;
        let mut rows = self.ports.store.services(principal.scope()).await?;
        rows.sort_by(|a, b| {
            (a.details().sort_order, &a.details().name)
                .cmp(&(b.details().sort_order, &b.details().name))
        });
        Ok(rows)
    }

    /// One Service.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn service(
        &self,
        principal: &Principal,
        id: StatusServiceId,
    ) -> Result<StatusService, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .store
            .service(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Service"))
    }

    /// Creates a Service of a Company, or a shared one (needs `service-status.configure`;
    /// shared ones need every Company).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, invalid input or a duplicate name.
    pub async fn create_service(
        &self,
        principal: &Principal,
        tenant: Option<TenantId>,
        details: ServiceDetails,
    ) -> Result<StatusService, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        check_company(principal, tenant, "company_id")?;
        let now = self.now();
        let s = StatusService::create(tenant, details, now)?;
        let a = audit(
            s.tenant_id(),
            "status_service",
            s.id().as_uuid(),
            None,
            s.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.store.save_service(&s, &a).await?;
        Ok(s)
    }

    /// Changes a Service, or deactivates it (needs `service-status.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_service(
        &self,
        principal: &Principal,
        id: StatusServiceId,
        details: ServiceDetails,
    ) -> Result<StatusService, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut s = self.service(principal, id).await?;
        check_company(principal, s.tenant_id(), "company_id")?;
        let before = s.audit_fields();
        let now = self.now();
        s.edit(details, now)?;
        let a = audit(
            s.tenant_id(),
            "status_service",
            s.id().as_uuid(),
            Some(before),
            s.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.store.save_service(&s, &a).await?;
        Ok(s)
    }

    async fn with_status(&self, scope: &TenantScope) -> Result<Vec<ServiceWithStatus>, AppError> {
        let levels = self.levels().await?;
        let impacts = self.ports.store.unresolved_impacts(scope).await?;
        let mut services: Vec<StatusService> = self
            .ports
            .store
            .services(scope)
            .await?
            .into_iter()
            .filter(|s| s.details().is_active)
            .collect();
        services.sort_by(|a, b| {
            (a.details().sort_order, &a.details().name)
                .cmp(&(b.details().sort_order, &b.details().name))
        });
        services
            .into_iter()
            .map(|service| {
                let unresolved = impacts
                    .iter()
                    .filter(|(s, _)| *s == service.id())
                    .map(|(_, l)| *l);
                let status = current_status(&levels, unresolved)
                    .cloned()
                    .ok_or_else(|| {
                        tracing::error!("no default Impact Level");
                        AppError::Internal
                    })?;
                Ok(ServiceWithStatus { service, status })
            })
            .collect()
    }

    /// The board (FR-SST-006): active Services with their Current Status.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn board(&self, principal: &Principal) -> Result<Vec<ServiceWithStatus>, AppError> {
        require(principal, capabilities::READ)?;
        self.with_status(principal.scope()).await
    }

    /// Validates requested impacts: visible Services of one Company (or all shared),
    /// known Impact Levels, active unless already set that way.
    async fn impacts(
        &self,
        principal: &Principal,
        requested: &[(StatusServiceId, Uuid)],
        already: &Impacts,
    ) -> Result<(Option<TenantId>, Impacts), AppError> {
        let levels = self.levels().await?;
        let mut impacts = Impacts::new();
        let mut services = Vec::new();
        for (sid, lid) in requested {
            if impacts.insert(*sid, *lid).is_some() {
                return Err(
                    ServiceStatusError::invalid("services", "lists a Service twice").into(),
                );
            }
            let service = self
                .ports
                .store
                .service(principal.scope(), *sid)
                .await?
                .ok_or(AppError::NotFound("Service"))?;
            let level = levels.iter().find(|l| l.id == *lid).ok_or_else(|| {
                ServiceStatusError::invalid("impact_level_id", "is not an Impact Level")
            })?;
            if !level.is_active && already.get(sid) != Some(lid) {
                return Err(ServiceStatusError::invalid("impact_level_id", "is not active").into());
            }
            services.push(service);
        }
        let refs: Vec<&StatusService> = services.iter().collect();
        let tenant = affected_company(&refs, already)?;
        Ok((tenant, impacts))
    }

    async fn status(&self, id: Option<Uuid>) -> Result<ServiceIncidentStatus, AppError> {
        let statuses = self.statuses().await?;
        match id {
            Some(id) => statuses.into_iter().find(|s| s.id == id).ok_or_else(|| {
                ServiceStatusError::invalid("status_id", "is not an Incident Status").into()
            }),
            None => statuses.into_iter().find(|s| s.is_default).ok_or_else(|| {
                tracing::error!("no default Incident Status");
                AppError::Internal
            }),
        }
    }

    // --- Service Incidents (FR-SST-004, -005) -------------------------------------------

    /// Opens a Service Incident (needs `service-status.update`). It takes the Company of
    /// its Services.
    ///
    /// # Errors
    ///
    /// Forbidden, unknown Services, mixed Companies or invalid input.
    pub async fn open_incident(
        &self,
        principal: &Principal,
        new: OpenIncident,
    ) -> Result<IncidentView, AppError> {
        require(principal, capabilities::UPDATE)?;
        let (tenant, impacts) = self
            .impacts(principal, &new.services, &Impacts::new())
            .await?;
        check_company(principal, tenant, "services")?;
        let status = self.status(new.status_id).await?;
        let now = self.now();
        let (incident, first) = StatusIncident::open(
            NewIncident {
                tenant_id: tenant,
                title: new.title,
                status,
                impacts,
                body: new.body,
                is_public: new.is_public,
                started_at: new.started_at,
            },
            principal.actor(),
            now,
        )?;
        let events = opened_events(&incident, principal.actor(), now)?;
        let a = incident_audit(None, &incident, principal.actor(), now);
        self.ports
            .store
            .insert_incident(&incident, &first, &a, &events)
            .await?;
        Ok(IncidentView {
            incident,
            updates: vec![first],
        })
    }

    /// One Incident with every update.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn incident(
        &self,
        principal: &Principal,
        id: StatusIncidentId,
    ) -> Result<IncidentView, AppError> {
        let incident = self.get_incident(principal, id).await?;
        let updates = self
            .ports
            .store
            .updates(principal.scope(), &[id], false)
            .await?;
        Ok(IncidentView { incident, updates })
    }

    async fn get_incident(
        &self,
        principal: &Principal,
        id: StatusIncidentId,
    ) -> Result<StatusIncident, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .store
            .incident(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Service Incident"))
    }

    /// Incidents, newest first.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn incidents(
        &self,
        principal: &Principal,
        q: &IncidentQuery,
    ) -> Result<Vec<StatusIncident>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.store.incidents(principal.scope(), q).await?)
    }

    /// Posts an Incident Update (needs `service-status.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, nothing new, or invalid input.
    pub async fn post_update(
        &self,
        principal: &Principal,
        id: StatusIncidentId,
        post: PostUpdate,
    ) -> Result<(StatusIncident, StatusIncidentUpdate), AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut incident = self.get_incident(principal, id).await?;
        check_company(principal, incident.tenant_id(), "services")?;
        let status = match post.status_id {
            Some(s) => Some(self.status(Some(s)).await?),
            None => None,
        };
        let impacts = match &post.services {
            Some(requested) => {
                let (tenant, impacts) = self
                    .impacts(principal, requested, incident.impacts())
                    .await?;
                if tenant != incident.tenant_id() {
                    return Err(ServiceStatusError::invalid(
                        "services",
                        "must be of the Incident's Company",
                    )
                    .into());
                }
                Some(impacts)
            }
            None => None,
        };
        let before = incident.clone();
        let now = self.now();
        let (update, transition) = incident.post_update(
            IncidentChange {
                status,
                impacts,
                body: post.body,
                is_public: post.is_public,
            },
            principal.actor(),
            now,
            now,
        )?;
        let events = update_events(&incident, &update, transition, principal.actor(), now)?;
        let a = incident_audit(Some(&before), &incident, principal.actor(), now);
        self.ports
            .store
            .update_incident(&incident, Some(&update), &a, &events)
            .await?;
        Ok((incident, update))
    }

    /// Changes an Incident's title (needs `service-status.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or a bad title.
    pub async fn retitle(
        &self,
        principal: &Principal,
        id: StatusIncidentId,
        title: &str,
    ) -> Result<StatusIncident, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut incident = self.get_incident(principal, id).await?;
        check_company(principal, incident.tenant_id(), "services")?;
        let before = incident.clone();
        let now = self.now();
        incident.retitle(title, now)?;
        let a = incident_audit(Some(&before), &incident, principal.actor(), now);
        self.ports
            .store
            .update_incident(&incident, None, &a, &[])
            .await?;
        Ok(incident)
    }

    /// Corrects the wording and visibility of a posted update (needs
    /// `service-status.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or text too long.
    pub async fn correct_update(
        &self,
        principal: &Principal,
        id: StatusIncidentId,
        update_id: Uuid,
        body: &str,
        is_public: bool,
    ) -> Result<StatusIncidentUpdate, AppError> {
        require(principal, capabilities::UPDATE)?;
        let incident = self.get_incident(principal, id).await?;
        check_company(principal, incident.tenant_id(), "services")?;
        let mut update = self
            .ports
            .store
            .updates(principal.scope(), &[id], false)
            .await?
            .into_iter()
            .find(|u| u.id == update_id)
            .ok_or(AppError::NotFound("Incident Update"))?;
        let before = update.clone();
        let now = self.now();
        update.correct(body, is_public, now)?;
        let a = AuditRecord::new(
            incident.tenant_id(),
            "status_incident",
            id.as_uuid(),
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change(
            format!("update.{update_id}.body"),
            Some(&before.body),
            Some(&update.body),
        )
        .change(
            format!("update.{update_id}.is_public"),
            Some(&before.is_public),
            Some(&update.is_public),
        );
        self.ports.store.correct_update(&update, &a).await?;
        Ok(update)
    }

    // --- Planned Maintenance (FR-SST-007) ----------------------------------------------

    /// Schedules maintenance (needs `service-status.update`). A start that has already
    /// passed starts it at once.
    ///
    /// # Errors
    ///
    /// Forbidden, unknown Services, mixed Companies or invalid input.
    pub async fn schedule(
        &self,
        principal: &Principal,
        plan: PlanMaintenance,
    ) -> Result<PlannedMaintenance, AppError> {
        require(principal, capabilities::UPDATE)?;
        let (tenant, impacts) = self
            .impacts(principal, &plan.services, &Impacts::new())
            .await?;
        check_company(principal, tenant, "services")?;
        let now = self.now();
        let mut m = PlannedMaintenance::schedule(
            tenant,
            MaintenanceDetails {
                title: plan.title,
                description: plan.description,
                planned_start: plan.planned_start,
                planned_end: plan.planned_end,
                impacts,
            },
            principal.actor(),
            now,
        )?;
        let a = maintenance_audit(None, &m, principal.actor(), now);
        self.ports.store.insert_maintenance(&m, &a).await?;
        if m.due_to_start(now) {
            self.start_due(&mut m, principal.actor()).await?;
        }
        Ok(m)
    }

    async fn get_maintenance(
        &self,
        principal: &Principal,
        id: PlannedMaintenanceId,
    ) -> Result<PlannedMaintenance, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .store
            .maintenance(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Planned Maintenance"))
    }

    /// One Planned Maintenance.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn maintenance(
        &self,
        principal: &Principal,
        id: PlannedMaintenanceId,
    ) -> Result<PlannedMaintenance, AppError> {
        self.get_maintenance(principal, id).await
    }

    /// Planned Maintenance, soonest first.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn maintenances(
        &self,
        principal: &Principal,
        q: &MaintenanceQuery,
    ) -> Result<Vec<PlannedMaintenance>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.store.maintenances(principal.scope(), q).await?)
    }

    async fn writable(
        &self,
        principal: &Principal,
        id: PlannedMaintenanceId,
    ) -> Result<PlannedMaintenance, AppError> {
        require(principal, capabilities::UPDATE)?;
        let m = self.get_maintenance(principal, id).await?;
        check_company(principal, m.tenant_id(), "services")?;
        Ok(m)
    }

    fn moved_on() -> AppError {
        AppError::Conflict("the maintenance has moved on; reload it".into())
    }

    /// Changes scheduled maintenance (needs `service-status.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, not scheduled, or invalid input.
    pub async fn edit_maintenance(
        &self,
        principal: &Principal,
        id: PlannedMaintenanceId,
        plan: PlanMaintenance,
    ) -> Result<PlannedMaintenance, AppError> {
        let mut m = self.writable(principal, id).await?;
        let (tenant, impacts) = self
            .impacts(principal, &plan.services, &m.details().impacts)
            .await?;
        if tenant != m.tenant_id() {
            return Err(ServiceStatusError::invalid(
                "services",
                "must be of the maintenance's Company",
            )
            .into());
        }
        let before = m.clone();
        let now = self.now();
        m.edit(
            MaintenanceDetails {
                title: plan.title,
                description: plan.description,
                planned_start: plan.planned_start,
                planned_end: plan.planned_end,
                impacts,
            },
            now,
        )?;
        let a = maintenance_audit(Some(&before), &m, principal.actor(), now);
        if !self
            .ports
            .store
            .transition_maintenance(
                &m,
                MaintenanceState::Scheduled,
                IncidentWrite::None,
                &[a],
                &[],
            )
            .await?
        {
            return Err(Self::moved_on());
        }
        if m.due_to_start(now) {
            self.start_due(&mut m, principal.actor()).await?;
        }
        Ok(m)
    }

    /// Cancels scheduled maintenance (needs `service-status.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or not scheduled.
    pub async fn cancel_maintenance(
        &self,
        principal: &Principal,
        id: PlannedMaintenanceId,
    ) -> Result<PlannedMaintenance, AppError> {
        let mut m = self.writable(principal, id).await?;
        let before = m.clone();
        let now = self.now();
        m.cancel(now)?;
        let a = maintenance_audit(Some(&before), &m, principal.actor(), now);
        if !self
            .ports
            .store
            .transition_maintenance(
                &m,
                MaintenanceState::Scheduled,
                IncidentWrite::None,
                &[a],
                &[],
            )
            .await?
        {
            return Err(Self::moved_on());
        }
        Ok(m)
    }

    /// Starts scheduled maintenance now (needs `service-status.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, not scheduled, or another writer started it first.
    pub async fn start_maintenance(
        &self,
        principal: &Principal,
        id: PlannedMaintenanceId,
    ) -> Result<PlannedMaintenance, AppError> {
        let mut m = self.writable(principal, id).await?;
        if !self.start_due(&mut m, principal.actor()).await? {
            return Err(Self::moved_on());
        }
        Ok(m)
    }

    /// Finishes started maintenance now (needs `service-status.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, not started, or another writer finished it first.
    pub async fn finish_maintenance(
        &self,
        principal: &Principal,
        id: PlannedMaintenanceId,
    ) -> Result<PlannedMaintenance, AppError> {
        let mut m = self.writable(principal, id).await?;
        if !self.finish_due(&mut m, principal.actor()).await? {
            return Err(Self::moved_on());
        }
        Ok(m)
    }

    /// Starts `m`, opening its Incident. `false` when another writer moved it first.
    async fn start_due(&self, m: &mut PlannedMaintenance, actor: Actor) -> Result<bool, AppError> {
        let statuses = self.statuses().await?;
        let status = maintenance_status(&statuses).cloned().ok_or_else(|| {
            tracing::error!("no open Incident Status for maintenance");
            AppError::Internal
        })?;
        let before = m.clone();
        let now = self.now();
        let (incident, first) = m.start(&status, now)?;
        let audits = [
            maintenance_audit(Some(&before), m, actor, now),
            incident_audit(None, &incident, actor, now),
        ];
        let events = opened_events(&incident, actor, now)?;
        Ok(self
            .ports
            .store
            .transition_maintenance(
                m,
                MaintenanceState::Scheduled,
                IncidentWrite::Open(&incident, &first),
                &audits,
                &events,
            )
            .await?)
    }

    /// Finishes `m`, resolving its Incident unless someone already did. `false` when
    /// another writer moved it first.
    async fn finish_due(&self, m: &mut PlannedMaintenance, actor: Actor) -> Result<bool, AppError> {
        let Some(incident_id) = m.incident_id() else {
            return Err(ServiceStatusError::state(format!(
                "only started maintenance can be finished (this one is {})",
                m.state()
            ))
            .into());
        };
        // The maintenance was checked against the caller (or is the job's); its Incident
        // shares its Company.
        let mut incident = self
            .ports
            .store
            .incident(&TenantScope::All, incident_id)
            .await?
            .ok_or(AppError::NotFound("Service Incident"))?;
        let statuses = self.statuses().await?;
        let resolved = resolution_status(&statuses);
        if resolved.is_none() && !incident.is_resolved() {
            tracing::warn!(maintenance = %m.id(), "no resolved Incident Status: the maintenance Incident stays open");
        }
        let (before_m, before_i) = (m.clone(), incident.clone());
        let now = self.now();
        let update = m.finish(&mut incident, resolved, actor, now)?;
        let mut audits = vec![maintenance_audit(Some(&before_m), m, actor, now)];
        let mut events = Vec::new();
        if let Some(u) = &update {
            audits.push(incident_audit(Some(&before_i), &incident, actor, now));
            let transition = if incident.is_resolved() {
                Transition::Resolved
            } else {
                Transition::None
            };
            events = update_events(&incident, u, transition, actor, now)?;
        }
        Ok(self
            .ports
            .store
            .transition_maintenance(
                m,
                MaintenanceState::Started,
                IncidentWrite::Change(&incident, update.as_ref()),
                &audits,
                &events,
            )
            .await?)
    }

    /// Starts every maintenance whose start has come and finishes every one whose end
    /// has come, in every Company (the `planned-maintenance` job). Returns how many
    /// started and finished.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn run_due(&self) -> Result<(usize, usize), AppError> {
        let now = self.now();
        let (mut started, mut finished) = (0, 0);
        for mut m in self.ports.store.due_maintenance(now).await? {
            if m.due_to_start(now) && self.start_due(&mut m, Actor::Automation).await? {
                started += 1;
            }
            if m.due_to_finish(now) && self.finish_due(&mut m, Actor::Automation).await? {
                finished += 1;
            }
        }
        Ok((started, finished))
    }

    // --- Portal and public page (FR-PRT-006, FR-SST-009) --------------------------------

    /// What the Portal shows the signed-in Requester: shared Services and those of the
    /// Company the session acts for.
    ///
    /// # Errors
    ///
    /// Not a Portal session.
    pub async fn portal(&self, principal: &Principal) -> Result<StatusPage, AppError> {
        if !matches!(principal.actor(), Actor::Requester(_)) {
            return Err(Forbidden("requester".into()).into());
        }
        self.page(principal.scope(), false).await
    }

    /// The Public Status Page: shared records only, read with an empty Tenant Scope so
    /// Row-Level Security admits nothing that belongs to a Company.
    ///
    /// # Errors
    ///
    /// Not found while the page is off.
    pub async fn public_page(&self) -> Result<StatusPage, AppError> {
        if !self.ports.settings.settings().await.public_page {
            return Err(AppError::NotFound("page"));
        }
        self.page(&TenantScope::none(), true).await
    }

    async fn page(&self, scope: &TenantScope, shared_only: bool) -> Result<StatusPage, AppError> {
        let settings = self.ports.settings.settings().await;
        let keep = |t: Option<TenantId>| !shared_only || t.is_none();
        let service_names = self
            .ports
            .store
            .services(scope)
            .await?
            .into_iter()
            .filter(|s| keep(s.tenant_id()))
            .map(|s| (s.id(), s.details().name.clone()))
            .collect();
        let services = self
            .with_status(scope)
            .await?
            .into_iter()
            .filter(|s| keep(s.service.tenant_id()))
            .collect();
        let mut incidents = Vec::new();
        if settings.show_incidents {
            let since = self.now() - Duration::days(i64::from(settings.recent_days));
            let rows: Vec<StatusIncident> = self
                .ports
                .store
                .published_incidents(scope, since, Limit::DEFAULT)
                .await?
                .into_iter()
                .filter(|i| keep(i.tenant_id()))
                .collect();
            let ids: Vec<StatusIncidentId> = rows.iter().map(StatusIncident::id).collect();
            let mut updates: BTreeMap<StatusIncidentId, Vec<StatusIncidentUpdate>> =
                BTreeMap::new();
            for u in self.ports.store.updates(scope, &ids, true).await? {
                if u.is_public {
                    updates.entry(u.incident_id).or_default().push(u);
                }
            }
            incidents = rows
                .into_iter()
                .map(|incident| IncidentView {
                    updates: updates.remove(&incident.id()).unwrap_or_default(),
                    incident,
                })
                .filter(|v| !v.updates.is_empty())
                .collect();
        }
        let maintenance = self
            .ports
            .store
            .maintenances(
                scope,
                &MaintenanceQuery {
                    states: vec![MaintenanceState::Scheduled, MaintenanceState::Started],
                    limit: Limit::DEFAULT,
                    after: None,
                },
            )
            .await?
            .into_iter()
            .filter(|m| keep(m.tenant_id()))
            .collect();
        Ok(StatusPage {
            levels: self.levels().await?,
            statuses: self.statuses().await?,
            service_names,
            services,
            incidents,
            maintenance,
        })
    }
}
