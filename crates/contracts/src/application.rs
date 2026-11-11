//! Contracts use cases (ADR-0034).

use std::sync::Arc;

use chrono::{DateTime, Days, NaiveDate, Utc};
use chrono_tz::Tz;
use cooper_kernel::{
    Actor, AuditAction, AuditRecord, Clock, DomainEvent, EventEnvelope, Forbidden, Principal,
    TenantId, TenantScope,
};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Contact, ContactDetails, ContactId, Contract, ContractDetails, ContractId, ContractParty,
    ContractReminderDue, ContractsError, Lookup, LookupInput, LookupKind, NoticeStatus, Supplier,
    SupplierDetails, SupplierId,
};
use crate::ports::{
    ContractQuery, ContractSettings, ContractSettingsProvider, ContractsRepository, Directory,
    RepoError, SupplierQuery,
};

/// Capabilities Contracts checks (FR-IAM-010).
pub mod capabilities {
    /// See Suppliers, Contacts and Contracts.
    pub const READ: &str = "contracts.read";
    /// Create and edit Suppliers, Contacts and Contracts; delete Contracts.
    pub const WRITE: &str = "contracts.write";
    /// Configure Supplier Types, Supplier Statuses, Contract Statuses and Payment
    /// Schedules.
    pub const CONFIGURE: &str = "contracts.configure";

    /// Every Capability declared by Contracts.
    pub const ALL: &[&str] = &[READ, WRITE, CONFIGURE];
}

/// System Settings declared by Contracts (FR-SYS-001).
pub mod settings {
    use cooper_kernel::{SettingDefinition, SettingKind};

    /// Days ahead of a Notice Date or end date to remind the Contract owner.
    pub const REMINDER_DAYS: SettingDefinition = SettingDefinition {
        key: "contracts.reminder_days",
        description: "Days ahead of a Contract's Notice Date and end date to remind its owner (0 turns reminders off). Contracts list a Notice Date within this window as upcoming.",
        kind: SettingKind::Integer { min: 0, max: 365 },
        default: "30",
    };

    /// Every setting declared by Contracts.
    pub const ALL: &[SettingDefinition] = &[REMINDER_DAYS];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] ContractsError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Conflict(c) if c == "contracts_number_key" => {
                Self::Conflict("a Contract with this number already exists in the Company".into())
            }
            RepoError::Conflict(c) if c == "suppliers_legal_name_key" => Self::Conflict(
                "a Supplier with this legal name already exists in the Company".into(),
            ),
            RepoError::Conflict(c) if c.ends_with("_name_key") => {
                Self::Conflict("a row with this name already exists".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(ContractsError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "contracts storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct ContractsPorts {
    /// Storage.
    pub contracts: Arc<dyn ContractsRepository>,
    /// Analysts, Requesters and Cost Centres.
    pub directory: Arc<dyn Directory>,
    /// System Settings.
    pub settings: Arc<dyn ContractSettingsProvider>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A Contract with where its Notice Date stands today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractView {
    /// The Contract.
    pub contract: Contract,
    /// Its Notice Date relative to today and the reminder window.
    pub notice_status: NoticeStatus,
}

/// What another context may know about a Supplier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierSummary {
    /// ID.
    pub id: SupplierId,
    /// Company.
    pub tenant_id: TenantId,
    /// Legal name.
    pub legal_name: String,
    /// Trading name.
    pub trading_name: Option<String>,
    /// Active.
    pub is_active: bool,
}

/// What another context may know about a Contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractSummary {
    /// ID.
    pub id: ContractId,
    /// Company.
    pub tenant_id: TenantId,
    /// Contract number.
    pub contract_number: String,
    /// Title.
    pub title: String,
    /// Supplier, if the Contract is with one.
    pub supplier_id: Option<SupplierId>,
    /// End date.
    pub end_date: Option<NaiveDate>,
    /// Notice Date.
    pub notice_date: Option<NaiveDate>,
    /// Not in a closed Contract Status.
    pub is_active: bool,
}

/// Contracts use cases.
#[derive(Clone)]
pub struct ContractsService {
    ports: ContractsPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

fn event<E: DomainEvent>(
    e: &E,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<EventEnvelope, AppError> {
    EventEnvelope::wrap(e, actor, now).map_err(|err| {
        tracing::error!(error = %err, "cannot serialize domain event");
        AppError::Internal
    })
}

/// An audit record of `fields` against what they were (`before`).
fn audit(
    tenant: TenantId,
    aggregate: (&'static str, Uuid),
    action: AuditAction,
    before: &[(&'static str, Value)],
    after: Vec<(&'static str, Value)>,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    after.into_iter().fold(
        AuditRecord::new(Some(tenant), aggregate.0, aggregate.1, action, actor, now),
        |r, (f, v)| {
            r.change(
                f,
                before.iter().find(|(n, _)| *n == f).map(|(_, v)| v),
                Some(&v),
            )
        },
    )
}

/// The calendar day of `now` in `time_zone` (UTC when the name is not an IANA zone).
fn local_today(now: DateTime<Utc>, time_zone: &str) -> NaiveDate {
    let tz = time_zone.parse::<Tz>().unwrap_or_else(|_| {
        tracing::warn!(
            time_zone,
            "general.time_zone is not an IANA time zone; using UTC"
        );
        chrono_tz::UTC
    });
    now.with_timezone(&tz).date_naive()
}

impl ContractsService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: ContractsPorts) -> Self {
        Self { ports }
    }

    /// Today in the instance time zone, and the reminder window in days.
    async fn today(&self) -> (NaiveDate, u32) {
        let ContractSettings {
            reminder_days,
            time_zone,
        } = self.ports.settings.settings().await;
        (
            local_today(self.ports.clock.now(), &time_zone),
            reminder_days,
        )
    }

    // --- Reference data ------------------------------------------------------------------

    /// Lookups of a kind in display order (needs `contracts.read`).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn lookups(
        &self,
        principal: &Principal,
        kind: LookupKind,
    ) -> Result<Vec<Lookup>, AppError> {
        require(principal, capabilities::READ)?;
        let mut rows = self.ports.contracts.lookups(kind).await?;
        rows.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
        Ok(rows)
    }

    async fn lookup(&self, kind: LookupKind, id: Uuid) -> Result<Lookup, AppError> {
        self.ports
            .contracts
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(AppError::NotFound(kind.label()))
    }

    /// Checks a lookup reference: a new value must be an active row of the kind; an
    /// unchanged one only has to exist.
    async fn check_lookup(
        &self,
        kind: LookupKind,
        field: &'static str,
        new: Option<Uuid>,
        old: Option<Uuid>,
    ) -> Result<Option<Lookup>, AppError> {
        let Some(id) = new else {
            return Ok(None);
        };
        let row = self
            .ports
            .contracts
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or_else(|| ContractsError::invalid(field, format!("is not a {}", kind.label())))?;
        if !row.is_active && old != Some(id) {
            return Err(ContractsError::invalid(field, "is not active").into());
        }
        Ok(Some(row))
    }

    /// Creates a lookup (needs `contracts.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_lookup(
        &self,
        principal: &Principal,
        kind: LookupKind,
        input: LookupInput,
    ) -> Result<Lookup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.ports.clock.now();
        let l = Lookup::create(kind, input, now)?;
        let audit = AuditRecord::new(
            None,
            kind.audit_type(),
            l.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("name", None, Some(&l.name))
        .change("is_closed", None, Some(&l.is_closed))
        .change("is_active", None, Some(&l.is_active));
        self.ports.contracts.save_lookup(&l, &audit, &[]).await?;
        Ok(l)
    }

    /// Changes a lookup (needs `contracts.configure`). Opening or closing a Contract
    /// Status changes whether its Contracts are active, which the Calendar hears about.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_lookup(
        &self,
        principal: &Principal,
        kind: LookupKind,
        id: Uuid,
        input: LookupInput,
    ) -> Result<Lookup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut l = self.lookup(kind, id).await?;
        let before = l.clone();
        let now = self.ports.clock.now();
        l.apply(input, now)?;
        let audit = AuditRecord::new(
            None,
            kind.audit_type(),
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("name", Some(&before.name), Some(&l.name))
        .change("is_closed", Some(&before.is_closed), Some(&l.is_closed))
        .change("sort_order", Some(&before.sort_order), Some(&l.sort_order))
        .change("is_active", Some(&before.is_active), Some(&l.is_active));
        let mut events = Vec::new();
        if before.is_closed != l.is_closed {
            for mut c in self.ports.contracts.contracts_with_status(id).await? {
                c.refresh_status(&l);
                events.push(event(&c.dates_changed(), principal.actor(), now)?);
            }
        }
        self.ports
            .contracts
            .save_lookup(&l, &audit, &events)
            .await?;
        Ok(l)
    }

    // --- Suppliers (FR-CON-001) -----------------------------------------------------------

    async fn check_supplier(
        &self,
        d: &SupplierDetails,
        before: Option<&SupplierDetails>,
    ) -> Result<(), AppError> {
        self.check_lookup(
            LookupKind::SupplierType,
            "supplier_type_id",
            d.supplier_type_id,
            before.and_then(|b| b.supplier_type_id),
        )
        .await?;
        self.check_lookup(
            LookupKind::SupplierStatus,
            "supplier_status_id",
            d.supplier_status_id,
            before.and_then(|b| b.supplier_status_id),
        )
        .await?;
        Ok(())
    }

    /// Suppliers in the caller's Companies by legal name (needs `contracts.read`; fetches
    /// `limit + 1`).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn list_suppliers(
        &self,
        principal: &Principal,
        query: &SupplierQuery,
    ) -> Result<Vec<Supplier>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self
            .ports
            .contracts
            .suppliers(principal.scope(), query)
            .await?)
    }

    /// One Supplier (needs `contracts.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get_supplier(
        &self,
        principal: &Principal,
        id: SupplierId,
    ) -> Result<Supplier, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .contracts
            .supplier(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Supplier"))
    }

    /// Creates a Supplier in a Company the caller can see (needs `contracts.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, invalid input or a duplicate legal name.
    pub async fn create_supplier(
        &self,
        principal: &Principal,
        tenant: TenantId,
        details: SupplierDetails,
    ) -> Result<Supplier, AppError> {
        require(principal, capabilities::WRITE)?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        self.check_supplier(&details, None).await?;
        let now = self.ports.clock.now();
        let s = Supplier::create(tenant, details, principal.actor(), now)?;
        let record = audit(
            tenant,
            ("supplier", s.id().as_uuid()),
            AuditAction::Created,
            &[],
            s.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.contracts.save_supplier(&s, &record).await?;
        Ok(s)
    }

    /// Edits a Supplier (needs `contracts.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate legal name.
    pub async fn edit_supplier(
        &self,
        principal: &Principal,
        id: SupplierId,
        details: SupplierDetails,
    ) -> Result<Supplier, AppError> {
        require(principal, capabilities::WRITE)?;
        let mut s = self.get_supplier(principal, id).await?;
        self.check_supplier(&details, Some(s.details())).await?;
        let before = s.audit_fields();
        let now = self.ports.clock.now();
        s.edit(details, now)?;
        let record = audit(
            s.tenant_id(),
            ("supplier", id.as_uuid()),
            AuditAction::Updated,
            &before,
            s.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.contracts.save_supplier(&s, &record).await?;
        Ok(s)
    }

    // --- Contacts (FR-CON-001) ------------------------------------------------------------

    /// The Contacts of a Supplier (needs `contracts.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or the Supplier is not found.
    pub async fn contacts(
        &self,
        principal: &Principal,
        supplier: SupplierId,
    ) -> Result<Vec<Contact>, AppError> {
        let s = self.get_supplier(principal, supplier).await?;
        Ok(self
            .ports
            .contracts
            .contacts(principal.scope(), s.id())
            .await?)
    }

    /// One Contact (needs `contracts.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get_contact(
        &self,
        principal: &Principal,
        id: ContactId,
    ) -> Result<Contact, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .contracts
            .contact(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Contact"))
    }

    /// Adds a Contact to a Supplier (needs `contracts.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, the Supplier is not found, or invalid input.
    pub async fn create_contact(
        &self,
        principal: &Principal,
        supplier: SupplierId,
        details: ContactDetails,
    ) -> Result<Contact, AppError> {
        require(principal, capabilities::WRITE)?;
        let s = self.get_supplier(principal, supplier).await?;
        let now = self.ports.clock.now();
        let c = Contact::create(&s, details, now)?;
        let record = audit(
            c.tenant_id(),
            ("contact", c.id().as_uuid()),
            AuditAction::Created,
            &[],
            c.audit_fields(),
            principal.actor(),
            now,
        )
        .change("supplier_id", None, Some(&s.id()));
        self.ports.contracts.save_contact(&c, &record).await?;
        Ok(c)
    }

    /// Edits a Contact (needs `contracts.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid input.
    pub async fn edit_contact(
        &self,
        principal: &Principal,
        id: ContactId,
        details: ContactDetails,
    ) -> Result<Contact, AppError> {
        require(principal, capabilities::WRITE)?;
        let mut c = self.get_contact(principal, id).await?;
        let before = c.audit_fields();
        let now = self.ports.clock.now();
        c.edit(details, now)?;
        let record = audit(
            c.tenant_id(),
            ("contact", id.as_uuid()),
            AuditAction::Updated,
            &before,
            c.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.contracts.save_contact(&c, &record).await?;
        Ok(c)
    }

    // --- Contracts (FR-CON-002 to FR-CON-004) ----------------------------------------------

    /// Checks the references of Contract details in `tenant`; returns the Contract Status.
    /// Unchanged references only have to exist; new ones must be active and in the
    /// Contract's Company.
    async fn check_contract(
        &self,
        tenant: TenantId,
        d: &ContractDetails,
        before: Option<&ContractDetails>,
    ) -> Result<Option<Lookup>, AppError> {
        let changed =
            |f: &dyn Fn(&ContractDetails) -> Option<Uuid>| before.is_none_or(|b| f(b) != f(d));
        match d.party {
            ContractParty::Supplier(id) => {
                let supplier = self
                    .ports
                    .contracts
                    .supplier(&TenantScope::single(tenant), id)
                    .await?
                    .ok_or_else(|| {
                        ContractsError::invalid("supplier_id", "is not a Supplier of the Company")
                    })?;
                if !supplier.is_active() && changed(&|x| x.party.supplier_id().map(|s| s.as_uuid()))
                {
                    return Err(ContractsError::invalid("supplier_id", "is not active").into());
                }
            }
            ContractParty::Customer(Some(r))
                if changed(&|x| x.party.requester_id().map(|r| r.as_uuid())) =>
            {
                if self.ports.directory.requester_company(r).await? != Some(tenant) {
                    return Err(ContractsError::invalid(
                        "requester_id",
                        "is not a Requester of the Company",
                    )
                    .into());
                }
            }
            ContractParty::Customer(_) => {}
        }
        if let Some(owner) = d.owner_id
            && changed(&|x| x.owner_id.map(|a| a.as_uuid()))
            && !self.ports.directory.analyst_active(owner).await?
        {
            return Err(ContractsError::invalid("owner_id", "is not an active Analyst").into());
        }
        if let Some(cc) = d.cost_centre_id
            && changed(&|x| x.cost_centre_id)
            && self.ports.directory.cost_centre_company(cc).await? != Some(tenant)
        {
            return Err(ContractsError::invalid(
                "cost_centre_id",
                "is not a Cost Centre of the Company",
            )
            .into());
        }
        self.check_lookup(
            LookupKind::PaymentSchedule,
            "payment_schedule_id",
            d.payment_schedule_id,
            before.and_then(|b| b.payment_schedule_id),
        )
        .await?;
        self.check_lookup(
            LookupKind::ContractStatus,
            "status_id",
            d.status_id,
            before.and_then(|b| b.status_id),
        )
        .await
    }

    /// Contracts in the caller's Companies by number (needs `contracts.read`; fetches
    /// `limit + 1`). With `notice_due`, only active Contracts whose Notice Date is from
    /// today up to the reminder window ahead.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn list_contracts(
        &self,
        principal: &Principal,
        mut query: ContractQuery,
        notice_due: bool,
    ) -> Result<Vec<ContractView>, AppError> {
        require(principal, capabilities::READ)?;
        let (today, window) = self.today().await;
        if notice_due {
            let until = today
                .checked_add_days(Days::new(u64::from(window)))
                .unwrap_or(NaiveDate::MAX);
            query.notice_between = Some((today, until));
            query.active = Some(true);
        }
        Ok(self
            .ports
            .contracts
            .contracts(principal.scope(), &query)
            .await?
            .into_iter()
            .map(|contract| ContractView {
                notice_status: contract.notice_status(today, window),
                contract,
            })
            .collect())
    }

    async fn find_contract(
        &self,
        principal: &Principal,
        id: ContractId,
    ) -> Result<Contract, AppError> {
        self.ports
            .contracts
            .contract(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Contract"))
    }

    async fn view(&self, contract: Contract) -> ContractView {
        let (today, window) = self.today().await;
        ContractView {
            notice_status: contract.notice_status(today, window),
            contract,
        }
    }

    /// One Contract (needs `contracts.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get_contract(
        &self,
        principal: &Principal,
        id: ContractId,
    ) -> Result<ContractView, AppError> {
        require(principal, capabilities::READ)?;
        let c = self.find_contract(principal, id).await?;
        Ok(self.view(c).await)
    }

    /// Creates a Contract in a Company the caller can see (needs `contracts.write`). The
    /// Calendar hears about its dates.
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, invalid input or a duplicate number.
    pub async fn create_contract(
        &self,
        principal: &Principal,
        tenant: TenantId,
        details: ContractDetails,
    ) -> Result<ContractView, AppError> {
        require(principal, capabilities::WRITE)?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        let status = self.check_contract(tenant, &details, None).await?;
        let now = self.ports.clock.now();
        let c = Contract::create(tenant, details, status.as_ref(), principal.actor(), now)?;
        let record = audit(
            tenant,
            ("contract", c.id().as_uuid()),
            AuditAction::Created,
            &[],
            c.audit_fields(),
            principal.actor(),
            now,
        );
        let events = [event(&c.dates_changed(), principal.actor(), now)?];
        self.ports
            .contracts
            .insert_contract(&c, &record, &events)
            .await?;
        Ok(self.view(c).await)
    }

    /// Edits a Contract (needs `contracts.write`). When its number, title, Notice Date,
    /// end date or active state changes, the Calendar hears about it.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate number.
    pub async fn edit_contract(
        &self,
        principal: &Principal,
        id: ContractId,
        details: ContractDetails,
    ) -> Result<ContractView, AppError> {
        require(principal, capabilities::WRITE)?;
        let mut c = self.find_contract(principal, id).await?;
        let status = self
            .check_contract(c.tenant_id(), &details, Some(c.details()))
            .await?;
        let (before, facts) = (c.audit_fields(), c.dates_changed());
        let now = self.ports.clock.now();
        c.edit(details, status.as_ref(), now)?;
        let record = audit(
            c.tenant_id(),
            ("contract", id.as_uuid()),
            AuditAction::Updated,
            &before,
            c.audit_fields(),
            principal.actor(),
            now,
        );
        let after = c.dates_changed();
        let events = if after == facts {
            Vec::new()
        } else {
            vec![event(&after, principal.actor(), now)?]
        };
        self.ports
            .contracts
            .update_contract(&c, &record, &events)
            .await?;
        Ok(self.view(c).await)
    }

    /// Deletes a Contract (soft delete; needs `contracts.write`). Its Calendar Events go.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn delete_contract(
        &self,
        principal: &Principal,
        id: ContractId,
    ) -> Result<(), AppError> {
        require(principal, capabilities::WRITE)?;
        let mut c = self.find_contract(principal, id).await?;
        let now = self.ports.clock.now();
        c.delete(now)?;
        let record = AuditRecord::new(
            Some(c.tenant_id()),
            "contract",
            id.as_uuid(),
            AuditAction::Deleted,
            principal.actor(),
            now,
        );
        let events = [event(&c.dates_changed(), principal.actor(), now)?];
        self.ports
            .contracts
            .update_contract(&c, &record, &events)
            .await?;
        Ok(())
    }

    /// Audit history of a Supplier, Contact or Contract the caller can see (needs
    /// `contracts.read`), oldest first.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn history(
        &self,
        principal: &Principal,
        aggregate_type: &'static str,
        id: Uuid,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, AppError> {
        match aggregate_type {
            "supplier" => {
                self.get_supplier(principal, SupplierId::from_uuid(id))
                    .await?;
            }
            "contact" => {
                self.get_contact(principal, ContactId::from_uuid(id))
                    .await?;
            }
            _ => {
                require(principal, capabilities::READ)?;
                self.find_contract(principal, ContractId::from_uuid(id))
                    .await?;
            }
        }
        Ok(self
            .ports
            .contracts
            .history(principal.scope(), aggregate_type, id)
            .await?)
    }

    // --- Reminders (FR-CON-003) --------------------------------------------------------------

    /// Reminds Contract owners of Notice Dates and end dates coming up within
    /// `contracts.reminder_days` (the background job). Each reminder is raised at most once
    /// per Contract, date kind and date; a changed date is reminded again.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn send_reminders(&self) -> Result<usize, AppError> {
        let (today, window) = self.today().await;
        if window == 0 {
            return Ok(0);
        }
        let until = today
            .checked_add_days(Days::new(u64::from(window)))
            .unwrap_or(NaiveDate::MAX);
        let now = self.ports.clock.now();
        let mut sent = 0;
        for c in self
            .ports
            .contracts
            .reminder_candidates(today, until)
            .await?
        {
            let Some(owner) = c.details().owner_id else {
                continue;
            };
            for (kind, date) in c.due_reminders(today, window) {
                let e = ContractReminderDue {
                    contract_id: c.id().as_uuid(),
                    tenant_id: c.tenant_id(),
                    owner_id: owner,
                    kind,
                    date,
                    contract_number: c.details().contract_number.clone(),
                    title: c.details().title.clone(),
                };
                let envelope = event(&e, Actor::Automation, now)?;
                if self
                    .ports
                    .contracts
                    .record_reminder(&c, kind, date, &envelope)
                    .await?
                {
                    sent += 1;
                }
            }
        }
        Ok(sent)
    }

    /// A Contract for a Notification (Automation; no permission checks). `None` when
    /// deleted.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn for_notifications(&self, id: ContractId) -> Result<Option<Contract>, AppError> {
        Ok(self.ports.contracts.contract(&TenantScope::All, id).await?)
    }

    // --- Public API for other contexts -------------------------------------------------------

    /// Whether a Supplier visible in `scope` exists in Company `tenant` (for other contexts
    /// validating a `supplier_id`, for example Assets).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn supplier_exists(
        &self,
        scope: &TenantScope,
        tenant: TenantId,
        id: SupplierId,
    ) -> Result<bool, AppError> {
        Ok(self
            .supplier_summary(scope, id)
            .await?
            .is_some_and(|s| s.tenant_id == tenant))
    }

    /// Whether a Contract (not deleted) visible in `scope` exists in Company `tenant` (for
    /// other contexts validating a Contract link, for example Assets and Tasks).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn contract_exists(
        &self,
        scope: &TenantScope,
        tenant: TenantId,
        id: ContractId,
    ) -> Result<bool, AppError> {
        Ok(self
            .contract_summary(scope, id)
            .await?
            .is_some_and(|c| c.tenant_id == tenant))
    }

    /// A Supplier visible in `scope`, for other contexts.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn supplier_summary(
        &self,
        scope: &TenantScope,
        id: SupplierId,
    ) -> Result<Option<SupplierSummary>, AppError> {
        Ok(self
            .ports
            .contracts
            .supplier(scope, id)
            .await?
            .map(|s| SupplierSummary {
                id: s.id(),
                tenant_id: s.tenant_id(),
                legal_name: s.details().legal_name.clone(),
                trading_name: s.details().trading_name.clone(),
                is_active: s.is_active(),
            }))
    }

    /// A Contract (not deleted) visible in `scope`, for other contexts.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn contract_summary(
        &self,
        scope: &TenantScope,
        id: ContractId,
    ) -> Result<Option<ContractSummary>, AppError> {
        Ok(self
            .ports
            .contracts
            .contract(scope, id)
            .await?
            .map(|c| ContractSummary {
                id: c.id(),
                tenant_id: c.tenant_id(),
                contract_number: c.details().contract_number.clone(),
                title: c.details().title.clone(),
                supplier_id: c.details().party.supplier_id(),
                end_date: c.details().end_date,
                notice_date: c.notice_date(),
                is_active: c.is_active(),
            }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::TimeZone;
    use cooper_kernel::{AnalystId, Capability, FixedClock, RequesterId};

    use super::*;
    use crate::domain::{NoticePeriod, NoticeUnit, ReminderKind};

    #[derive(Default)]
    struct Fake {
        lookups: Mutex<Vec<Lookup>>,
        suppliers: Mutex<BTreeMap<SupplierId, Supplier>>,
        contacts: Mutex<BTreeMap<ContactId, Contact>>,
        contracts: Mutex<BTreeMap<ContractId, Contract>>,
        reminders: Mutex<BTreeSet<(ContractId, ReminderKind, NaiveDate)>>,
        events: Mutex<Vec<EventEnvelope>>,
        audits: Mutex<Vec<AuditRecord>>,
    }

    impl Fake {
        fn events(&self, kind: &str) -> Vec<Value> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.event_type == kind)
                .map(|e| e.payload.clone())
                .collect()
        }
    }

    #[async_trait]
    impl ContractsRepository for Fake {
        async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
            Ok(self
                .lookups
                .lock()
                .unwrap()
                .iter()
                .filter(|l| l.kind == kind)
                .cloned()
                .collect())
        }
        async fn save_lookup(
            &self,
            lookup: &Lookup,
            audit: &AuditRecord,
            events: &[EventEnvelope],
        ) -> Result<(), RepoError> {
            let mut rows = self.lookups.lock().unwrap();
            rows.retain(|l| l.id != lookup.id);
            rows.push(lookup.clone());
            for c in self.contracts.lock().unwrap().values_mut() {
                c.refresh_status(lookup);
            }
            self.audits.lock().unwrap().push(audit.clone());
            self.events.lock().unwrap().extend_from_slice(events);
            Ok(())
        }
        async fn contracts_with_status(&self, status: Uuid) -> Result<Vec<Contract>, RepoError> {
            Ok(self
                .contracts
                .lock()
                .unwrap()
                .values()
                .filter(|c| c.deleted_at().is_none() && c.details().status_id == Some(status))
                .cloned()
                .collect())
        }
        async fn supplier(
            &self,
            scope: &TenantScope,
            id: SupplierId,
        ) -> Result<Option<Supplier>, RepoError> {
            Ok(self
                .suppliers
                .lock()
                .unwrap()
                .get(&id)
                .filter(|s| scope.allows(s.tenant_id()))
                .cloned())
        }
        async fn suppliers(
            &self,
            scope: &TenantScope,
            _q: &SupplierQuery,
        ) -> Result<Vec<Supplier>, RepoError> {
            Ok(self
                .suppliers
                .lock()
                .unwrap()
                .values()
                .filter(|s| scope.allows(s.tenant_id()))
                .cloned()
                .collect())
        }
        async fn save_supplier(&self, s: &Supplier, audit: &AuditRecord) -> Result<(), RepoError> {
            self.suppliers.lock().unwrap().insert(s.id(), s.clone());
            self.audits.lock().unwrap().push(audit.clone());
            Ok(())
        }
        async fn contact(
            &self,
            scope: &TenantScope,
            id: ContactId,
        ) -> Result<Option<Contact>, RepoError> {
            Ok(self
                .contacts
                .lock()
                .unwrap()
                .get(&id)
                .filter(|c| scope.allows(c.tenant_id()))
                .cloned())
        }
        async fn contacts(
            &self,
            scope: &TenantScope,
            supplier: SupplierId,
        ) -> Result<Vec<Contact>, RepoError> {
            Ok(self
                .contacts
                .lock()
                .unwrap()
                .values()
                .filter(|c| scope.allows(c.tenant_id()) && c.supplier_id() == supplier)
                .cloned()
                .collect())
        }
        async fn save_contact(&self, c: &Contact, audit: &AuditRecord) -> Result<(), RepoError> {
            self.contacts.lock().unwrap().insert(c.id(), c.clone());
            self.audits.lock().unwrap().push(audit.clone());
            Ok(())
        }
        async fn contract(
            &self,
            scope: &TenantScope,
            id: ContractId,
        ) -> Result<Option<Contract>, RepoError> {
            Ok(self
                .contracts
                .lock()
                .unwrap()
                .get(&id)
                .filter(|c| scope.allows(c.tenant_id()) && c.deleted_at().is_none())
                .cloned())
        }
        async fn contracts(
            &self,
            scope: &TenantScope,
            q: &ContractQuery,
        ) -> Result<Vec<Contract>, RepoError> {
            Ok(self
                .contracts
                .lock()
                .unwrap()
                .values()
                .filter(|c| scope.allows(c.tenant_id()) && c.deleted_at().is_none())
                .filter(|c| {
                    q.notice_between.is_none_or(|(from, to)| {
                        c.notice_date().is_some_and(|d| (from..=to).contains(&d))
                    })
                })
                .cloned()
                .collect())
        }
        async fn insert_contract(
            &self,
            c: &Contract,
            audit: &AuditRecord,
            events: &[EventEnvelope],
        ) -> Result<(), RepoError> {
            let mut all = self.contracts.lock().unwrap();
            if all.values().any(|x| {
                x.tenant_id() == c.tenant_id()
                    && x.deleted_at().is_none()
                    && x.details().contract_number == c.details().contract_number
            }) {
                return Err(RepoError::Conflict("contracts_number_key".into()));
            }
            all.insert(c.id(), c.clone());
            self.audits.lock().unwrap().push(audit.clone());
            self.events.lock().unwrap().extend_from_slice(events);
            Ok(())
        }
        async fn update_contract(
            &self,
            c: &Contract,
            audit: &AuditRecord,
            events: &[EventEnvelope],
        ) -> Result<(), RepoError> {
            self.contracts.lock().unwrap().insert(c.id(), c.clone());
            self.audits.lock().unwrap().push(audit.clone());
            self.events.lock().unwrap().extend_from_slice(events);
            Ok(())
        }
        async fn reminder_candidates(
            &self,
            from: NaiveDate,
            to: NaiveDate,
        ) -> Result<Vec<Contract>, RepoError> {
            let window = u32::try_from((to - from).num_days()).unwrap();
            let sent = self.reminders.lock().unwrap().clone();
            Ok(self
                .contracts
                .lock()
                .unwrap()
                .values()
                .filter(|c| {
                    c.due_reminders(from, window)
                        .iter()
                        .any(|(k, d)| !sent.contains(&(c.id(), *k, *d)))
                })
                .cloned()
                .collect())
        }
        async fn record_reminder(
            &self,
            c: &Contract,
            kind: ReminderKind,
            date: NaiveDate,
            event: &EventEnvelope,
        ) -> Result<bool, RepoError> {
            if !self.reminders.lock().unwrap().insert((c.id(), kind, date)) {
                return Ok(false);
            }
            self.events.lock().unwrap().push(event.clone());
            Ok(true)
        }
        async fn history(
            &self,
            _scope: &TenantScope,
            _aggregate_type: &str,
            _id: Uuid,
        ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError> {
            Ok(Vec::new())
        }
    }

    struct People {
        active: BTreeSet<AnalystId>,
        requesters: BTreeMap<RequesterId, TenantId>,
    }

    #[async_trait]
    impl Directory for People {
        async fn analyst_active(&self, analyst: AnalystId) -> Result<bool, RepoError> {
            Ok(self.active.contains(&analyst))
        }
        async fn requester_company(
            &self,
            requester: RequesterId,
        ) -> Result<Option<TenantId>, RepoError> {
            Ok(self.requesters.get(&requester).copied())
        }
        async fn cost_centre_company(&self, _cc: Uuid) -> Result<Option<TenantId>, RepoError> {
            Ok(None)
        }
    }

    struct Settings(u32);

    #[async_trait]
    impl ContractSettingsProvider for Settings {
        async fn settings(&self) -> ContractSettings {
            ContractSettings {
                reminder_days: self.0,
                time_zone: "Europe/London".into(),
            }
        }
    }

    struct World {
        repo: Arc<Fake>,
        clock: Arc<FixedClock>,
        service: ContractsService,
        owner: AnalystId,
        requester: RequesterId,
        tenant: TenantId,
        other: TenantId,
    }

    fn day(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn world(reminder_days: u32) -> World {
        let repo = Arc::new(Fake::default());
        let clock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2027, 3, 1, 8, 0, 0).unwrap(),
        ));
        let (owner, requester, tenant, other) = (
            AnalystId::new(),
            RequesterId::new(),
            TenantId::new(),
            TenantId::new(),
        );
        let service = ContractsService::new(ContractsPorts {
            contracts: repo.clone(),
            directory: Arc::new(People {
                active: BTreeSet::from([owner]),
                requesters: BTreeMap::from([(requester, tenant)]),
            }),
            settings: Arc::new(Settings(reminder_days)),
            clock: clock.clone(),
        });
        World {
            repo,
            clock,
            service,
            owner,
            requester,
            tenant,
            other,
        }
    }

    fn analyst(scope: TenantScope) -> Principal {
        let caps = capabilities::ALL
            .iter()
            .filter_map(|c| Capability::from_static(c))
            .collect();
        Principal::new(Actor::Analyst(AnalystId::new()), false, caps, scope)
    }

    impl World {
        fn me(&self) -> Principal {
            analyst(TenantScope::single(self.tenant))
        }

        fn details(&self, number: &str) -> ContractDetails {
            ContractDetails {
                contract_number: number.into(),
                title: "Printer lease".into(),
                owner_id: Some(self.owner),
                start_date: Some(day("2026-01-01")),
                end_date: Some(day("2027-06-30")),
                notice_period: Some(NoticePeriod::new(3, NoticeUnit::Months).unwrap()),
                ..ContractDetails::default()
            }
        }

        async fn status(&self, name: &str, closed: bool) -> Lookup {
            self.service
                .create_lookup(
                    &self.me(),
                    LookupKind::ContractStatus,
                    LookupInput {
                        name: name.into(),
                        is_closed: closed,
                        sort_order: 0,
                        is_active: true,
                    },
                )
                .await
                .unwrap()
        }

        async fn supplier(&self, tenant: TenantId, name: &str) -> Supplier {
            self.service
                .create_supplier(
                    &analyst(TenantScope::All),
                    tenant,
                    SupplierDetails {
                        legal_name: name.into(),
                        is_active: true,
                        ..SupplierDetails::default()
                    },
                )
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn creating_editing_and_deleting_a_contract_tells_the_calendar() {
        let w = world(30);
        let c = w
            .service
            .create_contract(&w.me(), w.tenant, w.details("CON-1"))
            .await
            .unwrap()
            .contract;
        let events = w.repo.events("contract.dates_changed");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["notice_date"], "2027-03-31");
        assert_eq!(events[0]["active"], true);

        // A description change is not a Calendar fact.
        let mut d = w.details("CON-1");
        d.description = "Floor 2".into();
        w.service
            .edit_contract(&w.me(), c.id(), d.clone())
            .await
            .unwrap();
        assert_eq!(w.repo.events("contract.dates_changed").len(), 1);

        d.notice_period = Some(NoticePeriod::new(30, NoticeUnit::Days).unwrap());
        w.service.edit_contract(&w.me(), c.id(), d).await.unwrap();
        let events = w.repo.events("contract.dates_changed");
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["notice_date"], "2027-05-31");

        w.service.delete_contract(&w.me(), c.id()).await.unwrap();
        let events = w.repo.events("contract.dates_changed");
        assert_eq!(events.len(), 3);
        assert_eq!(events[2]["active"], false);
        assert_eq!(
            w.service.get_contract(&w.me(), c.id()).await,
            Err(AppError::NotFound("Contract"))
        );
        let actions: Vec<_> = w
            .repo
            .audits
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.action)
            .collect();
        assert_eq!(
            actions,
            [
                AuditAction::Created,
                AuditAction::Updated,
                AuditAction::Updated,
                AuditAction::Deleted
            ]
        );
    }

    #[tokio::test]
    async fn closing_a_contract_status_deactivates_its_contracts_on_the_calendar() {
        let w = world(30);
        let live = w.status("Live", false).await;
        let mut d = w.details("CON-1");
        d.status_id = Some(live.id);
        let c = w
            .service
            .create_contract(&w.me(), w.tenant, d)
            .await
            .unwrap();
        assert!(c.contract.is_active());
        w.service
            .update_lookup(
                &w.me(),
                LookupKind::ContractStatus,
                live.id,
                LookupInput {
                    name: "Ended".into(),
                    is_closed: true,
                    sort_order: 0,
                    is_active: true,
                },
            )
            .await
            .unwrap();
        let events = w.repo.events("contract.dates_changed");
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["active"], false);
        // Renaming alone is not a Calendar fact.
        w.service
            .update_lookup(
                &w.me(),
                LookupKind::ContractStatus,
                live.id,
                LookupInput {
                    name: "Expired".into(),
                    is_closed: true,
                    sort_order: 0,
                    is_active: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(w.repo.events("contract.dates_changed").len(), 2);
    }

    #[tokio::test]
    async fn contract_references_must_belong_to_the_company() {
        let w = world(30);
        let theirs = w.supplier(w.other, "Other Ltd").await;
        let ours = w.supplier(w.tenant, "Acme Ltd").await;
        let mut d = w.details("CON-1");
        d.party = ContractParty::Supplier(theirs.id());
        let err = w
            .service
            .create_contract(&w.me(), w.tenant, d.clone())
            .await;
        assert!(matches!(
            err,
            Err(AppError::Rule(ContractsError::Invalid {
                field: "supplier_id",
                ..
            }))
        ));
        d.party = ContractParty::Supplier(ours.id());
        assert!(
            w.service
                .create_contract(&w.me(), w.tenant, d)
                .await
                .is_ok()
        );

        let mut d = w.details("CON-2");
        d.party = ContractParty::Customer(Some(RequesterId::new()));
        let err = w
            .service
            .create_contract(&w.me(), w.tenant, d.clone())
            .await;
        assert!(matches!(
            err,
            Err(AppError::Rule(ContractsError::Invalid {
                field: "requester_id",
                ..
            }))
        ));
        d.party = ContractParty::Customer(Some(w.requester));
        assert!(
            w.service
                .create_contract(&w.me(), w.tenant, d)
                .await
                .is_ok()
        );

        let mut d = w.details("CON-3");
        d.owner_id = Some(AnalystId::new());
        let err = w.service.create_contract(&w.me(), w.tenant, d).await;
        assert!(matches!(
            err,
            Err(AppError::Rule(ContractsError::Invalid {
                field: "owner_id",
                ..
            }))
        ));

        let err = w
            .service
            .create_contract(&w.me(), w.other, w.details("CON-4"))
            .await;
        assert_eq!(err, Err(AppError::NotFound("Company")));
        let err = w
            .service
            .create_contract(&w.me(), w.tenant, w.details("CON-1"))
            .await;
        assert!(matches!(err, Err(AppError::Conflict(_))));
    }

    #[tokio::test]
    async fn inactive_lookups_cannot_be_chosen_but_stay_on_existing_contracts() {
        let w = world(30);
        let live = w.status("Live", false).await;
        let mut d = w.details("CON-1");
        d.status_id = Some(live.id);
        let c = w
            .service
            .create_contract(&w.me(), w.tenant, d.clone())
            .await
            .unwrap();
        w.service
            .update_lookup(
                &w.me(),
                LookupKind::ContractStatus,
                live.id,
                LookupInput {
                    name: "Live".into(),
                    is_closed: false,
                    sort_order: 0,
                    is_active: false,
                },
            )
            .await
            .unwrap();
        d.title = "Renamed".into();
        assert!(
            w.service
                .edit_contract(&w.me(), c.contract.id(), d.clone())
                .await
                .is_ok()
        );
        d.contract_number = "CON-2".into();
        assert!(
            w.service
                .create_contract(&w.me(), w.tenant, d)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reminders_are_sent_once_per_contract_kind_and_date() {
        let w = world(30);
        // Today is 2027-03-01: the Notice Date 2027-03-31 is inside the window.
        let c = w
            .service
            .create_contract(&w.me(), w.tenant, w.details("CON-1"))
            .await
            .unwrap()
            .contract;
        assert_eq!(w.service.send_reminders().await.unwrap(), 1);
        assert_eq!(w.service.send_reminders().await.unwrap(), 0, "idempotent");
        let sent = w.repo.events("contract.reminder_due");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["kind"], "notice_date");
        assert_eq!(sent[0]["date"], "2027-03-31");
        assert_eq!(sent[0]["owner_id"], w.owner.as_uuid().to_string());

        // A changed date is reminded again.
        let mut d = w.details("CON-1");
        d.notice_period = Some(NoticePeriod::new(100, NoticeUnit::Days).unwrap());
        d.end_date = Some(day("2027-06-20"));
        w.service.edit_contract(&w.me(), c.id(), d).await.unwrap();
        assert_eq!(w.service.send_reminders().await.unwrap(), 1);

        // The end date comes into the window later.
        w.clock
            .set(Utc.with_ymd_and_hms(2027, 6, 1, 8, 0, 0).unwrap());
        assert_eq!(w.service.send_reminders().await.unwrap(), 1);
        assert_eq!(w.service.send_reminders().await.unwrap(), 0);
        let kinds: Vec<_> = w
            .repo
            .events("contract.reminder_due")
            .iter()
            .map(|e| (e["kind"].clone(), e["date"].clone()))
            .collect();
        assert_eq!(
            kinds,
            [
                (Value::from("notice_date"), Value::from("2027-03-31")),
                (Value::from("notice_date"), Value::from("2027-03-12")),
                (Value::from("end_date"), Value::from("2027-06-20")),
            ]
        );
    }

    #[tokio::test]
    async fn no_reminders_when_switched_off_or_closed() {
        let w = world(0);
        w.service
            .create_contract(&w.me(), w.tenant, w.details("CON-1"))
            .await
            .unwrap();
        assert_eq!(w.service.send_reminders().await.unwrap(), 0);

        let w = world(30);
        let ended = w.status("Ended", true).await;
        let mut d = w.details("CON-1");
        d.status_id = Some(ended.id);
        w.service
            .create_contract(&w.me(), w.tenant, d)
            .await
            .unwrap();
        assert_eq!(w.service.send_reminders().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn notice_due_lists_upcoming_notice_dates_with_their_status() {
        let w = world(30);
        w.service
            .create_contract(&w.me(), w.tenant, w.details("CON-1"))
            .await
            .unwrap();
        let mut later = w.details("CON-2");
        later.end_date = Some(day("2028-06-30"));
        w.service
            .create_contract(&w.me(), w.tenant, later)
            .await
            .unwrap();
        let all = w
            .service
            .list_contracts(&w.me(), ContractQuery::default(), false)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        let due = w
            .service
            .list_contracts(&w.me(), ContractQuery::default(), true)
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].notice_status, NoticeStatus::Upcoming);
        assert_eq!(due[0].contract.details().contract_number, "CON-1");
    }

    #[tokio::test]
    async fn other_contexts_check_suppliers_and_contracts_by_company() {
        let w = world(30);
        let s = w.supplier(w.tenant, "Acme Ltd").await;
        let c = w
            .service
            .create_contract(&w.me(), w.tenant, w.details("CON-1"))
            .await
            .unwrap()
            .contract;
        let all = TenantScope::All;
        assert!(
            w.service
                .supplier_exists(&all, w.tenant, s.id())
                .await
                .unwrap()
        );
        assert!(
            !w.service
                .supplier_exists(&all, w.other, s.id())
                .await
                .unwrap()
        );
        assert!(
            !w.service
                .supplier_exists(&TenantScope::single(w.other), w.tenant, s.id())
                .await
                .unwrap()
        );
        assert!(
            w.service
                .contract_exists(&all, w.tenant, c.id())
                .await
                .unwrap()
        );
        assert!(
            !w.service
                .contract_exists(&all, w.other, c.id())
                .await
                .unwrap()
        );
        w.service.delete_contract(&w.me(), c.id()).await.unwrap();
        assert!(
            !w.service
                .contract_exists(&all, w.tenant, c.id())
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn suppliers_and_contacts_stay_in_the_callers_companies() {
        let w = world(30);
        let theirs = w.supplier(w.other, "Other Ltd").await;
        assert_eq!(
            w.service.get_supplier(&w.me(), theirs.id()).await,
            Err(AppError::NotFound("Supplier"))
        );
        let err = w
            .service
            .create_contact(
                &w.me(),
                theirs.id(),
                ContactDetails {
                    full_name: "Pat".into(),
                    is_active: true,
                    ..ContactDetails::default()
                },
            )
            .await;
        assert_eq!(err, Err(AppError::NotFound("Supplier")));
        let ours = w.supplier(w.tenant, "Acme Ltd").await;
        let contact = w
            .service
            .create_contact(
                &w.me(),
                ours.id(),
                ContactDetails {
                    full_name: "Pat".into(),
                    is_active: true,
                    ..ContactDetails::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            w.service.contacts(&w.me(), ours.id()).await.unwrap(),
            [contact]
        );
        let reader = Principal::new(
            Actor::Analyst(AnalystId::new()),
            false,
            [Capability::from_static(capabilities::READ).unwrap()].into(),
            TenantScope::single(w.tenant),
        );
        assert!(matches!(
            w.service
                .create_supplier(&reader, w.tenant, SupplierDetails::default())
                .await,
            Err(AppError::Forbidden(_))
        ));
    }
}
