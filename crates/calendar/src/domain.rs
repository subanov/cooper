//! Calendar domain (ADR-0034). Pure Rust: no I/O.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Days, NaiveDate, NaiveTime, Utc};
use cooper_kernel::{Actor, TenantId, define_id};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

define_id!(
    /// Identifies a Calendar Event.
    CalendarEventId
);

define_id!(
    /// Identifies a Calendar Category.
    CalendarCategoryId
);

/// A Calendar rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CalendarError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// Not allowed in the current state (for example editing a generated event).
    #[error("{0}")]
    InvalidState(String),
}

impl CalendarError {
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

// ----- Colour -----------------------------------------------------------------------------

/// A Calendar Category colour: `#rrggbb`, stored in lower case.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Colour(String);

impl Colour {
    /// The colour as `#rrggbb`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Colour {
    type Error = CalendarError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let v = value.trim();
        let hex = v.strip_prefix('#').unwrap_or_default();
        if v.len() == 7 && hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(Self(v.to_ascii_lowercase()))
        } else {
            Err(CalendarError::invalid(
                "colour",
                "must be a hex colour like #2563eb",
            ))
        }
    }
}

impl FromStr for Colour {
    type Err = CalendarError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s.to_owned())
    }
}

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ----- Sources of generated events --------------------------------------------------------

/// The kind of record a generated Calendar Event comes from (FR-CAL-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceKind {
    /// A Contract (Contracts context).
    Contract,
    /// An Asset (Assets context).
    Asset,
}

impl SourceKind {
    /// Every source kind.
    pub const ALL: [Self; 2] = [Self::Contract, Self::Asset];

    /// Storage and API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::Asset => "asset",
        }
    }

    /// Key of the system Calendar Category its events use.
    #[must_use]
    pub const fn category_key(self) -> &'static str {
        match self {
            Self::Contract => "contracts",
            Self::Asset => "assets",
        }
    }

    /// Name the system Calendar Category starts with (Administrators may rename it).
    #[must_use]
    pub const fn category_name(self) -> &'static str {
        match self {
            Self::Contract => "Contracts",
            Self::Asset => "Assets",
        }
    }

    /// Colour the system Calendar Category starts with.
    #[must_use]
    pub const fn category_colour(self) -> &'static str {
        match self {
            Self::Contract => "#7c3aed",
            Self::Asset => "#0f766e",
        }
    }
}

impl FromStr for SourceKind {
    type Err = CalendarError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "contract" => Ok(Self::Contract),
            "asset" => Ok(Self::Asset),
            _ => Err(CalendarError::invalid(
                "source_kind",
                "must be contract or asset",
            )),
        }
    }
}

/// Which date of the source a generated Calendar Event marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceDate {
    /// A Contract's Notice Date.
    NoticeDate,
    /// A Contract's end date.
    EndDate,
    /// An Asset's warranty expiry.
    WarrantyExpires,
    /// An Asset's lease expiry.
    LeaseExpires,
}

impl SourceDate {
    /// Storage and API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoticeDate => "notice_date",
            Self::EndDate => "end_date",
            Self::WarrantyExpires => "warranty_expires_on",
            Self::LeaseExpires => "lease_expires_on",
        }
    }

    /// The source kind this date belongs to.
    #[must_use]
    pub const fn kind(self) -> SourceKind {
        match self {
            Self::NoticeDate | Self::EndDate => SourceKind::Contract,
            Self::WarrantyExpires | Self::LeaseExpires => SourceKind::Asset,
        }
    }

    /// Title prefix of the generated event.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoticeDate => "Notice date",
            Self::EndDate => "Contract ends",
            Self::WarrantyExpires => "Warranty expires",
            Self::LeaseExpires => "Lease expires",
        }
    }
}

impl FromStr for SourceDate {
    type Err = CalendarError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "notice_date" => Ok(Self::NoticeDate),
            "end_date" => Ok(Self::EndDate),
            "warranty_expires_on" => Ok(Self::WarrantyExpires),
            "lease_expires_on" => Ok(Self::LeaseExpires),
            _ => Err(CalendarError::invalid("source_date", "unknown source date")),
        }
    }
}

/// Where a generated Calendar Event comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventSource {
    /// Contract or Asset.
    pub kind: SourceKind,
    /// The Contract or Asset ID.
    pub id: Uuid,
    /// Which of its dates.
    pub date: SourceDate,
}

/// The dates another context reported for one record, as the Calendar sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFacts {
    /// Contract or Asset.
    pub kind: SourceKind,
    /// Its ID.
    pub id: Uuid,
    /// How the record is named in titles (`<number> <title>`, or the asset tag).
    pub label: String,
    /// Whether the record is active; inactive records have no generated events.
    pub active: bool,
    /// Each date of the record, `None` when it has none.
    pub dates: Vec<(SourceDate, Option<NaiveDate>)>,
}

/// One generated event a source should have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedEvent {
    /// Which date.
    pub date_kind: SourceDate,
    /// The day.
    pub date: NaiveDate,
    /// Title.
    pub title: String,
}

/// Most characters in a title.
pub const MAX_TITLE: usize = 255;
/// Most characters in a description.
pub const MAX_DESCRIPTION: usize = 10_000;
/// Most characters in a location.
pub const MAX_LOCATION: usize = 255;
/// Most characters in a Calendar Category name.
pub const MAX_CATEGORY_NAME: usize = 50;

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

impl SourceFacts {
    /// The generated events this source should have: one per date it has, none when the
    /// record is inactive (FR-CON-003, FR-AST-012).
    #[must_use]
    pub fn wanted(&self) -> Vec<WantedEvent> {
        if !self.active {
            return Vec::new();
        }
        let label = self.label.split_whitespace().collect::<Vec<_>>().join(" ");
        self.dates
            .iter()
            .filter(|(kind, _)| kind.kind() == self.kind)
            .filter_map(|(kind, date)| {
                date.map(|date| WantedEvent {
                    date_kind: *kind,
                    date,
                    title: truncate(&format!("{}: {label}", kind.label()), MAX_TITLE),
                })
            })
            .collect()
    }
}

// ----- Timing and ranges ------------------------------------------------------------------

/// When a Calendar Event happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventTiming {
    /// Whole days, `start` to `end` inclusive (no time zone: a day is a day everywhere).
    AllDay {
        /// First day.
        start: NaiveDate,
        /// Last day.
        end: NaiveDate,
    },
    /// From one instant to another.
    Timed {
        /// Start.
        start: DateTime<Utc>,
        /// End.
        end: DateTime<Utc>,
    },
}

impl EventTiming {
    /// All-day timing.
    ///
    /// # Errors
    ///
    /// The end is before the start.
    pub fn all_day(start: NaiveDate, end: NaiveDate) -> Result<Self, CalendarError> {
        if end < start {
            return Err(CalendarError::invalid(
                "end_date",
                "must be on or after the start date",
            ));
        }
        Ok(Self::AllDay { start, end })
    }

    /// Timed timing.
    ///
    /// # Errors
    ///
    /// The end is before the start.
    pub fn timed(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self, CalendarError> {
        if end < start {
            return Err(CalendarError::invalid(
                "ends_at",
                "must be at or after the start",
            ));
        }
        Ok(Self::Timed { start, end })
    }

    /// Whether the event touches `range` (timed events: its UTC days).
    #[must_use]
    pub fn overlaps(&self, range: &DateRange) -> bool {
        match *self {
            Self::AllDay { start, end } => start <= range.to && end >= range.from,
            Self::Timed { start, end } => {
                start < range.end_instant() && end >= range.start_instant()
            }
        }
    }

    /// Whether the event lasts whole days.
    #[must_use]
    pub const fn is_all_day(&self) -> bool {
        matches!(self, Self::AllDay { .. })
    }
}

/// Days a list request may span at most.
pub const MAX_RANGE_DAYS: u64 = 366;

/// The days a calendar list covers, `from` to `to` inclusive (at most
/// [`MAX_RANGE_DAYS`] days). Timed events are matched on UTC days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    from: NaiveDate,
    to: NaiveDate,
}

impl DateRange {
    /// A range.
    ///
    /// # Errors
    ///
    /// `to` is before `from`, or the range is longer than [`MAX_RANGE_DAYS`].
    pub fn new(from: NaiveDate, to: NaiveDate) -> Result<Self, CalendarError> {
        if to < from {
            return Err(CalendarError::invalid("to", "must be on or after `from`"));
        }
        let longest = from.checked_add_days(Days::new(MAX_RANGE_DAYS - 1));
        if longest.is_some_and(|last| to > last) {
            return Err(CalendarError::invalid(
                "to",
                format!("a range covers at most {MAX_RANGE_DAYS} days"),
            ));
        }
        Ok(Self { from, to })
    }

    /// First day.
    #[must_use]
    pub const fn from(&self) -> NaiveDate {
        self.from
    }

    /// Last day.
    #[must_use]
    pub const fn to(&self) -> NaiveDate {
        self.to
    }

    /// Midnight UTC at the start of `from`.
    #[must_use]
    pub fn start_instant(&self) -> DateTime<Utc> {
        self.from.and_time(NaiveTime::MIN).and_utc()
    }

    /// Midnight UTC after `to` (exclusive end).
    #[must_use]
    pub fn end_instant(&self) -> DateTime<Utc> {
        self.to.succ_opt().map_or(DateTime::<Utc>::MAX_UTC, |d| {
            d.and_time(NaiveTime::MIN).and_utc()
        })
    }
}

// ----- Calendar Category ------------------------------------------------------------------

/// What an Administrator edits on a Calendar Category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryInput {
    /// Name.
    pub name: String,
    /// Colour.
    pub colour: Colour,
    /// Active: offered for new events.
    pub is_active: bool,
}

/// A colour-coded Calendar Category (FR-CAL-001). Instance-wide reference data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarCategory {
    id: CalendarCategoryId,
    key: Option<String>,
    name: String,
    colour: Colour,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// A Calendar Category as stored.
#[derive(Debug, Clone)]
pub struct CategoryRecord {
    /// ID.
    pub id: CalendarCategoryId,
    /// System key (generated events), `None` for Administrators' categories.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// Colour.
    pub colour: Colour,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

fn category_name(name: &str) -> Result<String, CalendarError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_CATEGORY_NAME {
        return Err(CalendarError::invalid(
            "name",
            format!("must be 1 to {MAX_CATEGORY_NAME} characters"),
        ));
    }
    Ok(n.to_owned())
}

impl CalendarCategory {
    /// A new Calendar Category.
    ///
    /// # Errors
    ///
    /// Invalid name.
    pub fn create(input: CategoryInput, now: DateTime<Utc>) -> Result<Self, CalendarError> {
        Ok(Self {
            id: CalendarCategoryId::new(),
            key: None,
            name: category_name(&input.name)?,
            colour: input.colour,
            is_active: input.is_active,
            created_at: now,
            updated_at: now,
        })
    }

    /// The system category for events generated from `kind`.
    #[must_use]
    pub fn system(kind: SourceKind, now: DateTime<Utc>) -> Self {
        Self {
            id: CalendarCategoryId::new(),
            key: Some(kind.category_key().to_owned()),
            name: kind.category_name().to_owned(),
            colour: Colour(kind.category_colour().to_owned()),
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Restores a stored category.
    #[must_use]
    pub fn restore(r: CategoryRecord) -> Self {
        Self {
            id: r.id,
            key: r.key,
            name: r.name,
            colour: r.colour,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    /// Changes the category. System categories may be renamed and recoloured but stay
    /// active: generated events always need them.
    ///
    /// # Errors
    ///
    /// Invalid name, or deactivating a system category.
    pub fn edit(&mut self, input: CategoryInput, now: DateTime<Utc>) -> Result<(), CalendarError> {
        let name = category_name(&input.name)?;
        if self.is_system() && !input.is_active {
            return Err(CalendarError::state(
                "a system Calendar Category stays active: generated events use it",
            ));
        }
        self.name = name;
        self.colour = input.colour;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }

    /// Whether manual events may be put in this category.
    ///
    /// # Errors
    ///
    /// It is a system or inactive category.
    pub fn accepts_manual_events(&self) -> Result<(), CalendarError> {
        if self.is_system() {
            return Err(CalendarError::invalid(
                "category_id",
                "is reserved for generated events",
            ));
        }
        if !self.is_active {
            return Err(CalendarError::invalid("category_id", "is not active"));
        }
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> CalendarCategoryId {
        self.id
    }
    /// System key.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }
    /// Whether Cooper made it for generated events.
    #[must_use]
    pub const fn is_system(&self) -> bool {
        self.key.is_some()
    }
    /// Name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Colour.
    #[must_use]
    pub const fn colour(&self) -> &Colour {
        &self.colour
    }
    /// Active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
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

    /// Fields recorded in the audit trail.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("colour", json!(self.colour.as_str())),
            ("is_active", json!(self.is_active)),
        ]
    }
}

// ----- Calendar Event ---------------------------------------------------------------------

/// What an Analyst edits on a manual Calendar Event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDetails {
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Category.
    pub category_id: CalendarCategoryId,
    /// When.
    pub timing: EventTiming,
    /// Where (optional).
    pub location: Option<String>,
}

impl EventDetails {
    fn normalised(self) -> Result<Self, CalendarError> {
        let title = self.title.trim().to_owned();
        if title.is_empty() || title.chars().count() > MAX_TITLE {
            return Err(CalendarError::invalid(
                "title",
                format!("must be 1 to {MAX_TITLE} characters"),
            ));
        }
        if self.description.chars().count() > MAX_DESCRIPTION {
            return Err(CalendarError::invalid(
                "description",
                format!("must be at most {MAX_DESCRIPTION} characters"),
            ));
        }
        let location = self
            .location
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty());
        if location
            .as_ref()
            .is_some_and(|l| l.chars().count() > MAX_LOCATION)
        {
            return Err(CalendarError::invalid(
                "location",
                format!("must be at most {MAX_LOCATION} characters"),
            ));
        }
        Ok(Self {
            title,
            location,
            ..self
        })
    }
}

/// An entry on the IT Calendar of one Company: manual, or generated from another
/// context's date (then read-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    id: CalendarEventId,
    tenant_id: TenantId,
    details: EventDetails,
    source: Option<EventSource>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// A Calendar Event as stored.
#[derive(Debug, Clone)]
pub struct EventRecord {
    /// ID.
    pub id: CalendarEventId,
    /// Company.
    pub tenant_id: TenantId,
    /// Details.
    pub details: EventDetails,
    /// Source of a generated event.
    pub source: Option<EventSource>,
    /// Creator.
    pub created_by: Actor,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

fn read_only() -> CalendarError {
    CalendarError::state("generated Calendar Events are read-only: change their source instead")
}

impl CalendarEvent {
    /// A manual event. The caller checks the category accepts manual events.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn create(
        tenant_id: TenantId,
        details: EventDetails,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, CalendarError> {
        Ok(Self {
            id: CalendarEventId::new(),
            tenant_id,
            details: details.normalised()?,
            source: None,
            created_by: actor,
            created_at: now,
            updated_at: now,
        })
    }

    /// A generated all-day event for one date of a source.
    #[must_use]
    pub fn generated(
        tenant_id: TenantId,
        source_id: Uuid,
        wanted: &WantedEvent,
        category_id: CalendarCategoryId,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: CalendarEventId::new(),
            tenant_id,
            details: EventDetails {
                title: wanted.title.clone(),
                description: String::new(),
                category_id,
                timing: EventTiming::AllDay {
                    start: wanted.date,
                    end: wanted.date,
                },
                location: None,
            },
            source: Some(EventSource {
                kind: wanted.date_kind.kind(),
                id: source_id,
                date: wanted.date_kind,
            }),
            created_by: Actor::Automation,
            created_at: now,
            updated_at: now,
        }
    }

    /// Restores a stored event.
    #[must_use]
    pub fn restore(r: EventRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            details: r.details,
            source: r.source,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    /// Changes a manual event.
    ///
    /// # Errors
    ///
    /// The event is generated, or the details are invalid.
    pub fn edit(&mut self, details: EventDetails, now: DateTime<Utc>) -> Result<(), CalendarError> {
        self.ensure_manual()?;
        self.details = details.normalised()?;
        self.updated_at = now;
        Ok(())
    }

    /// Fails for generated events, which only their source changes.
    ///
    /// # Errors
    ///
    /// The event is generated.
    pub fn ensure_manual(&self) -> Result<(), CalendarError> {
        if self.source.is_some() {
            return Err(read_only());
        }
        Ok(())
    }

    /// Brings a generated event in line with its source. `true` when something changed.
    pub fn refresh_generated(
        &mut self,
        wanted: &WantedEvent,
        category_id: CalendarCategoryId,
        now: DateTime<Utc>,
    ) -> bool {
        let timing = EventTiming::AllDay {
            start: wanted.date,
            end: wanted.date,
        };
        if self.details.title == wanted.title
            && self.details.timing == timing
            && self.details.category_id == category_id
        {
            return false;
        }
        self.details.title.clone_from(&wanted.title);
        self.details.timing = timing;
        self.details.category_id = category_id;
        self.updated_at = now;
        true
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> CalendarEventId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &EventDetails {
        &self.details
    }
    /// Source of a generated event.
    #[must_use]
    pub const fn source(&self) -> Option<EventSource> {
        self.source
    }
    /// Whether Cooper generated it.
    #[must_use]
    pub const fn is_generated(&self) -> bool {
        self.source.is_some()
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

    /// Fields recorded in the audit trail.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        let (start, end) = match d.timing {
            EventTiming::AllDay { start, end } => (json!(start), json!(end)),
            EventTiming::Timed { start, end } => (json!(start), json!(end)),
        };
        vec![
            ("title", json!(d.title)),
            ("description", json!(d.description)),
            ("category_id", json!(d.category_id)),
            ("all_day", json!(d.timing.is_all_day())),
            ("start", start),
            ("end", end),
            ("location", json!(d.location)),
        ]
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;

    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap()
    }

    fn colour(s: &str) -> Colour {
        s.parse().unwrap()
    }

    fn details(timing: EventTiming) -> EventDetails {
        EventDetails {
            title: "  CAB meeting ".into(),
            description: String::new(),
            category_id: CalendarCategoryId::new(),
            timing,
            location: Some("  ".into()),
        }
    }

    #[test]
    fn colours_are_hex_triplets_stored_in_lower_case() {
        assert_eq!(colour("#A1B2C3").as_str(), "#a1b2c3");
        assert_eq!(colour(" #2563eb ").as_str(), "#2563eb");
        for bad in [
            "", "#", "2563eb", "#2563e", "#2563ebb", "#gggggg", "red", "#25 3eb",
        ] {
            assert!(bad.parse::<Colour>().is_err(), "{bad}");
        }
    }

    proptest! {
        #[test]
        fn any_six_hex_digits_are_a_colour(hex in "[0-9a-fA-F]{6}") {
            let c: Colour = format!("#{hex}").parse().unwrap();
            prop_assert_eq!(c.as_str(), format!("#{}", hex.to_ascii_lowercase()));
        }

        #[test]
        fn anything_else_is_not_a_colour(s in "\\PC{0,10}") {
            let t = s.trim();
            let ok = t.len() == 7 && t.starts_with('#') && t[1..].chars().all(|c| c.is_ascii_hexdigit());
            prop_assert_eq!(s.parse::<Colour>().is_ok(), ok);
        }

        #[test]
        fn ranges_accept_up_to_a_year(start in 0i64..20_000, len in 0u64..800) {
            let from = d(2000, 1, 1) + chrono::Duration::days(start);
            let to = from.checked_add_days(Days::new(len)).unwrap();
            prop_assert_eq!(DateRange::new(from, to).is_ok(), len < MAX_RANGE_DAYS);
        }
    }

    #[test]
    fn ranges_end_on_or_after_their_start() {
        assert!(DateRange::new(d(2026, 11, 2), d(2026, 11, 1)).is_err());
        assert!(DateRange::new(d(2026, 11, 1), d(2026, 11, 1)).is_ok());
        assert!(DateRange::new(d(2026, 1, 1), d(2026, 12, 31)).is_ok());
        assert!(DateRange::new(d(2026, 1, 1), d(2027, 1, 2)).is_err());
    }

    #[test]
    fn events_end_on_or_after_their_start() {
        assert!(EventTiming::all_day(d(2026, 11, 2), d(2026, 11, 1)).is_err());
        assert!(EventTiming::all_day(d(2026, 11, 2), d(2026, 11, 2)).is_ok());
        let t = now();
        assert!(EventTiming::timed(t, t - chrono::Duration::minutes(1)).is_err());
        assert!(EventTiming::timed(t, t).is_ok());
    }

    #[test]
    fn overlap_includes_both_ends_of_the_range() {
        let nov = DateRange::new(d(2026, 11, 1), d(2026, 11, 30)).unwrap();
        let all_day = |a, b| EventTiming::all_day(a, b).unwrap();
        assert!(all_day(d(2026, 10, 30), d(2026, 11, 1)).overlaps(&nov));
        assert!(all_day(d(2026, 11, 30), d(2026, 12, 2)).overlaps(&nov));
        assert!(!all_day(d(2026, 10, 1), d(2026, 10, 31)).overlaps(&nov));
        let at = |m, day, h| Utc.with_ymd_and_hms(2026, m, day, h, 0, 0).unwrap();
        let timed = |a, b| EventTiming::timed(a, b).unwrap();
        assert!(timed(at(11, 30, 23), at(12, 1, 1)).overlaps(&nov));
        assert!(!timed(at(12, 1, 0), at(12, 1, 1)).overlaps(&nov));
        assert!(timed(at(10, 31, 22), at(11, 1, 0)).overlaps(&nov));
        assert!(!timed(at(10, 31, 22), at(10, 31, 23)).overlaps(&nov));
    }

    #[test]
    fn manual_events_are_trimmed_and_validated() {
        let timing = EventTiming::all_day(d(2026, 11, 3), d(2026, 11, 3)).unwrap();
        let e = CalendarEvent::create(TenantId::new(), details(timing), Actor::Automation, now())
            .unwrap();
        assert_eq!(e.details().title, "CAB meeting");
        assert_eq!(e.details().location, None);
        assert!(!e.is_generated());
        let mut bad = details(timing);
        bad.title = " ".into();
        assert!(CalendarEvent::create(TenantId::new(), bad, Actor::Automation, now()).is_err());
        let mut long = details(timing);
        long.location = Some("x".repeat(MAX_LOCATION + 1));
        assert!(CalendarEvent::create(TenantId::new(), long, Actor::Automation, now()).is_err());
    }

    fn contract(active: bool, notice: Option<NaiveDate>, end: Option<NaiveDate>) -> SourceFacts {
        SourceFacts {
            kind: SourceKind::Contract,
            id: Uuid::now_v7(),
            label: "CON-7  Printer   lease".into(),
            active,
            dates: vec![(SourceDate::NoticeDate, notice), (SourceDate::EndDate, end)],
        }
    }

    #[test]
    fn active_sources_want_one_event_per_date() {
        let w = contract(true, Some(d(2027, 1, 31)), Some(d(2027, 4, 30))).wanted();
        assert_eq!(
            w,
            vec![
                WantedEvent {
                    date_kind: SourceDate::NoticeDate,
                    date: d(2027, 1, 31),
                    title: "Notice date: CON-7 Printer lease".into()
                },
                WantedEvent {
                    date_kind: SourceDate::EndDate,
                    date: d(2027, 4, 30),
                    title: "Contract ends: CON-7 Printer lease".into()
                },
            ]
        );
        assert_eq!(contract(true, None, Some(d(2027, 4, 30))).wanted().len(), 1);
        assert!(
            contract(false, Some(d(2027, 1, 31)), Some(d(2027, 4, 30)))
                .wanted()
                .is_empty()
        );
    }

    #[test]
    fn dates_of_another_kind_are_ignored_and_titles_are_capped() {
        let mut f = contract(true, None, None);
        f.dates
            .push((SourceDate::WarrantyExpires, Some(d(2027, 1, 1))));
        assert!(f.wanted().is_empty());
        f.dates.push((SourceDate::EndDate, Some(d(2027, 1, 1))));
        f.label = "x".repeat(400);
        assert_eq!(f.wanted()[0].title.chars().count(), MAX_TITLE);
    }

    #[test]
    fn generated_events_are_read_only_and_follow_their_source() {
        let cat = CalendarCategoryId::new();
        let wanted = &contract(true, Some(d(2027, 1, 31)), None).wanted()[0];
        let mut e = CalendarEvent::generated(TenantId::new(), Uuid::now_v7(), wanted, cat, now());
        assert!(e.is_generated());
        assert_eq!(e.source().unwrap().date, SourceDate::NoticeDate);
        assert_eq!(e.ensure_manual(), Err(read_only()));
        let timing = EventTiming::all_day(d(2026, 11, 3), d(2026, 11, 3)).unwrap();
        assert!(matches!(
            e.edit(details(timing), now()),
            Err(CalendarError::InvalidState(_))
        ));
        assert!(!e.refresh_generated(wanted, cat, now()));
        let moved = WantedEvent {
            date: d(2027, 2, 28),
            ..wanted.clone()
        };
        assert!(e.refresh_generated(&moved, cat, now()));
        assert_eq!(
            e.details().timing,
            EventTiming::AllDay {
                start: d(2027, 2, 28),
                end: d(2027, 2, 28)
            }
        );
    }

    #[test]
    fn system_categories_stay_active_and_take_no_manual_events() {
        let mut c = CalendarCategory::system(SourceKind::Asset, now());
        assert!(c.is_system());
        assert!(c.accepts_manual_events().is_err());
        let input = |active| CategoryInput {
            name: "Hardware".into(),
            colour: colour("#123456"),
            is_active: active,
        };
        assert!(c.edit(input(false), now()).is_err());
        c.edit(input(true), now()).unwrap();
        assert_eq!(c.name(), "Hardware");
        let mut m = CalendarCategory::create(input(true), now()).unwrap();
        assert!(m.accepts_manual_events().is_ok());
        m.edit(input(false), now()).unwrap();
        assert!(m.accepts_manual_events().is_err());
        let blank = CategoryInput {
            name: " ".into(),
            ..input(true)
        };
        assert!(CalendarCategory::create(blank, now()).is_err());
    }

    #[test]
    fn source_names_round_trip() {
        for k in SourceKind::ALL {
            assert_eq!(k.as_str().parse::<SourceKind>().unwrap(), k);
        }
        for s in [
            SourceDate::NoticeDate,
            SourceDate::EndDate,
            SourceDate::WarrantyExpires,
            SourceDate::LeaseExpires,
        ] {
            assert_eq!(s.as_str().parse::<SourceDate>().unwrap(), s);
        }
    }
}
