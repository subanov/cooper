//! PostgreSQL storage of Service Status. Company records are tenant-scoped (explicit
//! filter plus RLS); shared records (no Company) are visible in every scope.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditRecord, EventEnvelope, Limit, TenantId, TenantScope};
use cooper_platform::db::Tx;
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    ImpactLevel, Impacts, MaintenanceDetails, MaintenanceState, PlannedMaintenance,
    PlannedMaintenanceId, PlannedMaintenanceRecord, ServiceDetails, ServiceIncidentStatus,
    StatusIncident, StatusIncidentId, StatusIncidentRecord, StatusIncidentUpdate, StatusService,
    StatusServiceId,
};
use crate::ports::{
    IncidentQuery, IncidentWrite, MaintenanceQuery, RepoError, ServiceStatusRepository,
};

/// sqlx implementation of [`ServiceStatusRepository`].
#[derive(Clone)]
pub struct PgServiceStatusStore {
    pool: PgPool,
}

impl PgServiceStatusStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn begin(&self, scope: &TenantScope) -> Result<Tx, RepoError> {
        tenancy::begin(&self.pool, scope).await.map_err(db_err)
    }
}

fn db_err(err: sqlx::Error) -> RepoError {
    if let sqlx::Error::Database(db) = &err {
        let constraint = db.constraint().unwrap_or_default().to_owned();
        match db.code().as_deref() {
            Some("23505") => return RepoError::Conflict(constraint),
            Some("23503") => return RepoError::MissingReference(constraint),
            _ => {}
        }
    }
    RepoError::Storage(err.to_string())
}

fn corrupt(e: impl std::fmt::Display) -> RepoError {
    RepoError::Storage(format!("invalid stored value: {e}"))
}

fn ids(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
    }
}

/// The narrowest scope that can write a record of `tenant`: its Company, or none at all
/// for shared records.
fn scope_for(tenant: Option<TenantId>) -> TenantScope {
    tenant.map_or_else(TenantScope::none, TenantScope::single)
}

fn tenant(t: Option<Uuid>) -> Option<TenantId> {
    t.map(TenantId::from_uuid)
}

async fn record(
    tx: &mut Tx,
    audits: &[AuditRecord],
    events: &[EventEnvelope],
) -> Result<(), RepoError> {
    for a in audits.iter().filter(|a| a.is_meaningful()) {
        cooper_platform::audit::record(tx, a)
            .await
            .map_err(db_err)?;
    }
    for e in events {
        cooper_platform::outbox::append(tx, e)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

// ----- Rows -------------------------------------------------------------------------------

struct ServiceRow {
    id: Uuid,
    tenant_id: Option<Uuid>,
    name: String,
    description: String,
    sort_order: i32,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ServiceRow> for StatusService {
    fn from(r: ServiceRow) -> Self {
        Self::restore(
            StatusServiceId::from_uuid(r.id),
            tenant(r.tenant_id),
            ServiceDetails {
                name: r.name,
                description: r.description,
                sort_order: r.sort_order,
                is_active: r.is_active,
            },
            r.created_at,
            r.updated_at,
        )
    }
}

struct IncidentRow {
    id: Uuid,
    tenant_id: Option<Uuid>,
    title: String,
    status_id: Uuid,
    is_resolved: bool,
    started_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
    planned_maintenance_id: Option<Uuid>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct UpdateRow {
    id: Uuid,
    tenant_id: Option<Uuid>,
    incident_id: Uuid,
    status_id: Uuid,
    body: String,
    is_public: bool,
    author_kind: String,
    author_id: Option<Uuid>,
    posted_at: DateTime<Utc>,
    edited_at: Option<DateTime<Utc>>,
}

struct MaintenanceRow {
    id: Uuid,
    tenant_id: Option<Uuid>,
    title: String,
    description: String,
    planned_start: DateTime<Utc>,
    planned_end: DateTime<Utc>,
    state: String,
    incident_id: Option<Uuid>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct ImpactRow {
    owner: Uuid,
    service_id: Uuid,
    impact_level_id: Uuid,
}

fn group(rows: Vec<ImpactRow>) -> BTreeMap<Uuid, Impacts> {
    let mut out: BTreeMap<Uuid, Impacts> = BTreeMap::new();
    for r in rows {
        out.entry(r.owner)
            .or_default()
            .insert(StatusServiceId::from_uuid(r.service_id), r.impact_level_id);
    }
    out
}

async fn incident_impacts(tx: &mut Tx, ids: &[Uuid]) -> Result<BTreeMap<Uuid, Impacts>, RepoError> {
    let rows = sqlx::query_as!(
        ImpactRow,
        r#"SELECT incident_id AS "owner!", service_id, impact_level_id FROM status_incident_services WHERE incident_id = ANY($1)"#,
        ids
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(group(rows))
}

async fn incidents_from(
    tx: &mut Tx,
    rows: Vec<IncidentRow>,
) -> Result<Vec<StatusIncident>, RepoError> {
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut impacts = incident_impacts(tx, &ids).await?;
    rows.into_iter()
        .map(|r| {
            Ok(StatusIncident::restore(StatusIncidentRecord {
                id: StatusIncidentId::from_uuid(r.id),
                tenant_id: tenant(r.tenant_id),
                title: r.title,
                status_id: r.status_id,
                is_resolved: r.is_resolved,
                impacts: impacts.remove(&r.id).unwrap_or_default(),
                started_at: r.started_at,
                resolved_at: r.resolved_at,
                planned_maintenance_id: r
                    .planned_maintenance_id
                    .map(PlannedMaintenanceId::from_uuid),
                created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id)
                    .map_err(corrupt)?,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }))
        })
        .collect()
}

async fn maintenances_from(
    tx: &mut Tx,
    rows: Vec<MaintenanceRow>,
) -> Result<Vec<PlannedMaintenance>, RepoError> {
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let impact_rows = sqlx::query_as!(
        ImpactRow,
        r#"SELECT planned_maintenance_id AS "owner!", service_id, impact_level_id FROM planned_maintenance_services
           WHERE planned_maintenance_id = ANY($1)"#,
        &ids
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let mut impacts = group(impact_rows);
    rows.into_iter()
        .map(|r| {
            Ok(PlannedMaintenance::restore(PlannedMaintenanceRecord {
                id: PlannedMaintenanceId::from_uuid(r.id),
                tenant_id: tenant(r.tenant_id),
                details: MaintenanceDetails {
                    title: r.title,
                    description: r.description,
                    planned_start: r.planned_start,
                    planned_end: r.planned_end,
                    impacts: impacts.remove(&r.id).unwrap_or_default(),
                },
                state: r.state.parse().map_err(corrupt)?,
                incident_id: r.incident_id.map(StatusIncidentId::from_uuid),
                created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id)
                    .map_err(corrupt)?,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }))
        })
        .collect()
}

// ----- Writes -----------------------------------------------------------------------------

async fn write_incident_impacts(tx: &mut Tx, i: &StatusIncident) -> Result<(), RepoError> {
    sqlx::query!(
        "DELETE FROM status_incident_services WHERE incident_id = $1",
        i.id().as_uuid()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    let (services, levels): (Vec<Uuid>, Vec<Uuid>) =
        i.impacts().iter().map(|(s, l)| (s.as_uuid(), *l)).unzip();
    sqlx::query!(
        "INSERT INTO status_incident_services (tenant_id, incident_id, service_id, impact_level_id)
         SELECT $1, $2, s, l FROM UNNEST($3::uuid[], $4::uuid[]) AS x(s, l)",
        i.tenant_id().map(|t| t.as_uuid()),
        i.id().as_uuid(),
        &services,
        &levels
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn insert_update(tx: &mut Tx, u: &StatusIncidentUpdate) -> Result<(), RepoError> {
    sqlx::query!(
        "INSERT INTO status_incident_updates (id, tenant_id, incident_id, status_id, body, is_public, author_kind, author_id,
                                              posted_at, edited_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        u.id,
        u.tenant_id.map(|t| t.as_uuid()),
        u.incident_id.as_uuid(),
        u.status_id,
        u.body,
        u.is_public,
        u.author.kind().as_str(),
        u.author.id(),
        u.posted_at,
        u.edited_at
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    let (services, levels): (Vec<Uuid>, Vec<Uuid>) =
        u.impacts.iter().map(|(s, l)| (s.as_uuid(), *l)).unzip();
    sqlx::query!(
        "INSERT INTO status_incident_update_services (tenant_id, update_id, service_id, impact_level_id)
         SELECT $1, $2, s, l FROM UNNEST($3::uuid[], $4::uuid[]) AS x(s, l)",
        u.tenant_id.map(|t| t.as_uuid()),
        u.id,
        &services,
        &levels
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn insert_incident(
    tx: &mut Tx,
    i: &StatusIncident,
    first: &StatusIncidentUpdate,
) -> Result<(), RepoError> {
    sqlx::query!(
        "INSERT INTO status_incidents (id, tenant_id, title, status_id, started_at, resolved_at, planned_maintenance_id,
                                       created_by_kind, created_by_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        i.id().as_uuid(),
        i.tenant_id().map(|t| t.as_uuid()),
        i.title(),
        i.status_id(),
        i.started_at(),
        i.resolved_at(),
        i.planned_maintenance_id().map(|m| m.as_uuid()),
        i.created_by().kind().as_str(),
        i.created_by().id(),
        i.created_at(),
        i.updated_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    write_incident_impacts(tx, i).await?;
    insert_update(tx, first).await
}

async fn update_incident(
    tx: &mut Tx,
    i: &StatusIncident,
    update: Option<&StatusIncidentUpdate>,
) -> Result<(), RepoError> {
    let n = sqlx::query!(
        "UPDATE status_incidents SET title = $2, status_id = $3, resolved_at = $4, updated_at = $5 WHERE id = $1",
        i.id().as_uuid(),
        i.title(),
        i.status_id(),
        i.resolved_at(),
        i.updated_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if n == 0 {
        return Err(RepoError::NotFound);
    }
    write_incident_impacts(tx, i).await?;
    if let Some(u) = update {
        insert_update(tx, u).await?;
    }
    Ok(())
}

async fn write_maintenance(
    tx: &mut Tx,
    m: &PlannedMaintenance,
    insert: bool,
) -> Result<(), RepoError> {
    let d = m.details();
    if insert {
        sqlx::query!(
            "INSERT INTO planned_maintenances (id, tenant_id, title, description, planned_start, planned_end, state, incident_id,
                                               created_by_kind, created_by_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            m.id().as_uuid(),
            m.tenant_id().map(|t| t.as_uuid()),
            d.title,
            d.description,
            d.planned_start,
            d.planned_end,
            m.state().as_str(),
            m.incident_id().map(|i| i.as_uuid()),
            m.created_by().kind().as_str(),
            m.created_by().id(),
            m.created_at(),
            m.updated_at()
        )
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    } else {
        sqlx::query!(
            "UPDATE planned_maintenances SET title = $2, description = $3, planned_start = $4, planned_end = $5, state = $6,
                    incident_id = $7, updated_at = $8
             WHERE id = $1",
            m.id().as_uuid(),
            d.title,
            d.description,
            d.planned_start,
            d.planned_end,
            m.state().as_str(),
            m.incident_id().map(|i| i.as_uuid()),
            m.updated_at()
        )
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    sqlx::query!(
        "DELETE FROM planned_maintenance_services WHERE planned_maintenance_id = $1",
        m.id().as_uuid()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    let (services, levels): (Vec<Uuid>, Vec<Uuid>) =
        d.impacts.iter().map(|(s, l)| (s.as_uuid(), *l)).unzip();
    sqlx::query!(
        "INSERT INTO planned_maintenance_services (tenant_id, planned_maintenance_id, service_id, impact_level_id)
         SELECT $1, $2, s, l FROM UNNEST($3::uuid[], $4::uuid[]) AS x(s, l)",
        m.tenant_id().map(|t| t.as_uuid()),
        m.id().as_uuid(),
        &services,
        &levels
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[async_trait]
impl ServiceStatusRepository for PgServiceStatusStore {
    async fn impact_levels(&self) -> Result<Vec<ImpactLevel>, RepoError> {
        Ok(sqlx::query!(
            "SELECT id, key, name, colour, severity_order, counts_as_downtime, is_default, is_active, created_at, updated_at
             FROM impact_levels"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|r| ImpactLevel {
            id: r.id,
            key: r.key,
            name: r.name,
            colour: r.colour,
            severity_order: r.severity_order,
            counts_as_downtime: r.counts_as_downtime,
            is_default: r.is_default,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
    }

    async fn save_impact_level(
        &self,
        l: &ImpactLevel,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        if l.is_default {
            sqlx::query!(
                "UPDATE impact_levels SET is_default = false WHERE is_default AND id <> $1",
                l.id
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        sqlx::query!(
            "INSERT INTO impact_levels (id, key, name, colour, severity_order, counts_as_downtime, is_default, is_active,
                                        created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, colour = EXCLUDED.colour,
                 severity_order = EXCLUDED.severity_order, counts_as_downtime = EXCLUDED.counts_as_downtime,
                 is_default = EXCLUDED.is_default, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            l.id,
            l.key,
            l.name,
            l.colour,
            l.severity_order,
            l.counts_as_downtime,
            l.is_default,
            l.is_active,
            l.created_at,
            l.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn incident_statuses(&self) -> Result<Vec<ServiceIncidentStatus>, RepoError> {
        Ok(sqlx::query!(
            "SELECT id, key, name, colour, is_resolved, is_default, sort_order, is_active, created_at, updated_at
             FROM service_incident_statuses"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|r| ServiceIncidentStatus {
            id: r.id,
            key: r.key,
            name: r.name,
            colour: r.colour,
            is_resolved: r.is_resolved,
            is_default: r.is_default,
            sort_order: r.sort_order,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
    }

    async fn save_incident_status(
        &self,
        s: &ServiceIncidentStatus,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        if s.is_default {
            sqlx::query!(
                "UPDATE service_incident_statuses SET is_default = false WHERE is_default AND id <> $1",
                s.id
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        sqlx::query!(
            "INSERT INTO service_incident_statuses (id, key, name, colour, is_resolved, is_default, sort_order, is_active,
                                                    created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, colour = EXCLUDED.colour, is_resolved = EXCLUDED.is_resolved,
                 is_default = EXCLUDED.is_default, sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active,
                 updated_at = EXCLUDED.updated_at",
            s.id,
            s.key,
            s.name,
            s.colour,
            s.is_resolved,
            s.is_default,
            s.sort_order,
            s.is_active,
            s.created_at,
            s.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn services(&self, scope: &TenantScope) -> Result<Vec<StatusService>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = self.begin(scope).await?;
        let rows = sqlx::query_as!(
            ServiceRow,
            "SELECT id, tenant_id, name, description, sort_order, is_active, created_at, updated_at FROM status_services
             WHERE ($1 OR tenant_id IS NULL OR tenant_id = ANY($2))",
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn service(
        &self,
        scope: &TenantScope,
        id: StatusServiceId,
    ) -> Result<Option<StatusService>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = self.begin(scope).await?;
        let row = sqlx::query_as!(
            ServiceRow,
            "SELECT id, tenant_id, name, description, sort_order, is_active, created_at, updated_at FROM status_services
             WHERE id = $1 AND ($2 OR tenant_id IS NULL OR tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(row.map(Into::into))
    }

    async fn save_service(&self, s: &StatusService, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.begin(&scope_for(s.tenant_id())).await?;
        let d = s.details();
        sqlx::query!(
            "INSERT INTO status_services (id, tenant_id, name, description, sort_order, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description,
                 sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            s.id().as_uuid(),
            s.tenant_id().map(|t| t.as_uuid()),
            d.name,
            d.description,
            d.sort_order,
            d.is_active,
            s.created_at(),
            s.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, std::slice::from_ref(audit), &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn unresolved_impacts(
        &self,
        scope: &TenantScope,
    ) -> Result<Vec<(StatusServiceId, Uuid)>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = self.begin(scope).await?;
        let rows = sqlx::query!(
            "SELECT x.service_id, x.impact_level_id
             FROM status_incident_services x
             JOIN status_incidents i ON i.id = x.incident_id
             JOIN service_incident_statuses s ON s.id = i.status_id
             WHERE NOT s.is_resolved AND ($1 OR i.tenant_id IS NULL OR i.tenant_id = ANY($2))",
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| (StatusServiceId::from_uuid(r.service_id), r.impact_level_id))
            .collect())
    }

    async fn insert_incident(
        &self,
        i: &StatusIncident,
        first: &StatusIncidentUpdate,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = self.begin(&scope_for(i.tenant_id())).await?;
        insert_incident(&mut tx, i, first).await?;
        record(&mut tx, std::slice::from_ref(audit), events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update_incident(
        &self,
        i: &StatusIncident,
        update: Option<&StatusIncidentUpdate>,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = self.begin(&scope_for(i.tenant_id())).await?;
        update_incident(&mut tx, i, update).await?;
        record(&mut tx, std::slice::from_ref(audit), events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn incident(
        &self,
        scope: &TenantScope,
        id: StatusIncidentId,
    ) -> Result<Option<StatusIncident>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = self.begin(scope).await?;
        let rows = sqlx::query_as!(
            IncidentRow,
            "SELECT i.id, i.tenant_id, i.title, i.status_id, s.is_resolved, i.started_at, i.resolved_at, i.planned_maintenance_id,
                    i.created_by_kind, i.created_by_id, i.created_at, i.updated_at
             FROM status_incidents i JOIN service_incident_statuses s ON s.id = i.status_id
             WHERE i.id = $1 AND ($2 OR i.tenant_id IS NULL OR i.tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = incidents_from(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out.into_iter().next())
    }

    async fn incidents(
        &self,
        scope: &TenantScope,
        q: &IncidentQuery,
    ) -> Result<Vec<StatusIncident>, RepoError> {
        let (all, tenants) = ids(scope);
        let (after_at, after_id) = q.after.unzip();
        let mut tx = self.begin(scope).await?;
        let rows = sqlx::query_as!(
            IncidentRow,
            "SELECT i.id, i.tenant_id, i.title, i.status_id, s.is_resolved, i.started_at, i.resolved_at, i.planned_maintenance_id,
                    i.created_by_kind, i.created_by_id, i.created_at, i.updated_at
             FROM status_incidents i JOIN service_incident_statuses s ON s.id = i.status_id
             WHERE ($1 OR i.tenant_id IS NULL OR i.tenant_id = ANY($2))
               AND ($3::boolean IS NULL OR s.is_resolved = NOT $3)
               AND ($4::uuid IS NULL OR EXISTS (SELECT 1 FROM status_incident_services x WHERE x.incident_id = i.id AND x.service_id = $4))
               AND ($5::timestamptz IS NULL OR (i.started_at, i.id) < ($5, $6::uuid))
             ORDER BY i.started_at DESC, i.id DESC
             LIMIT $7",
            all,
            &tenants,
            q.open,
            q.service_id.map(|s| s.as_uuid()),
            after_at,
            after_id,
            q.limit.fetch_size()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = incidents_from(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }

    async fn published_incidents(
        &self,
        scope: &TenantScope,
        since: DateTime<Utc>,
        limit: Limit,
    ) -> Result<Vec<StatusIncident>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = self.begin(scope).await?;
        let rows = sqlx::query_as!(
            IncidentRow,
            "SELECT i.id, i.tenant_id, i.title, i.status_id, s.is_resolved, i.started_at, i.resolved_at, i.planned_maintenance_id,
                    i.created_by_kind, i.created_by_id, i.created_at, i.updated_at
             FROM status_incidents i JOIN service_incident_statuses s ON s.id = i.status_id
             WHERE ($1 OR i.tenant_id IS NULL OR i.tenant_id = ANY($2))
               AND (NOT s.is_resolved OR i.resolved_at >= $3)
               AND EXISTS (SELECT 1 FROM status_incident_updates u WHERE u.incident_id = i.id AND u.is_public)
             ORDER BY i.started_at DESC, i.id DESC
             LIMIT $4",
            all,
            &tenants,
            since,
            i64::from(limit.get())
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = incidents_from(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }

    async fn updates(
        &self,
        scope: &TenantScope,
        incidents: &[StatusIncidentId],
        public_only: bool,
    ) -> Result<Vec<StatusIncidentUpdate>, RepoError> {
        let (all, tenants) = ids(scope);
        let incident_ids: Vec<Uuid> = incidents.iter().map(|i| i.as_uuid()).collect();
        let mut tx = self.begin(scope).await?;
        let rows = sqlx::query_as!(
            UpdateRow,
            "SELECT id, tenant_id, incident_id, status_id, body, is_public, author_kind, author_id, posted_at, edited_at
             FROM status_incident_updates
             WHERE incident_id = ANY($1) AND (NOT $2 OR is_public) AND ($3 OR tenant_id IS NULL OR tenant_id = ANY($4))
             ORDER BY posted_at, id",
            &incident_ids,
            public_only,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let update_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
        let snapshot = sqlx::query_as!(
            ImpactRow,
            r#"SELECT update_id AS "owner!", service_id, impact_level_id FROM status_incident_update_services
               WHERE update_id = ANY($1)"#,
            &update_ids
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        let mut snapshot = group(snapshot);
        rows.into_iter()
            .map(|r| {
                Ok(StatusIncidentUpdate {
                    id: r.id,
                    incident_id: StatusIncidentId::from_uuid(r.incident_id),
                    tenant_id: tenant(r.tenant_id),
                    status_id: r.status_id,
                    impacts: snapshot.remove(&r.id).unwrap_or_default(),
                    body: r.body,
                    is_public: r.is_public,
                    author: Actor::from_parts(&r.author_kind, r.author_id).map_err(corrupt)?,
                    posted_at: r.posted_at,
                    edited_at: r.edited_at,
                })
            })
            .collect()
    }

    async fn correct_update(
        &self,
        u: &StatusIncidentUpdate,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = self.begin(&scope_for(u.tenant_id)).await?;
        let n = sqlx::query!(
            "UPDATE status_incident_updates SET body = $2, is_public = $3, edited_at = $4 WHERE id = $1",
            u.id,
            u.body,
            u.is_public,
            u.edited_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        record(&mut tx, std::slice::from_ref(audit), &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn insert_maintenance(
        &self,
        m: &PlannedMaintenance,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = self.begin(&scope_for(m.tenant_id())).await?;
        write_maintenance(&mut tx, m, true).await?;
        record(&mut tx, std::slice::from_ref(audit), &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn transition_maintenance(
        &self,
        m: &PlannedMaintenance,
        from: MaintenanceState,
        change: IncidentWrite<'_>,
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<bool, RepoError> {
        let mut tx = self.begin(&scope_for(m.tenant_id())).await?;
        // Claim it: concurrent writers queue here, and only the first sees `from`.
        let state = sqlx::query_scalar!(
            "SELECT state FROM planned_maintenances WHERE id = $1 FOR UPDATE",
            m.id().as_uuid()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        if state.as_deref() != Some(from.as_str()) {
            return Ok(false);
        }
        match change {
            IncidentWrite::None => {}
            IncidentWrite::Open(i, first) => insert_incident(&mut tx, i, first).await?,
            IncidentWrite::Change(i, update) => update_incident(&mut tx, i, update).await?,
        }
        write_maintenance(&mut tx, m, false).await?;
        record(&mut tx, audits, events).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(true)
    }

    async fn maintenance(
        &self,
        scope: &TenantScope,
        id: PlannedMaintenanceId,
    ) -> Result<Option<PlannedMaintenance>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = self.begin(scope).await?;
        let rows = sqlx::query_as!(
            MaintenanceRow,
            "SELECT id, tenant_id, title, description, planned_start, planned_end, state, incident_id, created_by_kind,
                    created_by_id, created_at, updated_at
             FROM planned_maintenances WHERE id = $1 AND ($2 OR tenant_id IS NULL OR tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = maintenances_from(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out.into_iter().next())
    }

    async fn maintenances(
        &self,
        scope: &TenantScope,
        q: &MaintenanceQuery,
    ) -> Result<Vec<PlannedMaintenance>, RepoError> {
        let (all, tenants) = ids(scope);
        let states: Vec<String> = q.states.iter().map(|s| s.as_str().to_owned()).collect();
        let (after_at, after_id) = q.after.unzip();
        let mut tx = self.begin(scope).await?;
        let rows = sqlx::query_as!(
            MaintenanceRow,
            "SELECT id, tenant_id, title, description, planned_start, planned_end, state, incident_id, created_by_kind,
                    created_by_id, created_at, updated_at
             FROM planned_maintenances
             WHERE ($1 OR tenant_id IS NULL OR tenant_id = ANY($2))
               AND (cardinality($3::text[]) = 0 OR state = ANY($3))
               AND ($4::timestamptz IS NULL OR (planned_start, id) > ($4, $5::uuid))
             ORDER BY planned_start, id
             LIMIT $6",
            all,
            &tenants,
            &states,
            after_at,
            after_id,
            q.limit.fetch_size()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = maintenances_from(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }

    async fn due_maintenance(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<PlannedMaintenance>, RepoError> {
        let mut tx = self.begin(&TenantScope::All).await?;
        let rows = sqlx::query_as!(
            MaintenanceRow,
            "SELECT id, tenant_id, title, description, planned_start, planned_end, state, incident_id, created_by_kind,
                    created_by_id, created_at, updated_at
             FROM planned_maintenances
             WHERE (state = 'scheduled' AND planned_start <= $1) OR (state = 'started' AND planned_end <= $1)
             ORDER BY planned_start, id
             LIMIT 200",
            now
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let out = maintenances_from(&mut tx, rows).await?;
        tx.commit().await.map_err(db_err)?;
        Ok(out)
    }
}

/// The `planned-maintenance` Background Job: starts Planned Maintenance whose start has
/// come and finishes what has ended (FR-SST-007).
pub struct PlannedMaintenanceJob {
    /// The service.
    pub service: crate::application::ServiceStatusService,
}

#[async_trait]
impl Job for PlannedMaintenanceJob {
    fn name(&self) -> &'static str {
        "planned-maintenance"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(60)
    }

    async fn run(&self, _ctx: &JobContext) -> Result<Option<String>, JobError> {
        let (started, finished) = self.service.run_due().await.map_err(|e| e.to_string())?;
        Ok(Some(format!("started {started}, finished {finished}")))
    }
}
