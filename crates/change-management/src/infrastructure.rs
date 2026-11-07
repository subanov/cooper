//! PostgreSQL storage of Change Management (tenant-scoped: explicit filter plus RLS).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AnalystId, AuditRecord, EventEnvelope, TeamId, TenantId, TenantScope};
use cooper_platform::db::Tx;
use cooper_platform::live::{self, LiveChange};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    ApprovalRoute, CabApprovalType, CabMember, Change, ChangeDetails, ChangeStage, ChangeTemplate,
    Decision, Lookup, LookupKind, Pir, RiskLevel, TemplateContent, Vote,
};
use crate::ports::{
    ChangeAttachment, ChangeLink, ChangeQuery, ChangeRepository, ChecklistItem, Comment, LinkKind,
    RepoError, TextRevision,
};

/// sqlx implementation of [`ChangeRepository`].
#[derive(Clone)]
pub struct PgChangeStore {
    pool: PgPool,
}

impl PgChangeStore {
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

async fn finish(
    tx: &mut Tx,
    c: &Change,
    audit: &AuditRecord,
    events: &[EventEnvelope],
) -> Result<(), RepoError> {
    if audit.is_meaningful() {
        cooper_platform::audit::record(tx, audit)
            .await
            .map_err(db_err)?;
    }
    for e in events {
        cooper_platform::outbox::append(tx, e)
            .await
            .map_err(db_err)?;
    }
    live::publish(tx, &LiveChange::new(c.tenant_id(), "change", c.id()))
        .await
        .map_err(db_err)?;
    Ok(())
}

async fn touch(tx: &mut Tx, c: &Change) -> Result<(), RepoError> {
    sqlx::query!(
        "UPDATE changes SET updated_at = $2 WHERE id = $1",
        c.id(),
        c.updated_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn insert_revisions(tx: &mut Tx, c: &Change, revs: &[TextRevision]) -> Result<(), RepoError> {
    for r in revs {
        sqlx::query!(
            "INSERT INTO change_text_revisions (id, tenant_id, change_id, field, body, author_kind, author_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            r.id, c.tenant_id().as_uuid(), c.id(), r.field, r.body, r.author.kind().as_str(), r.author.id(), r.created_at
        )
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

struct ChangeRow {
    id: Uuid,
    tenant_id: Uuid,
    change_number: String,
    title: String,
    type_id: Uuid,
    status_id: Uuid,
    stage: String,
    priority_id: Uuid,
    impact_id: Uuid,
    category_id: Option<Uuid>,
    requested_by_id: Option<Uuid>,
    assigned_to_id: Option<Uuid>,
    approver_id: Option<Uuid>,
    work_start: Option<DateTime<Utc>>,
    work_end: Option<DateTime<Utc>>,
    outage_start: Option<DateTime<Utc>>,
    outage_end: Option<DateTime<Utc>>,
    description: String,
    reason_for_change: String,
    risk_evaluation: String,
    risk_likelihood: Option<i16>,
    risk_impact_score: Option<i16>,
    test_plan: String,
    rollback_plan: String,
    cab_required: bool,
    cab_approval_type: String,
    approval_round: i32,
    approved_at: Option<DateTime<Utc>>,
    template_id: Option<Uuid>,
    template_version: Option<i32>,
    pir_successful: Option<bool>,
    pir_actual_start: Option<DateTime<Utc>>,
    pir_actual_end: Option<DateTime<Utc>>,
    pir_lessons_learned: Option<String>,
    pir_follow_up: Option<String>,
    pir_recorded_at: Option<DateTime<Utc>>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

fn small(v: Option<i16>) -> Option<u8> {
    v.and_then(|x| u8::try_from(x).ok())
}

impl TryFrom<ChangeRow> for Change {
    type Error = RepoError;
    fn try_from(r: ChangeRow) -> Result<Self, RepoError> {
        let pir = match (r.pir_successful, r.pir_recorded_at) {
            (Some(successful), Some(recorded_at)) => Some(Pir {
                successful,
                actual_start: r.pir_actual_start,
                actual_end: r.pir_actual_end,
                lessons_learned: r.pir_lessons_learned.unwrap_or_default(),
                follow_up: r.pir_follow_up.unwrap_or_default(),
                recorded_at,
            }),
            _ => None,
        };
        Ok(Self::restore(
            r.id,
            TenantId::from_uuid(r.tenant_id),
            r.change_number,
            ChangeDetails {
                title: r.title,
                type_id: r.type_id,
                priority_id: r.priority_id,
                impact_id: r.impact_id,
                category_id: r.category_id,
                requested_by: r.requested_by_id.map(AnalystId::from_uuid),
                assigned_to: r.assigned_to_id.map(AnalystId::from_uuid),
                approver: r.approver_id.map(AnalystId::from_uuid),
                work_start: r.work_start,
                work_end: r.work_end,
                outage_start: r.outage_start,
                outage_end: r.outage_end,
                description: r.description,
                reason_for_change: r.reason_for_change,
                risk_evaluation: r.risk_evaluation,
                risk_likelihood: small(r.risk_likelihood),
                risk_impact_score: small(r.risk_impact_score),
                test_plan: r.test_plan,
                rollback_plan: r.rollback_plan,
                cab_required: r.cab_required,
                cab_approval_type: Some(
                    r.cab_approval_type
                        .parse::<CabApprovalType>()
                        .map_err(corrupt)?,
                ),
            },
            r.status_id,
            r.stage.parse::<ChangeStage>().map_err(corrupt)?,
            r.approval_round,
            r.approved_at,
            r.template_id.zip(r.template_version),
            pir,
            Actor::from_parts(&r.created_by_kind, r.created_by_id).map_err(corrupt)?,
            r.created_at,
            r.updated_at,
            r.closed_at,
        ))
    }
}

type SimpleRow = (
    Uuid,
    Option<String>,
    String,
    bool,
    i32,
    bool,
    DateTime<Utc>,
    DateTime<Utc>,
);

/// Priorities, Impacts and Categories share one shape; the table name comes from the
/// closed [`LookupKind`] set, never from input (hence `AssertSqlSafe`).
async fn simple_lookups(pool: &PgPool, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
    let sql = format!(
        "SELECT id, key, name, is_default, sort_order, is_active, created_at, updated_at FROM {}",
        kind.table()
    );
    let rows: Vec<SimpleRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(
            |(id, key, name, is_default, sort_order, is_active, created_at, updated_at)| Lookup {
                id,
                kind,
                key,
                name,
                stage: ChangeStage::Draft,
                is_closed: false,
                route: ApprovalRoute::Review,
                is_standard: false,
                is_default,
                sort_order,
                is_active,
                created_at,
                updated_at,
            },
        )
        .collect())
}

async fn save_simple(tx: &mut Tx, l: &Lookup) -> Result<(), RepoError> {
    let table = l.kind.table();
    if l.is_default {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET is_default = false WHERE is_default AND id <> $1"
        )))
        .bind(l.id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {table} (id, key, name, is_default, sort_order, is_active, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_default = EXCLUDED.is_default,
             sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at"
    )))
    .bind(l.id)
    .bind(&l.key)
    .bind(&l.name)
    .bind(l.is_default)
    .bind(l.sort_order)
    .bind(l.is_active)
    .bind(l.created_at)
    .bind(l.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[async_trait]
impl ChangeRepository for PgChangeStore {
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
        Ok(match kind {
            LookupKind::Type => sqlx::query!(
                "SELECT id, key, name, approval_route, is_standard, is_default, sort_order, is_active, created_at, updated_at FROM change_types"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| {
                Ok(Lookup {
                    id: r.id,
                    kind,
                    key: r.key,
                    name: r.name,
                    stage: ChangeStage::Draft,
                    is_closed: false,
                    route: r.approval_route.parse().map_err(corrupt)?,
                    is_standard: r.is_standard,
                    is_default: r.is_default,
                    sort_order: r.sort_order,
                    is_active: r.is_active,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                })
            })
            .collect::<Result<_, RepoError>>()?,
            LookupKind::Status => sqlx::query!(
                "SELECT id, key, name, stage, is_closed, is_default, sort_order, is_active, created_at, updated_at FROM change_statuses"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| {
                Ok(Lookup {
                    id: r.id,
                    kind,
                    key: r.key,
                    name: r.name,
                    stage: r.stage.parse().map_err(corrupt)?,
                    is_closed: r.is_closed,
                    route: ApprovalRoute::Review,
                    is_standard: false,
                    is_default: r.is_default,
                    sort_order: r.sort_order,
                    is_active: r.is_active,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                })
            })
            .collect::<Result<_, RepoError>>()?,
            LookupKind::Priority | LookupKind::Impact | LookupKind::Category => simple_lookups(&self.pool, kind).await?,
        })
    }

    async fn save_lookup(&self, l: &Lookup, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        match l.kind {
            LookupKind::Type => {
                if l.is_default {
                    sqlx::query!(
                        "UPDATE change_types SET is_default = false WHERE is_default AND id <> $1",
                        l.id
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?;
                }
                sqlx::query!(
                    "INSERT INTO change_types (id, key, name, approval_route, is_standard, is_default, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, approval_route = EXCLUDED.approval_route,
                         is_standard = EXCLUDED.is_standard, is_default = EXCLUDED.is_default, sort_order = EXCLUDED.sort_order,
                         is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id, l.key, l.name, l.route.as_str(), l.is_standard, l.is_default, l.sort_order, l.is_active, l.created_at, l.updated_at
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
            LookupKind::Status => {
                if l.is_default {
                    sqlx::query!("UPDATE change_statuses SET is_default = false WHERE is_default AND id <> $1", l.id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db_err)?;
                }
                sqlx::query!(
                    "INSERT INTO change_statuses (id, key, name, stage, is_closed, is_default, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_closed = EXCLUDED.is_closed,
                         is_default = EXCLUDED.is_default, sort_order = EXCLUDED.sort_order,
                         is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id, l.key, l.name, l.stage.as_str(), l.is_closed, l.is_default, l.sort_order, l.is_active, l.created_at, l.updated_at
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
            LookupKind::Priority | LookupKind::Impact | LookupKind::Category => {
                save_simple(&mut tx, l).await?
            }
        }
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn risk_bands(&self) -> Result<Vec<RiskLevel>, RepoError> {
        let rows = sqlx::query!("SELECT id, name, min_score, max_score, sort_order FROM change_risk_levels ORDER BY min_score")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(RiskLevel {
                    id: r.id,
                    name: r.name,
                    min_score: u8::try_from(r.min_score).map_err(corrupt)?,
                    max_score: u8::try_from(r.max_score).map_err(corrupt)?,
                    sort_order: r.sort_order,
                })
            })
            .collect()
    }

    async fn save_risk_bands(
        &self,
        bands: &[RiskLevel],
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!("DELETE FROM change_risk_levels")
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for b in bands {
            sqlx::query!(
                "INSERT INTO change_risk_levels (id, name, min_score, max_score, sort_order, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, now(), now())",
                b.id, b.name, i32::from(b.min_score), i32::from(b.max_score), b.sort_order
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn next_sequence(&self) -> Result<u64, RepoError> {
        let v = sqlx::query_scalar!(r#"SELECT nextval('change_number_seq') AS "v!""#)
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        u64::try_from(v).map_err(corrupt)
    }

    async fn insert(
        &self,
        c: &Change,
        revs: &[TextRevision],
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let d = c.details();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO changes (id, tenant_id, change_number, title, type_id, status_id, priority_id, impact_id, category_id,
                                  requested_by_id, assigned_to_id, approver_id, work_start, work_end, outage_start, outage_end,
                                  description, reason_for_change, risk_evaluation, risk_likelihood, risk_impact_score, test_plan,
                                  rollback_plan, cab_required, cab_approval_type, approval_round, template_id, template_version,
                                  created_by_kind, created_by_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24,
                     $25, $26, $27, $28, $29, $30, $31, $32)",
            c.id(), c.tenant_id().as_uuid(), c.number(), d.title, d.type_id, c.status_id(), d.priority_id, d.impact_id, d.category_id,
            d.requested_by.map(|a| a.as_uuid()), d.assigned_to.map(|a| a.as_uuid()), d.approver.map(|a| a.as_uuid()),
            d.work_start, d.work_end, d.outage_start, d.outage_end, d.description, d.reason_for_change, d.risk_evaluation,
            d.risk_likelihood.map(i16::from), d.risk_impact_score.map(i16::from), d.test_plan, d.rollback_plan, d.cab_required,
            d.cab_approval_type.unwrap_or(CabApprovalType::All).as_str(), c.approval_round(), c.template().map(|t| t.0),
            c.template().map(|t| t.1), c.created_by().kind().as_str(), c.created_by().id(), c.created_at(), c.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        insert_revisions(&mut tx, c, revs).await?;
        finish(&mut tx, c, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(
        &self,
        c: &Change,
        revs: &[TextRevision],
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        update_row(&mut tx, c).await?;
        insert_revisions(&mut tx, c, revs).await?;
        finish(&mut tx, c, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn get(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Change>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            ChangeRow,
            r#"SELECT c.id, c.tenant_id, c.change_number, c.title, c.type_id, c.status_id, s.stage, c.priority_id, c.impact_id, c.category_id,
                      c.requested_by_id, c.assigned_to_id, c.approver_id, c.work_start, c.work_end, c.outage_start, c.outage_end,
                      c.description, c.reason_for_change, c.risk_evaluation, c.risk_likelihood, c.risk_impact_score, c.test_plan,
                      c.rollback_plan, c.cab_required, c.cab_approval_type, c.approval_round, c.approved_at, c.template_id,
                      c.template_version, c.pir_successful, c.pir_actual_start, c.pir_actual_end, c.pir_lessons_learned, c.pir_follow_up,
                      c.pir_recorded_at, c.created_by_kind, c.created_by_id, c.created_at, c.updated_at, c.closed_at
               FROM changes c JOIN change_statuses s ON s.id = c.status_id
               WHERE c.id = $1 AND ($2 OR c.tenant_id = ANY($3))"#,
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Change::try_from).transpose()
    }

    async fn list(&self, scope: &TenantScope, q: &ChangeQuery) -> Result<Vec<Change>, RepoError> {
        let (all, tenants) = ids(scope);
        let stages: Vec<String> = q.stages.iter().map(|s| s.as_str().to_owned()).collect();
        let (after_at, after_id) = q.after.map_or((None, None), |(a, i)| (Some(a), Some(i)));
        let (link_kind, link_target) = q
            .linked
            .map_or((None, None), |(k, t)| (Some(k.as_str()), Some(t)));
        let search = q
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{}%", s.replace('%', "\\%").replace('_', "\\_")));
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ChangeRow,
            r#"SELECT c.id, c.tenant_id, c.change_number, c.title, c.type_id, c.status_id, s.stage, c.priority_id, c.impact_id, c.category_id,
                      c.requested_by_id, c.assigned_to_id, c.approver_id, c.work_start, c.work_end, c.outage_start, c.outage_end,
                      c.description, c.reason_for_change, c.risk_evaluation, c.risk_likelihood, c.risk_impact_score, c.test_plan,
                      c.rollback_plan, c.cab_required, c.cab_approval_type, c.approval_round, c.approved_at, c.template_id,
                      c.template_version, c.pir_successful, c.pir_actual_start, c.pir_actual_end, c.pir_lessons_learned, c.pir_follow_up,
                      c.pir_recorded_at, c.created_by_kind, c.created_by_id, c.created_at, c.updated_at, c.closed_at
               FROM changes c JOIN change_statuses s ON s.id = c.status_id
               WHERE ($1 OR c.tenant_id = ANY($2))
                 AND (cardinality($3::text[]) = 0 OR s.stage = ANY($3))
                 AND ($4::uuid IS NULL OR c.assigned_to_id = $4)
                 AND ($5::uuid IS NULL OR (s.stage = 'awaiting' AND c.approved_at IS NULL
                      AND (c.approver_id = $5 OR EXISTS (SELECT 1 FROM change_cab_members m WHERE m.change_id = c.id AND m.analyst_id = $5))
                      AND NOT EXISTS (SELECT 1 FROM change_approval_votes v WHERE v.change_id = c.id AND v.round_no = c.approval_round AND v.analyst_id = $5)))
                 AND ($6::text IS NULL OR EXISTS (SELECT 1 FROM change_links l WHERE l.change_id = c.id AND l.kind = $6 AND l.target_id = $7))
                 AND ($8::text IS NULL OR c.title ILIKE $8 OR c.change_number ILIKE $8)
                 AND ($9::timestamptz IS NULL OR (c.updated_at, c.id) < ($9, $10::uuid))
               ORDER BY c.updated_at DESC, c.id DESC
               LIMIT $11"#,
            all,
            &tenants,
            &stages,
            q.assigned_to.map(|a| a.as_uuid()),
            q.awaiting_vote_of.map(|a| a.as_uuid()),
            link_kind,
            link_target,
            search,
            after_at,
            after_id,
            q.limit.fetch_size()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Change::try_from).collect()
    }

    async fn in_window(
        &self,
        scope: &TenantScope,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Change>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ChangeRow,
            r#"SELECT c.id, c.tenant_id, c.change_number, c.title, c.type_id, c.status_id, s.stage, c.priority_id, c.impact_id, c.category_id,
                      c.requested_by_id, c.assigned_to_id, c.approver_id, c.work_start, c.work_end, c.outage_start, c.outage_end,
                      c.description, c.reason_for_change, c.risk_evaluation, c.risk_likelihood, c.risk_impact_score, c.test_plan,
                      c.rollback_plan, c.cab_required, c.cab_approval_type, c.approval_round, c.approved_at, c.template_id,
                      c.template_version, c.pir_successful, c.pir_actual_start, c.pir_actual_end, c.pir_lessons_learned, c.pir_follow_up,
                      c.pir_recorded_at, c.created_by_kind, c.created_by_id, c.created_at, c.updated_at, c.closed_at
               FROM changes c JOIN change_statuses s ON s.id = c.status_id
               WHERE ($1 OR c.tenant_id = ANY($2))
                 AND ((c.work_start < $4 AND c.work_end > $3) OR (c.outage_start < $4 AND c.outage_end > $3))
               ORDER BY coalesce(c.work_start, c.outage_start), c.id
               LIMIT 1000"#,
            all,
            &tenants,
            from,
            to
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Change::try_from).collect()
    }

    async fn cab(&self, scope: &TenantScope, change: Uuid) -> Result<Vec<CabMember>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT analyst_id, is_required FROM change_cab_members WHERE change_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY analyst_id",
            change, all, &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| CabMember {
                analyst_id: AnalystId::from_uuid(r.analyst_id),
                is_required: r.is_required,
            })
            .collect())
    }

    async fn set_cab(
        &self,
        c: &Change,
        members: &[CabMember],
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "DELETE FROM change_cab_members WHERE change_id = $1",
            c.id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for m in members {
            sqlx::query!(
                "INSERT INTO change_cab_members (tenant_id, change_id, analyst_id, is_required) VALUES ($1, $2, $3, $4)",
                c.tenant_id().as_uuid(), c.id(), m.analyst_id.as_uuid(), m.is_required
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        touch(&mut tx, c).await?;
        finish(&mut tx, c, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn votes(
        &self,
        scope: &TenantScope,
        change: Uuid,
        round: i32,
    ) -> Result<Vec<Vote>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, round_no, analyst_id, decision, comment, voted_at FROM change_approval_votes
             WHERE change_id = $1 AND round_no = $2 AND ($3 OR tenant_id = ANY($4)) ORDER BY voted_at, id",
            change, round, all, &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(Vote {
                    id: r.id,
                    round_no: r.round_no,
                    analyst_id: AnalystId::from_uuid(r.analyst_id),
                    decision: r.decision.parse::<Decision>().map_err(corrupt)?,
                    comment: r.comment,
                    voted_at: r.voted_at,
                })
            })
            .collect()
    }

    async fn vote(
        &self,
        c: &Change,
        v: &Vote,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        // Serialise votes on one Change so two last votes cannot both miss the outcome.
        sqlx::query!("SELECT id FROM changes WHERE id = $1 FOR UPDATE", c.id())
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO change_approval_votes (id, tenant_id, change_id, round_no, analyst_id, decision, comment, voted_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            v.id, c.tenant_id().as_uuid(), c.id(), v.round_no, v.analyst_id.as_uuid(), v.decision.as_str(), v.comment, v.voted_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        update_row(&mut tx, c).await?;
        finish(&mut tx, c, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn checklist(
        &self,
        scope: &TenantScope,
        change: Uuid,
    ) -> Result<Vec<ChecklistItem>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, text, is_done, done_by_id, done_at, sort_order, created_at FROM change_checklist_items
             WHERE change_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY sort_order, created_at, id",
            change, all, &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| ChecklistItem {
                id: r.id,
                text: r.text,
                is_done: r.is_done,
                done_by: r.done_by_id.map(AnalystId::from_uuid),
                done_at: r.done_at,
                sort_order: r.sort_order,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn save_item(
        &self,
        c: &Change,
        i: &ChecklistItem,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO change_checklist_items (id, tenant_id, change_id, text, is_done, done_by_id, done_at, sort_order, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET text = EXCLUDED.text, is_done = EXCLUDED.is_done, done_by_id = EXCLUDED.done_by_id,
                 done_at = EXCLUDED.done_at, sort_order = EXCLUDED.sort_order",
            i.id, c.tenant_id().as_uuid(), c.id(), i.text, i.is_done, i.done_by.map(|a| a.as_uuid()), i.done_at, i.sort_order, i.created_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        touch(&mut tx, c).await?;
        finish(&mut tx, c, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn delete_item(
        &self,
        c: &Change,
        item: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "DELETE FROM change_checklist_items WHERE id = $1 AND change_id = $2",
            item,
            c.id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        touch(&mut tx, c).await?;
        finish(&mut tx, c, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn comments(&self, scope: &TenantScope, change: Uuid) -> Result<Vec<Comment>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, author_id, body, created_at FROM change_comments WHERE change_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY created_at, id",
            change, all, &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| Comment {
                id: r.id,
                author_id: r.author_id.map(AnalystId::from_uuid),
                body: r.body,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn add_comment(
        &self,
        c: &Change,
        m: &Comment,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO change_comments (id, tenant_id, change_id, author_id, body, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
            m.id, c.tenant_id().as_uuid(), c.id(), m.author_id.map(|a| a.as_uuid()), m.body, m.created_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        touch(&mut tx, c).await?;
        finish(&mut tx, c, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn links(&self, scope: &TenantScope, change: Uuid) -> Result<Vec<ChangeLink>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT kind, target_id, linked_by_kind, linked_by_id, linked_at FROM change_links
             WHERE change_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY linked_at, target_id",
            change,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(ChangeLink {
                    kind: r.kind.parse::<LinkKind>().map_err(corrupt)?,
                    target_id: r.target_id,
                    linked_by: Actor::from_parts(&r.linked_by_kind, r.linked_by_id)
                        .map_err(corrupt)?,
                    linked_at: r.linked_at,
                })
            })
            .collect()
    }

    async fn add_link(
        &self,
        c: &Change,
        l: &ChangeLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO change_links (tenant_id, change_id, kind, target_id, linked_by_kind, linked_by_id, linked_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            c.tenant_id().as_uuid(), c.id(), l.kind.as_str(), l.target_id, l.linked_by.kind().as_str(), l.linked_by.id(), l.linked_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        touch(&mut tx, c).await?;
        finish(&mut tx, c, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn remove_link(
        &self,
        c: &Change,
        kind: LinkKind,
        target: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "DELETE FROM change_links WHERE change_id = $1 AND kind = $2 AND target_id = $3",
            c.id(),
            kind.as_str(),
            target
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        touch(&mut tx, c).await?;
        finish(&mut tx, c, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn revisions(
        &self,
        scope: &TenantScope,
        change: Uuid,
        field: &str,
    ) -> Result<Vec<TextRevision>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, field, body, author_kind, author_id, created_at FROM change_text_revisions
             WHERE change_id = $1 AND field = $2 AND ($3 OR tenant_id = ANY($4)) ORDER BY created_at, id",
            change, field, all, &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(TextRevision {
                    id: r.id,
                    field: r.field,
                    body: r.body,
                    author: Actor::from_parts(&r.author_kind, r.author_id).map_err(corrupt)?,
                    created_at: r.created_at,
                })
            })
            .collect()
    }

    async fn attachments(
        &self,
        scope: &TenantScope,
        change: Uuid,
    ) -> Result<Vec<ChangeAttachment>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = cooper_platform::attachments::of_owner(&mut tx, "change", change)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|a| ChangeAttachment {
                id: a.id,
                file_name: a.file_name,
                media_type: a.media_type,
                size_bytes: u64::try_from(a.size_bytes).unwrap_or_default(),
                sha256: a.sha256,
                created_by: a.created_by,
                created_at: a.created_at,
            })
            .collect())
    }

    async fn add_attachment(
        &self,
        c: &Change,
        a: &ChangeAttachment,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        cooper_platform::attachments::insert(
            &mut tx,
            &cooper_platform::attachments::Attachment {
                id: a.id,
                tenant_id: c.tenant_id(),
                owner_type: "change".into(),
                owner_id: c.id(),
                file_name: a.file_name.clone(),
                media_type: a.media_type.clone(),
                size_bytes: i64::try_from(a.size_bytes).map_err(corrupt)?,
                sha256: a.sha256.clone(),
                extraction_status: "pending".into(),
                created_by: a.created_by,
                created_at: a.created_at,
            },
        )
        .await
        .map_err(db_err)?;
        touch(&mut tx, c).await?;
        finish(&mut tx, c, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn templates(&self) -> Result<Vec<ChangeTemplate>, RepoError> {
        let rows = sqlx::query!(
            "SELECT t.id, t.name, t.description, t.team_ids, t.current_version, t.is_active, t.created_at, t.updated_at, v.content
             FROM change_templates t JOIN change_template_versions v ON v.template_id = t.id AND v.version_no = t.current_version
             ORDER BY lower(t.name)"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(ChangeTemplate {
                    id: r.id,
                    name: r.name,
                    description: r.description,
                    team_ids: r.team_ids.into_iter().map(TeamId::from_uuid).collect(),
                    version: r.current_version,
                    content: serde_json::from_value(r.content).map_err(corrupt)?,
                    is_active: r.is_active,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                })
            })
            .collect()
    }

    async fn save_template(
        &self,
        t: &ChangeTemplate,
        by: Actor,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let content = serde_json::to_value(&t.content).map_err(corrupt)?;
        let teams: Vec<Uuid> = t.team_ids.iter().map(|x| x.as_uuid()).collect();
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO change_templates (id, name, description, team_ids, current_version, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description, team_ids = EXCLUDED.team_ids,
                 current_version = EXCLUDED.current_version, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            t.id, t.name, t.description, &teams, t.version, t.is_active, t.created_at, t.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO change_template_versions (template_id, version_no, content, created_by_kind, created_by_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (template_id, version_no) DO NOTHING",
            t.id, t.version, content, by.kind().as_str(), by.id(), t.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn template_version(
        &self,
        template: Uuid,
        version: i32,
    ) -> Result<Option<TemplateContent>, RepoError> {
        let row = sqlx::query_scalar!("SELECT content FROM change_template_versions WHERE template_id = $1 AND version_no = $2", template, version)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.map(|c| serde_json::from_value(c).map_err(corrupt))
            .transpose()
    }
}

async fn update_row(tx: &mut Tx, c: &Change) -> Result<(), RepoError> {
    let d = c.details();
    let pir = c.pir();
    let n = sqlx::query!(
        "UPDATE changes SET title = $2, type_id = $3, status_id = $4, priority_id = $5, impact_id = $6, category_id = $7,
                requested_by_id = $8, assigned_to_id = $9, approver_id = $10, work_start = $11, work_end = $12, outage_start = $13,
                outage_end = $14, description = $15, reason_for_change = $16, risk_evaluation = $17, risk_likelihood = $18,
                risk_impact_score = $19, test_plan = $20, rollback_plan = $21, cab_required = $22, cab_approval_type = $23,
                approval_round = $24, approved_at = $25, pir_successful = $26, pir_actual_start = $27, pir_actual_end = $28,
                pir_lessons_learned = $29, pir_follow_up = $30, pir_recorded_at = $31, updated_at = $32, closed_at = $33
         WHERE id = $1",
        c.id(), d.title, d.type_id, c.status_id(), d.priority_id, d.impact_id, d.category_id, d.requested_by.map(|a| a.as_uuid()),
        d.assigned_to.map(|a| a.as_uuid()), d.approver.map(|a| a.as_uuid()), d.work_start, d.work_end, d.outage_start, d.outage_end,
        d.description, d.reason_for_change, d.risk_evaluation, d.risk_likelihood.map(i16::from), d.risk_impact_score.map(i16::from),
        d.test_plan, d.rollback_plan, d.cab_required, d.cab_approval_type.unwrap_or(CabApprovalType::All).as_str(), c.approval_round(),
        c.approved_at(), pir.map(|p| p.successful), pir.and_then(|p| p.actual_start), pir.and_then(|p| p.actual_end),
        pir.map(|p| p.lessons_learned.clone()), pir.map(|p| p.follow_up.clone()), pir.map(|p| p.recorded_at), c.updated_at(), c.closed_at()
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
