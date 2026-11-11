//! Contracts domain (ADR-0034). Pure Rust: no I/O.
//!
//! - [`Supplier`]: an external vendor of a Company, with its [`Contact`]s.
//! - [`Contract`]: an agreement with a Supplier or with the Company's customer side,
//!   with its [`NoticePeriod`], calculated Notice Date ([`Contract::notice_date`]),
//!   [`Money`] value and [`DataProtection`] fields.
//! - Configurable lookups ([`Lookup`]): Supplier Types, Supplier Statuses, Contract
//!   Statuses (with the `is_closed` behaviour flag) and Payment Schedules.

use chrono::{DateTime, Datelike, Days, Months, NaiveDate, Utc};
use cooper_kernel::{Actor, AnalystId, DomainEvent, RequesterId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

cooper_kernel::define_id!(
    /// Identifies a Supplier.
    SupplierId
);
cooper_kernel::define_id!(
    /// Identifies a Contact (a person at a Supplier).
    ContactId
);
cooper_kernel::define_id!(
    /// Identifies a Contract.
    ContractId
);

/// A Contracts rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContractsError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Not allowed in the current state.
    #[error("{0}")]
    InvalidState(String),
}

impl ContractsError {
    /// A validation error.
    #[must_use]
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }

    /// A state error.
    #[must_use]
    pub fn state(reason: impl Into<String>) -> Self {
        Self::InvalidState(reason.into())
    }
}

// ----- Text helpers -----------------------------------------------------------------------

fn required(field: &'static str, value: &str, max: usize) -> Result<String, ContractsError> {
    let v = value.trim();
    if v.is_empty() || v.chars().count() > max {
        return Err(ContractsError::invalid(
            field,
            format!("must be 1 to {max} characters"),
        ));
    }
    Ok(v.to_owned())
}

fn optional(
    field: &'static str,
    value: Option<String>,
    max: usize,
) -> Result<Option<String>, ContractsError> {
    match value.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) if v.chars().count() > max => Err(ContractsError::invalid(
            field,
            format!("at most {max} characters"),
        )),
        Some(v) => Ok(Some(v.to_owned())),
    }
}

fn notes(field: &'static str, value: &str) -> Result<String, ContractsError> {
    if value.chars().count() > 100_000 {
        return Err(ContractsError::invalid(field, "at most 100000 characters"));
    }
    Ok(value.trim().to_owned())
}

/// An optional `http(s)://` link of at most 2000 characters (DMS and DPIA links).
fn link(field: &'static str, value: Option<String>) -> Result<Option<String>, ContractsError> {
    let Some(v) = optional(field, value, 2000)? else {
        return Ok(None);
    };
    let lower = v.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    if rest.is_none_or(str::is_empty) || v.chars().any(char::is_whitespace) {
        return Err(ContractsError::invalid(
            field,
            "must be an http:// or https:// address",
        ));
    }
    Ok(Some(v))
}

fn email(field: &'static str, value: Option<String>) -> Result<Option<String>, ContractsError> {
    let Some(v) = optional(field, value, 254)? else {
        return Ok(None);
    };
    let valid = v.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && !domain.contains('@')
    }) && !v.chars().any(char::is_whitespace);
    if !valid {
        return Err(ContractsError::invalid(field, "is not an email address"));
    }
    Ok(Some(v))
}

// ----- Reference data ---------------------------------------------------------------------

/// Which configurable lookup (Vocabulary: no seed values, Administrators create them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupKind {
    /// Supplier Type.
    SupplierType,
    /// Supplier Status.
    SupplierStatus,
    /// Contract Status.
    ContractStatus,
    /// Payment Schedule.
    PaymentSchedule,
}

impl LookupKind {
    /// Display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SupplierType => "Supplier Type",
            Self::SupplierStatus => "Supplier Status",
            Self::ContractStatus => "Contract Status",
            Self::PaymentSchedule => "Payment Schedule",
        }
    }

    /// Audit aggregate type.
    #[must_use]
    pub const fn audit_type(self) -> &'static str {
        match self {
            Self::SupplierType => "supplier_type",
            Self::SupplierStatus => "supplier_status",
            Self::ContractStatus => "contract_status",
            Self::PaymentSchedule => "payment_schedule",
        }
    }
}

/// A Supplier Type, Supplier Status, Contract Status or Payment Schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// ID.
    pub id: Uuid,
    /// Kind.
    pub kind: LookupKind,
    /// Name.
    pub name: String,
    /// Contract Statuses: a Contract in this status is finished (terminated, expired,
    /// cancelled). It no longer shows on the Calendar and gets no reminders.
    pub is_closed: bool,
    /// Display order.
    pub sort_order: i32,
    /// Active (inactive rows cannot be chosen for new values).
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// Fields an Administrator edits on a lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LookupInput {
    /// Name.
    pub name: String,
    /// Closed (Contract Statuses only).
    pub is_closed: bool,
    /// Order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl Lookup {
    /// A new lookup.
    ///
    /// # Errors
    ///
    /// Invalid input.
    pub fn create(
        kind: LookupKind,
        input: LookupInput,
        now: DateTime<Utc>,
    ) -> Result<Self, ContractsError> {
        let mut l = Self {
            id: Uuid::now_v7(),
            kind,
            name: String::new(),
            is_closed: false,
            sort_order: 0,
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        l.apply(input, now)?;
        Ok(l)
    }

    /// Changes the lookup. `is_closed` is kept only on Contract Statuses.
    ///
    /// # Errors
    ///
    /// An empty or long name.
    pub fn apply(&mut self, input: LookupInput, now: DateTime<Utc>) -> Result<(), ContractsError> {
        self.name = required("name", &input.name, 50)?;
        self.is_closed = self.kind == LookupKind::ContractStatus && input.is_closed;
        self.sort_order = input.sort_order;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }
}

// ----- Notice period and Notice Date (FR-CON-003) ------------------------------------------

/// The unit of a [`NoticePeriod`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeUnit {
    /// Calendar days.
    Days,
    /// Calendar months.
    Months,
}

impl NoticeUnit {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Days => "days",
            Self::Months => "months",
        }
    }
}

impl std::str::FromStr for NoticeUnit {
    type Err = ContractsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "days" => Ok(Self::Days),
            "months" => Ok(Self::Months),
            _ => Err(ContractsError::invalid(
                "notice_period",
                "unit must be days or months",
            )),
        }
    }
}

/// How long before the end of a Contract notice must be given: a number of calendar
/// days or calendar months.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoticePeriod {
    amount: u16,
    unit: NoticeUnit,
}

/// The last day of `d`'s month.
fn last_day_of_month(d: NaiveDate) -> NaiveDate {
    let mut day = d;
    while let Some(next) = day.succ_opt() {
        if next.month() != day.month() {
            break;
        }
        day = next;
    }
    day
}

/// Whether `d` is the last day of its month.
#[must_use]
pub fn is_month_end(d: NaiveDate) -> bool {
    last_day_of_month(d) == d
}

impl NoticePeriod {
    /// Longest period in days (10 years).
    pub const MAX_DAYS: u16 = 3650;
    /// Longest period in months (10 years).
    pub const MAX_MONTHS: u16 = 120;

    /// A notice period.
    ///
    /// # Errors
    ///
    /// More than 3650 days or 120 months.
    pub fn new(amount: u32, unit: NoticeUnit) -> Result<Self, ContractsError> {
        let max = match unit {
            NoticeUnit::Days => Self::MAX_DAYS,
            NoticeUnit::Months => Self::MAX_MONTHS,
        };
        match u16::try_from(amount) {
            Ok(a) if a <= max => Ok(Self { amount: a, unit }),
            _ => Err(ContractsError::invalid(
                "notice_period",
                format!("at most {max} {}", unit.as_str()),
            )),
        }
    }

    /// The number of days or months.
    #[must_use]
    pub const fn amount(self) -> u16 {
        self.amount
    }

    /// Days or months.
    #[must_use]
    pub const fn unit(self) -> NoticeUnit {
        self.unit
    }

    /// The Notice Date for a Contract ending on `end`: the last date on which notice can
    /// be given.
    ///
    /// - Days: `end` minus that many calendar days.
    /// - Months: the same day of the month that many months earlier. A day that does not
    ///   exist in the earlier month is clamped to its last day (31 May minus 3 months is
    ///   28 or 29 February), and a Contract ending on the last day of a month gives
    ///   notice on the last day of the earlier month (30 June minus 1 month is 31 May).
    ///
    /// `None` only when the date would fall outside the supported calendar.
    #[must_use]
    pub fn notice_date(self, end: NaiveDate) -> Option<NaiveDate> {
        match self.unit {
            NoticeUnit::Days => end.checked_sub_days(Days::new(u64::from(self.amount))),
            NoticeUnit::Months => {
                let earlier = end.checked_sub_months(Months::new(u32::from(self.amount)))?;
                Some(if is_month_end(end) {
                    last_day_of_month(earlier)
                } else {
                    earlier
                })
            }
        }
    }
}

// ----- Money ------------------------------------------------------------------------------

/// An ISO 4217 currency code: three upper-case letters (`GBP`, `EUR`, `USD`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CurrencyCode(String);

impl CurrencyCode {
    /// The code.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CurrencyCode {
    type Error = ContractsError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let v = value.trim().to_ascii_uppercase();
        if v.len() == 3 && v.chars().all(|c| c.is_ascii_uppercase()) {
            Ok(Self(v))
        } else {
            Err(ContractsError::invalid(
                "currency",
                "must be an ISO 4217 code such as GBP",
            ))
        }
    }
}

impl From<CurrencyCode> for String {
    fn from(c: CurrencyCode) -> Self {
        c.0
    }
}

/// An amount of money with two decimal places, stored as `NUMERIC(14,2)`, and its
/// currency. Never a float.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Money {
    minor: i64,
    currency: CurrencyCode,
}

impl Money {
    /// The largest amount in minor units (999 999 999 999.99, `NUMERIC(14,2)`).
    pub const MAX_MINOR: i64 = 99_999_999_999_999;

    /// From minor units (pence, cents).
    ///
    /// # Errors
    ///
    /// Negative or larger than [`Self::MAX_MINOR`].
    pub fn new(minor: i64, currency: CurrencyCode) -> Result<Self, ContractsError> {
        if !(0..=Self::MAX_MINOR).contains(&minor) {
            return Err(ContractsError::invalid(
                "value",
                "must be 0 to 999999999999.99",
            ));
        }
        Ok(Self { minor, currency })
    }

    /// Parses a decimal amount such as `1250`, `1250.5` or `1250.50`.
    ///
    /// # Errors
    ///
    /// Not a non-negative decimal with at most two decimal places, too large, or a bad
    /// currency.
    pub fn parse(amount: &str, currency: &str) -> Result<Self, ContractsError> {
        let currency = CurrencyCode::try_from(currency.to_owned())?;
        let bad = || {
            ContractsError::invalid(
                "value",
                "must be a decimal amount with at most two decimal places",
            )
        };
        let a = amount.trim();
        let (whole, fraction) = a.split_once('.').unwrap_or((a, ""));
        if whole.is_empty()
            || whole.len() > 12
            || fraction.len() > 2
            || (a.contains('.') && fraction.is_empty())
            || !whole
                .chars()
                .chain(fraction.chars())
                .all(|c| c.is_ascii_digit())
        {
            return Err(bad());
        }
        let whole: i64 = whole.parse().map_err(|_| bad())?;
        let cents: i64 = format!("{fraction:0<2}").parse().map_err(|_| bad())?;
        Self::new(whole * 100 + cents, currency)
    }

    /// Minor units.
    #[must_use]
    pub const fn minor_units(&self) -> i64 {
        self.minor
    }

    /// Currency.
    #[must_use]
    pub const fn currency(&self) -> &CurrencyCode {
        &self.currency
    }

    /// The amount with two decimal places, for example `1250.50`.
    #[must_use]
    pub fn amount(&self) -> String {
        format!("{}.{:02}", self.minor / 100, self.minor % 100)
    }
}

// ----- Suppliers and Contacts (FR-CON-001) -------------------------------------------------

/// What an Analyst edits on a Supplier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupplierDetails {
    /// Registered legal name.
    pub legal_name: String,
    /// Name it trades under.
    pub trading_name: Option<String>,
    /// Company registration number.
    pub registration_number: Option<String>,
    /// VAT number.
    pub vat_number: Option<String>,
    /// Supplier Type.
    pub supplier_type_id: Option<Uuid>,
    /// Supplier Status.
    pub supplier_status_id: Option<Uuid>,
    /// Free-text notes.
    pub notes: String,
    /// Active (inactive Suppliers cannot be chosen for new Contracts).
    pub is_active: bool,
}

impl SupplierDetails {
    /// Validates and normalises.
    ///
    /// # Errors
    ///
    /// Missing or long names and numbers.
    pub fn validate(self) -> Result<Self, ContractsError> {
        Ok(Self {
            legal_name: required("legal_name", &self.legal_name, 255)?,
            trading_name: optional("trading_name", self.trading_name, 255)?,
            registration_number: optional("registration_number", self.registration_number, 50)?,
            vat_number: optional("vat_number", self.vat_number, 50)?,
            supplier_type_id: self.supplier_type_id,
            supplier_status_id: self.supplier_status_id,
            notes: notes("notes", &self.notes)?,
            is_active: self.is_active,
        })
    }
}

/// An external vendor of a Company (Vocabulary: Supplier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supplier {
    id: SupplierId,
    tenant_id: TenantId,
    details: SupplierDetails,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Stored state of a Supplier, for [`Supplier::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierRecord {
    /// ID.
    pub id: SupplierId,
    /// Company.
    pub tenant_id: TenantId,
    /// Details.
    pub details: SupplierDetails,
    /// Creator.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl Supplier {
    /// A new Supplier.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn create(
        tenant_id: TenantId,
        details: SupplierDetails,
        created_by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, ContractsError> {
        Ok(Self {
            id: SupplierId::new(),
            tenant_id,
            details: details.validate()?,
            created_by,
            created_at: now,
            updated_at: now,
        })
    }

    /// Restores stored state.
    #[must_use]
    pub fn restore(r: SupplierRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            details: r.details,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    /// Changes the details.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn edit(
        &mut self,
        details: SupplierDetails,
        now: DateTime<Utc>,
    ) -> Result<(), ContractsError> {
        self.details = details.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> SupplierId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &SupplierDetails {
        &self.details
    }
    /// Active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.details.is_active
    }
    /// Creator.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Field values for the audit trail.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        vec![
            ("legal_name", json!(d.legal_name)),
            ("trading_name", json!(d.trading_name)),
            ("registration_number", json!(d.registration_number)),
            ("vat_number", json!(d.vat_number)),
            ("supplier_type_id", json!(d.supplier_type_id)),
            ("supplier_status_id", json!(d.supplier_status_id)),
            ("notes", json!(d.notes)),
            ("is_active", json!(d.is_active)),
        ]
    }
}

/// What an Analyst edits on a Contact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactDetails {
    /// Full name.
    pub full_name: String,
    /// Job title.
    pub job_title: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Phone (direct dial or switchboard).
    pub phone: Option<String>,
    /// Mobile.
    pub mobile: Option<String>,
    /// Free-text notes.
    pub notes: String,
    /// Active.
    pub is_active: bool,
}

impl ContactDetails {
    /// Validates and normalises.
    ///
    /// # Errors
    ///
    /// A missing or long name, a bad email address or long phone numbers.
    pub fn validate(self) -> Result<Self, ContractsError> {
        Ok(Self {
            full_name: required("full_name", &self.full_name, 255)?,
            job_title: optional("job_title", self.job_title, 255)?,
            email: email("email", self.email)?,
            phone: optional("phone", self.phone, 50)?,
            mobile: optional("mobile", self.mobile, 50)?,
            notes: notes("notes", &self.notes)?,
            is_active: self.is_active,
        })
    }
}

/// A person at a Supplier (Vocabulary: Contact; never a Requester).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    id: ContactId,
    tenant_id: TenantId,
    supplier_id: SupplierId,
    details: ContactDetails,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Stored state of a Contact, for [`Contact::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactRecord {
    /// ID.
    pub id: ContactId,
    /// Company.
    pub tenant_id: TenantId,
    /// Supplier.
    pub supplier_id: SupplierId,
    /// Details.
    pub details: ContactDetails,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl Contact {
    /// A new Contact of `supplier`, in the Supplier's Company.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn create(
        supplier: &Supplier,
        details: ContactDetails,
        now: DateTime<Utc>,
    ) -> Result<Self, ContractsError> {
        Ok(Self {
            id: ContactId::new(),
            tenant_id: supplier.tenant_id(),
            supplier_id: supplier.id(),
            details: details.validate()?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Restores stored state.
    #[must_use]
    pub fn restore(r: ContactRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            supplier_id: r.supplier_id,
            details: r.details,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    /// Changes the details.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn edit(
        &mut self,
        details: ContactDetails,
        now: DateTime<Utc>,
    ) -> Result<(), ContractsError> {
        self.details = details.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> ContactId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Supplier.
    #[must_use]
    pub const fn supplier_id(&self) -> SupplierId {
        self.supplier_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &ContactDetails {
        &self.details
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Field values for the audit trail.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        vec![
            ("full_name", json!(d.full_name)),
            ("job_title", json!(d.job_title)),
            ("email", json!(d.email)),
            ("phone", json!(d.phone)),
            ("mobile", json!(d.mobile)),
            ("notes", json!(d.notes)),
            ("is_active", json!(d.is_active)),
        ]
    }
}

// ----- Contracts (FR-CON-002, FR-CON-004) -----------------------------------------------------

/// Who the Contract is with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractParty {
    /// A Supplier of the Company (the Company buys).
    Supplier(SupplierId),
    /// The Company itself as customer (the Company is supplied by IT), optionally with a
    /// Requester as its contact person.
    Customer(Option<RequesterId>),
}

impl Default for ContractParty {
    fn default() -> Self {
        Self::Customer(None)
    }
}

impl ContractParty {
    /// Storage name: `supplier` or `customer`.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Supplier(_) => "supplier",
            Self::Customer(_) => "customer",
        }
    }

    /// The Supplier, if any.
    #[must_use]
    pub const fn supplier_id(self) -> Option<SupplierId> {
        match self {
            Self::Supplier(s) => Some(s),
            Self::Customer(_) => None,
        }
    }

    /// The customer Requester, if any.
    #[must_use]
    pub const fn requester_id(self) -> Option<RequesterId> {
        match self {
            Self::Supplier(_) => None,
            Self::Customer(r) => r,
        }
    }
}

/// Data protection fields of a Contract (FR-CON-004).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataProtection {
    /// Personal data is transferred to the other party.
    pub personal_data_transferred: bool,
    /// A Data Protection Impact Assessment is required.
    pub dpia_required: bool,
    /// When the DPIA was completed.
    pub dpia_completed_on: Option<NaiveDate>,
    /// Link to the DPIA document.
    pub dpia_link: Option<String>,
}

/// What an Analyst edits on a Contract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContractDetails {
    /// Contract number, unique per Company.
    pub contract_number: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Who the Contract is with.
    pub party: ContractParty,
    /// Owner (the Analyst reminded of the Notice Date and end date).
    pub owner_id: Option<AnalystId>,
    /// Contract Status.
    pub status_id: Option<Uuid>,
    /// Start date.
    pub start_date: Option<NaiveDate>,
    /// End date.
    pub end_date: Option<NaiveDate>,
    /// Notice period.
    pub notice_period: Option<NoticePeriod>,
    /// Value and currency.
    pub value: Option<Money>,
    /// Payment Schedule.
    pub payment_schedule_id: Option<Uuid>,
    /// Cost Centre of the Company.
    pub cost_centre_id: Option<Uuid>,
    /// Link to the document management system.
    pub dms_link: Option<String>,
    /// Data protection fields.
    pub data_protection: DataProtection,
}

impl ContractDetails {
    /// Validates and normalises.
    ///
    /// # Errors
    ///
    /// Missing or long texts, dates out of order, bad links, or DPIA details without a
    /// required DPIA.
    pub fn validate(self) -> Result<Self, ContractsError> {
        if let (Some(s), Some(e)) = (self.start_date, self.end_date)
            && s > e
        {
            return Err(ContractsError::invalid(
                "end_date",
                "must not be before the start date",
            ));
        }
        let dp = self.data_protection;
        let dpia_link = link("dpia_link", dp.dpia_link)?;
        if !dp.dpia_required && (dp.dpia_completed_on.is_some() || dpia_link.is_some()) {
            return Err(ContractsError::invalid(
                "dpia_required",
                "a DPIA completion date or link needs a required DPIA",
            ));
        }
        Ok(Self {
            contract_number: required("contract_number", &self.contract_number, 50)?,
            title: required("title", &self.title, 255)?,
            description: notes("description", &self.description)?,
            party: self.party,
            owner_id: self.owner_id,
            status_id: self.status_id,
            start_date: self.start_date,
            end_date: self.end_date,
            notice_period: self.notice_period,
            value: self.value,
            payment_schedule_id: self.payment_schedule_id,
            cost_centre_id: self.cost_centre_id,
            dms_link: link("dms_link", self.dms_link)?,
            data_protection: DataProtection {
                personal_data_transferred: dp.personal_data_transferred,
                dpia_required: dp.dpia_required,
                dpia_completed_on: dp.dpia_completed_on,
                dpia_link,
            },
        })
    }
}

/// Where a Contract's Notice Date stands relative to today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeStatus {
    /// No Notice Date, or the Contract is closed or deleted.
    None,
    /// Further away than the reminder window.
    Later,
    /// Today or within the reminder window: notice must be decided soon.
    Upcoming,
    /// Already passed.
    Passed,
}

/// Which date a reminder is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderKind {
    /// The Notice Date.
    NoticeDate,
    /// The end date.
    EndDate,
}

impl ReminderKind {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoticeDate => "notice_date",
            Self::EndDate => "end_date",
        }
    }
}

/// An agreement with a Supplier or a customer (FR-CON-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    id: ContractId,
    tenant_id: TenantId,
    details: ContractDetails,
    status_closed: bool,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

/// Stored state of a Contract, for [`Contract::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractRecord {
    /// ID.
    pub id: ContractId,
    /// Company.
    pub tenant_id: TenantId,
    /// Details.
    pub details: ContractDetails,
    /// Whether the Contract Status is closed.
    pub status_closed: bool,
    /// Creator.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
    /// Deleted.
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Checks that `status` is the Contract Status named by `status_id`; returns its
/// `is_closed` flag.
fn status_flag(status_id: Option<Uuid>, status: Option<&Lookup>) -> Result<bool, ContractsError> {
    match (status_id, status) {
        (None, None) => Ok(false),
        (Some(id), Some(s)) if s.id == id && s.kind == LookupKind::ContractStatus => {
            Ok(s.is_closed)
        }
        _ => Err(ContractsError::invalid(
            "status_id",
            "is not a Contract Status",
        )),
    }
}

impl Contract {
    /// A new Contract.
    ///
    /// # Errors
    ///
    /// Invalid details, or `status` is not the Contract Status of the details.
    pub fn create(
        tenant_id: TenantId,
        details: ContractDetails,
        status: Option<&Lookup>,
        created_by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, ContractsError> {
        let details = details.validate()?;
        let status_closed = status_flag(details.status_id, status)?;
        Ok(Self {
            id: ContractId::new(),
            tenant_id,
            details,
            status_closed,
            created_by,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }

    /// Restores stored state.
    #[must_use]
    pub fn restore(r: ContractRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            details: r.details,
            status_closed: r.status_closed,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        }
    }

    /// Changes the details.
    ///
    /// # Errors
    ///
    /// Deleted, invalid details, or `status` is not the Contract Status of the details.
    pub fn edit(
        &mut self,
        details: ContractDetails,
        status: Option<&Lookup>,
        now: DateTime<Utc>,
    ) -> Result<(), ContractsError> {
        self.ensure_live()?;
        let details = details.validate()?;
        self.status_closed = status_flag(details.status_id, status)?;
        self.details = details;
        self.updated_at = now;
        Ok(())
    }

    /// Picks up a changed `is_closed` flag of its Contract Status.
    pub fn refresh_status(&mut self, status: &Lookup) {
        if self.details.status_id == Some(status.id) {
            self.status_closed = status.is_closed;
        }
    }

    /// Deletes the Contract (soft delete).
    ///
    /// # Errors
    ///
    /// Already deleted.
    pub fn delete(&mut self, now: DateTime<Utc>) -> Result<(), ContractsError> {
        self.ensure_live()?;
        self.deleted_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    fn ensure_live(&self) -> Result<(), ContractsError> {
        if self.deleted_at.is_some() {
            return Err(ContractsError::state("the Contract is deleted"));
        }
        Ok(())
    }

    /// The Notice Date: the end date minus the notice period (FR-CON-003). `None` without
    /// an end date or a notice period.
    #[must_use]
    pub fn notice_date(&self) -> Option<NaiveDate> {
        self.details
            .notice_period
            .zip(self.details.end_date)
            .and_then(|(p, end)| p.notice_date(end))
    }

    /// Whether the Contract counts: not deleted and not in a closed Contract Status.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.deleted_at.is_none() && !self.status_closed
    }

    /// Where the Notice Date stands on `today`, with reminders `window_days` ahead.
    #[must_use]
    pub fn notice_status(&self, today: NaiveDate, window_days: u32) -> NoticeStatus {
        match self.notice_date() {
            Some(date) if self.is_active() => {
                if date < today {
                    NoticeStatus::Passed
                } else if today
                    .checked_add_days(Days::new(u64::from(window_days)))
                    .is_none_or(|limit| date <= limit)
                {
                    NoticeStatus::Upcoming
                } else {
                    NoticeStatus::Later
                }
            }
            _ => NoticeStatus::None,
        }
    }

    /// The reminders due on `today` with a window of `window_days` (FR-CON-003): the
    /// Notice Date and the end date that fall from today up to `window_days` ahead, for an
    /// active Contract with an owner. Nothing when `window_days` is 0 (reminders off).
    #[must_use]
    pub fn due_reminders(
        &self,
        today: NaiveDate,
        window_days: u32,
    ) -> Vec<(ReminderKind, NaiveDate)> {
        if window_days == 0 || !self.is_active() || self.details.owner_id.is_none() {
            return Vec::new();
        }
        let Some(limit) = today.checked_add_days(Days::new(u64::from(window_days))) else {
            return Vec::new();
        };
        [
            (ReminderKind::NoticeDate, self.notice_date()),
            (ReminderKind::EndDate, self.details.end_date),
        ]
        .into_iter()
        .filter_map(|(k, d)| d.filter(|d| (today..=limit).contains(d)).map(|d| (k, d)))
        .collect()
    }

    /// The Calendar facts of the Contract, as the `contract.dates_changed` event.
    #[must_use]
    pub fn dates_changed(&self) -> ContractDatesChanged {
        ContractDatesChanged {
            contract_id: self.id.as_uuid(),
            contract_number: self.details.contract_number.clone(),
            title: self.details.title.clone(),
            notice_date: self.notice_date(),
            end_date: self.details.end_date,
            active: self.is_active(),
            tenant_id: self.tenant_id,
        }
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> ContractId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &ContractDetails {
        &self.details
    }
    /// Whether the Contract Status is closed.
    #[must_use]
    pub const fn status_closed(&self) -> bool {
        self.status_closed
    }
    /// Creator.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    /// Deleted.
    #[must_use]
    pub const fn deleted_at(&self) -> Option<DateTime<Utc>> {
        self.deleted_at
    }

    /// Field values for the audit trail.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        let dp = &d.data_protection;
        vec![
            ("contract_number", json!(d.contract_number)),
            ("title", json!(d.title)),
            ("description", json!(d.description)),
            ("party", json!(d.party.kind())),
            ("supplier_id", json!(d.party.supplier_id())),
            ("requester_id", json!(d.party.requester_id())),
            ("owner_id", json!(d.owner_id)),
            ("status_id", json!(d.status_id)),
            ("start_date", json!(d.start_date)),
            ("end_date", json!(d.end_date)),
            ("notice_period", json!(d.notice_period)),
            ("notice_date", json!(self.notice_date())),
            ("value", json!(d.value.as_ref().map(Money::amount))),
            (
                "currency",
                json!(d.value.as_ref().map(|v| v.currency().as_str())),
            ),
            ("payment_schedule_id", json!(d.payment_schedule_id)),
            ("cost_centre_id", json!(d.cost_centre_id)),
            ("dms_link", json!(d.dms_link)),
            (
                "personal_data_transferred",
                json!(dp.personal_data_transferred),
            ),
            ("dpia_required", json!(dp.dpia_required)),
            ("dpia_completed_on", json!(dp.dpia_completed_on)),
            ("dpia_link", json!(dp.dpia_link)),
        ]
    }
}

// ----- Domain events -----------------------------------------------------------------------

/// A Contract's Calendar facts changed: it was created or deleted, or its number, title,
/// Notice Date, end date or active state changed (FR-CON-003). The Calendar context
/// consumes it to keep the generated Calendar Events of the Contract up to date.
///
/// Payload (cross-context contract, ADR-0034): `contract_id`, `contract_number`,
/// `title`, `notice_date`, `end_date` (`YYYY-MM-DD` or `null`) and `active`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContractDatesChanged {
    /// The Contract.
    pub contract_id: Uuid,
    /// Contract number.
    pub contract_number: String,
    /// Title.
    pub title: String,
    /// Notice Date.
    pub notice_date: Option<NaiveDate>,
    /// End date.
    pub end_date: Option<NaiveDate>,
    /// Neither deleted nor in a closed Contract Status.
    pub active: bool,
    /// Company (envelope only, not in the payload).
    #[serde(skip)]
    pub tenant_id: TenantId,
}

impl DomainEvent for ContractDatesChanged {
    const EVENT_TYPE: &'static str = "contract.dates_changed";
    const AGGREGATE_TYPE: &'static str = "contract";

    fn aggregate_id(&self) -> Uuid {
        self.contract_id
    }

    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// A Notice Date or end date of a Contract is coming up and its owner should be
/// reminded (FR-CON-003). Raised at most once per Contract, kind and date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractReminderDue {
    /// The Contract.
    pub contract_id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// The owner to remind.
    pub owner_id: AnalystId,
    /// Which date.
    pub kind: ReminderKind,
    /// The date.
    pub date: NaiveDate,
    /// Contract number.
    pub contract_number: String,
    /// Title.
    pub title: String,
}

impl DomainEvent for ContractReminderDue {
    const EVENT_TYPE: &'static str = "contract.reminder_due";
    const AGGREGATE_TYPE: &'static str = "contract";

    fn aggregate_id(&self) -> Uuid {
        self.contract_id
    }

    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use chrono::TimeZone;
    use proptest::prelude::*;

    fn day(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap()
    }

    fn months(n: u32) -> NoticePeriod {
        NoticePeriod::new(n, NoticeUnit::Months).unwrap()
    }

    fn days(n: u32) -> NoticePeriod {
        NoticePeriod::new(n, NoticeUnit::Days).unwrap()
    }

    fn status(closed: bool) -> Lookup {
        Lookup::create(
            LookupKind::ContractStatus,
            LookupInput {
                name: if closed { "Terminated" } else { "Live" }.into(),
                is_closed: closed,
                sort_order: 0,
                is_active: true,
            },
            now(),
        )
        .unwrap()
    }

    fn details() -> ContractDetails {
        ContractDetails {
            contract_number: " CON-001 ".into(),
            title: "Printer lease".into(),
            owner_id: Some(AnalystId::new()),
            start_date: Some(day("2026-01-01")),
            end_date: Some(day("2027-06-30")),
            notice_period: Some(months(3)),
            ..ContractDetails::default()
        }
    }

    fn contract(d: ContractDetails, s: Option<&Lookup>) -> Contract {
        Contract::create(TenantId::new(), d, s, Actor::Automation, now()).unwrap()
    }

    // --- Notice Date ------------------------------------------------------------------

    #[test]
    fn notice_date_is_end_date_minus_days() {
        assert_eq!(
            days(30).notice_date(day("2027-03-31")),
            Some(day("2027-03-01"))
        );
        assert_eq!(
            days(0).notice_date(day("2027-03-31")),
            Some(day("2027-03-31"))
        );
        assert_eq!(
            days(1).notice_date(day("2028-03-01")),
            Some(day("2028-02-29"))
        );
    }

    #[test]
    fn notice_date_in_months_keeps_the_day_of_the_month() {
        assert_eq!(
            months(3).notice_date(day("2027-06-15")),
            Some(day("2027-03-15"))
        );
        assert_eq!(
            months(12).notice_date(day("2027-06-15")),
            Some(day("2026-06-15"))
        );
    }

    #[test]
    fn notice_date_in_months_clamps_to_a_shorter_month() {
        assert_eq!(
            months(3).notice_date(day("2027-05-29")),
            Some(day("2027-02-28"))
        );
        assert_eq!(
            months(3).notice_date(day("2028-05-29")),
            Some(day("2028-02-29"))
        );
        assert_eq!(
            months(1).notice_date(day("2027-03-30")),
            Some(day("2027-02-28"))
        );
    }

    #[test]
    fn contract_ending_on_a_month_end_gives_notice_on_a_month_end() {
        assert_eq!(
            months(1).notice_date(day("2027-06-30")),
            Some(day("2027-05-31"))
        );
        assert_eq!(
            months(3).notice_date(day("2027-05-31")),
            Some(day("2027-02-28"))
        );
        assert_eq!(
            months(1).notice_date(day("2028-02-29")),
            Some(day("2028-01-31"))
        );
        assert_eq!(
            months(12).notice_date(day("2028-02-29")),
            Some(day("2027-02-28"))
        );
        assert_eq!(
            months(1).notice_date(day("2027-02-28")),
            Some(day("2027-01-31"))
        );
    }

    #[test]
    fn notice_period_is_bounded() {
        assert!(NoticePeriod::new(3650, NoticeUnit::Days).is_ok());
        assert!(NoticePeriod::new(3651, NoticeUnit::Days).is_err());
        assert!(NoticePeriod::new(120, NoticeUnit::Months).is_ok());
        assert!(NoticePeriod::new(121, NoticeUnit::Months).is_err());
        assert!(NoticePeriod::new(u32::MAX, NoticeUnit::Days).is_err());
    }

    fn any_date() -> impl Strategy<Value = NaiveDate> {
        (0i64..80_000).prop_map(|n| day("1990-01-01") + chrono::Duration::days(n))
    }

    proptest! {
        #[test]
        fn notice_date_in_days_is_exactly_that_many_days_earlier(end in any_date(), n in 0u32..=3650) {
            let notice = days(n).notice_date(end).unwrap();
            prop_assert_eq!((end - notice).num_days(), i64::from(n));
        }

        #[test]
        fn notice_date_in_months_is_that_many_calendar_months_earlier(end in any_date(), n in 0u32..=120) {
            let notice = months(n).notice_date(end).unwrap();
            let index = |d: NaiveDate| i64::from(d.year()) * 12 + i64::from(d.month0());
            prop_assert_eq!(index(end) - index(notice), i64::from(n));
            prop_assert!(notice <= end);
            prop_assert_eq!(notice == end, n == 0);
            if is_month_end(end) {
                prop_assert!(is_month_end(notice));
            } else {
                prop_assert_eq!(notice.day(), end.day().min(last_day_of_month(notice).day()));
            }
        }

        #[test]
        fn a_longer_notice_period_never_gives_a_later_notice_date(end in any_date(), n in 0u32..120) {
            prop_assert!(months(n + 1).notice_date(end) <= months(n).notice_date(end));
            prop_assert!(days(n + 1).notice_date(end) <= days(n).notice_date(end));
        }

        #[test]
        fn money_round_trips_through_its_decimal_text(minor in 0i64..=Money::MAX_MINOR) {
            let m = Money::new(minor, CurrencyCode::try_from("EUR".to_owned()).unwrap()).unwrap();
            prop_assert_eq!(Money::parse(&m.amount(), "EUR").unwrap(), m);
        }
    }

    #[test]
    fn contract_notice_date_needs_an_end_date_and_a_notice_period() {
        assert_eq!(
            contract(details(), None).notice_date(),
            Some(day("2027-03-31"))
        );
        let no_end = ContractDetails {
            end_date: None,
            ..details()
        };
        assert_eq!(contract(no_end, None).notice_date(), None);
        let no_period = ContractDetails {
            notice_period: None,
            ..details()
        };
        assert_eq!(contract(no_period, None).notice_date(), None);
    }

    // --- Money ------------------------------------------------------------------------

    #[test]
    fn money_parses_decimal_amounts() {
        assert_eq!(Money::parse("1250", "gbp").unwrap().minor_units(), 125_000);
        assert_eq!(Money::parse("1250.5", "GBP").unwrap().amount(), "1250.50");
        assert_eq!(Money::parse(" 0.07 ", "GBP").unwrap().minor_units(), 7);
        assert_eq!(
            Money::parse("1250.5", "gbp").unwrap().currency().as_str(),
            "GBP"
        );
        assert_eq!(
            Money::parse("999999999999.99", "USD")
                .unwrap()
                .minor_units(),
            Money::MAX_MINOR
        );
        for bad in [
            "",
            "-1",
            "1.234",
            "1.",
            ".5",
            "1e3",
            "1,000",
            "1000000000000",
            "NaN",
        ] {
            assert!(Money::parse(bad, "GBP").is_err(), "{bad}");
        }
        for bad in ["GB", "GBPX", "G1P", ""] {
            assert!(Money::parse("1", bad).is_err(), "{bad}");
        }
    }

    // --- Contracts --------------------------------------------------------------------

    #[test]
    fn contract_details_are_validated() {
        let c = contract(details(), None);
        assert_eq!(c.details().contract_number, "CON-001");
        let bad = |d: ContractDetails| {
            Contract::create(TenantId::new(), d, None, Actor::Automation, now())
        };
        assert!(
            bad(ContractDetails {
                title: " ".into(),
                ..details()
            })
            .is_err()
        );
        assert!(
            bad(ContractDetails {
                contract_number: "x".repeat(51),
                ..details()
            })
            .is_err()
        );
        assert!(
            bad(ContractDetails {
                start_date: Some(day("2028-01-01")),
                ..details()
            })
            .is_err()
        );
        assert!(
            bad(ContractDetails {
                dms_link: Some("ftp://x".into()),
                ..details()
            })
            .is_err()
        );
        assert!(
            bad(ContractDetails {
                dms_link: Some("https://".into()),
                ..details()
            })
            .is_err()
        );
        let ok = bad(ContractDetails {
            dms_link: Some(" https://dms.example/c/1 ".into()),
            ..details()
        })
        .unwrap();
        assert_eq!(
            ok.details().dms_link.as_deref(),
            Some("https://dms.example/c/1")
        );
    }

    #[test]
    fn dpia_details_need_a_required_dpia() {
        let dp = |required: bool, done: Option<NaiveDate>| ContractDetails {
            data_protection: DataProtection {
                personal_data_transferred: true,
                dpia_required: required,
                dpia_completed_on: done,
                dpia_link: None,
            },
            ..details()
        };
        let bad = |d| Contract::create(TenantId::new(), d, None, Actor::Automation, now());
        assert!(bad(dp(false, Some(day("2026-02-01")))).is_err());
        assert!(bad(dp(true, Some(day("2026-02-01")))).is_ok());
        assert!(bad(dp(false, None)).is_ok());
    }

    #[test]
    fn status_must_be_the_contract_status_of_the_details() {
        let live = status(false);
        let d = ContractDetails {
            status_id: Some(live.id),
            ..details()
        };
        assert!(
            Contract::create(TenantId::new(), d.clone(), None, Actor::Automation, now()).is_err()
        );
        assert!(
            Contract::create(
                TenantId::new(),
                details(),
                Some(&live),
                Actor::Automation,
                now()
            )
            .is_err()
        );
        let mut schedule = live.clone();
        schedule.kind = LookupKind::PaymentSchedule;
        assert!(
            Contract::create(
                TenantId::new(),
                d.clone(),
                Some(&schedule),
                Actor::Automation,
                now()
            )
            .is_err()
        );
        assert!(
            Contract::create(TenantId::new(), d, Some(&live), Actor::Automation, now()).is_ok()
        );
    }

    #[test]
    fn closed_status_and_delete_make_a_contract_inactive() {
        let live = status(false);
        let mut ended = status(true);
        let mut c = contract(
            ContractDetails {
                status_id: Some(live.id),
                ..details()
            },
            Some(&live),
        );
        assert!(c.is_active());
        c.edit(
            ContractDetails {
                status_id: Some(ended.id),
                ..details()
            },
            Some(&ended),
            now(),
        )
        .unwrap();
        assert!(!c.is_active());
        ended.is_closed = false;
        c.refresh_status(&ended);
        assert!(c.is_active(), "the status was reopened");
        c.delete(now()).unwrap();
        assert!(!c.is_active());
        assert!(c.delete(now()).is_err());
        assert!(c.edit(details(), None, now()).is_err());
    }

    #[test]
    fn dates_changed_payload_is_the_calendar_contract() {
        let c = contract(details(), None);
        let payload = serde_json::to_value(c.dates_changed()).unwrap();
        assert_eq!(
            payload,
            json!({
                "contract_id": c.id().as_uuid(),
                "contract_number": "CON-001",
                "title": "Printer lease",
                "notice_date": "2027-03-31",
                "end_date": "2027-06-30",
                "active": true,
            })
        );
        assert_eq!(ContractDatesChanged::EVENT_TYPE, "contract.dates_changed");
        assert_eq!(ContractDatesChanged::AGGREGATE_TYPE, "contract");
        let open = contract(
            ContractDetails {
                end_date: None,
                ..details()
            },
            None,
        );
        assert_eq!(
            serde_json::to_value(open.dates_changed()).unwrap()["notice_date"],
            Value::Null
        );
    }

    #[test]
    fn notice_status_follows_today_and_the_window() {
        let c = contract(details(), None); // Notice Date 2027-03-31
        assert_eq!(c.notice_status(day("2027-01-01"), 30), NoticeStatus::Later);
        assert_eq!(
            c.notice_status(day("2027-03-01"), 30),
            NoticeStatus::Upcoming
        );
        assert_eq!(
            c.notice_status(day("2027-03-31"), 30),
            NoticeStatus::Upcoming
        );
        assert_eq!(c.notice_status(day("2027-04-01"), 30), NoticeStatus::Passed);
        let mut gone = c.clone();
        gone.delete(now()).unwrap();
        assert_eq!(
            gone.notice_status(day("2027-03-01"), 30),
            NoticeStatus::None
        );
    }

    #[test]
    fn reminders_cover_dates_inside_the_window_for_owned_active_contracts() {
        let c = contract(details(), None); // notice 2027-03-31, end 2027-06-30
        assert_eq!(
            c.due_reminders(day("2027-03-01"), 30),
            vec![(ReminderKind::NoticeDate, day("2027-03-31"))]
        );
        assert_eq!(c.due_reminders(day("2027-02-28"), 30), vec![]);
        assert_eq!(
            c.due_reminders(day("2027-06-01"), 30),
            vec![(ReminderKind::EndDate, day("2027-06-30"))]
        );
        assert_eq!(
            c.due_reminders(day("2027-03-31"), 120),
            vec![
                (ReminderKind::NoticeDate, day("2027-03-31")),
                (ReminderKind::EndDate, day("2027-06-30"))
            ]
        );
        assert_eq!(
            c.due_reminders(day("2027-03-01"), 0),
            vec![],
            "0 turns reminders off"
        );
        let unowned = contract(
            ContractDetails {
                owner_id: None,
                ..details()
            },
            None,
        );
        assert_eq!(unowned.due_reminders(day("2027-03-01"), 30), vec![]);
    }

    // --- Suppliers, Contacts and lookups ----------------------------------------------

    #[test]
    fn supplier_and_contact_details_are_validated() {
        let s = Supplier::create(
            TenantId::new(),
            SupplierDetails {
                legal_name: " Acme Ltd ".into(),
                trading_name: Some("  ".into()),
                vat_number: Some("GB123".into()),
                is_active: true,
                ..SupplierDetails::default()
            },
            Actor::Automation,
            now(),
        )
        .unwrap();
        assert_eq!(s.details().legal_name, "Acme Ltd");
        assert_eq!(s.details().trading_name, None);
        assert!(
            Supplier::create(
                TenantId::new(),
                SupplierDetails::default(),
                Actor::Automation,
                now()
            )
            .is_err()
        );
        let contact = |email: &str| {
            Contact::create(
                &s,
                ContactDetails {
                    full_name: "Pat Jones".into(),
                    email: Some(email.into()),
                    is_active: true,
                    ..ContactDetails::default()
                },
                now(),
            )
        };
        let c = contact("pat@acme.example").unwrap();
        assert_eq!((c.tenant_id(), c.supplier_id()), (s.tenant_id(), s.id()));
        for bad in [
            "pat",
            "pat@acme",
            "@acme.example",
            "pat @acme.example",
            "pat@acme.example.",
        ] {
            assert!(contact(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_contract_statuses_can_be_closed() {
        let input = LookupInput {
            name: "Terminated".into(),
            is_closed: true,
            sort_order: 1,
            is_active: true,
        };
        assert!(
            Lookup::create(LookupKind::ContractStatus, input.clone(), now())
                .unwrap()
                .is_closed
        );
        assert!(
            !Lookup::create(LookupKind::SupplierStatus, input.clone(), now())
                .unwrap()
                .is_closed
        );
        assert!(
            Lookup::create(
                LookupKind::PaymentSchedule,
                LookupInput {
                    name: String::new(),
                    ..input
                },
                now()
            )
            .is_err()
        );
    }
}
