//! User Groups and Manager Grants (ADR-0025).

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditRecord, RequesterId, TenantId, TenantScope};
use cooper_platform::tenancy;
use uuid::Uuid;

use super::{PgIdentityStore, audit, db_err, stored};
use crate::domain::{
    CostCentreId, GrantKind, GroupRepository, ManagerGrant, Member, Membership, RepoError,
    RequesterSummary, UserGroup,
};

#[async_trait]
impl GroupRepository for PgIdentityStore {
    async fn groups(&self) -> Result<Vec<UserGroup>, RepoError> {
        let rows = sqlx::query!(
            "SELECT id, name, description, is_active, created_at, updated_at FROM user_groups ORDER BY lower(name)"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                UserGroup::restore(
                    r.id,
                    r.name,
                    r.description,
                    r.is_active,
                    r.created_at,
                    r.updated_at,
                )
            })
            .collect())
    }

    async fn save_group(&self, g: &UserGroup, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO user_groups (id, name, description, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, description = EXCLUDED.description,
                 is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            g.id(),
            g.name(),
            g.description(),
            g.is_active(),
            g.created_at(),
            g.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn members(&self, group: Uuid) -> Result<Vec<Membership>, RepoError> {
        let rows = sqlx::query!(
            "SELECT member_kind, member_id, expires_at FROM user_group_members WHERE group_id = $1 ORDER BY member_kind, member_id",
            group
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(Membership {
                    member: stored(Member::from_parts(&r.member_kind, r.member_id))?,
                    expires_at: r.expires_at,
                })
            })
            .collect()
    }

    async fn set_members(
        &self,
        group: Uuid,
        members: &[Membership],
        record: &AuditRecord,
        now: DateTime<Utc>,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!("DELETE FROM user_group_members WHERE group_id = $1", group)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for m in members {
            sqlx::query!(
                "INSERT INTO user_group_members (group_id, member_kind, member_id, expires_at, added_at) VALUES ($1, $2, $3, $4, $5)",
                group,
                m.member.kind(),
                m.member.id(),
                m.expires_at,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn requester_members(
        &self,
        now: DateTime<Utc>,
    ) -> Result<BTreeMap<Uuid, BTreeSet<RequesterId>>, RepoError> {
        let rows = sqlx::query!(
            "SELECT m.group_id, m.member_id FROM user_group_members m JOIN user_groups g ON g.id = m.group_id
             WHERE g.is_active AND m.member_kind = 'requester' AND (m.expires_at IS NULL OR m.expires_at > $1)",
            now
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let mut out: BTreeMap<Uuid, BTreeSet<RequesterId>> = BTreeMap::new();
        for r in rows {
            out.entry(r.group_id)
                .or_default()
                .insert(RequesterId::from_uuid(r.member_id));
        }
        Ok(out)
    }

    async fn grants(&self, tenant: TenantId) -> Result<Vec<ManagerGrant>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant))
            .await
            .map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id, manager_id, kind, target_id, is_exclusion, created_by_kind, created_by_id, created_at
             FROM manager_grants WHERE tenant_id = $1 ORDER BY created_at, id",
            tenant.as_uuid()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(ManagerGrant::restore(
                    r.id,
                    TenantId::from_uuid(r.tenant_id),
                    RequesterId::from_uuid(r.manager_id),
                    stored(GrantKind::from_parts(&r.kind, r.target_id))?,
                    r.is_exclusion,
                    stored(Actor::from_parts(&r.created_by_kind, r.created_by_id))?,
                    r.created_at,
                ))
            })
            .collect()
    }

    async fn add_grant(&self, g: &ManagerGrant, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(g.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO manager_grants (id, tenant_id, manager_id, kind, target_id, is_exclusion, created_by_kind, created_by_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            g.id(),
            g.tenant_id().as_uuid(),
            g.manager_id().as_uuid(),
            g.kind().as_str(),
            g.kind().target(),
            g.is_exclusion(),
            g.created_by().kind().as_str(),
            g.created_by().id(),
            g.created_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn remove_grant(&self, g: &ManagerGrant, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(g.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!("DELETE FROM manager_grants WHERE id = $1", g.id())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?
            .rows_affected();
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn company_requesters(
        &self,
        tenant: TenantId,
    ) -> Result<Vec<RequesterSummary>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant))
            .await
            .map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, manager_id, cost_centre_id, deactivated_at FROM requesters WHERE tenant_id = $1",
            tenant.as_uuid()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| RequesterSummary {
                id: RequesterId::from_uuid(r.id),
                manager_id: r.manager_id.map(RequesterId::from_uuid),
                cost_centre_id: r.cost_centre_id.map(CostCentreId::from_uuid),
                active: r.deactivated_at.is_none(),
            })
            .collect())
    }
}
