//! Assets domain (ADR-0034). Pure Rust: no I/O.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AnalystId, DomainEvent, RequesterId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

cooper_kernel::define_id!(
    /// Identifies an Asset.
    AssetId
);

/// An Assets rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AssetError {
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

impl AssetError {
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

// ----- Money ------------------------------------------------------------------------------

/// An ISO-4217 currency code: three upper-case letters.
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
    type Error = AssetError;
    fn try_from(s: String) -> Result<Self, AssetError> {
        let code = s.trim().to_ascii_uppercase();
        if code.len() == 3 && code.chars().all(|c| c.is_ascii_uppercase()) {
            Ok(Self(code))
        } else {
            Err(AssetError::invalid(
                "purchase_currency",
                "must be an ISO-4217 code such as GBP",
            ))
        }
    }
}

impl From<CurrencyCode> for String {
    fn from(c: CurrencyCode) -> Self {
        c.0
    }
}

impl fmt::Display for CurrencyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A non-negative amount with two decimals, as stored in `NUMERIC(14,2)`, in hundredths
/// (so no floating point is ever involved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Amount(i64);

impl Amount {
    /// The largest amount `NUMERIC(14,2)` holds, in hundredths.
    pub const MAX_HUNDREDTHS: i64 = 99_999_999_999_999;

    /// An amount from hundredths.
    ///
    /// # Errors
    ///
    /// Negative or too large.
    pub fn from_hundredths(h: i64) -> Result<Self, AssetError> {
        if (0..=Self::MAX_HUNDREDTHS).contains(&h) {
            Ok(Self(h))
        } else {
            Err(AssetError::invalid(
                "purchase_cost",
                "must be 0 to 999999999999.99",
            ))
        }
    }

    /// Hundredths.
    #[must_use]
    pub const fn hundredths(self) -> i64 {
        self.0
    }
}

impl FromStr for Amount {
    type Err = AssetError;

    /// Parses `123`, `123.4` or `123.45` (no sign, no exponent, at most two decimals).
    fn from_str(s: &str) -> Result<Self, AssetError> {
        let bad = || AssetError::invalid("purchase_cost", "must be a decimal like 1299.00");
        let s = s.trim();
        let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
        if whole.is_empty()
            || whole.len() > 12
            || frac.len() > 2
            || !whole.chars().all(|c| c.is_ascii_digit())
            || !frac.chars().all(|c| c.is_ascii_digit())
            || (s.contains('.') && frac.is_empty())
        {
            return Err(bad());
        }
        let whole: i64 = whole.parse().map_err(|_| bad())?;
        let frac: i64 = format!("{frac:0<2}").parse().map_err(|_| bad())?;
        Self::from_hundredths(whole * 100 + frac)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

/// An amount of money in a currency.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Money {
    /// The amount.
    pub amount: Amount,
    /// The currency.
    pub currency: CurrencyCode,
}

impl Money {
    /// Parses an amount and a currency code.
    ///
    /// # Errors
    ///
    /// Either part is invalid.
    pub fn parse(amount: &str, currency: &str) -> Result<Self, AssetError> {
        Ok(Self {
            amount: amount.parse()?,
            currency: CurrencyCode::try_from(currency.to_owned())?,
        })
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.currency)
    }
}

// ----- Reference data ---------------------------------------------------------------------

/// Which reference data (FR-AST-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupKind {
    /// Asset Type.
    Type,
    /// Asset Status.
    Status,
    /// Location.
    Location,
}

impl LookupKind {
    /// Display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Type => "Asset Type",
            Self::Status => "Asset Status",
            Self::Location => "Location",
        }
    }

    /// Audit aggregate type.
    #[must_use]
    pub const fn aggregate_type(self) -> &'static str {
        match self {
            Self::Type => "asset_type",
            Self::Status => "asset_status",
            Self::Location => "asset_location",
        }
    }
}

/// The deepest a Location tree goes (site, building, floor, room, rack, …).
pub const MAX_LOCATION_DEPTH: usize = 6;

/// An Asset Type, Asset Status or Location (FR-AST-002). No seed values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// ID.
    pub id: Uuid,
    /// Kind.
    pub kind: LookupKind,
    /// Name.
    pub name: String,
    /// Locations: the parent Location.
    pub parent_id: Option<Uuid>,
    /// Statuses: the Asset has left the estate.
    pub is_retired: bool,
    /// Statuses: `#rrggbb`.
    pub colour: Option<String>,
    /// Display order.
    pub sort_order: i32,
    /// Active: offered for new choices.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// Fields an Administrator edits on reference data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LookupInput {
    /// Name.
    pub name: String,
    /// Locations: parent.
    pub parent_id: Option<Uuid>,
    /// Statuses: retired.
    pub is_retired: bool,
    /// Statuses: colour.
    pub colour: Option<String>,
    /// Order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

fn valid_colour(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

impl Lookup {
    /// New reference data. `siblings` are every row of the same kind (for the Location
    /// tree checks).
    ///
    /// # Errors
    ///
    /// Invalid input.
    pub fn create(
        kind: LookupKind,
        input: LookupInput,
        all: &[Self],
        now: DateTime<Utc>,
    ) -> Result<Self, AssetError> {
        let mut l = Self {
            id: Uuid::now_v7(),
            kind,
            name: String::new(),
            parent_id: None,
            is_retired: false,
            colour: None,
            sort_order: 0,
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        l.apply(input, all, now)?;
        Ok(l)
    }

    /// Changes the row. `all` are every row of the same kind.
    ///
    /// # Errors
    ///
    /// An empty or long name, a bad colour, or (Locations) a parent that is missing,
    /// inactive, the row itself or one of its descendants, or too deep a tree.
    pub fn apply(
        &mut self,
        input: LookupInput,
        all: &[Self],
        now: DateTime<Utc>,
    ) -> Result<(), AssetError> {
        let name = input.name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(AssetError::invalid("name", "must be 1 to 100 characters"));
        }
        if let Some(c) = &input.colour
            && !valid_colour(c)
        {
            return Err(AssetError::invalid("colour", "must be #rrggbb"));
        }
        let (parent_id, is_retired, colour) = match self.kind {
            LookupKind::Type => (None, false, None),
            LookupKind::Status => (None, input.is_retired, input.colour),
            LookupKind::Location => {
                check_location_parent(all, self.id, input.parent_id)?;
                (input.parent_id, false, None)
            }
        };
        self.name = name.to_owned();
        self.parent_id = parent_id;
        self.is_retired = is_retired;
        self.colour = colour;
        self.sort_order = input.sort_order;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }
}

fn depth_below(all: &[Lookup], id: Uuid) -> usize {
    all.iter()
        .filter(|l| l.parent_id == Some(id))
        .map(|c| 1 + depth_below(all, c.id))
        .max()
        .unwrap_or(0)
}

/// Checks that `location` may hang under `parent`: the parent exists, is active when it
/// changes, is not the Location itself or below it, and the tree stays at most
/// [`MAX_LOCATION_DEPTH`] deep.
///
/// # Errors
///
/// Any of those rules.
pub fn check_location_parent(
    all: &[Lookup],
    location: Uuid,
    parent: Option<Uuid>,
) -> Result<(), AssetError> {
    let Some(parent) = parent else {
        return Ok(());
    };
    let unchanged = all
        .iter()
        .any(|l| l.id == location && l.parent_id == Some(parent));
    let mut depth = 1;
    let mut cursor = Some(parent);
    while let Some(id) = cursor {
        if id == location {
            return Err(AssetError::invalid(
                "parent_id",
                "a Location cannot sit inside itself",
            ));
        }
        let Some(row) = all.iter().find(|l| l.id == id) else {
            return Err(AssetError::invalid("parent_id", "is not a Location"));
        };
        if id == parent && !row.is_active && !unchanged {
            return Err(AssetError::invalid("parent_id", "is not active"));
        }
        depth += 1;
        if depth > all.len() + 1 {
            return Err(AssetError::invalid("parent_id", "the tree has a loop"));
        }
        cursor = row.parent_id;
    }
    if depth + depth_below(all, location) > MAX_LOCATION_DEPTH {
        return Err(AssetError::invalid(
            "parent_id",
            format!("Locations nest at most {MAX_LOCATION_DEPTH} deep"),
        ));
    }
    Ok(())
}

/// The Location and every Location below it.
#[must_use]
pub fn location_subtree(all: &[Lookup], root: Uuid) -> Vec<Uuid> {
    let mut out = vec![root];
    let mut i = 0;
    while let Some(id) = out.get(i).copied() {
        let children: Vec<Uuid> = all
            .iter()
            .filter(|l| l.parent_id == Some(id) && !out.contains(&l.id))
            .map(|l| l.id)
            .collect();
        out.extend(children);
        i += 1;
    }
    out
}

// ----- Holders, checkouts, retirement -----------------------------------------------------

/// The Requester or Analyst who has an Asset (FR-AST-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetHolder {
    /// A Requester.
    Requester(RequesterId),
    /// An Analyst.
    Analyst(AnalystId),
}

impl AssetHolder {
    /// `requester` or `analyst`.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Requester(_) => "requester",
            Self::Analyst(_) => "analyst",
        }
    }

    /// The person's ID.
    #[must_use]
    pub const fn id(self) -> Uuid {
        match self {
            Self::Requester(r) => r.as_uuid(),
            Self::Analyst(a) => a.as_uuid(),
        }
    }

    /// From a stored kind and ID.
    ///
    /// # Errors
    ///
    /// An unknown kind.
    pub fn from_parts(kind: &str, id: Uuid) -> Result<Self, AssetError> {
        match kind {
            "requester" => Ok(Self::Requester(RequesterId::from_uuid(id))),
            "analyst" => Ok(Self::Analyst(AnalystId::from_uuid(id))),
            _ => Err(AssetError::invalid(
                "holder_kind",
                "must be requester or analyst",
            )),
        }
    }

    fn audit(self) -> Value {
        json!({ "kind": self.kind(), "id": self.id() })
    }
}

/// One Check Out of an Asset to a Holder, and its Check In (FR-AST-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetCheckout {
    /// ID.
    pub id: Uuid,
    /// Who has (or had) it.
    pub holder: AssetHolder,
    /// When it was checked out.
    pub checked_out_at: DateTime<Utc>,
    /// Who checked it out.
    pub checked_out_by: Actor,
    /// Note at Check Out.
    pub checkout_note: String,
    /// Expected return, if agreed.
    pub due_back_on: Option<NaiveDate>,
    /// When it was checked in (`None` while open).
    pub checked_in_at: Option<DateTime<Utc>>,
    /// Who checked it in.
    pub checked_in_by: Option<Actor>,
    /// Note at Check In.
    pub checkin_note: String,
}

/// An Asset's retirement (FR-AST-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retirement {
    /// The day it left the estate.
    pub retired_on: NaiveDate,
    /// Why.
    pub reason: String,
    /// When it was recorded.
    pub recorded_at: DateTime<Utc>,
    /// Who recorded it.
    pub recorded_by: Actor,
}

fn note(field: &'static str, text: &str) -> Result<String, AssetError> {
    let t = text.trim();
    if t.chars().count() > 1000 {
        return Err(AssetError::invalid(field, "at most 1000 characters"));
    }
    Ok(t.to_owned())
}

// ----- Assets -----------------------------------------------------------------------------

/// What an Analyst edits on an Asset (FR-AST-001).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetDetails {
    /// Network name.
    pub hostname: Option<String>,
    /// Manufacturer serial number.
    pub service_tag: Option<String>,
    /// The label stuck on the item, unique per Company.
    pub asset_tag: Option<String>,
    /// Asset Type.
    pub asset_type_id: Option<Uuid>,
    /// Asset Status.
    pub asset_status_id: Option<Uuid>,
    /// Location.
    pub location_id: Option<Uuid>,
    /// Supplier (Contracts context).
    pub supplier_id: Option<Uuid>,
    /// Cost Centre (identity context).
    pub cost_centre_id: Option<Uuid>,
    /// Purchase date.
    pub purchase_date: Option<NaiveDate>,
    /// Purchase cost.
    pub purchase_cost: Option<Money>,
    /// Warranty expiry.
    pub warranty_expires_on: Option<NaiveDate>,
    /// Lease start.
    pub lease_starts_on: Option<NaiveDate>,
    /// Lease expiry.
    pub lease_expires_on: Option<NaiveDate>,
}

fn text(
    field: &'static str,
    value: Option<String>,
    max: usize,
) -> Result<Option<String>, AssetError> {
    let Some(v) = value else { return Ok(None) };
    let v = v.trim();
    if v.is_empty() {
        return Ok(None);
    }
    if v.chars().count() > max || v.chars().any(char::is_control) {
        return Err(AssetError::invalid(
            field,
            format!("at most {max} printable characters"),
        ));
    }
    Ok(Some(v.to_owned()))
}

impl AssetDetails {
    /// Validates and normalises (blank identifiers become `None`).
    ///
    /// # Errors
    ///
    /// Long identifiers, none of hostname, Service Tag and Asset Tag, or a lease that
    /// ends before it starts.
    pub fn validate(mut self) -> Result<Self, AssetError> {
        self.hostname = text("hostname", self.hostname, 255)?;
        self.service_tag = text("service_tag", self.service_tag, 100)?;
        self.asset_tag = text("asset_tag", self.asset_tag, 50)?;
        if self.hostname.is_none() && self.service_tag.is_none() && self.asset_tag.is_none() {
            return Err(AssetError::invalid(
                "asset_tag",
                "an Asset needs a hostname, Service Tag or Asset Tag",
            ));
        }
        if let (Some(s), Some(e)) = (self.lease_starts_on, self.lease_expires_on)
            && e < s
        {
            return Err(AssetError::invalid(
                "lease_expires_on",
                "must not be before the lease start",
            ));
        }
        Ok(self)
    }
}

/// The reference data an Asset's details point at, resolved by the application layer.
#[derive(Debug, Clone, Copy, Default)]
pub struct References<'a> {
    /// The Asset Type, if any.
    pub asset_type: Option<&'a Lookup>,
    /// The Asset Status, if any.
    pub status: Option<&'a Lookup>,
    /// The Location, if any.
    pub location: Option<&'a Lookup>,
}

fn check_ref(
    field: &'static str,
    kind: LookupKind,
    wanted: Option<Uuid>,
    found: Option<&Lookup>,
    before: Option<Uuid>,
) -> Result<(), AssetError> {
    let Some(id) = wanted else { return Ok(()) };
    let Some(l) = found.filter(|l| l.id == id && l.kind == kind) else {
        return Err(AssetError::invalid(
            field,
            format!("is not an {}", kind.label()),
        ));
    };
    if !l.is_active && before != Some(id) {
        return Err(AssetError::invalid(field, "is not active"));
    }
    Ok(())
}

/// What an edit changed that others care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edited {
    /// Warranty or lease expiry changed.
    pub dates_changed: bool,
}

/// An Asset (FR-AST-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    id: AssetId,
    tenant_id: TenantId,
    details: AssetDetails,
    checkout: Option<AssetCheckout>,
    retirement: Option<Retirement>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Stored state, for [`Asset::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRecord {
    /// ID.
    pub id: AssetId,
    /// Company.
    pub tenant_id: TenantId,
    /// Details.
    pub details: AssetDetails,
    /// The open Check Out.
    pub checkout: Option<AssetCheckout>,
    /// Retirement.
    pub retirement: Option<Retirement>,
    /// Creator.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl Asset {
    /// Registers a new Asset in a Company.
    ///
    /// # Errors
    ///
    /// Invalid details, unknown or inactive reference data, or an `is_retired` Asset
    /// Status (use [`Asset::retire`]).
    pub fn register(
        tenant_id: TenantId,
        details: AssetDetails,
        refs: References<'_>,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, AssetError> {
        let mut asset = Self {
            id: AssetId::new(),
            tenant_id,
            details: AssetDetails::default(),
            checkout: None,
            retirement: None,
            created_by: by,
            created_at: now,
            updated_at: now,
        };
        let details = asset.checked(details, refs, true)?;
        asset.details = details;
        Ok(asset)
    }

    fn checked(
        &self,
        details: AssetDetails,
        refs: References<'_>,
        new: bool,
    ) -> Result<AssetDetails, AssetError> {
        let details = details.validate()?;
        let before = |f: fn(&AssetDetails) -> Option<Uuid>| {
            if new { None } else { f(&self.details) }
        };
        check_ref(
            "asset_type_id",
            LookupKind::Type,
            details.asset_type_id,
            refs.asset_type,
            before(|d| d.asset_type_id),
        )?;
        check_ref(
            "asset_status_id",
            LookupKind::Status,
            details.asset_status_id,
            refs.status,
            before(|d| d.asset_status_id),
        )?;
        check_ref(
            "location_id",
            LookupKind::Location,
            details.location_id,
            refs.location,
            before(|d| d.location_id),
        )?;
        if let Some(s) = refs.status.filter(|_| details.asset_status_id.is_some()) {
            match (self.retirement.is_some(), s.is_retired) {
                (false, true) => {
                    return Err(AssetError::invalid(
                        "asset_status_id",
                        "is a retired status: Retire the Asset instead",
                    ));
                }
                (true, false) => {
                    return Err(AssetError::invalid(
                        "asset_status_id",
                        "a retired Asset keeps a retired status",
                    ));
                }
                _ => {}
            }
        }
        Ok(details)
    }

    /// Rebuilds a stored Asset.
    #[must_use]
    pub fn restore(r: AssetRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            details: r.details,
            checkout: r.checkout,
            retirement: r.retirement,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    /// Replaces the details.
    ///
    /// # Errors
    ///
    /// As [`Asset::register`]; a retired Asset only takes retired statuses.
    pub fn edit(
        &mut self,
        details: AssetDetails,
        refs: References<'_>,
        now: DateTime<Utc>,
    ) -> Result<Edited, AssetError> {
        let details = self.checked(details, refs, false)?;
        let dates_changed = details.warranty_expires_on != self.details.warranty_expires_on
            || details.lease_expires_on != self.details.lease_expires_on;
        self.details = details;
        self.updated_at = now;
        Ok(Edited { dates_changed })
    }

    /// Checks the Asset out to a Holder (FR-AST-004).
    ///
    /// # Errors
    ///
    /// The Asset is retired or already checked out, a long note, or a due date before
    /// today.
    pub fn check_out(
        &mut self,
        holder: AssetHolder,
        note_text: &str,
        due_back_on: Option<NaiveDate>,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<&AssetCheckout, AssetError> {
        if self.retirement.is_some() {
            return Err(AssetError::state("a retired Asset cannot be checked out"));
        }
        if self.checkout.is_some() {
            return Err(AssetError::state(
                "the Asset is already checked out: check it in first",
            ));
        }
        if due_back_on.is_some_and(|d| d < now.date_naive()) {
            return Err(AssetError::invalid(
                "due_back_on",
                "must not be in the past",
            ));
        }
        self.updated_at = now;
        Ok(self.checkout.insert(AssetCheckout {
            id: Uuid::now_v7(),
            holder,
            checked_out_at: now,
            checked_out_by: by,
            checkout_note: note("note", note_text)?,
            due_back_on,
            checked_in_at: None,
            checked_in_by: None,
            checkin_note: String::new(),
        }))
    }

    /// Checks the Asset back in; returns the closed Check Out.
    ///
    /// # Errors
    ///
    /// The Asset is not checked out, or a long note.
    pub fn check_in(
        &mut self,
        note_text: &str,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<AssetCheckout, AssetError> {
        let note_text = note("note", note_text)?;
        let Some(mut c) = self.checkout.take() else {
            return Err(AssetError::state("the Asset is not checked out"));
        };
        c.checked_in_at = Some(now);
        c.checked_in_by = Some(by);
        c.checkin_note = note_text;
        self.updated_at = now;
        Ok(c)
    }

    /// Retires the Asset on a date with a reason, optionally moving it to an
    /// `is_retired` Asset Status (FR-AST-005).
    ///
    /// # Errors
    ///
    /// Already retired, still checked out, an empty or long reason, or a status that is
    /// not an active retired status.
    pub fn retire(
        &mut self,
        retired_on: NaiveDate,
        reason: &str,
        status: Option<&Lookup>,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<(), AssetError> {
        if self.retirement.is_some() {
            return Err(AssetError::state("the Asset is already retired"));
        }
        if self.checkout.is_some() {
            return Err(AssetError::state(
                "the Asset is checked out: check it in first",
            ));
        }
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > 1000 {
            return Err(AssetError::invalid(
                "reason",
                "must be 1 to 1000 characters",
            ));
        }
        if let Some(s) = status {
            if s.kind != LookupKind::Status || !s.is_retired || !s.is_active {
                return Err(AssetError::invalid(
                    "asset_status_id",
                    "must be an active retired Asset Status",
                ));
            }
            self.details.asset_status_id = Some(s.id);
        } else if self.details.asset_status_id.is_some() {
            // The old status described an Asset in the estate; it no longer does.
            self.details.asset_status_id = None;
        }
        self.retirement = Some(Retirement {
            retired_on,
            reason: reason.to_owned(),
            recorded_at: now,
            recorded_by: by,
        });
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> AssetId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &AssetDetails {
        &self.details
    }
    /// The open Check Out.
    #[must_use]
    pub const fn checkout(&self) -> Option<&AssetCheckout> {
        self.checkout.as_ref()
    }
    /// Who has the Asset now.
    #[must_use]
    pub fn holder(&self) -> Option<AssetHolder> {
        self.checkout.as_ref().map(|c| c.holder)
    }
    /// Retirement.
    #[must_use]
    pub const fn retirement(&self) -> Option<&Retirement> {
        self.retirement.as_ref()
    }
    /// In the estate (not retired): counted in active counts.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.retirement.is_none()
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

    /// Audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        let cost = d.purchase_cost.as_ref().map(ToString::to_string);
        vec![
            ("hostname", json!(d.hostname)),
            ("service_tag", json!(d.service_tag)),
            ("asset_tag", json!(d.asset_tag)),
            ("asset_type_id", json!(d.asset_type_id)),
            ("asset_status_id", json!(d.asset_status_id)),
            ("location_id", json!(d.location_id)),
            ("supplier_id", json!(d.supplier_id)),
            ("cost_centre_id", json!(d.cost_centre_id)),
            ("purchase_date", json!(d.purchase_date)),
            ("purchase_cost", json!(cost)),
            ("warranty_expires_on", json!(d.warranty_expires_on)),
            ("lease_starts_on", json!(d.lease_starts_on)),
            ("lease_expires_on", json!(d.lease_expires_on)),
            ("holder", json!(self.holder().map(AssetHolder::audit))),
            (
                "retired_on",
                json!(self.retirement.as_ref().map(|r| r.retired_on)),
            ),
            (
                "retirement_reason",
                json!(self.retirement.as_ref().map(|r| r.reason.clone())),
            ),
        ]
    }

    /// The `asset.dates_changed` event for this Asset's current state.
    #[must_use]
    pub fn dates_changed(&self) -> AssetDatesChanged {
        AssetDatesChanged {
            asset_id: self.id,
            tenant_id: self.tenant_id,
            asset_tag: self.details.asset_tag.clone(),
            hostname: self.details.hostname.clone(),
            warranty_expires_on: self.details.warranty_expires_on,
            lease_expires_on: self.details.lease_expires_on,
            active: self.is_active(),
        }
    }
}

// ----- Domain events ----------------------------------------------------------------------

/// An Asset was checked out to a Holder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetCheckedOut {
    /// The Asset.
    pub asset_id: AssetId,
    /// Its Company.
    pub tenant_id: TenantId,
    /// The Check Out.
    pub checkout_id: Uuid,
    /// `requester` or `analyst`.
    pub holder_kind: String,
    /// The Holder.
    pub holder_id: Uuid,
    /// Expected return.
    pub due_back_on: Option<NaiveDate>,
}

impl DomainEvent for AssetCheckedOut {
    const EVENT_TYPE: &'static str = "asset.checked_out";
    const AGGREGATE_TYPE: &'static str = "asset";
    fn aggregate_id(&self) -> Uuid {
        self.asset_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// An Asset was checked back in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetCheckedIn {
    /// The Asset.
    pub asset_id: AssetId,
    /// Its Company.
    pub tenant_id: TenantId,
    /// The Check Out that ended.
    pub checkout_id: Uuid,
    /// `requester` or `analyst`.
    pub holder_kind: String,
    /// The Holder who returned it.
    pub holder_id: Uuid,
}

impl DomainEvent for AssetCheckedIn {
    const EVENT_TYPE: &'static str = "asset.checked_in";
    const AGGREGATE_TYPE: &'static str = "asset";
    fn aggregate_id(&self) -> Uuid {
        self.asset_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// An Asset left the estate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRetired {
    /// The Asset.
    pub asset_id: AssetId,
    /// Its Company.
    pub tenant_id: TenantId,
    /// The day.
    pub retired_on: NaiveDate,
    /// Why.
    pub reason: String,
}

impl DomainEvent for AssetRetired {
    const EVENT_TYPE: &'static str = "asset.retired";
    const AGGREGATE_TYPE: &'static str = "asset";
    fn aggregate_id(&self) -> Uuid {
        self.asset_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// An Asset's warranty or lease expiry was set, changed or cleared, or the Asset was
/// retired (`active` is then `false`). Consumed by the Calendar (FR-AST-012). The
/// Company travels in the envelope, not the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetDatesChanged {
    /// The Asset.
    pub asset_id: AssetId,
    /// Its Company (envelope only).
    #[serde(skip)]
    pub tenant_id: TenantId,
    /// Asset Tag.
    pub asset_tag: Option<String>,
    /// Hostname.
    pub hostname: Option<String>,
    /// Warranty expiry.
    pub warranty_expires_on: Option<NaiveDate>,
    /// Lease expiry.
    pub lease_expires_on: Option<NaiveDate>,
    /// In the estate.
    pub active: bool,
}

impl DomainEvent for AssetDatesChanged {
    const EVENT_TYPE: &'static str = "asset.dates_changed";
    const AGGREGATE_TYPE: &'static str = "asset";
    fn aggregate_id(&self) -> Uuid {
        self.asset_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn now() -> DateTime<Utc> {
        "2026-11-11T10:00:00Z".parse().unwrap()
    }

    fn day(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn lookup(kind: LookupKind, input: LookupInput, all: &[Lookup]) -> Lookup {
        Lookup::create(kind, input, all, now()).unwrap()
    }

    fn named(name: &str) -> LookupInput {
        LookupInput {
            name: name.into(),
            is_active: true,
            ..LookupInput::default()
        }
    }

    fn status(retired: bool) -> Lookup {
        lookup(
            LookupKind::Status,
            LookupInput {
                is_retired: retired,
                ..named("S")
            },
            &[],
        )
    }

    fn tagged(tag: &str) -> AssetDetails {
        AssetDetails {
            asset_tag: Some(tag.into()),
            ..AssetDetails::default()
        }
    }

    fn asset() -> Asset {
        Asset::register(
            TenantId::new(),
            tagged("A-1"),
            References::default(),
            Actor::Automation,
            now(),
        )
        .unwrap()
    }

    #[test]
    fn amounts_parse_exactly_and_print_two_decimals() {
        assert_eq!("12".parse::<Amount>().unwrap().hundredths(), 1200);
        assert_eq!("12.5".parse::<Amount>().unwrap().hundredths(), 1250);
        assert_eq!("0.07".parse::<Amount>().unwrap().to_string(), "0.07");
        for bad in [
            "",
            "-1",
            "1.234",
            "1.",
            ".5",
            "1e3",
            "1,00",
            "9999999999999",
        ] {
            assert!(bad.parse::<Amount>().is_err(), "{bad}");
        }
        assert_eq!(
            Money::parse("1299", "gbp").unwrap().to_string(),
            "1299.00 GBP"
        );
        assert!(Money::parse("1", "POUND").is_err());
        assert!(Amount::from_hundredths(Amount::MAX_HUNDREDTHS + 1).is_err());
    }

    proptest! {
        #[test]
        fn amounts_round_trip(h in 0..=Amount::MAX_HUNDREDTHS) {
            let a = Amount::from_hundredths(h).unwrap();
            prop_assert_eq!(a.to_string().parse::<Amount>().unwrap(), a);
        }

        #[test]
        fn currency_codes_are_three_letters(s in "\\PC{0,6}") {
            if let Ok(c) = CurrencyCode::try_from(s) {
                prop_assert!(c.as_str().len() == 3 && c.as_str().chars().all(|ch| ch.is_ascii_uppercase()));
            }
        }
    }

    #[test]
    fn reference_data_keeps_only_its_own_flags() {
        let t = lookup(
            LookupKind::Type,
            LookupInput {
                is_retired: true,
                colour: Some("#ff0000".into()),
                ..named(" Laptop ")
            },
            &[],
        );
        assert_eq!(t.name, "Laptop");
        assert!(!t.is_retired && t.colour.is_none());
        assert!(Lookup::create(LookupKind::Status, named(""), &[], now()).is_err());
        let bad = LookupInput {
            colour: Some("red".into()),
            ..named("x")
        };
        assert!(Lookup::create(LookupKind::Status, bad, &[], now()).is_err());
        assert!(status(true).is_retired);
    }

    #[test]
    fn locations_form_a_tree_without_loops() {
        let site = lookup(LookupKind::Location, named("London"), &[]);
        let floor = lookup(
            LookupKind::Location,
            LookupInput {
                parent_id: Some(site.id),
                ..named("Floor 2")
            },
            std::slice::from_ref(&site),
        );
        let mut all = vec![site.clone(), floor.clone()];
        let mut moved = site.clone();
        let err = moved.apply(
            LookupInput {
                parent_id: Some(floor.id),
                ..named("London")
            },
            &all,
            now(),
        );
        assert!(err.is_err(), "no loops");
        assert!(
            check_location_parent(&all, site.id, Some(site.id)).is_err(),
            "not itself"
        );
        assert!(check_location_parent(&all, Uuid::now_v7(), Some(Uuid::now_v7())).is_err());
        assert_eq!(location_subtree(&all, site.id), vec![site.id, floor.id]);
        // Depth.
        let mut parent = floor.id;
        for n in 3..=MAX_LOCATION_DEPTH {
            let l = lookup(
                LookupKind::Location,
                LookupInput {
                    parent_id: Some(parent),
                    ..named(&format!("L{n}"))
                },
                &all,
            );
            parent = l.id;
            all.push(l);
        }
        assert!(
            Lookup::create(
                LookupKind::Location,
                LookupInput {
                    parent_id: Some(parent),
                    ..named("too deep")
                },
                &all,
                now()
            )
            .is_err()
        );
        // An inactive parent is refused for new children.
        let mut closed = lookup(LookupKind::Location, named("Closed"), &[]);
        closed.is_active = false;
        assert!(
            Lookup::create(
                LookupKind::Location,
                LookupInput {
                    parent_id: Some(closed.id),
                    ..named("x")
                },
                &[closed],
                now()
            )
            .is_err()
        );
    }

    #[test]
    fn assets_need_an_identifier_and_a_sane_lease() {
        let t = TenantId::new();
        let r = References::default();
        assert!(Asset::register(t, AssetDetails::default(), r, Actor::Automation, now()).is_err());
        let blank = AssetDetails {
            hostname: Some("  ".into()),
            ..AssetDetails::default()
        };
        assert!(Asset::register(t, blank, r, Actor::Automation, now()).is_err());
        let lease = AssetDetails {
            lease_starts_on: Some(day("2026-02-01")),
            lease_expires_on: Some(day("2026-01-01")),
            ..tagged("A")
        };
        assert!(Asset::register(t, lease, r, Actor::Automation, now()).is_err());
        let ok = AssetDetails {
            hostname: Some(" pc-001 ".into()),
            asset_tag: Some(String::new()),
            ..AssetDetails::default()
        };
        let a = Asset::register(t, ok, r, Actor::Automation, now()).unwrap();
        assert_eq!(a.details().hostname.as_deref(), Some("pc-001"));
        assert_eq!(a.details().asset_tag, None);
        assert!(a.is_active());
    }

    #[test]
    fn reference_data_must_match_and_be_active_when_chosen() {
        let t = TenantId::new();
        let laptop = lookup(LookupKind::Type, named("Laptop"), &[]);
        let wrong = AssetDetails {
            asset_type_id: Some(Uuid::now_v7()),
            ..tagged("A")
        };
        let refs = References {
            asset_type: Some(&laptop),
            ..References::default()
        };
        assert!(Asset::register(t, wrong, refs, Actor::Automation, now()).is_err());
        let mut old = laptop.clone();
        old.is_active = false;
        let d = AssetDetails {
            asset_type_id: Some(old.id),
            ..tagged("A")
        };
        let refs_old = References {
            asset_type: Some(&old),
            ..References::default()
        };
        assert!(
            Asset::register(t, d.clone(), refs_old, Actor::Automation, now()).is_err(),
            "inactive for new choices"
        );
        let mut a = Asset::register(t, d.clone(), refs, Actor::Automation, now()).unwrap();
        assert!(
            a.edit(d, refs_old, now()).is_ok(),
            "an Asset keeps a type that became inactive"
        );
    }

    #[test]
    fn retired_statuses_come_only_through_retire() {
        let t = TenantId::new();
        let gone = status(true);
        let in_use = status(false);
        let d = AssetDetails {
            asset_status_id: Some(gone.id),
            ..tagged("A")
        };
        let refs = References {
            status: Some(&gone),
            ..References::default()
        };
        assert!(Asset::register(t, d, refs, Actor::Automation, now()).is_err());

        let mut a = Asset::register(
            t,
            AssetDetails {
                asset_status_id: Some(in_use.id),
                ..tagged("A")
            },
            References {
                status: Some(&in_use),
                ..References::default()
            },
            Actor::Automation,
            now(),
        )
        .unwrap();
        assert!(
            a.retire(
                day("2026-11-10"),
                "x",
                Some(&in_use),
                Actor::Automation,
                now()
            )
            .is_err(),
            "the status must be a retired one"
        );
        assert!(
            a.retire(day("2026-11-10"), " ", None, Actor::Automation, now())
                .is_err(),
            "a reason is required"
        );
        a.retire(
            day("2026-11-10"),
            " Disposed ",
            Some(&gone),
            Actor::Automation,
            now(),
        )
        .unwrap();
        assert!(!a.is_active());
        assert_eq!(a.details().asset_status_id, Some(gone.id));
        assert_eq!(a.retirement().unwrap().reason, "Disposed");
        assert!(
            a.retire(day("2026-11-10"), "again", None, Actor::Automation, now())
                .is_err()
        );
        let back = AssetDetails {
            asset_status_id: Some(in_use.id),
            ..a.details().clone()
        };
        assert!(
            a.edit(
                back,
                References {
                    status: Some(&in_use),
                    ..References::default()
                },
                now()
            )
            .is_err(),
            "a retired Asset keeps a retired status"
        );
        let ev = a.dates_changed();
        assert!(!ev.active);
    }

    #[test]
    fn retiring_without_a_status_clears_the_in_estate_status() {
        let in_use = status(false);
        let mut a = Asset::register(
            TenantId::new(),
            AssetDetails {
                asset_status_id: Some(in_use.id),
                ..tagged("A")
            },
            References {
                status: Some(&in_use),
                ..References::default()
            },
            Actor::Automation,
            now(),
        )
        .unwrap();
        a.retire(day("2026-11-01"), "Lost", None, Actor::Automation, now())
            .unwrap();
        assert_eq!(a.details().asset_status_id, None);
    }

    #[test]
    fn an_asset_has_at_most_one_open_checkout() {
        let mut a = asset();
        let ann = AssetHolder::Analyst(AnalystId::new());
        let rita = AssetHolder::Requester(RequesterId::new());
        assert!(a.check_in("", Actor::Automation, now()).is_err());
        assert!(
            a.check_out(ann, "", Some(day("2026-11-10")), Actor::Automation, now())
                .is_err(),
            "due date in the past"
        );
        let c = a
            .check_out(ann, " loan ", None, Actor::Automation, now())
            .unwrap()
            .clone();
        assert_eq!(c.checkout_note, "loan");
        assert_eq!(a.holder(), Some(ann));
        assert!(
            a.check_out(rita, "", None, Actor::Automation, now())
                .is_err()
        );
        assert!(
            a.retire(day("2026-11-11"), "x", None, Actor::Automation, now())
                .is_err(),
            "check in before retiring"
        );
        let closed = a.check_in("ok", Actor::Automation, now()).unwrap();
        assert_eq!(closed.id, c.id);
        assert_eq!(closed.checked_in_at, Some(now()));
        assert_eq!(a.holder(), None);
        a.retire(day("2026-11-11"), "Broken", None, Actor::Automation, now())
            .unwrap();
        assert!(
            a.check_out(rita, "", None, Actor::Automation, now())
                .is_err(),
            "retired Assets cannot be checked out"
        );
    }

    #[test]
    fn edits_report_expiry_changes() {
        let mut a = asset();
        let same = a.details().clone();
        assert!(
            !a.edit(same, References::default(), now())
                .unwrap()
                .dates_changed
        );
        let warranty = AssetDetails {
            warranty_expires_on: Some(day("2029-01-01")),
            ..a.details().clone()
        };
        assert!(
            a.edit(warranty, References::default(), now())
                .unwrap()
                .dates_changed
        );
        let leased = AssetDetails {
            lease_starts_on: Some(day("2026-01-01")),
            ..a.details().clone()
        };
        assert!(
            !a.edit(leased, References::default(), now())
                .unwrap()
                .dates_changed,
            "the lease start is not an expiry"
        );
        let ev = a.dates_changed();
        let payload = serde_json::to_value(&ev).unwrap();
        assert_eq!(
            payload,
            json!({ "asset_id": a.id(), "asset_tag": "A-1", "hostname": null,
                    "warranty_expires_on": "2029-01-01", "lease_expires_on": null, "active": true })
        );
    }

    #[test]
    fn holders_round_trip_their_kind() {
        let a = AnalystId::new();
        let h = AssetHolder::from_parts("analyst", a.as_uuid()).unwrap();
        assert_eq!(h, AssetHolder::Analyst(a));
        assert_eq!(h.kind(), "analyst");
        assert!(AssetHolder::from_parts("team", a.as_uuid()).is_err());
    }
}
