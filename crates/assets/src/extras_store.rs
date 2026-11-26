//! PostgreSQL storage of the Asset extras (ADR-0041). Tenant-scoped tables use RLS; the
//! custom field definitions and Field Sets are instance-wide reference data.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AuditRecord, TenantId, TenantScope};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::custom_fields::{CustomKind, FieldDefinition, FieldSet};
use crate::extras_ports::{ExtrasRepoError, ExtrasRepository, FoundAsset, Installation, LicenceRecord};
use crate::inventory::{LicenceStatus, SoftwareItem};

/// sqlx implementation of [`ExtrasRepository`].
#[derive(Clone)]
pub struct PgExtrasStore {
    pool: PgPool,
}

impl PgExtrasStore {
    /// Creates the store.
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn db_err(err: sqlx::Error) -> ExtrasRepoError {
    if let sqlx::Error::Database(db) = &err {
        let constraint = db.constraint().unwrap_or_default().to_owned();
        match db.code().as_deref() {
            Some("23505") => return ExtrasRepoError::Conflict(constraint),
            Some("23503") => return ExtrasRepoError::MissingReference(constraint),
            _ => {}
        }
    }
    ExtrasRepoError::Storage(err.to_string())
}

fn corrupt(e: impl std::fmt::Display) -> ExtrasRepoError {
    ExtrasRepoError::Storage(format!("invalid stored value: {e}"))
}

fn ids(scope: &TenantScope) -> (bool, Vec<Uuid>) {
    match scope {
        TenantScope::All => (true, Vec::new()),
        TenantScope::Tenants(ids) => (false, ids.iter().map(|t| t.as_uuid()).collect()),
    }
}

async fn write(
    tx: &mut cooper_platform::db::Tx,
    audit: &AuditRecord,
) -> Result<(), ExtrasRepoError> {
    cooper_platform::audit::record(tx, audit).await.map_err(db_err)
}

struct SetRow {
    id: Uuid,
    name: String,
    is_active: bool,
}

struct TypeRow {
    field_set_id: Uuid,
    asset_type_id: Uuid,
}

struct FieldRow {
    id: Uuid,
    field_set_id: Uuid,
    field_key: String,
    label: String,
    kind: String,
    options: Vec<String>,
    is_required: bool,
    is_unique: bool,
    is_searchable: bool,
    show_in_list: bool,
    is_active: bool,
}

fn field_of(r: FieldRow) -> Result<(Uuid, FieldDefinition), ExtrasRepoError> {
    let kind = CustomKind::parse(&r.kind).map_err(corrupt)?;
    Ok((
        r.field_set_id,
        FieldDefinition {
            id: r.id,
            key: r.field_key,
            label: r.label,
            kind,
            options: r.options,
            required: r.is_required,
            unique: r.is_unique,
            searchable: r.is_searchable,
            show_in_list: r.show_in_list,
            active: r.is_active,
        },
    ))
}

struct LicenceRow {
    id: Uuid,
    tenant_id: Uuid,
    licence_type: String,
    key_hint: Option<String>,
    quantity: i32,
    renewal_date: Option<NaiveDate>,
    notice_period_days: Option<i32>,
    cost_minor: Option<i64>,
    currency: Option<String>,
    status: String,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<LicenceRow> for LicenceRecord {
    type Error = ExtrasRepoError;

    fn try_from(r: LicenceRow) -> Result<Self, ExtrasRepoError> {
        let cost = match (r.cost_minor, r.currency) {
            (Some(minor), Some(c)) => Some((minor, c)),
            _ => None,
        };
        Ok(Self {
            id: r.id,
            tenant_id: TenantId::from_uuid(r.tenant_id),
            licence_type: r.licence_type,
            key_hint: r.key_hint,
            quantity: u32::try_from(r.quantity).map_err(corrupt)?,
            renewal_date: r.renewal_date,
            notice_period_days: r.notice_period_days.map(u32::try_from).transpose().map_err(corrupt)?,
            cost,
            status: LicenceStatus::parse(&r.status).map_err(corrupt)?,
            created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id).map_err(corrupt)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

#[async_trait]
impl ExtrasRepository for PgExtrasStore {
    async fn field_sets(&self) -> Result<Vec<FieldSet>, ExtrasRepoError> {
        let sets = sqlx::query_as!(SetRow, "SELECT id, name, is_active FROM asset_field_sets ORDER BY sort_order, lower(name)")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let types = sqlx::query_as!(TypeRow, "SELECT field_set_id, asset_type_id FROM asset_field_set_types")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let fields = sqlx::query_as!(
            FieldRow,
            "SELECT id, field_set_id, field_key, label, kind, options, is_required, is_unique, is_searchable, show_in_list, is_active
             FROM asset_field_definitions ORDER BY sort_order, field_key"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let mut by_set: BTreeMap<Uuid, Vec<FieldDefinition>> = BTreeMap::new();
        for f in fields {
            let (set, def) = field_of(f)?;
            by_set.entry(set).or_default().push(def);
        }
        let mut type_by_set: BTreeMap<Uuid, BTreeSet<Uuid>> = BTreeMap::new();
        for t in types {
            type_by_set.entry(t.field_set_id).or_default().insert(t.asset_type_id);
        }
        sets.into_iter()
            .map(|s| {
                let mut set = FieldSet::new(s.id, &s.name, type_by_set.remove(&s.id).unwrap_or_default())
                    .map_err(corrupt)?;
                set.fields = by_set.remove(&s.id).unwrap_or_default();
                set.active = s.is_active;
                Ok(set)
            })
            .collect()
    }

    async fn field_set(&self, id: Uuid) -> Result<Option<FieldSet>, ExtrasRepoError> {
        Ok(self.field_sets().await?.into_iter().find(|s| s.id == id))
    }

    async fn save_field_set(&self, set: &FieldSet, audit: &AuditRecord) -> Result<(), ExtrasRepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO asset_field_sets (id, name, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, now(), now())
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_active = EXCLUDED.is_active, updated_at = now()",
            set.id,
            set.name,
            set.active
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query!("DELETE FROM asset_field_set_types WHERE field_set_id = $1", set.id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for t in &set.asset_type_ids {
            sqlx::query!(
                "INSERT INTO asset_field_set_types (field_set_id, asset_type_id) VALUES ($1, $2)",
                set.id,
                t
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        write(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn save_field(&self, set_id: Uuid, f: &FieldDefinition, audit: &AuditRecord) -> Result<(), ExtrasRepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let kind = f.kind.as_str();
        sqlx::query!(
            "INSERT INTO asset_field_definitions (id, field_set_id, field_key, label, kind, options, is_required, is_unique,
                                                  is_searchable, show_in_list, is_active)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET label = EXCLUDED.label, is_required = EXCLUDED.is_required,
                 is_unique = EXCLUDED.is_unique, is_searchable = EXCLUDED.is_searchable,
                 show_in_list = EXCLUDED.show_in_list, is_active = EXCLUDED.is_active",
            f.id,
            set_id,
            f.key,
            f.label,
            kind,
            &f.options,
            f.required,
            f.unique,
            f.searchable,
            f.show_in_list,
            f.active
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        write(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn custom_values(&self, scope: &TenantScope, asset: Uuid) -> Result<BTreeMap<Uuid, String>, ExtrasRepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT field_id, value FROM asset_custom_values WHERE asset_id = $1 AND ($2 OR tenant_id = ANY($3))",
            asset,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(|r| (r.field_id, r.value)).collect())
    }

    async fn value_taken(&self, tenant: TenantId, field: Uuid, value: &str, except_asset: Uuid) -> Result<bool, ExtrasRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        let taken = sqlx::query_scalar!(
            "SELECT EXISTS (SELECT 1 FROM asset_custom_values
                            WHERE tenant_id = $1 AND field_id = $2 AND lower(value) = lower($3) AND asset_id <> $4) AS \"taken!\"",
            tenant.as_uuid(),
            field,
            value,
            except_asset
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(taken)
    }

    async fn save_custom_values(
        &self,
        tenant: TenantId,
        asset: Uuid,
        values: &BTreeMap<Uuid, String>,
        audit: &AuditRecord,
    ) -> Result<(), ExtrasRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        sqlx::query!("DELETE FROM asset_custom_values WHERE asset_id = $1", asset)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        for (field, value) in values {
            sqlx::query!(
                "INSERT INTO asset_custom_values (asset_id, tenant_id, field_id, value) VALUES ($1, $2, $3, $4)",
                asset,
                tenant.as_uuid(),
                field,
                value
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        write(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn find_asset(
        &self,
        scope: &TenantScope,
        service_tag: Option<&str>,
        hostname: Option<&str>,
        asset_tag: Option<&str>,
    ) -> Result<Vec<FoundAsset>, ExtrasRepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, tenant_id FROM assets
             WHERE retired_on IS NULL AND ($1 OR tenant_id = ANY($2)) AND (
                   ($3::text IS NOT NULL AND lower(service_tag) = lower($3))
                OR ($4::text IS NOT NULL AND lower(hostname) = lower($4))
                OR ($5::text IS NOT NULL AND lower(asset_tag) = lower($5)))
             ORDER BY id
             LIMIT 5",
            all,
            &tenants,
            service_tag,
            hostname,
            asset_tag
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| FoundAsset {
                id: r.id,
                tenant_id: TenantId::from_uuid(r.tenant_id),
            })
            .collect())
    }

    async fn save_inventory(
        &self,
        tenant: TenantId,
        asset: Uuid,
        snapshot: &serde_json::Value,
        software: &[(SoftwareItem, Uuid)],
        at: DateTime<Utc>,
        audit: &AuditRecord,
    ) -> Result<(), ExtrasRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO asset_inventory (asset_id, tenant_id, snapshot, collected_at) VALUES ($1, $2, $3, $4)
             ON CONFLICT (asset_id) DO UPDATE SET snapshot = EXCLUDED.snapshot, collected_at = EXCLUDED.collected_at",
            asset,
            tenant.as_uuid(),
            snapshot,
            at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        let apps: Vec<Uuid> = software.iter().map(|(_, id)| *id).collect();
        sqlx::query!(
            "DELETE FROM asset_installations WHERE asset_id = $1 AND NOT (application_id = ANY($2))",
            asset,
            &apps
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for (item, app) in software {
            sqlx::query!(
                "INSERT INTO asset_installations (asset_id, tenant_id, application_id, version, last_seen_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (asset_id, application_id) DO UPDATE SET version = EXCLUDED.version, last_seen_at = EXCLUDED.last_seen_at",
                asset,
                tenant.as_uuid(),
                app,
                item.version,
                at
            )
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        write(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn application_id(&self, tenant: TenantId, item: &SoftwareItem) -> Result<Uuid, ExtrasRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        let id = sqlx::query_scalar!(
            "INSERT INTO software_applications (id, tenant_id, name, publisher, created_at, updated_at)
             VALUES ($1, $2, $3, $4, now(), now())
             ON CONFLICT (tenant_id, lower(name), lower(publisher)) DO UPDATE SET updated_at = software_applications.updated_at
             RETURNING id",
            Uuid::now_v7(),
            tenant.as_uuid(),
            item.name,
            item.publisher
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(id)
    }

    async fn installations(&self, scope: &TenantScope, asset: Uuid) -> Result<Vec<Installation>, ExtrasRepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT i.application_id, a.name, a.publisher, i.version
             FROM asset_installations i JOIN software_applications a ON a.id = i.application_id
             WHERE i.asset_id = $1 AND ($2 OR i.tenant_id = ANY($3))
             ORDER BY lower(a.name), lower(a.publisher)",
            asset,
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| Installation {
                application_id: r.application_id,
                name: r.name,
                publisher: r.publisher,
                version: r.version,
            })
            .collect())
    }

    async fn licences(&self, scope: &TenantScope) -> Result<Vec<LicenceRecord>, ExtrasRepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            LicenceRow,
            "SELECT id, tenant_id, licence_type, key_hint, quantity, renewal_date, notice_period_days,
                    (cost * 100)::bigint AS cost_minor, currency, status, created_by_kind, created_by_id, created_at, updated_at
             FROM licences WHERE ($1 OR tenant_id = ANY($2)) ORDER BY lower(licence_type), id",
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(LicenceRecord::try_from).collect()
    }

    async fn licence(&self, scope: &TenantScope, id: Uuid) -> Result<Option<LicenceRecord>, ExtrasRepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            LicenceRow,
            "SELECT id, tenant_id, licence_type, key_hint, quantity, renewal_date, notice_period_days,
                    (cost * 100)::bigint AS cost_minor, currency, status, created_by_kind, created_by_id, created_at, updated_at
             FROM licences WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id,
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(LicenceRecord::try_from).transpose()
    }

    async fn save_licence(
        &self,
        l: &LicenceRecord,
        sealed_key: Option<&str>,
        audit: &AuditRecord,
    ) -> Result<(), ExtrasRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(l.tenant_id)).await.map_err(db_err)?;
        let quantity = i32::try_from(l.quantity).map_err(corrupt)?;
        let notice = l.notice_period_days.map(i32::try_from).transpose().map_err(corrupt)?;
        let (cost_minor, currency) = match &l.cost {
            Some((m, c)) => (Some(*m), Some(c.clone())),
            None => (None, None),
        };
        let status = l.status.as_str();
        let (kind, by_id) = (l.created_by.kind().as_str(), l.created_by.id());
        sqlx::query!(
            "INSERT INTO licences (id, tenant_id, licence_type, licence_key, key_hint, quantity, renewal_date,
                                   notice_period_days, cost, currency, status, created_by_kind, created_by_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, ($9::bigint)::numeric / 100, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (id) DO UPDATE SET licence_type = EXCLUDED.licence_type,
                 licence_key = COALESCE(EXCLUDED.licence_key, licences.licence_key),
                 key_hint = COALESCE(EXCLUDED.key_hint, licences.key_hint), quantity = EXCLUDED.quantity,
                 renewal_date = EXCLUDED.renewal_date, notice_period_days = EXCLUDED.notice_period_days,
                 cost = EXCLUDED.cost, currency = EXCLUDED.currency, status = EXCLUDED.status,
                 updated_at = EXCLUDED.updated_at",
            l.id,
            l.tenant_id.as_uuid(),
            l.licence_type,
            sealed_key,
            l.key_hint,
            quantity,
            l.renewal_date,
            notice,
            cost_minor,
            currency,
            status,
            kind,
            by_id,
            l.created_at,
            l.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        write(&mut tx, audit).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn link_application(&self, tenant: TenantId, licence: Uuid, application: Uuid) -> Result<(), ExtrasRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO licence_applications (licence_id, tenant_id, application_id) VALUES ($1, $2, $3)
             ON CONFLICT (licence_id, application_id) DO NOTHING",
            licence,
            tenant.as_uuid(),
            application
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn unlink_application(&self, licence: Uuid, application: Uuid) -> Result<(), ExtrasRepoError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query!(
            "DELETE FROM licence_applications WHERE licence_id = $1 AND application_id = $2",
            licence,
            application
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn compliance_counts(&self, scope: &TenantScope, licence: Uuid) -> Result<(u32, u32), ExtrasRepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query!(
            "SELECT (SELECT count(*) FROM licence_applications la WHERE la.licence_id = $1 AND ($2 OR la.tenant_id = ANY($3)))::bigint AS \"linked!\",
                    (SELECT count(DISTINCT i.asset_id) FROM asset_installations i
                       JOIN licence_applications la ON la.application_id = i.application_id AND la.licence_id = $1
                       JOIN assets a ON a.id = i.asset_id AND a.retired_on IS NULL)::bigint AS \"installed!\"",
            licence,
            all,
            &tenants
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok((
            u32::try_from(row.linked).map_err(corrupt)?,
            u32::try_from(row.installed).map_err(corrupt)?,
        ))
    }

    async fn active_assets_of_type(
        &self,
        tenant: TenantId,
        asset_type: Uuid,
    ) -> Result<Vec<(Uuid, Option<String>, Option<String>, Option<String>)>, ExtrasRepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(tenant)).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT id, service_tag, hostname, asset_tag FROM assets
             WHERE tenant_id = $1 AND asset_type_id = $2 AND retired_on IS NULL",
            tenant.as_uuid(),
            asset_type
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(|r| (r.id, r.service_tag, r.hostname, r.asset_tag)).collect())
    }
}
