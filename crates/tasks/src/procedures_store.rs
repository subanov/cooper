//! PostgreSQL storage of procedures (tenant-scoped, RLS).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditRecord, TenantId, TenantScope};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::ports::RepoError;
use crate::procedures::Procedure;
use crate::procedures_service::ProcedureRepository;

/// sqlx implementation of [`ProcedureRepository`].
#[derive(Clone)]
pub struct PgProcedureStore {
    pool: PgPool,
}

impl PgProcedureStore {
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

struct Row {
    id: Uuid,
    tenant_id: Uuid,
    task_id: Uuid,
    article_id: Uuid,
    article_version: i32,
    title: String,
    body: String,
    attached_by_kind: String,
    attached_by_id: Option<Uuid>,
    attached_at: DateTime<Utc>,
}

impl TryFrom<Row> for Procedure {
    type Error = RepoError;

    fn try_from(r: Row) -> Result<Self, RepoError> {
        Ok(Self {
            id: r.id,
            tenant_id: TenantId::from_uuid(r.tenant_id),
            task_id: r.task_id,
            article_id: r.article_id,
            article_version: r.article_version,
            title: r.title,
            body: r.body,
            attached_by: Actor::from_parts(&r.attached_by_kind, r.attached_by_id).map_err(|e| RepoError::Storage(e.to_string()))?,
            attached_at: r.attached_at,
        })
    }
}

#[async_trait]
impl ProcedureRepository for PgProcedureStore {
    async fn for_task(&self, scope: &TenantScope, task: Uuid) -> Result<Vec<Procedure>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            Row,
            "SELECT id, tenant_id, task_id, article_id, article_version, title, body, attached_by_kind, attached_by_id, attached_at
             FROM task_procedures WHERE task_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY attached_at, id",
            task,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Procedure::try_from).collect()
    }

    async fn get(&self, scope: &TenantScope, id: Uuid) -> Result<Option<Procedure>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            Row,
            "SELECT id, tenant_id, task_id, article_id, article_version, title, body, attached_by_kind, attached_by_id, attached_at
             FROM task_procedures WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Procedure::try_from).transpose()
    }

    async fn insert(&self, p: &Procedure, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(p.tenant_id)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO task_procedures (id, tenant_id, task_id, article_id, article_version, title, body,
                                          attached_by_kind, attached_by_id, attached_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            p.id,
            p.tenant_id.as_uuid(),
            p.task_id,
            p.article_id,
            p.article_version,
            p.title,
            p.body,
            p.attached_by.kind().as_str(),
            p.attached_by.id(),
            p.attached_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn remove(&self, p: &Procedure, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(p.tenant_id)).await.map_err(db_err)?;
        sqlx::query!("DELETE FROM task_procedures WHERE id = $1", p.id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }
}
