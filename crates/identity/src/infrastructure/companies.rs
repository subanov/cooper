//! Companies, Freemail Domains and first-run setup.

use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cooper_kernel::{AuditRecord, TenantId, TenantScope};
use cooper_platform::db::Tx;
use cooper_platform::tenancy;
use uuid::Uuid;

use super::{PgIdentityStore, audit, db_err, like_pattern, stored};
use crate::domain::{
    Analyst, Company, CompanyRepository, DomainName, ListQuery, Name, RepoError, SetupRepository,
    TicketCode,
};

/// `(all, ids)` parameters for the explicit tenant filter that accompanies RLS.
pub(crate) fn scope_params(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|id| id.as_uuid()).collect()),
    }
}

struct CompanyRow {
    id: Uuid,
    name: String,
    ticket_code: String,
    is_default: bool,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    domains: Vec<String>,
}

impl TryFrom<CompanyRow> for Company {
    type Error = RepoError;

    fn try_from(r: CompanyRow) -> Result<Self, RepoError> {
        let domains = r
            .domains
            .into_iter()
            .map(|d| stored(DomainName::try_from(d)))
            .collect::<Result<BTreeSet<_>, _>>()?;
        Ok(Self::restore(
            TenantId::from_uuid(r.id),
            stored(Name::try_from(r.name))?,
            stored(TicketCode::try_from(r.ticket_code))?,
            domains,
            r.is_default,
            r.is_active,
            r.created_at,
            r.updated_at,
        ))
    }
}

async fn write_domains(tx: &mut Tx, company: &Company) -> Result<(), RepoError> {
    let id = company.id().as_uuid();
    let domains: Vec<String> = company
        .sender_domains()
        .iter()
        .map(|d| d.as_str().to_owned())
        .collect();
    sqlx::query!(
        "DELETE FROM tenant_domains WHERE tenant_id = $1 AND NOT (domain = ANY($2))",
        id,
        &domains
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    let ids: Vec<Uuid> = domains.iter().map(|_| Uuid::now_v7()).collect();
    // A domain owned by another Company violates tenant_domains_domain_key (a conflict).
    sqlx::query!(
        "INSERT INTO tenant_domains (id, tenant_id, domain, created_at)
         SELECT d.new_id, $2, d.domain, $3 FROM UNNEST($1::uuid[], $4::text[]) AS d(new_id, domain)
         WHERE NOT EXISTS (SELECT 1 FROM tenant_domains x WHERE x.tenant_id = $2 AND x.domain = d.domain)",
        &ids,
        id,
        company.updated_at(),
        &domains
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn insert_company(tx: &mut Tx, company: &Company) -> Result<(), RepoError> {
    sqlx::query!(
        "INSERT INTO tenants (id, name, ticket_code, is_default, is_active, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        company.id().as_uuid(),
        company.name().as_str(),
        company.ticket_code().as_str(),
        company.is_default(),
        company.is_active(),
        company.created_at(),
        company.updated_at(),
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    write_domains(tx, company).await
}

#[async_trait]
impl CompanyRepository for PgIdentityStore {
    async fn get(&self, scope: &TenantScope, id: TenantId) -> Result<Option<Company>, RepoError> {
        let (all, ids) = scope_params(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            CompanyRow,
            r#"SELECT t.id, t.name, t.ticket_code, t.is_default, t.is_active, t.created_at, t.updated_at,
                      COALESCE(ARRAY(SELECT d.domain FROM tenant_domains d WHERE d.tenant_id = t.id ORDER BY d.domain), '{}') AS "domains!"
               FROM tenants t
               WHERE t.id = $1 AND ($2 OR t.id = ANY($3))"#,
            id.as_uuid(),
            all,
            &ids
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Company::try_from).transpose()
    }

    async fn list(
        &self,
        scope: &TenantScope,
        query: &ListQuery,
    ) -> Result<Vec<Company>, RepoError> {
        let (all, ids) = scope_params(scope);
        let (after_name, after_id) = query.after.clone().unzip();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            CompanyRow,
            r#"SELECT t.id, t.name, t.ticket_code, t.is_default, t.is_active, t.created_at, t.updated_at,
                      COALESCE(ARRAY(SELECT d.domain FROM tenant_domains d WHERE d.tenant_id = t.id ORDER BY d.domain), '{}') AS "domains!"
               FROM tenants t
               WHERE ($1 OR t.id = ANY($2))
                 AND ($3 OR t.is_active)
                 AND ($4::text IS NULL OR lower(t.name) LIKE $4 OR lower(t.ticket_code) LIKE $4)
                 AND ($5::text IS NULL OR (lower(t.name), t.id) > ($5, $6))
               ORDER BY lower(t.name), t.id
               LIMIT $7"#,
            all,
            &ids,
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
        rows.into_iter().map(Company::try_from).collect()
    }

    async fn freemail_domains(&self) -> Result<BTreeSet<String>, RepoError> {
        let rows = sqlx::query_scalar!("SELECT domain FROM freemail_domains")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().collect())
    }

    async fn find_by_sender_domain(&self, domain: &str) -> Result<Option<TenantId>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        let id = sqlx::query_scalar!(
            "SELECT d.tenant_id FROM tenant_domains d JOIN tenants t ON t.id = d.tenant_id
             WHERE d.domain = lower($1) AND t.is_active
               AND NOT EXISTS (SELECT 1 FROM freemail_domains f WHERE f.domain = lower($1))",
            domain
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(id.map(TenantId::from_uuid))
    }

    async fn insert(&self, company: &Company, record: &AuditRecord) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        insert_company(&mut tx, company).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(&self, company: &Company, record: &AuditRecord) -> Result<(), RepoError> {
        let scope = TenantScope::single(company.id());
        let mut tx = tenancy::begin(&self.pool, &scope).await.map_err(db_err)?;
        let result = sqlx::query!(
            "UPDATE tenants SET name = $2, ticket_code = $3, is_active = $4, updated_at = $5 WHERE id = $1",
            company.id().as_uuid(),
            company.name().as_str(),
            company.ticket_code().as_str(),
            company.is_active(),
            company.updated_at(),
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        write_domains(&mut tx, company).await?;
        audit(&mut tx, record).await?;
        tx.commit().await.map_err(db_err)
    }
}

/// Serialises concurrent first-run setups.
const SETUP_LOCK_KEY: i64 = 0x636f_6f70_6572_0001;

#[async_trait]
impl SetupRepository for PgIdentityStore {
    async fn is_required(&self) -> Result<bool, RepoError> {
        let exists = sqlx::query_scalar!(r#"SELECT EXISTS (SELECT 1 FROM analysts) AS "exists!""#)
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(!exists)
    }

    async fn complete(
        &self,
        company: &Company,
        company_audit: &AuditRecord,
        admin: &Analyst,
        admin_audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", SETUP_LOCK_KEY)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let done = sqlx::query_scalar!(r#"SELECT EXISTS (SELECT 1 FROM analysts) AS "exists!""#)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        if done {
            return Err(RepoError::Conflict("setup".into()));
        }
        insert_company(&mut tx, company).await?;
        super::people::insert_analyst(&mut tx, &self.keyring, admin).await?;
        audit(&mut tx, company_audit).await?;
        audit(&mut tx, admin_audit).await?;
        tx.commit().await.map_err(db_err)
    }
}
