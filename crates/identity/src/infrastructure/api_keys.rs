//! API Keys.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, DurationRound as _, TimeDelta, Utc};
use cooper_kernel::{AnalystId, ApiKeyId, AuditRecord, Capability, TenantId};
use cooper_platform::db::Tx;
use uuid::Uuid;

use super::{PgIdentityStore, audit, db_err, like_pattern, stored};
use crate::domain::{
    ApiKey, ApiKeyRepository, ApiKeySettings, CompanyAccess, ListQuery, Name, RepoError,
};

struct ApiKeyRow {
    id: Uuid,
    name: String,
    prefix: String,
    key_hash: Vec<u8>,
    capabilities: Vec<String>,
    all_companies: bool,
    companies: Vec<Uuid>,
    rate_limit_per_minute: i32,
    expires_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ApiKeyRow> for ApiKey {
    type Error = RepoError;

    fn try_from(r: ApiKeyRow) -> Result<Self, RepoError> {
        let capabilities: BTreeSet<Capability> = r
            .capabilities
            .into_iter()
            .map(|c| stored(c.parse()))
            .collect::<Result<_, _>>()?;
        let company_access = if r.all_companies {
            CompanyAccess::All
        } else {
            CompanyAccess::companies(r.companies.into_iter().map(TenantId::from_uuid).collect())
        };
        Ok(Self::restore(
            ApiKeyId::from_uuid(r.id),
            r.prefix,
            r.key_hash,
            ApiKeySettings {
                name: stored(Name::try_from(r.name))?,
                capabilities,
                company_access,
                rate_limit_per_minute: u32::try_from(r.rate_limit_per_minute).unwrap_or(1),
                expires_at: r.expires_at,
            },
            r.created_by.map(AnalystId::from_uuid),
            r.last_used_at,
            r.revoked_at,
            r.created_at,
            r.updated_at,
        ))
    }
}

fn columns(k: &ApiKey) -> (Vec<String>, bool, Vec<Uuid>, i32) {
    let s = k.settings();
    let caps = s
        .capabilities
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect();
    let (all, companies) = match &s.company_access {
        CompanyAccess::All => (true, Vec::new()),
        CompanyAccess::Companies(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
        CompanyAccess::None => (false, Vec::new()),
    };
    (
        caps,
        all,
        companies,
        i32::try_from(s.rate_limit_per_minute).unwrap_or(i32::MAX),
    )
}

async fn write_companies(tx: &mut Tx, id: Uuid, companies: &[Uuid]) -> Result<(), RepoError> {
    sqlx::query!("DELETE FROM api_key_companies WHERE api_key_id = $1", id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    sqlx::query!(
        "INSERT INTO api_key_companies (api_key_id, tenant_id) SELECT $1, t FROM UNNEST($2::uuid[]) AS t",
        id,
        companies
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[async_trait]
impl ApiKeyRepository for PgIdentityStore {
    async fn get(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepoError> {
        sqlx::query_as!(
            ApiKeyRow,
            r#"SELECT k.id, k.name, k.prefix, k.key_hash, k.capabilities, k.all_companies,
                      ARRAY(SELECT c.tenant_id FROM api_key_companies c WHERE c.api_key_id = k.id) AS "companies!",
                      k.rate_limit_per_minute, k.expires_at, k.last_used_at, k.revoked_at, k.created_by,
                      k.created_at, k.updated_at
               FROM api_keys k WHERE k.id = $1"#,
            id.as_uuid()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(ApiKey::try_from)
        .transpose()
    }

    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, RepoError> {
        sqlx::query_as!(
            ApiKeyRow,
            r#"SELECT k.id, k.name, k.prefix, k.key_hash, k.capabilities, k.all_companies,
                      ARRAY(SELECT c.tenant_id FROM api_key_companies c WHERE c.api_key_id = k.id) AS "companies!",
                      k.rate_limit_per_minute, k.expires_at, k.last_used_at, k.revoked_at, k.created_by,
                      k.created_at, k.updated_at
               FROM api_keys k WHERE k.prefix = $1"#,
            prefix
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(ApiKey::try_from)
        .transpose()
    }

    async fn list(&self, query: &ListQuery) -> Result<Vec<ApiKey>, RepoError> {
        let (after_name, after_id) = query.after.clone().unzip();
        sqlx::query_as!(
            ApiKeyRow,
            r#"SELECT k.id, k.name, k.prefix, k.key_hash, k.capabilities, k.all_companies,
                      ARRAY(SELECT c.tenant_id FROM api_key_companies c WHERE c.api_key_id = k.id) AS "companies!",
                      k.rate_limit_per_minute, k.expires_at, k.last_used_at, k.revoked_at, k.created_by,
                      k.created_at, k.updated_at
               FROM api_keys k
               WHERE ($1 OR k.revoked_at IS NULL)
                 AND ($2::text IS NULL OR lower(k.name) LIKE $2 OR k.prefix LIKE $2)
                 AND ($3::text IS NULL OR (lower(k.name), k.id) > ($3, $4))
               ORDER BY lower(k.name), k.id
               LIMIT $5"#,
            query.include_inactive,
            like_pattern(query.search.as_ref()),
            after_name,
            after_id,
            query.limit.fetch_size()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(ApiKey::try_from)
        .collect()
    }

    async fn insert(&self, k: &ApiKey, record: &AuditRecord) -> Result<(), RepoError> {
        let (caps, all, companies, limit) = columns(k);
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO api_keys (id, name, prefix, key_hash, capabilities, all_companies, rate_limit_per_minute,
                                   expires_at, last_used_at, revoked_at, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            k.id().as_uuid(),
            k.settings().name.as_str(),
            k.prefix(),
            k.key_hash(),
            &caps,
            all,
            limit,
            k.settings().expires_at,
            k.last_used_at(),
            k.revoked_at(),
            k.created_by().map(|a| a.as_uuid()),
            k.created_at(),
            k.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        write_companies(&mut tx, k.id().as_uuid(), &companies).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(&self, k: &ApiKey, record: &AuditRecord) -> Result<(), RepoError> {
        let (caps, all, companies, limit) = columns(k);
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE api_keys SET name = $2, capabilities = $3, all_companies = $4, rate_limit_per_minute = $5,
                    expires_at = $6, revoked_at = $7, updated_at = $8
             WHERE id = $1",
            k.id().as_uuid(),
            k.settings().name.as_str(),
            &caps,
            all,
            limit,
            k.settings().expires_at,
            k.revoked_at(),
            k.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        write_companies(&mut tx, k.id().as_uuid(), &companies).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn record_use(&self, id: ApiKeyId, now: DateTime<Utc>) -> Result<u32, RepoError> {
        let window = now.duration_trunc(TimeDelta::minutes(1)).unwrap_or(now);
        let requests = sqlx::query_scalar!(
            "INSERT INTO api_key_usage (api_key_id, window_start, requests) VALUES ($1, $2, 1)
             ON CONFLICT (api_key_id, window_start) DO UPDATE SET requests = api_key_usage.requests + 1
             RETURNING requests",
            id.as_uuid(),
            window
        )
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        sqlx::query!(
            "UPDATE api_keys SET last_used_at = $2
             WHERE id = $1 AND (last_used_at IS NULL OR last_used_at < $2::timestamptz - interval '1 minute')",
            id.as_uuid(),
            now
        )
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(u32::try_from(requests).unwrap_or(u32::MAX))
    }
}
