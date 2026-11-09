//! PostgreSQL storage of Morning Checks (tenant-scoped: explicit filter plus RLS), and the
//! `morning-check-reminders` Background Job.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AnalystId, AuditRecord, EventEnvelope, TeamId, TenantId, TenantScope};
use cooper_platform::db::Tx;
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};
use cooper_platform::live::{self, LiveChange};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    Check, CheckAssignee, CheckDetails, CheckGroup, CheckGroupId, CheckId, CheckResult,
    CheckResultId, CheckResultRecord, CheckStatus, GroupDetails, LinkKind, ResultLink,
};
use crate::ports::{MorningChecksRepository, Reminder, RepoError, RoundTally};

/// Live Updates topic of Morning Checks: a Check Result changed.
const TOPIC: &str = "check_result";

/// sqlx implementation of [`MorningChecksRepository`].
#[derive(Clone)]
pub struct PgMorningChecksStore {
    pool: PgPool,
}

impl PgMorningChecksStore {
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
    r: &CheckResult,
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
    live::publish(tx, &LiveChange::new(r.tenant_id(), TOPIC, r.id().as_uuid()))
        .await
        .map_err(db_err)?;
    Ok(())
}

struct ResultRow {
    id: Uuid,
    tenant_id: Uuid,
    check_id: Uuid,
    check_date: NaiveDate,
    status_id: Uuid,
    notes: String,
    recorded_by_kind: String,
    recorded_by_id: Option<Uuid>,
    recorded_at: DateTime<Utc>,
    modified_by_kind: String,
    modified_by_id: Option<Uuid>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ResultRow> for CheckResult {
    type Error = RepoError;
    fn try_from(r: ResultRow) -> Result<Self, RepoError> {
        Ok(Self::restore(CheckResultRecord {
            id: CheckResultId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            check_id: CheckId::from_uuid(r.check_id),
            check_date: r.check_date,
            status_id: r.status_id,
            notes: r.notes,
            recorded_by: Actor::from_parts(&r.recorded_by_kind, r.recorded_by_id)
                .map_err(corrupt)?,
            recorded_at: r.recorded_at,
            modified_by: Actor::from_parts(&r.modified_by_kind, r.modified_by_id)
                .map_err(corrupt)?,
            updated_at: r.updated_at,
        }))
    }
}

struct GroupRow {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    description: String,
    assigned_team_id: Option<Uuid>,
    assigned_analyst_id: Option<Uuid>,
    sort_order: i32,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<GroupRow> for CheckGroup {
    fn from(r: GroupRow) -> Self {
        let assignee = match (r.assigned_analyst_id, r.assigned_team_id) {
            (Some(a), _) => Some(CheckAssignee::Analyst(AnalystId::from_uuid(a))),
            (None, Some(t)) => Some(CheckAssignee::Team(TeamId::from_uuid(t))),
            (None, None) => None,
        };
        Self::restore(
            CheckGroupId::from_uuid(r.id),
            TenantId::from_uuid(r.tenant_id),
            GroupDetails {
                name: r.name,
                description: r.description,
                assignee,
                sort_order: r.sort_order,
                is_active: r.is_active,
            },
            r.created_at,
            r.updated_at,
        )
    }
}

struct CheckRow {
    id: Uuid,
    tenant_id: Uuid,
    check_group_id: Option<Uuid>,
    name: String,
    description: String,
    assigned_analyst_id: Option<Uuid>,
    sort_order: i32,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<CheckRow> for Check {
    fn from(r: CheckRow) -> Self {
        Self::restore(
            CheckId::from_uuid(r.id),
            TenantId::from_uuid(r.tenant_id),
            CheckDetails {
                name: r.name,
                description: r.description,
                group_id: r.check_group_id.map(CheckGroupId::from_uuid),
                assigned_analyst: r.assigned_analyst_id.map(AnalystId::from_uuid),
                sort_order: r.sort_order,
                is_active: r.is_active,
            },
            r.created_at,
            r.updated_at,
        )
    }
}

#[async_trait]
impl MorningChecksRepository for PgMorningChecksStore {
    async fn statuses(&self) -> Result<Vec<CheckStatus>, RepoError> {
        Ok(sqlx::query!(
            "SELECT id, key, name, colour, requires_notes, sort_order, is_active, created_at, updated_at FROM check_statuses"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|r| CheckStatus {
            id: r.id,
            key: r.key,
            name: r.name,
            colour: r.colour,
            requires_notes: r.requires_notes,
            sort_order: r.sort_order,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
    }

    async fn save_status(&self, s: &CheckStatus, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO check_statuses (id, key, name, colour, requires_notes, sort_order, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, colour = EXCLUDED.colour,
                 requires_notes = EXCLUDED.requires_notes, sort_order = EXCLUDED.sort_order,
                 is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            s.id,
            s.key,
            s.name,
            s.colour,
            s.requires_notes,
            s.sort_order,
            s.is_active,
            s.created_at,
            s.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if audit.is_meaningful() {
            cooper_platform::audit::record(&mut tx, audit)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    async fn groups(&self, scope: &TenantScope) -> Result<Vec<CheckGroup>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            GroupRow,
            "SELECT id, tenant_id, name, description, assigned_team_id, assigned_analyst_id, sort_order, is_active,
                    created_at, updated_at
             FROM check_groups WHERE ($1 OR tenant_id = ANY($2))",
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(CheckGroup::from).collect())
    }

    async fn save_group(&self, g: &CheckGroup, audit: &AuditRecord) -> Result<(), RepoError> {
        let d = g.details();
        let (team, analyst) = match d.assignee {
            Some(CheckAssignee::Team(t)) => (Some(t.as_uuid()), None),
            Some(CheckAssignee::Analyst(a)) => (None, Some(a.as_uuid())),
            None => (None, None),
        };
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(g.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO check_groups (id, tenant_id, name, description, assigned_team_id, assigned_analyst_id, sort_order,
                                       is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description,
                 assigned_team_id = EXCLUDED.assigned_team_id, assigned_analyst_id = EXCLUDED.assigned_analyst_id,
                 sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            g.id().as_uuid(),
            g.tenant_id().as_uuid(),
            d.name,
            d.description,
            team,
            analyst,
            d.sort_order,
            d.is_active,
            g.created_at(),
            g.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if audit.is_meaningful() {
            cooper_platform::audit::record(&mut tx, audit)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    async fn checks(&self, scope: &TenantScope) -> Result<Vec<Check>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            CheckRow,
            "SELECT id, tenant_id, check_group_id, name, description, assigned_analyst_id, sort_order, is_active,
                    created_at, updated_at
             FROM checks WHERE ($1 OR tenant_id = ANY($2))",
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(Check::from).collect())
    }

    async fn save_check(&self, c: &Check, audit: &AuditRecord) -> Result<(), RepoError> {
        let d = c.details();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO checks (id, tenant_id, check_group_id, name, description, assigned_analyst_id, sort_order, is_active,
                                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET check_group_id = EXCLUDED.check_group_id, name = EXCLUDED.name,
                 description = EXCLUDED.description, assigned_analyst_id = EXCLUDED.assigned_analyst_id,
                 sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            c.id().as_uuid(),
            c.tenant_id().as_uuid(),
            d.group_id.map(|g| g.as_uuid()),
            d.name,
            d.description,
            d.assigned_analyst.map(|a| a.as_uuid()),
            d.sort_order,
            d.is_active,
            c.created_at(),
            c.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if audit.is_meaningful() {
            cooper_platform::audit::record(&mut tx, audit)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    async fn results(
        &self,
        scope: &TenantScope,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<CheckResult>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ResultRow,
            "SELECT id, tenant_id, check_id, check_date, status_id, notes, recorded_by_kind, recorded_by_id, recorded_at,
                    modified_by_kind, modified_by_id, updated_at
             FROM check_results
             WHERE ($1 OR tenant_id = ANY($2)) AND check_date BETWEEN $3 AND $4
             ORDER BY check_date, check_id",
            all,
            &tenants,
            from,
            to
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(CheckResult::try_from).collect()
    }

    async fn result_for(
        &self,
        scope: &TenantScope,
        check: CheckId,
        date: NaiveDate,
    ) -> Result<Option<CheckResult>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            ResultRow,
            "SELECT id, tenant_id, check_id, check_date, status_id, notes, recorded_by_kind, recorded_by_id, recorded_at,
                    modified_by_kind, modified_by_id, updated_at
             FROM check_results
             WHERE ($1 OR tenant_id = ANY($2)) AND check_id = $3 AND check_date = $4",
            all,
            &tenants,
            check.as_uuid(),
            date
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(CheckResult::try_from).transpose()
    }

    async fn result(
        &self,
        scope: &TenantScope,
        id: CheckResultId,
    ) -> Result<Option<CheckResult>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            ResultRow,
            "SELECT id, tenant_id, check_id, check_date, status_id, notes, recorded_by_kind, recorded_by_id, recorded_at,
                    modified_by_kind, modified_by_id, updated_at
             FROM check_results WHERE ($1 OR tenant_id = ANY($2)) AND id = $3",
            all,
            &tenants,
            id.as_uuid()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(CheckResult::try_from).transpose()
    }

    async fn insert_result(
        &self,
        r: &CheckResult,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(r.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO check_results (id, tenant_id, check_id, check_date, status_id, notes, recorded_by_kind, recorded_by_id,
                                        recorded_at, modified_by_kind, modified_by_id, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            r.id().as_uuid(),
            r.tenant_id().as_uuid(),
            r.check_id().as_uuid(),
            r.check_date(),
            r.status_id(),
            r.notes(),
            r.recorded_by().kind().as_str(),
            r.recorded_by().id(),
            r.recorded_at(),
            r.modified_by().kind().as_str(),
            r.modified_by().id(),
            r.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        finish(&mut tx, r, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update_result(
        &self,
        r: &CheckResult,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(r.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "UPDATE check_results SET status_id = $2, notes = $3, modified_by_kind = $4, modified_by_id = $5, updated_at = $6
             WHERE id = $1",
            r.id().as_uuid(),
            r.status_id(),
            r.notes(),
            r.modified_by().kind().as_str(),
            r.modified_by().id(),
            r.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        finish(&mut tx, r, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn clear_result(
        &self,
        r: &CheckResult,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(r.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!("DELETE FROM check_results WHERE id = $1", r.id().as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        finish(&mut tx, r, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn links(
        &self,
        scope: &TenantScope,
        results: &[CheckResultId],
    ) -> Result<Vec<(CheckResultId, ResultLink)>, RepoError> {
        let (all, tenants) = ids(scope);
        let ids: Vec<Uuid> = results.iter().map(CheckResultId::as_uuid).collect();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT check_result_id, kind, target_id, linked_by_kind, linked_by_id, linked_at FROM check_result_links
             WHERE ($1 OR tenant_id = ANY($2)) AND check_result_id = ANY($3) ORDER BY linked_at, target_id",
            all,
            &tenants,
            &ids
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok((
                    CheckResultId::from_uuid(r.check_result_id),
                    ResultLink {
                        kind: r.kind.parse::<LinkKind>().map_err(corrupt)?,
                        target_id: r.target_id,
                        linked_by: Actor::from_parts(&r.linked_by_kind, r.linked_by_id)
                            .map_err(corrupt)?,
                        linked_at: r.linked_at,
                    },
                ))
            })
            .collect()
    }

    async fn linked_to(
        &self,
        scope: &TenantScope,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<Vec<CheckResult>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ResultRow,
            "SELECT r.id, r.tenant_id, r.check_id, r.check_date, r.status_id, r.notes, r.recorded_by_kind, r.recorded_by_id,
                    r.recorded_at, r.modified_by_kind, r.modified_by_id, r.updated_at
             FROM check_results r JOIN check_result_links l ON l.check_result_id = r.id
             WHERE ($1 OR r.tenant_id = ANY($2)) AND l.kind = $3 AND l.target_id = $4
             ORDER BY r.check_date DESC, r.id",
            all,
            &tenants,
            kind.as_str(),
            target
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(CheckResult::try_from).collect()
    }

    async fn add_link(
        &self,
        r: &CheckResult,
        l: &ResultLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(r.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO check_result_links (tenant_id, check_result_id, kind, target_id, linked_by_kind, linked_by_id, linked_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            r.tenant_id().as_uuid(),
            r.id().as_uuid(),
            l.kind.as_str(),
            l.target_id,
            l.linked_by.kind().as_str(),
            l.linked_by.id(),
            l.linked_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        finish(&mut tx, r, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn remove_link(
        &self,
        r: &CheckResult,
        kind: LinkKind,
        target: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(r.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "DELETE FROM check_result_links WHERE check_result_id = $1 AND kind = $2 AND target_id = $3",
            r.id().as_uuid(),
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
        finish(&mut tx, r, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn record_reminders(&self, reminders: &[Reminder]) -> Result<usize, RepoError> {
        let mut sent = 0;
        for rm in reminders {
            let e = &rm.event;
            let mut tx = tenancy::begin(&self.pool, &TenantScope::single(e.tenant_id))
                .await
                .map_err(db_err)?;
            let n = sqlx::query!(
                "INSERT INTO check_reminders (id, tenant_id, check_date, check_group_id, analyst_id, unrecorded, sent_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT ON CONSTRAINT check_reminders_once DO NOTHING",
                e.reminder_id,
                e.tenant_id.as_uuid(),
                e.check_date,
                e.check_group_id.map(|g| g.as_uuid()),
                e.analyst_id.as_uuid(),
                i32::try_from(e.unrecorded).unwrap_or(i32::MAX),
                rm.envelope.occurred_at
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
            if n == 1 {
                cooper_platform::outbox::append(&mut tx, &rm.envelope)
                    .await
                    .map_err(db_err)?;
                sent += 1;
            }
            tx.commit().await.map_err(db_err)?;
        }
        Ok(sent)
    }

    async fn history(
        &self,
        scope: &TenantScope,
        result: CheckResultId,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let h = cooper_platform::audit::history(&mut tx, "check_result", result.as_uuid())
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        tx.commit().await.map_err(db_err)?;
        Ok(h)
    }

    async fn round_tally(
        &self,
        scope: &TenantScope,
        date: NaiveDate,
        routed_to: Option<(AnalystId, &std::collections::BTreeSet<TeamId>)>,
    ) -> Result<RoundTally, RepoError> {
        let (all, tenants) = ids(scope);
        let analyst = routed_to.map(|(a, _)| a.as_uuid());
        let teams: Vec<Uuid> = routed_to
            .map(|(_, t)| t.iter().map(|t| t.as_uuid()).collect())
            .unwrap_or_default();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        // The Round: active Checks whose group, if any, is active. Routing as in
        // `domain::routing`: the Check's Analyst, else its group's Analyst or Team.
        let rows = sqlx::query!(
            r#"SELECT r.status_id AS "status_id?", COUNT(*) AS "checks!"
               FROM checks c
               LEFT JOIN check_groups g ON g.id = c.check_group_id
               LEFT JOIN check_results r ON r.check_id = c.id AND r.check_date = $3
               WHERE ($1 OR c.tenant_id = ANY($2)) AND c.is_active AND (g.id IS NULL OR g.is_active)
                 AND ($4::uuid IS NULL OR c.assigned_analyst_id = $4
                      OR (c.assigned_analyst_id IS NULL
                          AND (g.assigned_analyst_id = $4 OR g.assigned_team_id = ANY($5))))
               GROUP BY r.status_id"#,
            all,
            &tenants,
            date,
            analyst,
            &teams
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        let mut tally = RoundTally {
            check_date: date,
            ..RoundTally::default()
        };
        for r in rows {
            let n = u64::try_from(r.checks).unwrap_or(0);
            tally.checks += n;
            if let Some(status) = r.status_id {
                tally.recorded += n;
                tally.by_status.push((status, n));
            }
        }
        Ok(tally)
    }
}

/// The `morning-check-reminders` Background Job: reminds the Analysts whose Checks are
/// still unrecorded at the reminder time (FR-MCK-006), once per Check Date, Check Group
/// and Analyst.
pub struct MorningCheckReminderJob {
    /// The service.
    pub service: crate::application::MorningChecksService,
}

#[async_trait]
impl Job for MorningCheckReminderJob {
    fn name(&self) -> &'static str {
        "morning-check-reminders"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(300)
    }

    async fn run(&self, _ctx: &JobContext) -> Result<Option<String>, JobError> {
        let n = self
            .service
            .send_reminders()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(format!("sent {n} reminders")))
    }
}
