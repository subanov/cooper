//! PostgreSQL storage of Contract Terms (tabs are instance-wide; values are tenant-scoped).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AuditRecord, TenantId, TenantScope};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::ports::RepoError;
use crate::terms::{TermTab, TermValues};
use crate::terms_service::TermsRepository;
use cooper_kernel::Actor;

/// sqlx implementation of [`TermsRepository`].
#[derive(Clone)]
pub struct PgTermsStore {
    pool: PgPool,
}

impl PgTermsStore {
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

struct TabRow {
    id: Uuid,
    name: String,
    sort_order: i32,
    is_active: bool,
    created_at: DateTime<Utc>,
}

#[async_trait]
impl TermsRepository for PgTermsStore {
    async fn tabs(&self) -> Result<Vec<TermTab>, RepoError> {
        let rows = sqlx::query_as!(
            TabRow,
            "SELECT id, name, sort_order, is_active, created_at FROM contract_term_tabs ORDER BY sort_order, lower(name)"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TermTab {
                id: r.id,
                name: r.name,
                sort_order: r.sort_order,
                active: r.is_active,
                created_at: r.created_at,
                created_by: Actor::Automation,
            })
            .collect())
    }

    async fn save_tab(&self, tab: &TermTab, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO contract_term_tabs (id, name, sort_order, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $5)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, sort_order = EXCLUDED.sort_order,
                 is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            tab.id,
            tab.name,
            tab.sort_order,
            tab.active,
            tab.created_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        cooper_platform::audit::record(&mut tx, audit).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn values(&self, scope: &TenantScope, contract: Uuid) -> Result<Vec<(Uuid, String)>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT v.tab_id, v.value FROM contract_term_values v JOIN contract_term_tabs t ON t.id = v.tab_id
             WHERE v.contract_id = $1 AND ($2 OR v.tenant_id = ANY($3))
             ORDER BY t.sort_order, lower(t.name)",
            contract,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(|r| (r.tab_id, r.value)).collect())
    }

    async fn save_values(
        &self,
        tenant: TenantId,
        contract: Uuid,
        values: &TermValues,
        at: DateTime<Utc>,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        sqlx::query!("DELETE FROM contract_term_values WHERE contract_id = $1", contract)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for (tab, value) in &values.0 {
            sqlx::query!(
                "INSERT INTO contract_term_values (contract_id, tenant_id, tab_id, value, updated_at) VALUES ($1, $2, $3, $4, $5)",
                contract,
                tenant.as_uuid(),
                tab,
                value,
                at
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        cooper_platform::audit::record(&mut tx, audit).await.map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }
}
