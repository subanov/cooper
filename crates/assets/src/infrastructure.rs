//! PostgreSQL storage of Assets (tenant-scoped: explicit filter plus RLS).

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AuditRecord, EventEnvelope, TenantId, TenantScope};
use cooper_platform::db::Tx;
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    Amount, Asset, AssetCheckout, AssetDetails, AssetHolder, AssetId, AssetRecord, CurrencyCode,
    Lookup, LookupKind, Money, Retirement,
};
use crate::ports::{AssetLink, AssetQuery, AssetRepository, AssetSummary, LinkKind, RepoError};

/// sqlx implementation of [`AssetRepository`].
#[derive(Clone)]
pub struct PgAssetStore {
    pool: PgPool,
}

impl PgAssetStore {
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
    Ok(())
}

/// Locks the Asset row and returns whether it is retired and whether it has an open
/// Check Out, so state changes cannot race each other.
async fn lock(tx: &mut Tx, asset: &Asset) -> Result<(bool, bool), RepoError> {
    let row = sqlx::query!(
        r#"SELECT a.retired_on IS NOT NULL AS "retired!",
                  EXISTS (SELECT 1 FROM asset_checkouts c WHERE c.asset_id = a.id AND c.checked_in_at IS NULL) AS "held!"
           FROM assets a WHERE a.id = $1 FOR UPDATE"#,
        asset.id().as_uuid()
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?
    .ok_or(RepoError::NotFound)?;
    Ok((row.retired, row.held))
}

async fn update_asset(tx: &mut Tx, a: &Asset) -> Result<(), RepoError> {
    let d = a.details();
    let (cost, currency) = money(d.purchase_cost.as_ref());
    let r = a.retirement();
    let n = sqlx::query!(
        "UPDATE assets SET hostname = $2, service_tag = $3, asset_tag = $4, asset_type_id = $5, asset_status_id = $6,
                location_id = $7, supplier_id = $8, cost_centre_id = $9, purchase_date = $10,
                purchase_cost = $11::bigint / 100.0, purchase_currency = $12, warranty_expires_on = $13,
                lease_starts_on = $14, lease_expires_on = $15, retired_on = $16, retirement_reason = $17,
                retired_at = $18, retired_by_kind = $19, retired_by_id = $20, updated_at = $21
         WHERE id = $1",
        a.id().as_uuid(),
        d.hostname,
        d.service_tag,
        d.asset_tag,
        d.asset_type_id,
        d.asset_status_id,
        d.location_id,
        d.supplier_id,
        d.cost_centre_id,
        d.purchase_date,
        cost,
        currency,
        d.warranty_expires_on,
        d.lease_starts_on,
        d.lease_expires_on,
        r.map(|r| r.retired_on),
        r.map(|r| r.reason.clone()),
        r.map(|r| r.recorded_at),
        r.map(|r| r.recorded_by.kind().as_str()),
        r.and_then(|r| r.recorded_by.id()),
        a.updated_at()
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

fn money(m: Option<&Money>) -> (Option<i64>, Option<String>) {
    (
        m.map(|m| m.amount.hundredths()),
        m.map(|m| m.currency.as_str().to_owned()),
    )
}

struct AssetRow {
    id: Uuid,
    tenant_id: Uuid,
    hostname: Option<String>,
    service_tag: Option<String>,
    asset_tag: Option<String>,
    asset_type_id: Option<Uuid>,
    asset_status_id: Option<Uuid>,
    location_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    cost_centre_id: Option<Uuid>,
    purchase_date: Option<NaiveDate>,
    purchase_cost: Option<i64>,
    purchase_currency: Option<String>,
    warranty_expires_on: Option<NaiveDate>,
    lease_starts_on: Option<NaiveDate>,
    lease_expires_on: Option<NaiveDate>,
    retired_on: Option<NaiveDate>,
    retirement_reason: Option<String>,
    retired_at: Option<DateTime<Utc>>,
    retired_by_kind: Option<String>,
    retired_by_id: Option<Uuid>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    checkout_id: Option<Uuid>,
    holder_kind: Option<String>,
    holder_id: Option<Uuid>,
    checked_out_at: Option<DateTime<Utc>>,
    checked_out_by_kind: Option<String>,
    checked_out_by_id: Option<Uuid>,
    checkout_note: Option<String>,
    due_back_on: Option<NaiveDate>,
}

impl TryFrom<AssetRow> for Asset {
    type Error = RepoError;
    fn try_from(r: AssetRow) -> Result<Self, RepoError> {
        let purchase_cost = match (r.purchase_cost, r.purchase_currency) {
            (Some(c), Some(cur)) => Some(Money {
                amount: Amount::from_hundredths(c).map_err(corrupt)?,
                currency: CurrencyCode::try_from(cur).map_err(corrupt)?,
            }),
            _ => None,
        };
        let retirement = match (
            r.retired_on,
            r.retirement_reason,
            r.retired_at,
            r.retired_by_kind,
        ) {
            (Some(on), Some(reason), Some(at), Some(kind)) => Some(Retirement {
                retired_on: on,
                reason,
                recorded_at: at,
                recorded_by: Actor::from_parts(&kind, r.retired_by_id).map_err(corrupt)?,
            }),
            _ => None,
        };
        let checkout = match (
            r.checkout_id,
            r.holder_kind,
            r.holder_id,
            r.checked_out_at,
            r.checked_out_by_kind,
        ) {
            (Some(id), Some(kind), Some(holder), Some(at), Some(by)) => Some(AssetCheckout {
                id,
                holder: AssetHolder::from_parts(&kind, holder).map_err(corrupt)?,
                checked_out_at: at,
                checked_out_by: Actor::from_parts(&by, r.checked_out_by_id).map_err(corrupt)?,
                checkout_note: r.checkout_note.unwrap_or_default(),
                due_back_on: r.due_back_on,
                checked_in_at: None,
                checked_in_by: None,
                checkin_note: String::new(),
            }),
            _ => None,
        };
        Ok(Self::restore(AssetRecord {
            id: AssetId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            details: AssetDetails {
                hostname: r.hostname,
                service_tag: r.service_tag,
                asset_tag: r.asset_tag,
                asset_type_id: r.asset_type_id,
                asset_status_id: r.asset_status_id,
                location_id: r.location_id,
                supplier_id: r.supplier_id,
                cost_centre_id: r.cost_centre_id,
                purchase_date: r.purchase_date,
                purchase_cost,
                warranty_expires_on: r.warranty_expires_on,
                lease_starts_on: r.lease_starts_on,
                lease_expires_on: r.lease_expires_on,
            },
            checkout,
            retirement,
            created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id).map_err(corrupt)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

struct AssetCheckoutRow {
    id: Uuid,
    holder_kind: String,
    holder_id: Uuid,
    checked_out_at: DateTime<Utc>,
    checked_out_by_kind: String,
    checked_out_by_id: Option<Uuid>,
    checkout_note: String,
    due_back_on: Option<NaiveDate>,
    checked_in_at: Option<DateTime<Utc>>,
    checked_in_by_kind: Option<String>,
    checked_in_by_id: Option<Uuid>,
    checkin_note: String,
}

impl TryFrom<AssetCheckoutRow> for AssetCheckout {
    type Error = RepoError;
    fn try_from(r: AssetCheckoutRow) -> Result<Self, RepoError> {
        Ok(Self {
            id: r.id,
            holder: AssetHolder::from_parts(&r.holder_kind, r.holder_id).map_err(corrupt)?,
            checked_out_at: r.checked_out_at,
            checked_out_by: Actor::from_parts(&r.checked_out_by_kind, r.checked_out_by_id)
                .map_err(corrupt)?,
            checkout_note: r.checkout_note,
            due_back_on: r.due_back_on,
            checked_in_at: r.checked_in_at,
            checked_in_by: r
                .checked_in_by_kind
                .map(|k| Actor::from_parts(&k, r.checked_in_by_id))
                .transpose()
                .map_err(corrupt)?,
            checkin_note: r.checkin_note,
        })
    }
}

fn tally(rows: Vec<(Option<Uuid>, i64)>) -> Vec<(Option<Uuid>, u64)> {
    rows.into_iter()
        .map(|(id, n)| (id, u64::try_from(n).unwrap_or(0)))
        .collect()
}

#[async_trait]
impl AssetRepository for PgAssetStore {
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
        Ok(match kind {
            LookupKind::Type => sqlx::query!(
                "SELECT id, name, sort_order, is_active, created_at, updated_at FROM asset_types"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| Lookup {
                id: r.id,
                kind,
                name: r.name,
                parent_id: None,
                is_retired: false,
                colour: None,
                sort_order: r.sort_order,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect(),
            LookupKind::Status => sqlx::query!(
                "SELECT id, name, is_retired, colour, sort_order, is_active, created_at, updated_at FROM asset_statuses"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| Lookup {
                id: r.id,
                kind,
                name: r.name,
                parent_id: None,
                is_retired: r.is_retired,
                colour: r.colour,
                sort_order: r.sort_order,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect(),
            LookupKind::Location => sqlx::query!(
                "SELECT id, parent_id, name, sort_order, is_active, created_at, updated_at FROM asset_locations"
            )
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| Lookup {
                id: r.id,
                kind,
                name: r.name,
                parent_id: r.parent_id,
                is_retired: false,
                colour: None,
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
            LookupKind::Type => {
                sqlx::query!(
                    "INSERT INTO asset_types (id, name, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, sort_order = EXCLUDED.sort_order,
                         is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id,
                    l.name,
                    l.sort_order,
                    l.is_active,
                    l.created_at,
                    l.updated_at
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
            LookupKind::Status => {
                sqlx::query!(
                    "INSERT INTO asset_statuses (id, name, is_retired, colour, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                     ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_retired = EXCLUDED.is_retired,
                         colour = EXCLUDED.colour, sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active,
                         updated_at = EXCLUDED.updated_at",
                    l.id,
                    l.name,
                    l.is_retired,
                    l.colour,
                    l.sort_order,
                    l.is_active,
                    l.created_at,
                    l.updated_at
                )
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
            }
            LookupKind::Location => {
                sqlx::query!(
                    "INSERT INTO asset_locations (id, parent_id, name, sort_order, is_active, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7)
                     ON CONFLICT (id) DO UPDATE SET parent_id = EXCLUDED.parent_id, name = EXCLUDED.name,
                         sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                    l.id,
                    l.parent_id,
                    l.name,
                    l.sort_order,
                    l.is_active,
                    l.created_at,
                    l.updated_at
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
        a: &Asset,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(a.tenant_id()))
            .await
            .map_err(db_err)?;
        let d = a.details();
        let (cost, currency) = money(d.purchase_cost.as_ref());
        sqlx::query!(
            "INSERT INTO assets (id, tenant_id, hostname, service_tag, asset_tag, asset_type_id, asset_status_id, location_id,
                                 supplier_id, cost_centre_id, purchase_date, purchase_cost, purchase_currency,
                                 warranty_expires_on, lease_starts_on, lease_expires_on, created_by_kind, created_by_id,
                                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::bigint / 100.0, $13, $14, $15, $16, $17, $18, $19, $20)",
            a.id().as_uuid(),
            a.tenant_id().as_uuid(),
            d.hostname,
            d.service_tag,
            d.asset_tag,
            d.asset_type_id,
            d.asset_status_id,
            d.location_id,
            d.supplier_id,
            d.cost_centre_id,
            d.purchase_date,
            cost,
            currency,
            d.warranty_expires_on,
            d.lease_starts_on,
            d.lease_expires_on,
            a.created_by().kind().as_str(),
            a.created_by().id(),
            a.created_at(),
            a.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit_and_publish(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update(
        &self,
        a: &Asset,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(a.tenant_id()))
            .await
            .map_err(db_err)?;
        update_asset(&mut tx, a).await?;
        audit_and_publish(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn check_out(
        &self,
        a: &Asset,
        c: &AssetCheckout,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(a.tenant_id()))
            .await
            .map_err(db_err)?;
        let (retired, held) = lock(&mut tx, a).await?;
        if retired || held {
            return Err(RepoError::Concurrent(
                "the Asset was retired or checked out meanwhile".into(),
            ));
        }
        sqlx::query!(
            "INSERT INTO asset_checkouts (id, tenant_id, asset_id, holder_kind, holder_id, checked_out_at, checked_out_by_kind,
                                          checked_out_by_id, checkout_note, due_back_on)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            c.id,
            a.tenant_id().as_uuid(),
            a.id().as_uuid(),
            c.holder.kind(),
            c.holder.id(),
            c.checked_out_at,
            c.checked_out_by.kind().as_str(),
            c.checked_out_by.id(),
            c.checkout_note,
            c.due_back_on
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        update_asset(&mut tx, a).await?;
        audit_and_publish(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn check_in(
        &self,
        a: &Asset,
        c: &AssetCheckout,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(a.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "UPDATE asset_checkouts SET checked_in_at = $3, checked_in_by_kind = $4, checked_in_by_id = $5, checkin_note = $6
             WHERE id = $1 AND asset_id = $2 AND checked_in_at IS NULL",
            c.id,
            a.id().as_uuid(),
            c.checked_in_at,
            c.checked_in_by.map(|b| b.kind().as_str()),
            c.checked_in_by.and_then(|b| b.id()),
            c.checkin_note
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 0 {
            return Err(RepoError::Concurrent(
                "the Asset was checked in meanwhile".into(),
            ));
        }
        update_asset(&mut tx, a).await?;
        audit_and_publish(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn retire(
        &self,
        a: &Asset,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(a.tenant_id()))
            .await
            .map_err(db_err)?;
        let (retired, held) = lock(&mut tx, a).await?;
        if retired || held {
            return Err(RepoError::Concurrent(
                "the Asset was retired or checked out meanwhile".into(),
            ));
        }
        update_asset(&mut tx, a).await?;
        audit_and_publish(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn get(&self, scope: &TenantScope, id: AssetId) -> Result<Option<Asset>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            AssetRow,
            r#"SELECT a.id, a.tenant_id, a.hostname, a.service_tag, a.asset_tag, a.asset_type_id, a.asset_status_id,
                      a.location_id, a.supplier_id, a.cost_centre_id, a.purchase_date,
                      (a.purchase_cost * 100)::bigint AS "purchase_cost?", a.purchase_currency, a.warranty_expires_on,
                      a.lease_starts_on, a.lease_expires_on, a.retired_on, a.retirement_reason, a.retired_at,
                      a.retired_by_kind, a.retired_by_id, a.created_by_kind, a.created_by_id, a.created_at, a.updated_at,
                      c.id AS "checkout_id?", c.holder_kind AS "holder_kind?", c.holder_id AS "holder_id?",
                      c.checked_out_at AS "checked_out_at?", c.checked_out_by_kind AS "checked_out_by_kind?",
                      c.checked_out_by_id AS "checked_out_by_id?", c.checkout_note AS "checkout_note?",
                      c.due_back_on AS "due_back_on?"
               FROM assets a
               LEFT JOIN asset_checkouts c ON c.asset_id = a.id AND c.checked_in_at IS NULL
               WHERE a.id = $1 AND ($2 OR a.tenant_id = ANY($3))"#,
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Asset::try_from).transpose()
    }

    async fn list(&self, scope: &TenantScope, q: &AssetQuery) -> Result<Vec<Asset>, RepoError> {
        let (all, tenants) = ids(scope);
        let (holder_kind, holder_id) = q.holder.map(|h| (h.kind(), h.id())).unzip();
        let search = q
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                format!(
                    "%{}%",
                    s.replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                )
            });
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            AssetRow,
            r#"SELECT a.id, a.tenant_id, a.hostname, a.service_tag, a.asset_tag, a.asset_type_id, a.asset_status_id,
                      a.location_id, a.supplier_id, a.cost_centre_id, a.purchase_date,
                      (a.purchase_cost * 100)::bigint AS "purchase_cost?", a.purchase_currency, a.warranty_expires_on,
                      a.lease_starts_on, a.lease_expires_on, a.retired_on, a.retirement_reason, a.retired_at,
                      a.retired_by_kind, a.retired_by_id, a.created_by_kind, a.created_by_id, a.created_at, a.updated_at,
                      c.id AS "checkout_id?", c.holder_kind AS "holder_kind?", c.holder_id AS "holder_id?",
                      c.checked_out_at AS "checked_out_at?", c.checked_out_by_kind AS "checked_out_by_kind?",
                      c.checked_out_by_id AS "checked_out_by_id?", c.checkout_note AS "checkout_note?",
                      c.due_back_on AS "due_back_on?"
               FROM assets a
               LEFT JOIN asset_checkouts c ON c.asset_id = a.id AND c.checked_in_at IS NULL
               WHERE ($1 OR a.tenant_id = ANY($2))
                 AND ($3::uuid IS NULL OR a.tenant_id = $3)
                 AND ($4::uuid IS NULL OR a.asset_type_id = $4)
                 AND ($5::uuid IS NULL OR a.asset_status_id = $5)
                 AND ($6::uuid[] IS NULL OR a.location_id = ANY($6))
                 AND ($7::text IS NULL OR (c.holder_kind = $7 AND c.holder_id = $8))
                 AND ($9::boolean IS NULL OR (c.id IS NOT NULL) = $9)
                 AND ($10 OR a.retired_on IS NULL)
                 AND (NOT $11 OR a.retired_on IS NOT NULL)
                 AND ($12::text IS NULL OR a.hostname ILIKE $12 OR a.asset_tag ILIKE $12 OR a.service_tag ILIKE $12)
                 AND ($13::uuid IS NULL OR a.id < $13)
               ORDER BY a.id DESC
               LIMIT $14"#,
            all,
            &tenants,
            q.tenant.map(|t| t.as_uuid()),
            q.asset_type_id,
            q.asset_status_id,
            q.locations.as_deref(),
            holder_kind,
            holder_id,
            q.checked_out,
            q.include_retired || q.retired_only,
            q.retired_only,
            search,
            q.after,
            q.limit.fetch_size()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Asset::try_from).collect()
    }

    async fn summary(
        &self,
        scope: &TenantScope,
        tenant: Option<TenantId>,
    ) -> Result<AssetSummary, RepoError> {
        let (all, tenants) = ids(scope);
        let tenant = tenant.map(|t| t.as_uuid());
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let totals = sqlx::query!(
            r#"SELECT COUNT(*) FILTER (WHERE a.retired_on IS NULL) AS "active!",
                      COUNT(*) FILTER (WHERE a.retired_on IS NOT NULL) AS "retired!",
                      COUNT(c.id) FILTER (WHERE a.retired_on IS NULL) AS "checked_out!"
               FROM assets a LEFT JOIN asset_checkouts c ON c.asset_id = a.id AND c.checked_in_at IS NULL
               WHERE ($1 OR a.tenant_id = ANY($2)) AND ($3::uuid IS NULL OR a.tenant_id = $3)"#,
            all,
            &tenants,
            tenant
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        let by_type = sqlx::query!(
            r#"SELECT asset_type_id, COUNT(*) AS "n!" FROM assets
               WHERE ($1 OR tenant_id = ANY($2)) AND ($3::uuid IS NULL OR tenant_id = $3) AND retired_on IS NULL
               GROUP BY asset_type_id"#,
            all,
            &tenants,
            tenant
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let by_status = sqlx::query!(
            r#"SELECT asset_status_id, COUNT(*) AS "n!" FROM assets
               WHERE ($1 OR tenant_id = ANY($2)) AND ($3::uuid IS NULL OR tenant_id = $3) AND retired_on IS NULL
               GROUP BY asset_status_id"#,
            all,
            &tenants,
            tenant
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        let n = |v: i64| u64::try_from(v).unwrap_or(0);
        Ok(AssetSummary {
            active: n(totals.active),
            retired: n(totals.retired),
            checked_out: n(totals.checked_out),
            by_type: tally(
                by_type
                    .into_iter()
                    .map(|r| (r.asset_type_id, r.n))
                    .collect(),
            ),
            by_status: tally(
                by_status
                    .into_iter()
                    .map(|r| (r.asset_status_id, r.n))
                    .collect(),
            ),
        })
    }

    async fn checkouts(
        &self,
        scope: &TenantScope,
        id: AssetId,
    ) -> Result<Vec<AssetCheckout>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            AssetCheckoutRow,
            "SELECT id, holder_kind, holder_id, checked_out_at, checked_out_by_kind, checked_out_by_id, checkout_note,
                    due_back_on, checked_in_at, checked_in_by_kind, checked_in_by_id, checkin_note
             FROM asset_checkouts WHERE asset_id = $1 AND ($2 OR tenant_id = ANY($3))
             ORDER BY checked_out_at DESC, id DESC",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(AssetCheckout::try_from).collect()
    }

    async fn links(&self, scope: &TenantScope, id: AssetId) -> Result<Vec<AssetLink>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query!(
            "SELECT kind, target_id, linked_by_kind, linked_by_id, linked_at FROM asset_links
             WHERE asset_id = $1 AND ($2 OR tenant_id = ANY($3)) ORDER BY linked_at, target_id",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter()
            .map(|r| {
                Ok(AssetLink {
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
        a: &Asset,
        l: &AssetLink,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(a.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO asset_links (tenant_id, asset_id, kind, target_id, linked_by_kind, linked_by_id, linked_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            a.tenant_id().as_uuid(),
            a.id().as_uuid(),
            l.kind.as_str(),
            l.target_id,
            l.linked_by.kind().as_str(),
            l.linked_by.id(),
            l.linked_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        audit_and_publish(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn remove_link(
        &self,
        a: &Asset,
        kind: LinkKind,
        target: Uuid,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(a.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "DELETE FROM asset_links WHERE asset_id = $1 AND kind = $2 AND target_id = $3",
            a.id().as_uuid(),
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
        audit_and_publish(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn history(
        &self,
        scope: &TenantScope,
        id: AssetId,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let h = cooper_platform::audit::history(&mut tx, "asset", id.as_uuid())
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        tx.commit().await.map_err(db_err)?;
        Ok(h)
    }
}
