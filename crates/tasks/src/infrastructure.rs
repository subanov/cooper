//! PostgreSQL storage of Tasks (tenant-scoped: explicit filter plus RLS).

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AnalystId, AuditRecord, EventEnvelope, TeamId, TenantId, TenantScope};
use cooper_platform::db::Tx;
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};
use cooper_platform::live::{self, LiveChange};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{Lookup, LookupKind, Task, TaskDetails, TaskRecord};
use crate::ports::{
    LinkKind, OpenTaskTally, Recurrence, RepoError, TaskComment, TaskLink, TaskQuery,
    TaskRepository, TimeEntry, TimeGrouping, TimeTally,
};

/// sqlx implementation of [`TaskRepository`].
#[derive(Clone)]
pub struct PgTaskStore {
    pool: PgPool,
}

impl PgTaskStore {
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

async fn audit_and_publish(
    tx: &mut Tx,
    task: &Task,
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
    live::publish(tx, &LiveChange::new(task.tenant_id(), "task", task.id()))
        .await
        .map_err(db_err)?;
    Ok(())
}

async fn touch(tx: &mut Tx, task: &Task) -> Result<(), RepoError> {
    sqlx::query!(
        "UPDATE tasks SET updated_at = now() WHERE id = $1",
        task.id()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn insert_task(
    tx: &mut Tx,
    t: &Task,
    collaborators: &BTreeSet<AnalystId>,
) -> Result<(), RepoError> {
    let d = t.details();
    sqlx::query!(
        "INSERT INTO tasks (id, tenant_id, title, description, status_id, priority_id, start_date, due_date, estimate_minutes,
                            assigned_analyst_id, assigned_team_id, parent_task_id, tags, board_position, recurrence_id,
                            previous_occurrence_id, created_by_kind, created_by_id, created_at, updated_at, completed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)",
        t.id(),
        t.tenant_id().as_uuid(),
        d.title,
        d.description,
        t.status_id(),
        d.priority_id,
        d.start_date,
        d.due_date,
        d.estimate_minutes,
        d.assigned_analyst.map(|a| a.as_uuid()),
        d.assigned_team.map(|x| x.as_uuid()),
        t.parent_task_id(),
        &d.tags,
        t.board_position(),
        t.recurrence_id(),
        t.previous_occurrence_id(),
        t.created_by().kind().as_str(),
        t.created_by().id(),
        t.created_at(),
        t.updated_at(),
        t.completed_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    for a in collaborators {
        sqlx::query!(
            "INSERT INTO task_collaborators (tenant_id, task_id, analyst_id) VALUES ($1, $2, $3)",
            t.tenant_id().as_uuid(),
            t.id(),
            a.as_uuid()
        )
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

async fn update_task(tx: &mut Tx, t: &Task) -> Result<(), RepoError> {
    let d = t.details();
    let n = sqlx::query!(
        "UPDATE tasks SET title = $2, description = $3, status_id = $4, priority_id = $5, start_date = $6, due_date = $7,
                estimate_minutes = $8, assigned_analyst_id = $9, assigned_team_id = $10, tags = $11, board_position = $12,
                recurrence_id = $13, updated_at = $14, completed_at = $15
         WHERE id = $1",
        t.id(),
        d.title,
        d.description,
        t.status_id(),
        d.priority_id,
        d.start_date,
        d.due_date,
        d.estimate_minutes,
        d.assigned_analyst.map(|a| a.as_uuid()),
        d.assigned_team.map(|x| x.as_uuid()),
        &d.tags,
        t.board_position(),
        t.recurrence_id(),
        t.updated_at(),
        t.completed_at()
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

async fn save_series(tx: &mut Tx, r: &Recurrence) -> Result<(), RepoError> {
    let rule = serde_json::to_value(&r.rule).map_err(corrupt)?;
    sqlx::query!(
        "INSERT INTO task_recurrences (id, tenant_id, rule, occurrences_created, next_date, is_active, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (id) DO UPDATE SET rule = EXCLUDED.rule, occurrences_created = EXCLUDED.occurrences_created,
             next_date = EXCLUDED.next_date, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
        r.id,
        r.tenant_id.as_uuid(),
        rule,
        i32::try_from(r.occurrences_created).unwrap_or(i32::MAX),
        r.next_date,
        r.is_active,
        r.created_at,
        r.updated_at
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

struct TaskRow {
    id: Uuid,
    tenant_id: Uuid,
    title: String,
    description: String,
    status_id: Uuid,
    is_closed: bool,
    priority_id: Uuid,
    start_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    estimate_minutes: Option<i32>,
    assigned_analyst_id: Option<Uuid>,
    assigned_team_id: Option<Uuid>,
    parent_task_id: Option<Uuid>,
    tags: Vec<String>,
    board_position: i64,
    recurrence_id: Option<Uuid>,
    previous_occurrence_id: Option<Uuid>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl TryFrom<TaskRow> for Task {
    type Error = RepoError;
    fn try_from(r: TaskRow) -> Result<Self, RepoError> {
        Ok(Self::restore(TaskRecord {
            id: r.id,
            tenant_id: TenantId::from_uuid(r.tenant_id),
            details: TaskDetails {
                title: r.title,
                description: r.description,
                priority_id: r.priority_id,
                start_date: r.start_date,
                due_date: r.due_date,
                estimate_minutes: r.estimate_minutes,
                assigned_analyst: r.assigned_analyst_id.map(AnalystId::from_uuid),
                assigned_team: r.assigned_team_id.map(TeamId::from_uuid),
                tags: r.tags,
            },
            status_id: r.status_id,
            is_closed: r.is_closed,
            parent_task_id: r.parent_task_id,
            board_position: r.board_position,
            recurrence_id: r.recurrence_id,
            previous_occurrence_id: r.previous_occurrence_id,
            created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id).map_err(corrupt)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
            completed_at: r.completed_at,
        }))
    }
}

struct SeriesRow {
    id: Uuid,
    tenant_id: Uuid,
    rule: serde_json::Value,
    occurrences_created: i32,
    next_date: Option<NaiveDate>,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<SeriesRow> for Recurrence {
    type Error = RepoError;
    fn try_from(r: SeriesRow) -> Result<Self, RepoError> {
        Ok(Self {
            id: r.id,
            tenant_id: TenantId::from_uuid(r.tenant_id),
            rule: serde_json::from_value(r.rule).map_err(corrupt)?,
            occurrences_created: u32::try_from(r.occurrences_created).map_err(corrupt)?,
            next_date: r.next_date,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

#[async_trait]
impl TaskRepository for PgTaskStore {
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
        Ok(match kind {
            LookupKind::Status => sqlx::query!(
                "SELECT id, key, name, is_closed, is_done, colour, is_default, sort_order, is_active, created_at, updated_at FROM task_statuses"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| Lookup {
                id: r.id,
                kind,
                key: r.key,
                name: r.name,
                is_closed: r.is_closed,
                is_done: r.is_done,
                colour: r.colour,
                is_default: r.is_default,
                sort_order: r.sort_order,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect(),
            LookupKind::Priority => sqlx::query!(
                "SELECT id, key, name, colour, is_default, sort_order, is_active, created_at, updated_at FROM task_priorities"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| Lookup {
                id: r.id,
                kind,
                key: r.key,
                name: r.name,
                is_closed: false,
                is_done: false,
                colour: r.colour,
                is_default: r.is_default,
                sort_order: r.sort_order,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect(),
        })
    }

    async fn save_lookup(&self, l: &Lookup, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        match l.kind {
            LookupKind::Status => {
                if l.is_default {
                    sqlx::query!(
                        "UPDATE task_statuses SET is_default = false WHERE is_default AND id <> $1",
                        l.id
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?;
                }
                sqlx::query!(
                    "INSERT INTO task_statuses (id, key, name, is_closed, is_done, colour, is_default, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_closed = EXCLUDED.is_closed, is_done = EXCLUDED.is_done,
                         colour = EXCLUDED.colour, is_default = EXCLUDED.is_default, sort_order = EXCLUDED.sort_order,
                         is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id, l.key, l.name, l.is_closed, l.is_done, l.colour, l.is_default, l.sort_order, l.is_active, l.created_at, l.updated_at
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
            LookupKind::Priority => {
                if l.is_default {
                    sqlx::query!("UPDATE task_priorities SET is_default = false WHERE is_default AND id <> $1", l.id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db_err)?;
                }
                sqlx::query!(
                    "INSERT INTO task_priorities (id, key, name, colour, is_default, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, colour = EXCLUDED.colour, is_default = EXCLUDED.is_default,
                         sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id, l.key, l.name, l.colour, l.is_default, l.sort_order, l.is_active, l.created_at, l.updated_at
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
        }
        cooper_platform::audit::record(&mut tx, audit)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn insert(
        &self,
        tasks: &[(Task, BTreeSet<AnalystId>)],
        audits: &[AuditRecord],
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let Some((first, _)) = tasks.first() else {
            return Ok(());
        };
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(first.tenant_id()))
            .await
            .map_err(db_err)?;
        for (t, who) in tasks {
            insert_task(&mut tx, t, who).await?;
            live::publish(&mut tx, &LiveChange::new(t.tenant_id(), "task", t.id()))
                .await
                .map_err(db_err)?;
        }
        for a in audits {
            cooper_platform::audit::record(&mut tx, a)
                .await
                .map_err(db_err)?;
        }
        for e in events {
            cooper_platform::outbox::append(&mut tx, e)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    async fn update(
        &self,
        task: &Task,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        update_task(&mut tx, task).await?;
        audit_and_publish(&mut tx, task, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn complete_with_next(
        &self,
        task: &Task,
        audit: &AuditRecord,
        next: &[(Task, BTreeSet<AnalystId>)],
        next_audits: &[AuditRecord],
        series: Option<&Recurrence>,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        update_task(&mut tx, task).await?;
        if let Some(s) = series {
            save_series(&mut tx, s).await?;
        }
        for (t, who) in next {
            insert_task(&mut tx, t, who).await?;
            live::publish(&mut tx, &LiveChange::new(t.tenant_id(), "task", t.id()))
                .await
                .map_err(db_err)?;
        }
        for a in next_audits {
            cooper_platform::audit::record(&mut tx, a)
                .await
                .map_err(db_err)?;
        }
        audit_and_publish(&mut tx, task, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn get(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Task>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            TaskRow,
            "SELECT t.id, t.tenant_id, t.title, t.description, t.status_id, s.is_closed, t.priority_id, t.start_date, t.due_date,
                    t.estimate_minutes, t.assigned_analyst_id, t.assigned_team_id, t.parent_task_id, t.tags, t.board_position,
                    t.recurrence_id, t.previous_occurrence_id, t.created_by_kind, t.created_by_id, t.created_at, t.updated_at,
                    t.completed_at
             FROM tasks t JOIN task_statuses s ON s.id = t.status_id
             WHERE t.id = $1 AND ($2 OR t.tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Task::try_from).transpose()
    }

    async fn list(&self, scope: &TenantScope, q: &TaskQuery) -> Result<Vec<Task>, RepoError> {
        let (all, tenants) = ids(scope);
        let (after_at, after_id) = q.after.unzip();
        let (link_kind, link_target) = q.linked.map(|(k, t)| (k.as_str(), t)).unzip();
        let search = q
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{}%", s.replace('%', "\\%").replace('_', "\\_")));
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            TaskRow,
            r#"SELECT t.id, t.tenant_id, t.title, t.description, t.status_id, s.is_closed, t.priority_id, t.start_date, t.due_date,
                      t.estimate_minutes, t.assigned_analyst_id, t.assigned_team_id, t.parent_task_id, t.tags, t.board_position,
                      t.recurrence_id, t.previous_occurrence_id, t.created_by_kind, t.created_by_id, t.created_at, t.updated_at,
                      t.completed_at
               FROM tasks t JOIN task_statuses s ON s.id = t.status_id
               WHERE ($1 OR t.tenant_id = ANY($2))
                 AND ($3::boolean IS NULL OR s.is_closed = NOT $3)
                 AND ($4::uuid IS NULL OR t.status_id = $4)
                 AND ($5::uuid IS NULL OR t.assigned_analyst_id = $5)
                 AND ($6::uuid IS NULL OR t.assigned_team_id = $6)
                 AND ($7::uuid IS NULL OR EXISTS (SELECT 1 FROM task_collaborators c WHERE c.task_id = t.id AND c.analyst_id = $7))
                 AND ($8::text IS NULL OR $8 = ANY(t.tags))
                 AND ($9::uuid IS NULL OR t.parent_task_id = $9)
                 AND (NOT $10 OR t.parent_task_id IS NULL)
                 AND ($11::date IS NULL OR t.due_date <= $11)
                 AND ($12::text IS NULL OR EXISTS (SELECT 1 FROM task_links l WHERE l.task_id = t.id AND l.kind = $12 AND l.target_id = $13))
                 AND ($14::text IS NULL OR t.title ILIKE $14)
                 AND ($15 OR $16::timestamptz IS NULL OR (t.updated_at, t.id) < ($16, $17::uuid))
               ORDER BY CASE WHEN $15 THEN t.board_position END, CASE WHEN $15 THEN t.id END,
                        t.updated_at DESC, t.id DESC
               LIMIT $18"#,
            all,
            &tenants,
            q.open,
            q.status_id,
            q.assigned_analyst.map(|a| a.as_uuid()),
            q.assigned_team.map(|a| a.as_uuid()),
            q.collaborator.map(|a| a.as_uuid()),
            q.tag,
            q.parent_id,
            q.top_level,
            q.due_by,
            link_kind,
            link_target,
            search,
            q.board_order,
            after_at,
            after_id,
            q.limit.fetch_size()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Task::try_from).collect()
    }

    async fn column(&self, tenant: TenantId, status: Uuid) -> Result<Vec<Uuid>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant))
            .await
            .map_err(db_err)?;
        let rows = sqlx::query_scalar!(
            "SELECT id FROM tasks WHERE tenant_id = $1 AND status_id = $2 AND parent_task_id IS NULL ORDER BY board_position, id",
            tenant.as_uuid(),
            status
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows)
    }

    async fn reposition(
        &self,
        task: &Task,
        positions: &[(Uuid, i64)],
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        update_task(&mut tx, task).await?;
        let (ids, pos): (Vec<Uuid>, Vec<i64>) = positions.iter().copied().unzip();
        sqlx::query!(
            "UPDATE tasks t SET board_position = p.pos FROM UNNEST($1::uuid[], $2::bigint[]) AS p(id, pos)
             WHERE t.id = p.id AND t.tenant_id = $3",
            &ids,
            &pos,
            task.tenant_id().as_uuid()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit_and_publish(&mut tx, task, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn collaborators(
        &self,
        scope: &TenantScope,
        task: Uuid,
    ) -> Result<BTreeSet<AnalystId>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_scalar!(
            "SELECT analyst_id FROM task_collaborators WHERE task_id = $1 AND ($2 OR tenant_id = ANY($3))",
            task,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(AnalystId::from_uuid).collect())
    }

    async fn set_collaborators(
        &self,
        task: &Task,
        who: &BTreeSet<AnalystId>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "DELETE FROM task_collaborators WHERE task_id = $1",
            task.id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for a in who {
            sqlx::query!(
                "INSERT INTO task_collaborators (tenant_id, task_id, analyst_id) VALUES ($1, $2, $3)",
                task.tenant_id().as_uuid(),
                task.id(),
                a.as_uuid()
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        touch(&mut tx, task).await?;
        audit_and_publish(&mut tx, task, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn comments(
        &self,
        scope: &TenantScope,
        task: Uuid,
    ) -> Result<Vec<TaskComment>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, author_id, body, created_at FROM task_comments WHERE task_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY created_at, id",
            task,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TaskComment {
                id: r.id,
                author_id: r.author_id.map(AnalystId::from_uuid),
                body: r.body,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn add_comment(
        &self,
        task: &Task,
        c: &TaskComment,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO task_comments (id, tenant_id, task_id, author_id, body, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
            c.id,
            task.tenant_id().as_uuid(),
            task.id(),
            c.author_id.map(|a| a.as_uuid()),
            c.body,
            c.created_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        touch(&mut tx, task).await?;
        audit_and_publish(&mut tx, task, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn time_entries(
        &self,
        scope: &TenantScope,
        task: Uuid,
    ) -> Result<Vec<TimeEntry>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, analyst_id, work_date, minutes, note, created_at FROM task_time_entries
             WHERE task_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY work_date, created_at, id",
            task,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TimeEntry {
                id: r.id,
                analyst_id: AnalystId::from_uuid(r.analyst_id),
                work_date: r.work_date,
                minutes: r.minutes,
                note: r.note,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn add_time(
        &self,
        task: &Task,
        e: &TimeEntry,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO task_time_entries (id, tenant_id, task_id, analyst_id, work_date, minutes, note, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            e.id,
            task.tenant_id().as_uuid(),
            task.id(),
            e.analyst_id.as_uuid(),
            e.work_date,
            e.minutes,
            e.note,
            e.created_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        touch(&mut tx, task).await?;
        audit_and_publish(&mut tx, task, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn delete_time(
        &self,
        task: &Task,
        entry: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "DELETE FROM task_time_entries WHERE id = $1 AND task_id = $2",
            entry,
            task.id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        touch(&mut tx, task).await?;
        audit_and_publish(&mut tx, task, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn links(&self, scope: &TenantScope, task: Uuid) -> Result<Vec<TaskLink>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT kind, target_id, linked_by_kind, linked_by_id, linked_at FROM task_links
             WHERE task_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY linked_at, target_id",
            task,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(TaskLink {
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
        task: &Task,
        l: &TaskLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO task_links (tenant_id, task_id, kind, target_id, linked_by_kind, linked_by_id, linked_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            task.tenant_id().as_uuid(),
            task.id(),
            l.kind.as_str(),
            l.target_id,
            l.linked_by.kind().as_str(),
            l.linked_by.id(),
            l.linked_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        touch(&mut tx, task).await?;
        audit_and_publish(&mut tx, task, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn remove_link(
        &self,
        task: &Task,
        kind: LinkKind,
        target: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "DELETE FROM task_links WHERE task_id = $1 AND kind = $2 AND target_id = $3",
            task.id(),
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
        touch(&mut tx, task).await?;
        audit_and_publish(&mut tx, task, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn recurrence(
        &self,
        scope: &TenantScope,
        id: Uuid,
    ) -> Result<Option<Recurrence>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            SeriesRow,
            "SELECT id, tenant_id, rule, occurrences_created, next_date, is_active, created_at, updated_at FROM task_recurrences
             WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Recurrence::try_from).transpose()
    }

    async fn save_recurrence(
        &self,
        task: &Task,
        r: &Recurrence,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(task.tenant_id()))
            .await
            .map_err(db_err)?;
        save_series(&mut tx, r).await?;
        sqlx::query!(
            "UPDATE tasks SET recurrence_id = $2, updated_at = now() WHERE id = $1",
            task.id(),
            task.recurrence_id()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit_and_publish(&mut tx, task, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn due_series(&self, today: NaiveDate) -> Result<Vec<Recurrence>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        let rows = sqlx::query_as!(
            SeriesRow,
            "SELECT id, tenant_id, rule, occurrences_created, next_date, is_active, created_at, updated_at FROM task_recurrences
             WHERE is_active AND next_date <= $1 ORDER BY next_date, id LIMIT 200",
            today
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Recurrence::try_from).collect()
    }

    async fn latest_occurrence(&self, s: &Recurrence) -> Result<Option<Task>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(s.tenant_id))
            .await
            .map_err(db_err)?;
        let row = sqlx::query_as!(
            TaskRow,
            "SELECT t.id, t.tenant_id, t.title, t.description, t.status_id, st.is_closed, t.priority_id, t.start_date, t.due_date,
                    t.estimate_minutes, t.assigned_analyst_id, t.assigned_team_id, t.parent_task_id, t.tags, t.board_position,
                    t.recurrence_id, t.previous_occurrence_id, t.created_by_kind, t.created_by_id, t.created_at, t.updated_at,
                    t.completed_at
             FROM tasks t JOIN task_statuses st ON st.id = t.status_id
             WHERE t.recurrence_id = $1 AND t.parent_task_id IS NULL ORDER BY t.created_at DESC, t.id DESC LIMIT 1",
            s.id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Task::try_from).transpose()
    }

    async fn history(
        &self,
        scope: &TenantScope,
        task: Uuid,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let h = cooper_platform::audit::history(&mut tx, "task", task)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        tx.commit().await.map_err(db_err)?;
        Ok(h)
    }

    async fn open_tally(
        &self,
        scope: &TenantScope,
        analyst: Option<AnalystId>,
        today: NaiveDate,
    ) -> Result<OpenTaskTally, RepoError> {
        let (all, tenants) = ids(scope);
        let analyst = analyst.map(|a| a.as_uuid());
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            r#"SELECT t.status_id, COUNT(*) AS "tasks!",
                      COUNT(*) FILTER (WHERE t.due_date < $4) AS "overdue!",
                      COUNT(*) FILTER (WHERE t.due_date = $4) AS "due_today!"
               FROM tasks t JOIN task_statuses s ON s.id = t.status_id
               WHERE ($1 OR t.tenant_id = ANY($2)) AND NOT s.is_closed AND t.parent_task_id IS NULL
                 AND ($3::uuid IS NULL OR t.assigned_analyst_id = $3)
               GROUP BY t.status_id"#,
            all,
            &tenants,
            analyst,
            today
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        let n = |v: i64| u64::try_from(v).unwrap_or(0);
        let mut tally = OpenTaskTally::default();
        for r in rows {
            tally.by_status.push((r.status_id, n(r.tasks)));
            tally.overdue += n(r.overdue);
            tally.due_today += n(r.due_today);
        }
        Ok(tally)
    }

    async fn time_tally(
        &self,
        scope: &TenantScope,
        group: TimeGrouping,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<TimeTally>, RepoError> {
        let (all, tenants) = ids(scope);
        let key = match group {
            TimeGrouping::Analyst => "analyst",
            TimeGrouping::Team => "team",
            TimeGrouping::Company => "company",
            TimeGrouping::Day => "day",
            TimeGrouping::Month => "month",
        };
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            r#"SELECT CASE $5 WHEN 'analyst' THEN e.analyst_id WHEN 'team' THEN t.assigned_team_id
                              WHEN 'company' THEN e.tenant_id END AS id,
                      CASE $5 WHEN 'day' THEN e.work_date
                              WHEN 'month' THEN date_trunc('month', e.work_date)::date END AS day,
                      COALESCE(SUM(e.minutes), 0)::bigint AS "minutes!", COUNT(*) AS "entries!"
               FROM task_time_entries e JOIN tasks t ON t.id = e.task_id
               WHERE ($1 OR e.tenant_id = ANY($2)) AND e.work_date >= $3 AND e.work_date <= $4
               GROUP BY 1, 2"#,
            all,
            &tenants,
            from,
            to,
            key
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TimeTally {
                id: r.id,
                date: r.day,
                minutes: u64::try_from(r.minutes).unwrap_or(0),
                entries: u64::try_from(r.entries).unwrap_or(0),
            })
            .collect())
    }
}

/// The `task-recurrence` Background Job: makes the occurrences of fixed-schedule series
/// that have come due (FR-TSK-006).
pub struct TaskRecurrenceJob {
    /// The service.
    pub service: crate::application::TaskService,
}

#[async_trait]
impl Job for TaskRecurrenceJob {
    fn name(&self) -> &'static str {
        "task-recurrence"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(3600)
    }

    async fn run(&self, _ctx: &JobContext) -> Result<Option<String>, JobError> {
        let n = self
            .service
            .run_schedules()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(format!("created {n} occurrences")))
    }
}
