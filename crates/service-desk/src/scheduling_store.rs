//! PostgreSQL storage of scheduled work and the rota (tenant-scoped, RLS).

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{AnalystId, AuditRecord, EventEnvelope, TenantId, TenantScope};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::scheduling::{RotaEntry, RotaLocation, ScheduledWork, Shift, Span};
use crate::scheduling_service::{SchedulingRepoError, SchedulingRepository, WorkQuery};

/// sqlx implementation of [`SchedulingRepository`].
#[derive(Clone)]
pub struct PgSchedulingStore {
    pool: PgPool,
}

impl PgSchedulingStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn db_err(err: sqlx::Error) -> SchedulingRepoError {
    if let sqlx::Error::Database(db) = &err {
        let constraint = db.constraint().unwrap_or_default().to_owned();
        match db.code().as_deref() {
            Some("23505") => return SchedulingRepoError::Conflict(constraint),
            Some("23503") => return SchedulingRepoError::MissingReference(constraint),
            _ => {}
        }
    }
    SchedulingRepoError::Storage(err.to_string())
}

fn corrupt(e: impl std::fmt::Display) -> SchedulingRepoError {
    SchedulingRepoError::Storage(format!("invalid stored value: {e}"))
}

fn ids(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
    }
}

struct WorkRow {
    id: Uuid,
    tenant_id: Uuid,
    ticket_id: Uuid,
    owner_id: Uuid,
    all_day: bool,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
    notes: String,
    is_active: bool,
}

impl TryFrom<WorkRow> for ScheduledWork {
    type Error = SchedulingRepoError;

    fn try_from(r: WorkRow) -> Result<Self, SchedulingRepoError> {
        let span = if r.all_day {
            Span::new(true, r.starts_on.and_hms_opt(0, 0, 0).ok_or_else(|| corrupt("a day"))?.and_utc(), r.ends_on.and_hms_opt(0, 0, 0).ok_or_else(|| corrupt("a day"))?.and_utc())
        } else {
            let (Some(a), Some(b)) = (r.starts_at, r.ends_at) else {
                return Err(corrupt("timed work without times"));
            };
            Span::new(false, a, b)
        }
        .map_err(corrupt)?;
        Ok(Self {
            id: r.id,
            tenant_id: r.tenant_id,
            ticket_id: r.ticket_id,
            owner: AnalystId::from_uuid(r.owner_id),
            span,
            notes: r.notes,
            active: r.is_active,
        })
    }
}

fn span_columns(span: Span) -> (bool, NaiveDate, NaiveDate, Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    match span {
        Span::AllDay { starts_on, ends_on } => (true, starts_on, ends_on, None, None),
        Span::Timed { starts_at, ends_at } => (false, starts_at.date_naive(), ends_at.date_naive(), Some(starts_at), Some(ends_at)),
    }
}

#[async_trait]
impl SchedulingRepository for PgSchedulingStore {
    async fn work(&self, scope: &TenantScope, id: Uuid) -> Result<Option<ScheduledWork>, SchedulingRepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            WorkRow,
            "SELECT id, tenant_id, ticket_id, owner_id, all_day, starts_on, ends_on, starts_at, ends_at, notes, is_active
             FROM ticket_scheduled_work WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(ScheduledWork::try_from).transpose()
    }

    async fn list_work(&self, scope: &TenantScope, q: &WorkQuery) -> Result<Vec<ScheduledWork>, SchedulingRepoError> {
        let (all, tenants) = ids(scope);
        let tenant = q.tenant_id.map(|t| t.as_uuid());
        let owner = q.owner.map(|o| o.as_uuid());
        let ticket = q.ticket_id;
        let from = q.from;
        let to = q.to;
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            WorkRow,
            "SELECT id, tenant_id, ticket_id, owner_id, all_day, starts_on, ends_on, starts_at, ends_at, notes, is_active
             FROM ticket_scheduled_work
             WHERE ($1 OR tenant_id = ANY($2))
               AND ($3::uuid IS NULL OR tenant_id = $3)
               AND ($4::uuid IS NULL OR owner_id = $4)
               AND ($5::uuid IS NULL OR ticket_id = $5)
               AND ($6::date IS NULL OR ends_on >= $6)
               AND ($7::date IS NULL OR starts_on <= $7)
             ORDER BY starts_on, starts_at NULLS FIRST, id",
            all,
            &tenants,
            tenant,
            owner,
            ticket,
            from,
            to
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(ScheduledWork::try_from).collect()
    }

    async fn save_work(&self, w: &ScheduledWork, event: &EventEnvelope, audit: &AuditRecord) -> Result<(), SchedulingRepoError> {
        let tenant = TenantId::from_uuid(w.tenant_id);
        let (all_day, starts_on, ends_on, starts_at, ends_at) = span_columns(w.span);
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO ticket_scheduled_work (id, tenant_id, ticket_id, owner_id, all_day, starts_on, ends_on, starts_at, ends_at,
                                                notes, is_active, created_by_kind, created_by_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now(), now())
             ON CONFLICT (id) DO UPDATE SET owner_id = EXCLUDED.owner_id, all_day = EXCLUDED.all_day,
                 starts_on = EXCLUDED.starts_on, ends_on = EXCLUDED.ends_on, starts_at = EXCLUDED.starts_at,
                 ends_at = EXCLUDED.ends_at, notes = EXCLUDED.notes, is_active = EXCLUDED.is_active, updated_at = now()",
            w.id,
            w.tenant_id,
            w.ticket_id,
            w.owner.as_uuid(),
            all_day,
            starts_on,
            ends_on,
            starts_at,
            ends_at,
            w.notes,
            w.active,
            audit.actor.kind().as_str(),
            audit.actor.id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit).await.map_err(db_err)?;
        cooper_platform::outbox::append(&mut tx, event).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn shifts(&self, scope: &TenantScope, tenant: TenantId) -> Result<Vec<Shift>, SchedulingRepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, name, starts_at, ends_at, is_active FROM rota_shifts WHERE tenant_id = $1 ORDER BY starts_at, lower(name)",
            tenant.as_uuid()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(|r| Shift { id: r.id, name: r.name, starts_at: r.starts_at, ends_at: r.ends_at, active: r.is_active }).collect())
    }

    async fn insert_shift(&self, tenant: TenantId, s: &Shift, audit: &AuditRecord) -> Result<(), SchedulingRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO rota_shifts (id, tenant_id, name, starts_at, ends_at, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now(), now())",
            s.id,
            tenant.as_uuid(),
            s.name,
            s.starts_at,
            s.ends_at,
            s.active
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn locations(&self, scope: &TenantScope, tenant: TenantId) -> Result<Vec<RotaLocation>, SchedulingRepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!("SELECT id, name, is_active FROM rota_locations WHERE tenant_id = $1 ORDER BY lower(name)", tenant.as_uuid())
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(|r| RotaLocation { id: r.id, name: r.name, active: r.is_active }).collect())
    }

    async fn insert_location(&self, tenant: TenantId, l: &RotaLocation, audit: &AuditRecord) -> Result<(), SchedulingRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO rota_locations (id, tenant_id, name, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4, now(), now())",
            l.id,
            tenant.as_uuid(),
            l.name,
            l.active
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn entries(&self, scope: &TenantScope, tenant: TenantId, from: NaiveDate, to: NaiveDate) -> Result<Vec<RotaEntry>, SchedulingRepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, analyst_id, work_date, shift_id, location_id, on_call FROM rota_entries
             WHERE tenant_id = $1 AND work_date BETWEEN $2 AND $3
             ORDER BY work_date, shift_id, analyst_id",
            tenant.as_uuid(),
            from,
            to
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| RotaEntry {
                id: r.id,
                analyst: AnalystId::from_uuid(r.analyst_id),
                work_date: r.work_date,
                shift_id: r.shift_id,
                location_id: r.location_id,
                on_call: r.on_call,
            })
            .collect())
    }

    async fn insert_entry(&self, tenant: TenantId, e: &RotaEntry, audit: &AuditRecord) -> Result<(), SchedulingRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO rota_entries (id, tenant_id, analyst_id, work_date, shift_id, location_id, on_call)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            e.id,
            tenant.as_uuid(),
            e.analyst.as_uuid(),
            e.work_date,
            e.shift_id,
            e.location_id,
            e.on_call
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }
}

