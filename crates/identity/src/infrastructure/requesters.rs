//! Requesters and Cost Centres (tenant-scoped; RLS plus explicit filters).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AuditRecord, RequesterId, TenantId, TenantScope};
use cooper_platform::tenancy;
use uuid::Uuid;

use super::companies::scope_params;
use super::{PgIdentityStore, audit, db_err, like_pattern, stored};
use crate::domain::{
    CostCentre, CostCentreCode, CostCentreId, CostCentreRepository, EmailAddress, ListQuery, Name,
    RepoError, Requester, RequesterRepository, Username,
};

struct RequesterRow {
    id: Uuid,
    tenant_id: Uuid,
    full_name: String,
    email: String,
    username: Option<String>,
    phone: Option<String>,
    manager_id: Option<Uuid>,
    cost_centre_id: Option<Uuid>,
    deactivated_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<RequesterRow> for Requester {
    type Error = RepoError;

    fn try_from(r: RequesterRow) -> Result<Self, RepoError> {
        Ok(Self::restore(
            RequesterId::from_uuid(r.id),
            TenantId::from_uuid(r.tenant_id),
            stored(Name::try_from(r.full_name))?,
            stored(EmailAddress::try_from(r.email))?,
            r.username
                .map(Username::try_from)
                .transpose()
                .map_err(|e| RepoError::Storage(e.to_string()))?,
            r.phone,
            r.manager_id.map(RequesterId::from_uuid),
            r.cost_centre_id.map(CostCentreId::from_uuid),
            r.deactivated_at,
            r.created_at,
            r.updated_at,
        ))
    }
}

#[async_trait]
impl RequesterRepository for PgIdentityStore {
    async fn get(
        &self,
        scope: &TenantScope,
        id: RequesterId,
    ) -> Result<Option<Requester>, RepoError> {
        let (all, ids) = scope_params(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            RequesterRow,
            "SELECT id, tenant_id, full_name, email, username, phone, manager_id, cost_centre_id,
                    deactivated_at, created_at, updated_at
             FROM requesters WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &ids
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Requester::try_from).transpose()
    }

    async fn list(
        &self,
        scope: &TenantScope,
        query: &ListQuery,
    ) -> Result<Vec<Requester>, RepoError> {
        let (all, ids) = scope_params(scope);
        let (after_name, after_id) = query.after.clone().unzip();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            RequesterRow,
            "SELECT id, tenant_id, full_name, email, username, phone, manager_id, cost_centre_id,
                    deactivated_at, created_at, updated_at
             FROM requesters
             WHERE ($1 OR tenant_id = ANY($2))
               AND ($3::uuid IS NULL OR tenant_id = $3)
               AND ($4 OR is_active)
               AND ($5::text IS NULL OR lower(full_name) LIKE $5 OR lower(email) LIKE $5 OR lower(username) LIKE $5)
               AND ($6::text IS NULL OR (lower(full_name), id) > ($6, $7))
             ORDER BY lower(full_name), id
             LIMIT $8",
            all,
            &ids,
            query.tenant_id.map(|t| t.as_uuid()),
            query.include_inactive,
            like_pattern(query.search.as_ref()),
            after_name,
            after_id,
            query.limit.fetch_size(),
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Requester::try_from).collect()
    }

    async fn manager_chain(
        &self,
        scope: &TenantScope,
        id: RequesterId,
        max: u32,
    ) -> Result<Vec<RequesterId>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_scalar!(
            r#"WITH RECURSIVE chain (id, manager_id, depth) AS (
                   SELECT id, manager_id, 0 FROM requesters WHERE id = $1
                   UNION ALL
                   SELECT r.id, r.manager_id, c.depth + 1
                   FROM requesters r JOIN chain c ON r.id = c.manager_id
                   WHERE c.depth < $2
               ) CYCLE id SET is_cycle USING path
               SELECT id AS "id!" FROM chain WHERE NOT is_cycle ORDER BY depth"#,
            id.as_uuid(),
            i32::try_from(max).unwrap_or(i32::MAX)
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(RequesterId::from_uuid).collect())
    }

    async fn insert(
        &self,
        scope: &TenantScope,
        r: &Requester,
        record: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO requesters (id, tenant_id, full_name, email, username, phone, manager_id,
                                     cost_centre_id, is_active, deactivated_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            r.id().as_uuid(),
            r.tenant_id().as_uuid(),
            r.full_name().as_str(),
            r.email().as_str(),
            r.username().map(Username::as_str),
            r.phone(),
            r.manager_id().map(|m| m.as_uuid()),
            r.cost_centre_id().map(|c| c.as_uuid()),
            r.is_active(),
            r.deactivated_at(),
            r.created_at(),
            r.updated_at(),
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(
        &self,
        scope: &TenantScope,
        r: &Requester,
        record: &AuditRecord,
    ) -> Result<(), RepoError> {
        let (all, ids) = scope_params(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE requesters SET full_name = $2, email = $3, username = $4, phone = $5, manager_id = $6,
                    cost_centre_id = $7, is_active = $8, deactivated_at = $9, updated_at = $10
             WHERE id = $1 AND tenant_id = $11 AND ($12 OR tenant_id = ANY($13))",
            r.id().as_uuid(),
            r.full_name().as_str(),
            r.email().as_str(),
            r.username().map(Username::as_str),
            r.phone(),
            r.manager_id().map(|m| m.as_uuid()),
            r.cost_centre_id().map(|c| c.as_uuid()),
            r.is_active(),
            r.deactivated_at(),
            r.updated_at(),
            r.tenant_id().as_uuid(),
            all,
            &ids,
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }
}

struct CostCentreRow {
    id: Uuid,
    tenant_id: Uuid,
    code: String,
    name: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<CostCentreRow> for CostCentre {
    type Error = RepoError;

    fn try_from(r: CostCentreRow) -> Result<Self, RepoError> {
        Ok(Self::restore(
            CostCentreId::from_uuid(r.id),
            TenantId::from_uuid(r.tenant_id),
            stored(CostCentreCode::try_from(r.code))?,
            stored(Name::try_from(r.name))?,
            r.is_active,
            r.created_at,
            r.updated_at,
        ))
    }
}

#[async_trait]
impl CostCentreRepository for PgIdentityStore {
    async fn get(
        &self,
        scope: &TenantScope,
        id: CostCentreId,
    ) -> Result<Option<CostCentre>, RepoError> {
        let (all, ids) = scope_params(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            CostCentreRow,
            "SELECT id, tenant_id, code, name, is_active, created_at, updated_at
             FROM cost_centres WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &ids
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(CostCentre::try_from).transpose()
    }

    async fn list(
        &self,
        scope: &TenantScope,
        query: &ListQuery,
    ) -> Result<Vec<CostCentre>, RepoError> {
        let (all, ids) = scope_params(scope);
        let (after_name, after_id) = query.after.clone().unzip();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            CostCentreRow,
            "SELECT id, tenant_id, code, name, is_active, created_at, updated_at
             FROM cost_centres
             WHERE ($1 OR tenant_id = ANY($2))
               AND ($3::uuid IS NULL OR tenant_id = $3)
               AND ($4 OR is_active)
               AND ($5::text IS NULL OR lower(name) LIKE $5 OR lower(code) LIKE $5)
               AND ($6::text IS NULL OR (lower(name), id) > ($6, $7))
             ORDER BY lower(name), id
             LIMIT $8",
            all,
            &ids,
            query.tenant_id.map(|t| t.as_uuid()),
            query.include_inactive,
            like_pattern(query.search.as_ref()),
            after_name,
            after_id,
            query.limit.fetch_size(),
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(CostCentre::try_from).collect()
    }

    async fn insert(
        &self,
        scope: &TenantScope,
        c: &CostCentre,
        record: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO cost_centres (id, tenant_id, code, name, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            c.id().as_uuid(),
            c.tenant_id().as_uuid(),
            c.code().as_str(),
            c.name().as_str(),
            c.is_active(),
            c.created_at(),
            c.updated_at(),
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(
        &self,
        scope: &TenantScope,
        c: &CostCentre,
        record: &AuditRecord,
    ) -> Result<(), RepoError> {
        let (all, ids) = scope_params(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE cost_centres SET code = $2, name = $3, is_active = $4, updated_at = $5
             WHERE id = $1 AND ($6 OR tenant_id = ANY($7))",
            c.id().as_uuid(),
            c.code().as_str(),
            c.name().as_str(),
            c.is_active(),
            c.updated_at(),
            all,
            &ids,
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }
}
