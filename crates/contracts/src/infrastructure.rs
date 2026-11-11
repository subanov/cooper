//! PostgreSQL storage of Contracts (tenant-scoped: explicit filter plus RLS), and the
//! `contract-reminders` Background Job.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, EventEnvelope, RequesterId, TenantId, TenantScope,
};
use cooper_platform::db::Tx;
use cooper_platform::jobs::{Job, JobContext, JobError, Schedule};
use cooper_platform::tenancy;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    Contact, ContactDetails, ContactId, ContactRecord, Contract, ContractDetails, ContractId,
    ContractParty, ContractRecord, CurrencyCode, DataProtection, Lookup, LookupKind, Money,
    NoticePeriod, ReminderKind, Supplier, SupplierDetails, SupplierId, SupplierRecord,
};
use crate::ports::{ContractQuery, ContractsRepository, RepoError, SupplierQuery};

/// sqlx implementation of [`ContractsRepository`].
#[derive(Clone)]
pub struct PgContractsStore {
    pool: PgPool,
}

impl PgContractsStore {
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

fn like(search: Option<&str>) -> Option<String> {
    search.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
        format!(
            "%{}%",
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    })
}

async fn write(
    tx: &mut Tx,
    audit: &AuditRecord,
    events: &[EventEnvelope],
) -> Result<(), RepoError> {
    cooper_platform::audit::record(tx, audit)
        .await
        .map_err(db_err)?;
    for e in events {
        cooper_platform::outbox::append(tx, e)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

// ----- Rows -------------------------------------------------------------------------------

struct LookupRow {
    id: Uuid,
    name: String,
    is_closed: bool,
    sort_order: i32,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl LookupRow {
    fn into_lookup(self, kind: LookupKind) -> Lookup {
        Lookup {
            id: self.id,
            kind,
            name: self.name,
            is_closed: self.is_closed,
            sort_order: self.sort_order,
            is_active: self.is_active,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

struct SupplierRow {
    id: Uuid,
    tenant_id: Uuid,
    legal_name: String,
    trading_name: Option<String>,
    registration_number: Option<String>,
    vat_number: Option<String>,
    supplier_type_id: Option<Uuid>,
    supplier_status_id: Option<Uuid>,
    notes: String,
    is_active: bool,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<SupplierRow> for Supplier {
    type Error = RepoError;
    fn try_from(r: SupplierRow) -> Result<Self, RepoError> {
        Ok(Self::restore(SupplierRecord {
            id: SupplierId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            details: SupplierDetails {
                legal_name: r.legal_name,
                trading_name: r.trading_name,
                registration_number: r.registration_number,
                vat_number: r.vat_number,
                supplier_type_id: r.supplier_type_id,
                supplier_status_id: r.supplier_status_id,
                notes: r.notes,
                is_active: r.is_active,
            },
            created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id).map_err(corrupt)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

struct ContactRow {
    id: Uuid,
    tenant_id: Uuid,
    supplier_id: Uuid,
    full_name: String,
    job_title: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    mobile: Option<String>,
    notes: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ContactRow> for Contact {
    fn from(r: ContactRow) -> Self {
        Self::restore(ContactRecord {
            id: ContactId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            supplier_id: SupplierId::from_uuid(r.supplier_id),
            details: ContactDetails {
                full_name: r.full_name,
                job_title: r.job_title,
                email: r.email,
                phone: r.phone,
                mobile: r.mobile,
                notes: r.notes,
                is_active: r.is_active,
            },
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

struct ContractRow {
    id: Uuid,
    tenant_id: Uuid,
    contract_number: String,
    title: String,
    description: String,
    party_kind: String,
    supplier_id: Option<Uuid>,
    requester_id: Option<Uuid>,
    owner_id: Option<Uuid>,
    contract_status_id: Option<Uuid>,
    status_closed: bool,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    notice_period_amount: Option<i32>,
    notice_period_unit: Option<String>,
    value_minor: Option<i64>,
    currency: Option<String>,
    payment_schedule_id: Option<Uuid>,
    cost_centre_id: Option<Uuid>,
    dms_link: Option<String>,
    personal_data_transferred: bool,
    dpia_required: bool,
    dpia_completed_on: Option<NaiveDate>,
    dpia_link: Option<String>,
    created_by_kind: String,
    created_by_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl TryFrom<ContractRow> for Contract {
    type Error = RepoError;
    fn try_from(r: ContractRow) -> Result<Self, RepoError> {
        let party = match (r.party_kind.as_str(), r.supplier_id) {
            ("supplier", Some(s)) => ContractParty::Supplier(SupplierId::from_uuid(s)),
            ("customer", None) => {
                ContractParty::Customer(r.requester_id.map(RequesterId::from_uuid))
            }
            (kind, _) => return Err(corrupt(format!("party {kind}"))),
        };
        let notice_period = match (r.notice_period_amount, r.notice_period_unit) {
            (Some(a), Some(u)) => Some(
                NoticePeriod::new(
                    u32::try_from(a).map_err(corrupt)?,
                    u.parse().map_err(corrupt)?,
                )
                .map_err(corrupt)?,
            ),
            _ => None,
        };
        let value = match (r.value_minor, r.currency) {
            (Some(v), Some(c)) => {
                Some(Money::new(v, CurrencyCode::try_from(c).map_err(corrupt)?).map_err(corrupt)?)
            }
            _ => None,
        };
        Ok(Self::restore(ContractRecord {
            id: ContractId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            details: ContractDetails {
                contract_number: r.contract_number,
                title: r.title,
                description: r.description,
                party,
                owner_id: r.owner_id.map(AnalystId::from_uuid),
                status_id: r.contract_status_id,
                start_date: r.start_date,
                end_date: r.end_date,
                notice_period,
                value,
                payment_schedule_id: r.payment_schedule_id,
                cost_centre_id: r.cost_centre_id,
                dms_link: r.dms_link,
                data_protection: DataProtection {
                    personal_data_transferred: r.personal_data_transferred,
                    dpia_required: r.dpia_required,
                    dpia_completed_on: r.dpia_completed_on,
                    dpia_link: r.dpia_link,
                },
            },
            status_closed: r.status_closed,
            created_by: Actor::from_parts(&r.created_by_kind, r.created_by_id).map_err(corrupt)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        }))
    }
}

fn contracts(rows: Vec<ContractRow>) -> Result<Vec<Contract>, RepoError> {
    rows.into_iter().map(Contract::try_from).collect()
}

async fn save_contract(tx: &mut Tx, c: &Contract) -> Result<(), RepoError> {
    let d = c.details();
    let dp = &d.data_protection;
    sqlx::query!(
        "INSERT INTO contracts (id, tenant_id, contract_number, title, description, party_kind, supplier_id, requester_id,
                                owner_id, contract_status_id, start_date, end_date, notice_period_amount, notice_period_unit,
                                notice_date, value, currency, payment_schedule_id, cost_centre_id, dms_link,
                                personal_data_transferred, dpia_required, dpia_completed_on, dpia_link,
                                created_by_kind, created_by_id, created_at, updated_at, deleted_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16::bigint::numeric / 100, $17, $18,
                 $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29)
         ON CONFLICT (id) DO UPDATE SET contract_number = EXCLUDED.contract_number, title = EXCLUDED.title,
             description = EXCLUDED.description, party_kind = EXCLUDED.party_kind, supplier_id = EXCLUDED.supplier_id,
             requester_id = EXCLUDED.requester_id, owner_id = EXCLUDED.owner_id,
             contract_status_id = EXCLUDED.contract_status_id, start_date = EXCLUDED.start_date,
             end_date = EXCLUDED.end_date, notice_period_amount = EXCLUDED.notice_period_amount,
             notice_period_unit = EXCLUDED.notice_period_unit, notice_date = EXCLUDED.notice_date,
             value = EXCLUDED.value, currency = EXCLUDED.currency, payment_schedule_id = EXCLUDED.payment_schedule_id,
             cost_centre_id = EXCLUDED.cost_centre_id, dms_link = EXCLUDED.dms_link,
             personal_data_transferred = EXCLUDED.personal_data_transferred, dpia_required = EXCLUDED.dpia_required,
             dpia_completed_on = EXCLUDED.dpia_completed_on, dpia_link = EXCLUDED.dpia_link,
             updated_at = EXCLUDED.updated_at, deleted_at = EXCLUDED.deleted_at",
        c.id().as_uuid(),
        c.tenant_id().as_uuid(),
        d.contract_number,
        d.title,
        d.description,
        d.party.kind(),
        d.party.supplier_id().map(|s| s.as_uuid()),
        d.party.requester_id().map(|r| r.as_uuid()),
        d.owner_id.map(|a| a.as_uuid()),
        d.status_id,
        d.start_date,
        d.end_date,
        d.notice_period.map(|p| i32::from(p.amount())),
        d.notice_period.map(|p| p.unit().as_str()),
        c.notice_date(),
        d.value.as_ref().map(Money::minor_units),
        d.value.as_ref().map(|v| v.currency().as_str()),
        d.payment_schedule_id,
        d.cost_centre_id,
        d.dms_link,
        dp.personal_data_transferred,
        dp.dpia_required,
        dp.dpia_completed_on,
        dp.dpia_link,
        c.created_by().kind().as_str(),
        c.created_by().id(),
        c.created_at(),
        c.updated_at(),
        c.deleted_at()
    )
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[async_trait]
impl ContractsRepository for PgContractsStore {
    async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
        let rows = match kind {
            LookupKind::SupplierType => sqlx::query_as!(
                LookupRow,
                r#"SELECT id, name, false AS "is_closed!", sort_order, is_active, created_at, updated_at FROM supplier_types"#
            )
            .fetch_all(&self.pool)
            .await,
            LookupKind::SupplierStatus => sqlx::query_as!(
                LookupRow,
                r#"SELECT id, name, false AS "is_closed!", sort_order, is_active, created_at, updated_at FROM supplier_statuses"#
            )
            .fetch_all(&self.pool)
            .await,
            LookupKind::ContractStatus => sqlx::query_as!(
                LookupRow,
                "SELECT id, name, is_closed, sort_order, is_active, created_at, updated_at FROM contract_statuses"
            )
            .fetch_all(&self.pool)
            .await,
            LookupKind::PaymentSchedule => sqlx::query_as!(
                LookupRow,
                r#"SELECT id, name, false AS "is_closed!", sort_order, is_active, created_at, updated_at FROM payment_schedules"#
            )
            .fetch_all(&self.pool)
            .await,
        }
        .map_err(db_err)?;
        Ok(rows.into_iter().map(|r| r.into_lookup(kind)).collect())
    }

    async fn save_lookup(
        &self,
        l: &Lookup,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        match l.kind {
            LookupKind::SupplierType => sqlx::query!(
                "INSERT INTO supplier_types (id, name, sort_order, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, sort_order = EXCLUDED.sort_order,
                     is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                l.id, l.name, l.sort_order, l.is_active, l.created_at, l.updated_at
            )
            .execute(&mut *tx)
            .await,
            LookupKind::SupplierStatus => sqlx::query!(
                "INSERT INTO supplier_statuses (id, name, sort_order, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, sort_order = EXCLUDED.sort_order,
                     is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                l.id, l.name, l.sort_order, l.is_active, l.created_at, l.updated_at
            )
            .execute(&mut *tx)
            .await,
            LookupKind::ContractStatus => sqlx::query!(
                "INSERT INTO contract_statuses (id, name, is_closed, sort_order, is_active, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, is_closed = EXCLUDED.is_closed,
                     sort_order = EXCLUDED.sort_order, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                l.id, l.name, l.is_closed, l.sort_order, l.is_active, l.created_at, l.updated_at
            )
            .execute(&mut *tx)
            .await,
            LookupKind::PaymentSchedule => sqlx::query!(
                "INSERT INTO payment_schedules (id, name, sort_order, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, sort_order = EXCLUDED.sort_order,
                     is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
                l.id, l.name, l.sort_order, l.is_active, l.created_at, l.updated_at
            )
            .execute(&mut *tx)
            .await,
        }
        .map_err(db_err)?;
        write(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn contracts_with_status(&self, status: Uuid) -> Result<Vec<Contract>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        let rows = sqlx::query_as!(
            ContractRow,
            r#"SELECT c.id, c.tenant_id, c.contract_number, c.title, c.description, c.party_kind, c.supplier_id, c.requester_id,
                      c.owner_id, c.contract_status_id, COALESCE(s.is_closed, false) AS "status_closed!", c.start_date, c.end_date,
                      c.notice_period_amount, c.notice_period_unit, (c.value * 100)::bigint AS "value_minor?", c.currency,
                      c.payment_schedule_id, c.cost_centre_id, c.dms_link, c.personal_data_transferred, c.dpia_required,
                      c.dpia_completed_on, c.dpia_link, c.created_by_kind, c.created_by_id, c.created_at, c.updated_at, c.deleted_at
               FROM contracts c LEFT JOIN contract_statuses s ON s.id = c.contract_status_id
               WHERE c.contract_status_id = $1 AND c.deleted_at IS NULL"#,
            status
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        contracts(rows)
    }

    async fn supplier(
        &self,
        scope: &TenantScope,
        id: SupplierId,
    ) -> Result<Option<Supplier>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            SupplierRow,
            "SELECT id, tenant_id, legal_name, trading_name, registration_number, vat_number, supplier_type_id,
                    supplier_status_id, notes, is_active, created_by_kind, created_by_id, created_at, updated_at
             FROM suppliers WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Supplier::try_from).transpose()
    }

    async fn suppliers(
        &self,
        scope: &TenantScope,
        q: &SupplierQuery,
    ) -> Result<Vec<Supplier>, RepoError> {
        let (all, tenants) = ids(scope);
        let (after_name, after_id) = q.after.clone().unzip();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            SupplierRow,
            r#"SELECT id, tenant_id, legal_name, trading_name, registration_number, vat_number, supplier_type_id,
                      supplier_status_id, notes, is_active, created_by_kind, created_by_id, created_at, updated_at
               FROM suppliers
               WHERE ($1 OR tenant_id = ANY($2))
                 AND ($3::uuid IS NULL OR tenant_id = $3)
                 AND ($4::text IS NULL OR legal_name ILIKE $4 OR trading_name ILIKE $4)
                 AND ($5::uuid IS NULL OR supplier_type_id = $5)
                 AND ($6::uuid IS NULL OR supplier_status_id = $6)
                 AND ($7::boolean IS NULL OR is_active = $7)
                 AND ($8::text IS NULL OR (lower(legal_name), id) > (lower($8), $9::uuid))
               ORDER BY lower(legal_name), id
               LIMIT $10"#,
            all,
            &tenants,
            q.tenant_id.map(|t| t.as_uuid()),
            like(q.search.as_deref()),
            q.supplier_type_id,
            q.supplier_status_id,
            q.active,
            after_name,
            after_id,
            q.limit.fetch_size()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        rows.into_iter().map(Supplier::try_from).collect()
    }

    async fn save_supplier(&self, s: &Supplier, audit: &AuditRecord) -> Result<(), RepoError> {
        let d = s.details();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(s.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO suppliers (id, tenant_id, legal_name, trading_name, registration_number, vat_number, supplier_type_id,
                                    supplier_status_id, notes, is_active, created_by_kind, created_by_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (id) DO UPDATE SET legal_name = EXCLUDED.legal_name, trading_name = EXCLUDED.trading_name,
                 registration_number = EXCLUDED.registration_number, vat_number = EXCLUDED.vat_number,
                 supplier_type_id = EXCLUDED.supplier_type_id, supplier_status_id = EXCLUDED.supplier_status_id,
                 notes = EXCLUDED.notes, is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            s.id().as_uuid(),
            s.tenant_id().as_uuid(),
            d.legal_name,
            d.trading_name,
            d.registration_number,
            d.vat_number,
            d.supplier_type_id,
            d.supplier_status_id,
            d.notes,
            d.is_active,
            s.created_by().kind().as_str(),
            s.created_by().id(),
            s.created_at(),
            s.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        write(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn contact(
        &self,
        scope: &TenantScope,
        id: ContactId,
    ) -> Result<Option<Contact>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            ContactRow,
            "SELECT id, tenant_id, supplier_id, full_name, job_title, email, phone, mobile, notes, is_active, created_at, updated_at
             FROM supplier_contacts WHERE id = $1 AND ($2 OR tenant_id = ANY($3))",
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(row.map(Contact::from))
    }

    async fn contacts(
        &self,
        scope: &TenantScope,
        supplier: SupplierId,
    ) -> Result<Vec<Contact>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ContactRow,
            "SELECT id, tenant_id, supplier_id, full_name, job_title, email, phone, mobile, notes, is_active, created_at, updated_at
             FROM supplier_contacts WHERE supplier_id = $1 AND ($2 OR tenant_id = ANY($3))
             ORDER BY lower(full_name), id LIMIT 1000",
            supplier.as_uuid(),
            all,
            &tenants
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(rows.into_iter().map(Contact::from).collect())
    }

    async fn save_contact(&self, c: &Contact, audit: &AuditRecord) -> Result<(), RepoError> {
        let d = c.details();
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        sqlx::query!(
            "INSERT INTO supplier_contacts (id, tenant_id, supplier_id, full_name, job_title, email, phone, mobile, notes,
                                            is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (id) DO UPDATE SET full_name = EXCLUDED.full_name, job_title = EXCLUDED.job_title,
                 email = EXCLUDED.email, phone = EXCLUDED.phone, mobile = EXCLUDED.mobile, notes = EXCLUDED.notes,
                 is_active = EXCLUDED.is_active, updated_at = EXCLUDED.updated_at",
            c.id().as_uuid(),
            c.tenant_id().as_uuid(),
            c.supplier_id().as_uuid(),
            d.full_name,
            d.job_title,
            d.email,
            d.phone,
            d.mobile,
            d.notes,
            d.is_active,
            c.created_at(),
            c.updated_at()
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        write(&mut tx, audit, &[]).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn contract(
        &self,
        scope: &TenantScope,
        id: ContractId,
    ) -> Result<Option<Contract>, RepoError> {
        let (all, tenants) = ids(scope);
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let row = sqlx::query_as!(
            ContractRow,
            r#"SELECT c.id, c.tenant_id, c.contract_number, c.title, c.description, c.party_kind, c.supplier_id, c.requester_id,
                      c.owner_id, c.contract_status_id, COALESCE(s.is_closed, false) AS "status_closed!", c.start_date, c.end_date,
                      c.notice_period_amount, c.notice_period_unit, (c.value * 100)::bigint AS "value_minor?", c.currency,
                      c.payment_schedule_id, c.cost_centre_id, c.dms_link, c.personal_data_transferred, c.dpia_required,
                      c.dpia_completed_on, c.dpia_link, c.created_by_kind, c.created_by_id, c.created_at, c.updated_at, c.deleted_at
               FROM contracts c LEFT JOIN contract_statuses s ON s.id = c.contract_status_id
               WHERE c.id = $1 AND c.deleted_at IS NULL AND ($2 OR c.tenant_id = ANY($3))"#,
            id.as_uuid(),
            all,
            &tenants
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        row.map(Contract::try_from).transpose()
    }

    async fn contracts(
        &self,
        scope: &TenantScope,
        q: &ContractQuery,
    ) -> Result<Vec<Contract>, RepoError> {
        let (all, tenants) = ids(scope);
        let (after_number, after_id) = q.after.clone().unzip();
        let (notice_from, notice_to) = q.notice_between.unzip();
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let rows = sqlx::query_as!(
            ContractRow,
            r#"SELECT c.id, c.tenant_id, c.contract_number, c.title, c.description, c.party_kind, c.supplier_id, c.requester_id,
                      c.owner_id, c.contract_status_id, COALESCE(s.is_closed, false) AS "status_closed!", c.start_date, c.end_date,
                      c.notice_period_amount, c.notice_period_unit, (c.value * 100)::bigint AS "value_minor?", c.currency,
                      c.payment_schedule_id, c.cost_centre_id, c.dms_link, c.personal_data_transferred, c.dpia_required,
                      c.dpia_completed_on, c.dpia_link, c.created_by_kind, c.created_by_id, c.created_at, c.updated_at, c.deleted_at
               FROM contracts c LEFT JOIN contract_statuses s ON s.id = c.contract_status_id
               WHERE c.deleted_at IS NULL AND ($1 OR c.tenant_id = ANY($2))
                 AND ($3::uuid IS NULL OR c.tenant_id = $3)
                 AND ($4::text IS NULL OR c.contract_number ILIKE $4 OR c.title ILIKE $4)
                 AND ($5::uuid IS NULL OR c.contract_status_id = $5)
                 AND ($6::uuid IS NULL OR c.supplier_id = $6)
                 AND ($7::uuid IS NULL OR c.owner_id = $7)
                 AND ($8::boolean IS NULL OR COALESCE(s.is_closed, false) = NOT $8)
                 AND ($9::date IS NULL OR c.notice_date BETWEEN $9 AND $10::date)
                 AND ($11::text IS NULL OR (lower(c.contract_number), c.id) > (lower($11), $12::uuid))
               ORDER BY lower(c.contract_number), c.id
               LIMIT $13"#,
            all,
            &tenants,
            q.tenant_id.map(|t| t.as_uuid()),
            like(q.search.as_deref()),
            q.status_id,
            q.supplier_id.map(|s| s.as_uuid()),
            q.owner_id.map(|a| a.as_uuid()),
            q.active,
            notice_from,
            notice_to,
            after_number,
            after_id,
            q.limit.fetch_size()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        contracts(rows)
    }

    async fn insert_contract(
        &self,
        c: &Contract,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        save_contract(&mut tx, c).await?;
        write(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn update_contract(
        &self,
        c: &Contract,
        audit: &AuditRecord,
        events: &[EventEnvelope],
    ) -> Result<(), RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        save_contract(&mut tx, c).await?;
        write(&mut tx, audit, events).await?;
        tx.commit().await.map_err(db_err)
    }

    async fn reminder_candidates(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<Contract>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, &TenantScope::All)
            .await
            .map_err(db_err)?;
        let rows = sqlx::query_as!(
            ContractRow,
            r#"SELECT c.id, c.tenant_id, c.contract_number, c.title, c.description, c.party_kind, c.supplier_id, c.requester_id,
                      c.owner_id, c.contract_status_id, COALESCE(s.is_closed, false) AS "status_closed!", c.start_date, c.end_date,
                      c.notice_period_amount, c.notice_period_unit, (c.value * 100)::bigint AS "value_minor?", c.currency,
                      c.payment_schedule_id, c.cost_centre_id, c.dms_link, c.personal_data_transferred, c.dpia_required,
                      c.dpia_completed_on, c.dpia_link, c.created_by_kind, c.created_by_id, c.created_at, c.updated_at, c.deleted_at
               FROM contracts c LEFT JOIN contract_statuses s ON s.id = c.contract_status_id
               WHERE c.deleted_at IS NULL AND c.owner_id IS NOT NULL AND NOT COALESCE(s.is_closed, false)
                 AND ((c.notice_date BETWEEN $1 AND $2
                       AND NOT EXISTS (SELECT 1 FROM contract_reminders r WHERE r.contract_id = c.id
                                       AND r.kind = 'notice_date' AND r.reminded_date = c.notice_date))
                   OR (c.end_date BETWEEN $1 AND $2
                       AND NOT EXISTS (SELECT 1 FROM contract_reminders r WHERE r.contract_id = c.id
                                       AND r.kind = 'end_date' AND r.reminded_date = c.end_date)))
               ORDER BY c.id"#,
            from,
            to
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        contracts(rows)
    }

    async fn record_reminder(
        &self,
        c: &Contract,
        kind: ReminderKind,
        date: NaiveDate,
        event: &EventEnvelope,
    ) -> Result<bool, RepoError> {
        let Some(owner) = c.details().owner_id else {
            return Ok(false);
        };
        let mut tx = tenancy::begin(&self.pool, &TenantScope::single(c.tenant_id()))
            .await
            .map_err(db_err)?;
        let n = sqlx::query!(
            "INSERT INTO contract_reminders (tenant_id, contract_id, kind, reminded_date, owner_id, sent_at)
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            c.tenant_id().as_uuid(),
            c.id().as_uuid(),
            kind.as_str(),
            date,
            owner.as_uuid(),
            event.occurred_at
        )
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n == 1 {
            cooper_platform::outbox::append(&mut tx, event)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(n == 1)
    }

    async fn history(
        &self,
        scope: &TenantScope,
        aggregate_type: &str,
        id: Uuid,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError> {
        let mut tx = tenancy::begin(&self.pool, scope).await.map_err(db_err)?;
        let h = cooper_platform::audit::history(&mut tx, aggregate_type, id)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        tx.commit().await.map_err(db_err)?;
        Ok(h)
    }
}

/// Reminds Contract owners of upcoming Notice Dates and end dates (FR-CON-003). Runs
/// hourly; every reminder is sent once, so a missed run or a changed setting catches up.
pub struct ContractReminderJob {
    /// The service.
    pub service: crate::application::ContractsService,
}

#[async_trait]
impl Job for ContractReminderJob {
    fn name(&self) -> &'static str {
        "contract-reminders"
    }

    fn default_schedule(&self) -> Schedule {
        Schedule::every_seconds(3600)
    }

    async fn run(&self, _ctx: &JobContext) -> Result<Option<String>, JobError> {
        let n = self
            .service
            .send_reminders()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(format!("sent {n} Contract reminders")))
    }
}
