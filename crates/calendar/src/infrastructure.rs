//! PostgreSQL storage of the Calendar. Calendar Events are tenant-scoped (explicit filter
//! plus Row-Level Security); Calendar Categories are instance-wide.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AuditRecord, TenantId, TenantScope};
use cooper_platform::db::Tx;
use cooper_platform::live::{self, LiveChange};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    CalendarCategory, CalendarCategoryId, CalendarEvent, CalendarEventId, CategoryRecord,
    EventDetails, EventRecord, EventSource, EventTiming, SourceKind,
};
use crate::ports::{CalendarRepository, EventQuery, GeneratedState, GeneratedWrite, RepoError};

/// Live Update topic of Calendar Events (ADR-0022).
pub const LIVE_TOPIC: &str = "calendar_event";

/// sqlx implementation of [`CalendarRepository`].
#[derive(Clone)]
pub struct PgCalendarStore {
    pool: PgPool,
}

impl PgCalendarStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
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

struct CategoryRow {
    id: Uuid,
    key: Option<String>,
    name: String,
    colour: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<CategoryRow> for CalendarCategory {
    type Error = RepoError;
    fn try_from(r: CategoryRow) -> Result<Self, RepoError> {
        Ok(Self::restore(CategoryRecord {
            id: CalendarCategoryId::from_uuid(r.id),
            key: r.key,
            name: r.name,
            colour: r.colour.parse().map_err(corrupt)?,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

struct EventRow {
    id: Uuid,
    tenant_id: Uuid,
    title: String,
    description: String,
    category_id: Uuid,
    location: Option<String>,
    all_day: bool,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
    source_kind: Option<String>,
    source_id: Option<Uuid>,
    source_date: Option<String>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<EventRow> for CalendarEvent {
    type Error = RepoError;
    fn try_from(r: EventRow) -> Result<Self, RepoError> {
        let timing = match (r.all_day, r.start_date, r.end_date, r.starts_at, r.ends_at) {
            (true, Some(start), Some(end), _, _) => EventTiming::AllDay { start, end },
            (false, _, _, Some(start), Some(end)) => EventTiming::Timed { start, end },
            _ => return Err(corrupt("calendar event timing")),
        };
        let source = match (r.source_kind, r.source_id, r.source_date) {
            (Some(kind), Some(id), Some(date)) => Some(EventSource {
                kind: kind.parse().map_err(corrupt)?,
                id,
                date: date.parse().map_err(corrupt)?,
            }),
            (None, None, None) => None,
            _ => return Err(corrupt("calendar event source")),
        };
        Ok(Self::restore(EventRecord {
            id: CalendarEventId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            details: EventDetails {
                title: r.title,
                description: r.description,
                category_id: CalendarCategoryId::from_uuid(r.category_id),
                timing,
                location: r.location,
            },
            source,
            created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id).map_err(corrupt)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

/// Storage columns of a timing: `(all_day, start_date, end_date, starts_at, ends_at)`.
type TimingColumns = (
    bool,
    Option<NaiveDate>,
    Option<NaiveDate>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

const fn timing_columns(t: EventTiming) -> TimingColumns {
    match t {
        EventTiming::AllDay { start, end } => (true, Some(start), Some(end), None, None),
        EventTiming::Timed { start, end } => (false, None, None, Some(start), Some(end)),
    }
}

async fn insert(tx: &mut Tx, e: &CalendarEvent) -> Result<(), RepoError> {
    let d = e.details();
    let (all_day, start_date, end_date, starts_at, ends_at) = timing_columns(d.timing);
    let s = e.source();
    sqlx::query!(
        "INSERT INTO calendar_events (id, tenant_id, title, description, category_id, location, all_day, start_date,
                                      end_date, starts_at, ends_at, source_kind, source_id, source_date,
                                      created_by_kind, created_by_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
        e.id().as_uuid(),
        e.tenant_id().as_uuid(),
        d.title,
        d.description,
        d.category_id.as_uuid(),
        d.location,
        all_day,
        start_date,
        end_date,
        starts_at,
        ends_at,
        s.map(|s| s.kind.as_str()),
        s.map(|s| s.id),
        s.map(|s| s.date.as_str()),
        e.created_by().kind().as_str(),
        e.created_by().id(),
        e.created_at(),
        e.updated_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn update(tx: &mut Tx, e: &CalendarEvent) -> Result<(), RepoError> {
    let d = e.details();
    let (all_day, start_date, end_date, starts_at, ends_at) = timing_columns(d.timing);
    let n = sqlx::query!(
        "UPDATE calendar_events SET title = $3, description = $4, category_id = $5, location = $6, all_day = $7,
                start_date = $8, end_date = $9, starts_at = $10, ends_at = $11, updated_at = $12
         WHERE id = $1 AND tenant_id = $2",
        e.id().as_uuid(),
        e.tenant_id().as_uuid(),
        d.title,
        d.description,
        d.category_id.as_uuid(),
        d.location,
        all_day,
        start_date,
        end_date,
        starts_at,
        ends_at,
        e.updated_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if n == 0 {
        return Err(RepoError::NotFound);
    }
    Ok(())
}

async fn delete(tx: &mut Tx, e: &CalendarEvent) -> Result<(), RepoError> {
    let n = sqlx::query!(
        "DELETE FROM calendar_events WHERE id = $1 AND tenant_id = $2",
        e.id().as_uuid(),
        e.tenant_id().as_uuid()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    if n == 0 {
        return Err(RepoError::NotFound);
    }
    Ok(())
}

async fn announce(tx: &mut Tx, e: &CalendarEvent) -> Result<(), RepoError> {
    live::publish(
        tx,
        &LiveChange::new(e.tenant_id(), LIVE_TOPIC, e.id().as_uuid()),
    )
    .await
    .map_err(db_err)
}

async fn record(tx: &mut Tx, audit: &AuditRecord) -> Result<(), RepoError> {
    if audit.is_meaningful() {
        cooper_platform::audit::record(tx, audit)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

impl PgCalendarStore {
    async fn write_one(
        &self,
        e: &CalendarEvent,
        audit: &AuditRecord,
        op: Op,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(e.tenant_id()))
            .await
            .map_err(db_err)?;
        match op {
            Op::Insert => insert(&mut tx, e).await?,
            Op::Update => update(&mut tx, e).await?,
            Op::Delete => delete(&mut tx, e).await?,
        }
        record(&mut tx, audit).await?;
        announce(&mut tx, e).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn category_by_key(&self, key: &str) -> Result<Option<CalendarCategory>, RepoError> {
        sqlx::query_as!(
            CategoryRow,
            "SELECT id, key, name, colour, is_active, created_at, updated_at FROM calendar_categories WHERE key = $1",
            key
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(TryInto::try_into)
        .transpose()
    }
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Insert,
    Update,
    Delete,
}

#[async_trait]
impl CalendarRepository for PgCalendarStore {
    async fn categories(&self) -> Result<Vec<CalendarCategory>, RepoError> {
        sqlx::query_as!(
            CategoryRow,
            "SELECT id, key, name, colour, is_active, created_at, updated_at FROM calendar_categories"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
    }

    async fn category(
        &self,
        id: CalendarCategoryId,
    ) -> Result<Option<CalendarCategory>, RepoError> {
        sqlx::query_as!(
            CategoryRow,
            "SELECT id, key, name, colour, is_active, created_at, updated_at FROM calendar_categories WHERE id = $1",
            id.as_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(TryInto::try_into)
        .transpose()
    }

    async fn save_category(
        &self,
        c: &CalendarCategory,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO calendar_categories (id, key, name, colour, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, colour = EXCLUDED.colour,
                 is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            c.id().as_uuid(),
            c.key(),
            c.name(),
            c.colour().as_str(),
            c.is_active(),
            c.created_at(),
            c.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn ensure_system_category(
        &self,
        candidate: &CalendarCategory,
        audit: &AuditRecord,
    ) -> Result<CalendarCategory, RepoError> {
        let key = candidate.key().unwrap_or_default();
        if let Some(c) = self.category_by_key(key).await? {
            return Ok(c);
        }
        // An Administrator may already use the name: fall back to a qualified one.
        let names = [
            candidate.name().to_owned(),
            format!("{} (generated)", candidate.name()),
        ];
        for name in names {
            let mut tx = self.pool.begin().await.map_err(db_err)?;
            let inserted = sqlx::query!(
                "INSERT INTO calendar_categories (id, key, name, colour, is_active, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, true, $5, $5)
                 ON CONFLICT DO NOTHING",
                candidate.id().as_uuid(),
                key,
                name,
                candidate.colour().as_str(),
                candidate.created_at()
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
            if inserted == 1 {
                record(&mut tx, audit).await?;
            }
            tx.commit().await.map_err(db_err)?;
            if let Some(c) = self.category_by_key(key).await? {
                return Ok(c);
            }
        }
        Err(RepoError::Conflict("calendar_categories_name_key".into()))
    }

    async fn events(
        &self,
        scope: &TenantScope,
        q: &EventQuery,
    ) -> Result<Vec<CalendarEvent>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            EventRow,
            r#"SELECT id, tenant_id, title, description, category_id, location, all_day, start_date, end_date,
                      starts_at, ends_at, source_kind, source_id, source_date, created_by_kind, created_by_id,
                      created_at, updated_at
               FROM calendar_events
               WHERE ($1 OR tenant_id = ANY($2))
                 AND ($3::uuid IS NULL OR tenant_id = $3)
                 AND ($4::uuid IS NULL OR category_id = $4)
                 AND CASE WHEN all_day THEN start_date <= $6 AND end_date >= $5
                          ELSE starts_at < $8 AND ends_at >= $7 END
               ORDER BY coalesce(starts_at, start_date::timestamp AT TIME ZONE 'UTC'), NOT all_day, title, id
               LIMIT $9"#,
            all,
            &tenants,
            q.tenant_id.map(|t| t.as_uuid()),
            q.category_id.map(|c| c.as_uuid()),
            q.range.from(),
            q.range.to(),
            q.range.start_instant(),
            q.range.end_instant(),
            i64::from(q.limit)
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn event(
        &self,
        scope: &TenantScope,
        id: CalendarEventId,
    ) -> Result<Option<CalendarEvent>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            EventRow,
            r#"SELECT id, tenant_id, title, description, category_id, location, all_day, start_date, end_date,
                      starts_at, ends_at, source_kind, source_id, source_date, created_by_kind, created_by_id,
                      created_at, updated_at
               FROM calendar_events WHERE id = $1 AND ($2 OR tenant_id = ANY($3))"#,
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn insert_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError> {
        self.write_one(e, audit, Op::Insert).await
    }

    async fn update_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError> {
        self.write_one(e, audit, Op::Update).await
    }

    async fn delete_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError> {
        self.write_one(e, audit, Op::Delete).await
    }

    async fn generated(
        &self,
        tenant: TenantId,
        kind: SourceKind,
        source_id: Uuid,
    ) -> Result<GeneratedState, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant))
            .await
            .map_err(db_err)?;
        let watermark = sqlx::query!(
            "SELECT synced_at, synced_event_id FROM calendar_sources
             WHERE tenant_id = $1 AND source_kind = $2 AND source_id = $3",
            tenant.as_uuid(),
            kind.as_str(),
            source_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .map(|r| (r.synced_at, r.synced_event_id));
        let rows = sqlx::query_as!(
            EventRow,
            r#"SELECT id, tenant_id, title, description, category_id, location, all_day, start_date, end_date,
                      starts_at, ends_at, source_kind, source_id, source_date, created_by_kind, created_by_id,
                      created_at, updated_at
               FROM calendar_events WHERE tenant_id = $1 AND source_kind = $2 AND source_id = $3"#,
            tenant.as_uuid(),
            kind.as_str(),
            source_id
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(GeneratedState {
            watermark,
            events: rows
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        })
    }

    async fn apply_generated(&self, w: GeneratedWrite<'_>) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(w.tenant_id))
            .await
            .map_err(db_err)?;
        let (at, event) = w.watermark;
        let moved = match w.expected {
            None => sqlx::query!(
                "INSERT INTO calendar_sources (tenant_id, source_kind, source_id, synced_event_id, synced_at)
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                w.tenant_id.as_uuid(),
                w.kind.as_str(),
                w.source_id,
                event,
                at
            )
            .execute(&mut *tx)
            .await,
            Some((old_at, old_event)) => sqlx::query!(
                "UPDATE calendar_sources SET synced_event_id = $4, synced_at = $5
                 WHERE tenant_id = $1 AND source_kind = $2 AND source_id = $3
                   AND synced_at = $6 AND synced_event_id = $7",
                w.tenant_id.as_uuid(),
                w.kind.as_str(),
                w.source_id,
                event,
                at,
                old_at,
                old_event
            )
            .execute(&mut *tx)
            .await,
        }
        .map_err(db_err)?
        .rows_affected();
        if moved != 1 {
            return Err(RepoError::Conflict(
                "the source was synced by another writer".into(),
            ));
        }
        for e in w.deletes {
            delete(&mut tx, e).await?;
            announce(&mut tx, e).await?;
        }
        for e in w.updates {
            update(&mut tx, e).await?;
            announce(&mut tx, e).await?;
        }
        for e in w.inserts {
            insert(&mut tx, e).await?;
            announce(&mut tx, e).await?;
        }
        for a in w.audits {
            record(&mut tx, a).await?;
        }
        tx.commit().await.map_err(db_err)
    }
}
