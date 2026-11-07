//! PostgreSQL storage of Problem Management (tenant-scoped: explicit filter plus RLS).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, EventEnvelope, TenantId, TenantScope, TicketId,
};
use cooper_platform::db::Tx;
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    HistoryEntry, LinkedTicket, Lookup, LookupKind, Problem, ProblemDetails, ProblemNote,
    ProblemQuery, ProblemRepository, RepoError,
};

/// sqlx implementation of [`ProblemRepository`].
#[derive(Clone)]
pub struct PgProblemStore {
    pool: PgPool,
}

impl PgProblemStore {
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

fn ids(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
    }
}

async fn record(
    tx: &mut Tx,
    audit: &AuditRecord,
    events: &[EventEnvelope],
) -> Result<(), RepoError> {
    if audit.is_meaningful() {
        cooper_platform::audit::record(tx, audit)
            .await
            .map_err(db_err)?;
    }
    for event in events {
        cooper_platform::outbox::append(tx, event)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

struct ProblemRow {
    id: Uuid,
    tenant_id: Uuid,
    problem_number: String,
    title: String,
    description: String,
    status_id: Uuid,
    priority_id: Uuid,
    assigned_analyst_id: Option<Uuid>,
    root_cause: String,
    workaround: String,
    is_known_error: bool,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

impl TryFrom<ProblemRow> for Problem {
    type Error = RepoError;
    fn try_from(r: ProblemRow) -> Result<Self, RepoError> {
        Ok(Self::restore(
            r.id,
            TenantId::from_uuid(r.tenant_id),
            r.problem_number,
            ProblemDetails {
                title: r.title,
                description: r.description,
                priority_id: r.priority_id,
                root_cause: r.root_cause,
                workaround: r.workaround,
                is_known_error: r.is_known_error,
            },
            r.status_id,
            r.assigned_analyst_id.map(AnalystId::from_uuid),
            Actor::from_parts(&r.created_by_kind, r.created_by_id)
                .map_err(|e| RepoError::Storage(e.to_string()))?,
            r.created_at,
            r.updated_at,
            r.closed_at,
        ))
    }
}

#[async_trait]
impl ProblemRepository for PgProblemStore {
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
        let rows = match kind {
            LookupKind::Status => sqlx::query!(
                "SELECT id, key, name, is_closed, is_default, sort_order, is_active, created_at, updated_at FROM problem_statuses"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| (r.id, r.key, r.name, r.is_closed, r.is_default, r.sort_order, r.is_active, r.created_at, r.updated_at))
            .collect::<Vec<_>>(),
            LookupKind::Priority => sqlx::query!(
                "SELECT id, key, name, is_default, sort_order, is_active, created_at, updated_at FROM problem_priorities"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| (r.id, r.key, r.name, false, r.is_default, r.sort_order, r.is_active, r.created_at, r.updated_at))
            .collect(),
        };
        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    key,
                    name,
                    is_closed,
                    is_default,
                    sort_order,
                    is_active,
                    created_at,
                    updated_at,
                )| Lookup {
                    id,
                    kind,
                    key,
                    name,
                    is_closed,
                    is_default,
                    sort_order,
                    is_active,
                    created_at,
                    updated_at,
                },
            )
            .collect())
    }

    async fn save_lookup(&self, l: &Lookup, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        match l.kind {
            LookupKind::Status => {
                if l.is_default {
                    sqlx::query!("UPDATE problem_statuses SET is_default = false WHERE is_default AND id <> $1", l.id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db_err)?;
                }
                sqlx::query!(
                    "INSERT INTO problem_statuses (id, key, name, is_closed, is_default, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_closed = EXCLUDED.is_closed,
                         is_default = EXCLUDED.is_default, sort_order = EXCLUDED.sort_order,
                         is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id, l.key, l.name, l.is_closed, l.is_default, l.sort_order, l.is_active, l.created_at, l.updated_at
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
            LookupKind::Priority => {
                if l.is_default {
                    sqlx::query!("UPDATE problem_priorities SET is_default = false WHERE is_default AND id <> $1", l.id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db_err)?;
                }
                sqlx::query!(
                    "INSERT INTO problem_priorities (id, key, name, is_default, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_default = EXCLUDED.is_default,
                         sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id, l.key, l.name, l.is_default, l.sort_order, l.is_active, l.created_at, l.updated_at
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
        }
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn next_sequence(&self) -> Result<u64, RepoError> {
        let v = sqlx::query_scalar!(r#"SELECT nextval('problem_number_seq') AS "v!""#)
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        u64::try_from(v).map_err(|_| RepoError::Storage("negative sequence".into()))
    }

    async fn insert(
        &self,
        p: &Problem,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let d = p.details();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(p.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO problems (id, tenant_id, problem_number, title, description, status_id, priority_id, assigned_analyst_id,
                                   root_cause, workaround, is_known_error, created_by_kind, created_by_id, created_at, updated_at, closed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
            p.id(),
            p.tenant_id().as_uuid(),
            p.number(),
            d.title,
            d.description,
            p.status_id(),
            d.priority_id,
            p.assigned_analyst_id().map(|a| a.as_uuid()),
            d.root_cause,
            d.workaround,
            d.is_known_error,
            p.created_by().kind().as_str(),
            p.created_by().id(),
            p.created_at(),
            p.updated_at(),
            p.closed_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        record(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(
        &self,
        p: &Problem,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let d = p.details();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(p.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "UPDATE problems SET title = $2, description = $3, status_id = $4, priority_id = $5, assigned_analyst_id = $6,
                    root_cause = $7, workaround = $8, is_known_error = $9, updated_at = $10, closed_at = $11
             WHERE id = $1",
            p.id(),
            d.title,
            d.description,
            p.status_id(),
            d.priority_id,
            p.assigned_analyst_id().map(|a| a.as_uuid()),
            d.root_cause,
            d.workaround,
            d.is_known_error,
            p.updated_at(),
            p.closed_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        record(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn get(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Problem>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            ProblemRow,
            "SELECT id, tenant_id, problem_number, title, description, status_id, priority_id, assigned_analyst_id, root_cause,
                    workaround, is_known_error, created_by_kind, created_by_id, created_at, updated_at, closed_at
             FROM problems WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Problem::try_from).transpose()
    }

    async fn list(&self, scope: &TenantScope, q: &ProblemQuery) -> Result<Vec<Problem>, RepoError> {
        let (all, tenants) = ids(scope);
        let (after_at, after_id) = q.after.map_or((None, None), |(a, i)| (Some(a), Some(i)));
        let search = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let limit = q.limit.fetch_size();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ProblemRow,
            "SELECT id, tenant_id, problem_number, title, description, status_id, priority_id, assigned_analyst_id, root_cause,
                    workaround, is_known_error, created_by_kind, created_by_id, created_at, updated_at, closed_at
             FROM problems p
             WHERE ($1 OR tenant_id = ANY($2))
               AND ($3::boolean IS NULL OR ($3 = (closed_at IS NULL)))
               AND ($4::boolean IS NULL OR is_known_error = $4)
               AND ($5::uuid IS NULL OR assigned_analyst_id = $5)
               AND ($6::uuid IS NULL OR EXISTS (SELECT 1 FROM problem_tickets t WHERE t.problem_id = p.id AND t.ticket_id = $6))
               AND ($7::text IS NULL OR problem_number ILIKE $7 || '%' OR search_vector @@ websearch_to_tsquery('simple', cooper_unaccent($7)))
               AND ($8::timestamptz IS NULL OR (updated_at, id) < ($8, $9::uuid))
             ORDER BY updated_at DESC, id DESC
             LIMIT $10",
            all,
            &tenants,
            q.open,
            q.known_error,
            q.analyst_id.map(|a| a.as_uuid()),
            q.ticket_id.map(|t| t.as_uuid()),
            search,
            after_at,
            after_id,
            limit
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Problem::try_from).collect()
    }

    async fn add_note(
        &self,
        p: &Problem,
        n: &ProblemNote,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(p.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO problem_notes (id, tenant_id, problem_id, author_id, body, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
            n.id,
            n.tenant_id.as_uuid(),
            n.problem_id,
            n.author_id.map(|a| a.as_uuid()),
            n.body,
            n.created_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        touch(&mut tx, p).await?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn notes(
        &self,
        scope: &TenantScope,
        problem: Uuid,
    ) -> Result<Vec<ProblemNote>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id, problem_id, author_id, body, created_at FROM problem_notes
             WHERE problem_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY created_at, id",
            problem,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| ProblemNote {
                id: r.id,
                tenant_id: TenantId::from_uuid(r.tenant_id),
                problem_id: r.problem_id,
                author_id: r.author_id.map(AnalystId::from_uuid),
                body: r.body,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn tickets(
        &self,
        scope: &TenantScope,
        problem: Uuid,
    ) -> Result<Vec<LinkedTicket>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT ticket_id, linked_by_kind, linked_by_id, linked_at FROM problem_tickets
             WHERE problem_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY linked_at, ticket_id",
            problem,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(LinkedTicket {
                    ticket_id: TicketId::from_uuid(r.ticket_id),
                    linked_by: Actor::from_parts(&r.linked_by_kind, r.linked_by_id)
                        .map_err(|e| RepoError::Storage(e.to_string()))?,
                    linked_at: r.linked_at,
                })
            })
            .collect()
    }

    async fn link(
        &self,
        p: &Problem,
        l: &LinkedTicket,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(p.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO problem_tickets (tenant_id, problem_id, ticket_id, linked_by_kind, linked_by_id, linked_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            p.tenant_id().as_uuid(),
            p.id(),
            l.ticket_id.as_uuid(),
            l.linked_by.kind().as_str(),
            l.linked_by.id(),
            l.linked_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        touch(&mut tx, p).await?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn unlink(
        &self,
        p: &Problem,
        ticket: TicketId,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(p.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "DELETE FROM problem_tickets WHERE problem_id = $1 AND ticket_id = $2",
            p.id(),
            ticket.as_uuid()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        touch(&mut tx, p).await?;
        record(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn history(
        &self,
        scope: &TenantScope,
        problem: Uuid,
    ) -> Result<Vec<HistoryEntry>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let entries = cooper_platform::audit::history(&mut tx, "problem", problem)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        tx.commit().await.map_err(db_err)?;
        Ok(entries
            .into_iter()
            .map(|e| HistoryEntry {
                occurred_at: e.occurred_at,
                actor: e.actor,
                action: e.action,
                field: e.field,
                old_value: e.old_value,
                new_value: e.new_value,
            })
            .collect())
    }
}

async fn touch(tx: &mut Tx, p: &Problem) -> Result<(), RepoError> {
    sqlx::query!(
        "UPDATE problems SET updated_at = $2 WHERE id = $1",
        p.id(),
        p.updated_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}
