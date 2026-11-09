//! Morning Checks domain (ADR-0031). Pure Rust: no I/O.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Utc, Weekday};
use chrono_tz::Tz;
use cooper_kernel::{Actor, AnalystId, DomainEvent, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

cooper_kernel::define_id!(
    /// Identifies a Check Group.
    CheckGroupId
);
cooper_kernel::define_id!(
    /// Identifies a Check.
    CheckId
);
cooper_kernel::define_id!(
    /// Identifies a Check Result.
    CheckResultId
);

/// A Morning Checks rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MorningChecksError {
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

impl MorningChecksError {
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

type Result<T> = std::result::Result<T, MorningChecksError>;

/// Longest Check Result notes.
pub const MAX_NOTES: usize = 10_000;
/// Longest description of a Check or Check Group.
pub const MAX_DESCRIPTION: usize = 2000;
/// Most days in one history grid (about three months).
pub const MAX_HISTORY_DAYS: u32 = 92;
/// Most days in one export.
pub const MAX_EXPORT_DAYS: u32 = 366;

fn name(field: &'static str, value: &str, max: usize) -> Result<String> {
    let v = value.trim();
    if v.is_empty() || v.chars().count() > max {
        return Err(MorningChecksError::invalid(
            field,
            format!("must be 1 to {max} characters"),
        ));
    }
    Ok(v.to_owned())
}

fn description(value: &str) -> Result<String> {
    let v = value.trim();
    if v.chars().count() > MAX_DESCRIPTION {
        return Err(MorningChecksError::invalid(
            "description",
            format!("at most {MAX_DESCRIPTION} characters"),
        ));
    }
    Ok(v.to_owned())
}

fn colour(c: &str) -> Result<String> {
    let c = c.trim();
    if c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(c.to_owned())
    } else {
        Err(MorningChecksError::invalid("colour", "must be #rrggbb"))
    }
}

// ----- Check Statuses (FR-MCK-002) --------------------------------------------------------

/// A Check Status: reference data with a colour and the *requires notes* flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckStatus {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// `#rrggbb`, used to colour-code results.
    pub colour: String,
    /// A result with this status must explain itself.
    pub requires_notes: bool,
    /// Display order.
    pub sort_order: i32,
    /// Selectable for new results.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// Fields an Administrator edits on a Check Status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckStatusInput {
    /// Name.
    pub name: String,
    /// `#rrggbb`.
    pub colour: String,
    /// Requires notes.
    pub requires_notes: bool,
    /// Order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl CheckStatus {
    /// A new Check Status.
    ///
    /// # Errors
    ///
    /// Invalid input.
    pub fn create(input: CheckStatusInput, now: DateTime<Utc>) -> Result<Self> {
        let mut s = Self {
            id: Uuid::now_v7(),
            key: None,
            name: String::new(),
            colour: String::new(),
            requires_notes: false,
            sort_order: 0,
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        s.apply(input, now)?;
        Ok(s)
    }

    /// Changes the Check Status. Statuses are deactivated, never deleted, so recorded
    /// results keep theirs.
    ///
    /// # Errors
    ///
    /// A bad name or colour.
    pub fn apply(&mut self, input: CheckStatusInput, now: DateTime<Utc>) -> Result<()> {
        let name = name("name", &input.name, 50)?;
        let colour = colour(&input.colour)?;
        self.name = name;
        self.colour = colour;
        self.requires_notes = input.requires_notes;
        self.sort_order = input.sort_order;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }
}

// ----- Check Groups (FR-MCK-001) ----------------------------------------------------------

/// Who a Check Group is routed to: a Team or an Analyst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckAssignee {
    /// Every active member of the Team.
    Team(TeamId),
    /// One Analyst.
    Analyst(AnalystId),
}

/// What an Administrator edits on a Check Group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupDetails {
    /// Name, unique within the Company.
    pub name: String,
    /// Description.
    pub description: String,
    /// Routed to.
    pub assignee: Option<CheckAssignee>,
    /// Display order.
    pub sort_order: i32,
    /// Its Checks are part of the daily round.
    pub is_active: bool,
}

impl GroupDetails {
    fn validate(mut self) -> Result<Self> {
        self.name = name("name", &self.name, 100)?;
        self.description = description(&self.description)?;
        Ok(self)
    }
}

/// A Check Group of one Company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckGroup {
    id: CheckGroupId,
    tenant_id: TenantId,
    details: GroupDetails,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl CheckGroup {
    /// A new Check Group.
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn create(tenant_id: TenantId, details: GroupDetails, now: DateTime<Utc>) -> Result<Self> {
        Ok(Self {
            id: CheckGroupId::new(),
            tenant_id,
            details: details.validate()?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a stored Check Group.
    #[must_use]
    pub const fn restore(
        id: CheckGroupId,
        tenant_id: TenantId,
        details: GroupDetails,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            details,
            created_at,
            updated_at,
        }
    }

    /// Changes the details (deactivating instead of deleting).
    ///
    /// # Errors
    ///
    /// Invalid details.
    pub fn edit(&mut self, details: GroupDetails, now: DateTime<Utc>) -> Result<()> {
        self.details = details.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> CheckGroupId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &GroupDetails {
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

    /// Audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let (team, analyst) = match self.details.assignee {
            Some(CheckAssignee::Team(t)) => (Some(t.as_uuid()), None),
            Some(CheckAssignee::Analyst(a)) => (None, Some(a.as_uuid())),
            None => (None, None),
        };
        vec![
            ("name", json!(self.details.name)),
            ("description", json!(self.details.description)),
            ("assigned_team_id", json!(team)),
            ("assigned_analyst_id", json!(analyst)),
            ("sort_order", json!(self.details.sort_order)),
            ("is_active", json!(self.details.is_active)),
        ]
    }
}

// ----- Checks (FR-MCK-001) ----------------------------------------------------------------

/// What an Administrator edits on a Check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckDetails {
    /// Name, for example "Backups completed".
    pub name: String,
    /// What to verify and how.
    pub description: String,
    /// Its Check Group, if any.
    pub group_id: Option<CheckGroupId>,
    /// Its own Analyst (more specific than the group's routing).
    pub assigned_analyst: Option<AnalystId>,
    /// Display order.
    pub sort_order: i32,
    /// Part of the daily round.
    pub is_active: bool,
}

impl CheckDetails {
    fn validate(mut self) -> Result<Self> {
        self.name = name("name", &self.name, 255)?;
        self.description = description(&self.description)?;
        Ok(self)
    }
}

/// A Check of one Company: one item to verify each day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    id: CheckId,
    tenant_id: TenantId,
    details: CheckDetails,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// The group of a Check must be the one named in its details, of its Company, and active
/// unless the Check is already in it.
fn check_group(
    tenant: TenantId,
    details: &CheckDetails,
    group: Option<&CheckGroup>,
    current: Option<CheckGroupId>,
) -> Result<()> {
    match (details.group_id, group) {
        (None, _) => Ok(()),
        (Some(id), Some(g)) if g.id == id && g.tenant_id == tenant => {
            if !g.details.is_active && current != Some(id) {
                return Err(MorningChecksError::invalid(
                    "check_group_id",
                    "a Check cannot be added to an inactive Check Group",
                ));
            }
            Ok(())
        }
        (Some(_), _) => Err(MorningChecksError::invalid(
            "check_group_id",
            "is not a Check Group of the Check's Company",
        )),
    }
}

impl Check {
    /// A new Check. `group` is the Check Group named in `details`, if any.
    ///
    /// # Errors
    ///
    /// Invalid details, or a group of another Company or inactive.
    pub fn create(
        tenant_id: TenantId,
        details: CheckDetails,
        group: Option<&CheckGroup>,
        now: DateTime<Utc>,
    ) -> Result<Self> {
        check_group(tenant_id, &details, group, None)?;
        Ok(Self {
            id: CheckId::new(),
            tenant_id,
            details: details.validate()?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a stored Check.
    #[must_use]
    pub const fn restore(
        id: CheckId,
        tenant_id: TenantId,
        details: CheckDetails,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            details,
            created_at,
            updated_at,
        }
    }

    /// Changes the details (deactivating instead of deleting, so history stays).
    ///
    /// # Errors
    ///
    /// Invalid details, or a new group of another Company or inactive.
    pub fn edit(
        &mut self,
        details: CheckDetails,
        group: Option<&CheckGroup>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        check_group(self.tenant_id, &details, group, self.details.group_id)?;
        self.details = details.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// Whether the Check is part of the daily round: active, and its group (if any) is
    /// active too.
    #[must_use]
    pub fn in_round(&self, group: Option<&CheckGroup>) -> bool {
        self.details.is_active && group.is_none_or(|g| g.details.is_active)
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> CheckId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &CheckDetails {
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

    /// Audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.details.name)),
            ("description", json!(self.details.description)),
            (
                "check_group_id",
                json!(self.details.group_id.map(|g| g.as_uuid())),
            ),
            (
                "assigned_analyst_id",
                json!(self.details.assigned_analyst.map(|a| a.as_uuid())),
            ),
            ("sort_order", json!(self.details.sort_order)),
            ("is_active", json!(self.details.is_active)),
        ]
    }
}

/// Where a Check's routing came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingSource {
    /// The Check's own Analyst.
    Check,
    /// Its Check Group's Analyst or Team.
    Group,
}

/// Who a Check is routed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routing {
    /// One Analyst.
    Analyst(AnalystId, RoutingSource),
    /// The members of a Team (always from the group).
    Team(TeamId),
    /// Nobody in particular.
    Nobody,
}

/// Who a Check is routed to, most specific first: the Check's own Analyst, then its
/// group's Analyst, then its group's Team.
#[must_use]
pub fn routing(check: &Check, group: Option<&CheckGroup>) -> Routing {
    if let Some(a) = check.details.assigned_analyst {
        return Routing::Analyst(a, RoutingSource::Check);
    }
    match group.and_then(|g| g.details.assignee) {
        Some(CheckAssignee::Analyst(a)) => Routing::Analyst(a, RoutingSource::Group),
        Some(CheckAssignee::Team(t)) => Routing::Team(t),
        None => Routing::Nobody,
    }
}

impl Routing {
    /// Whether the Check is the business of `analyst`, a member of `teams`.
    #[must_use]
    pub fn concerns(&self, analyst: AnalystId, teams: &BTreeSet<TeamId>) -> bool {
        match self {
            Self::Analyst(a, _) => *a == analyst,
            Self::Team(t) => teams.contains(t),
            Self::Nobody => false,
        }
    }
}

// ----- Check Dates ------------------------------------------------------------------------

/// Parses an IANA time zone name (`Europe/London`).
#[must_use]
pub fn parse_time_zone(name: &str) -> Option<Tz> {
    name.trim().parse().ok()
}

/// Today's Check Date: the calendar day of `now` in the time zone `tz`, never the
/// server's local day.
#[must_use]
pub fn check_date_today(now: DateTime<Utc>, tz: Tz) -> NaiveDate {
    now.with_timezone(&tz).date_naive()
}

/// An inclusive range of Check Dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    from: NaiveDate,
    to: NaiveDate,
}

impl DateRange {
    /// A range of at most `max_days` days, `from` not after `to`.
    ///
    /// # Errors
    ///
    /// Reversed or too long.
    pub fn new(from: NaiveDate, to: NaiveDate, max_days: u32) -> Result<Self> {
        if from > to {
            return Err(MorningChecksError::invalid(
                "from",
                "must not be after `to`",
            ));
        }
        if (to - from).num_days() >= i64::from(max_days) {
            return Err(MorningChecksError::invalid(
                "to",
                format!("the range is at most {max_days} days"),
            ));
        }
        Ok(Self { from, to })
    }

    /// The `days` days ending on `to`.
    ///
    /// # Errors
    ///
    /// `days` is zero or more than `max_days`.
    pub fn ending(to: NaiveDate, days: u32, max_days: u32) -> Result<Self> {
        let back = days
            .checked_sub(1)
            .ok_or_else(|| MorningChecksError::invalid("from", "at least one day"))?;
        let from = to
            .checked_sub_days(chrono::Days::new(u64::from(back)))
            .ok_or_else(|| MorningChecksError::invalid("from", "out of range"))?;
        Self::new(from, to, max_days)
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
    /// Every day, oldest first.
    #[must_use]
    pub fn days(&self) -> Vec<NaiveDate> {
        self.from
            .iter_days()
            .take_while(|d| *d <= self.to)
            .collect()
    }
}

// ----- Check Results (FR-MCK-003) ---------------------------------------------------------

/// The recorded Check Status and notes of one Check on one Check Date. Unique per Check
/// per date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    id: CheckResultId,
    tenant_id: TenantId,
    check_id: CheckId,
    check_date: NaiveDate,
    status_id: Uuid,
    notes: String,
    recorded_by: Actor,
    recorded_at: DateTime<Utc>,
    modified_by: Actor,
    updated_at: DateTime<Utc>,
}

/// Every field of a stored Check Result.
#[derive(Debug, Clone)]
pub struct CheckResultRecord {
    /// ID.
    pub id: CheckResultId,
    /// Company.
    pub tenant_id: TenantId,
    /// Check.
    pub check_id: CheckId,
    /// Check Date.
    pub check_date: NaiveDate,
    /// Check Status.
    pub status_id: Uuid,
    /// Notes.
    pub notes: String,
    /// Who first recorded it.
    pub recorded_by: Actor,
    /// When.
    pub recorded_at: DateTime<Utc>,
    /// Who last changed it.
    pub modified_by: Actor,
    /// When.
    pub updated_at: DateTime<Utc>,
}

/// A status and notes for a Check on a date, validated: the Check is in the round, the
/// date has come, the status is active and notes are given when it requires them.
fn entry(
    check: &Check,
    group: Option<&CheckGroup>,
    date: NaiveDate,
    status: &CheckStatus,
    notes: &str,
    today: NaiveDate,
) -> Result<String> {
    if !check.in_round(group) {
        return Err(MorningChecksError::state(
            "the Check is not active, so it is not recorded",
        ));
    }
    if date > today {
        return Err(MorningChecksError::invalid(
            "check_date",
            "cannot be after today",
        ));
    }
    if !status.is_active {
        return Err(MorningChecksError::invalid(
            "status_id",
            "is not an active Check Status",
        ));
    }
    let notes = notes.trim();
    if notes.chars().count() > MAX_NOTES {
        return Err(MorningChecksError::invalid(
            "notes",
            format!("at most {MAX_NOTES} characters"),
        ));
    }
    if status.requires_notes && notes.is_empty() {
        return Err(MorningChecksError::invalid(
            "notes",
            format!("are required for the {} status", status.name),
        ));
    }
    Ok(notes.to_owned())
}

impl CheckResult {
    /// Records a Check's first result for `date`.
    ///
    /// # Errors
    ///
    /// The Check is not in the round, the date is after `today`, the status is inactive,
    /// or notes are missing where the status requires them.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        check: &Check,
        group: Option<&CheckGroup>,
        date: NaiveDate,
        status: &CheckStatus,
        notes: &str,
        actor: Actor,
        today: NaiveDate,
        now: DateTime<Utc>,
    ) -> Result<Self> {
        let notes = entry(check, group, date, status, notes, today)?;
        Ok(Self {
            id: CheckResultId::new(),
            tenant_id: check.tenant_id,
            check_id: check.id,
            check_date: date,
            status_id: status.id,
            notes,
            recorded_by: actor,
            recorded_at: now,
            modified_by: actor,
            updated_at: now,
        })
    }

    /// Changes the status and notes, crediting `actor` as the last to modify it. Returns
    /// `false` (and changes nothing) when both are unchanged.
    ///
    /// # Errors
    ///
    /// As [`CheckResult::record`].
    #[allow(clippy::too_many_arguments)]
    pub fn revise(
        &mut self,
        check: &Check,
        group: Option<&CheckGroup>,
        status: &CheckStatus,
        notes: &str,
        actor: Actor,
        today: NaiveDate,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        if check.id != self.check_id {
            return Err(MorningChecksError::state("the result is of another Check"));
        }
        let notes = entry(check, group, self.check_date, status, notes, today)?;
        if status.id == self.status_id && notes == self.notes {
            return Ok(false);
        }
        self.status_id = status.id;
        self.notes = notes;
        self.modified_by = actor;
        self.updated_at = now;
        Ok(true)
    }

    /// Rebuilds a stored Check Result.
    #[must_use]
    pub fn restore(r: CheckResultRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            check_id: r.check_id,
            check_date: r.check_date,
            status_id: r.status_id,
            notes: r.notes,
            recorded_by: r.recorded_by,
            recorded_at: r.recorded_at,
            modified_by: r.modified_by,
            updated_at: r.updated_at,
        }
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> CheckResultId {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Check.
    #[must_use]
    pub const fn check_id(&self) -> CheckId {
        self.check_id
    }
    /// Check Date.
    #[must_use]
    pub const fn check_date(&self) -> NaiveDate {
        self.check_date
    }
    /// Check Status.
    #[must_use]
    pub const fn status_id(&self) -> Uuid {
        self.status_id
    }
    /// Notes.
    #[must_use]
    pub fn notes(&self) -> &str {
        &self.notes
    }
    /// Who first recorded it.
    #[must_use]
    pub const fn recorded_by(&self) -> Actor {
        self.recorded_by
    }
    /// When it was first recorded.
    #[must_use]
    pub const fn recorded_at(&self) -> DateTime<Utc> {
        self.recorded_at
    }
    /// Who last changed it.
    #[must_use]
    pub const fn modified_by(&self) -> Actor {
        self.modified_by
    }
    /// When it last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("check_id", json!(self.check_id.as_uuid())),
            ("check_date", json!(self.check_date)),
            ("status_id", json!(self.status_id)),
            ("notes", json!(self.notes)),
        ]
    }
}

// ----- Result Links (FR-MCK-004) ----------------------------------------------------------

/// What a Result Link points at (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkKind {
    /// A follow-up ticket.
    Ticket,
    /// A follow-up Task.
    Task,
}

impl LinkKind {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Task => "task",
        }
    }
}

impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LinkKind {
    type Err = MorningChecksError;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "ticket" => Ok(Self::Ticket),
            "task" => Ok(Self::Task),
            _ => Err(MorningChecksError::invalid(
                "kind",
                "must be ticket or task",
            )),
        }
    }
}

/// A Result Link: a Check Result's follow-up ticket or Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultLink {
    /// Kind.
    pub kind: LinkKind,
    /// The ticket or Task.
    pub target_id: Uuid,
    /// Who linked it.
    pub linked_by: Actor,
    /// When.
    pub linked_at: DateTime<Utc>,
}

// ----- Reminders (FR-MCK-006) -------------------------------------------------------------

/// Parses a reminder time of day, `HH:MM` (24-hour).
#[must_use]
pub fn parse_reminder_time(value: &str) -> Option<NaiveTime> {
    let v = value.trim();
    if v.len() != 5 {
        return None;
    }
    NaiveTime::parse_from_str(v, "%H:%M").ok()
}

/// When reminders for unrecorded Checks are sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderPolicy {
    /// Reminders are on.
    pub enabled: bool,
    /// Local time of day from which they are sent.
    pub at: NaiveTime,
    /// Also on Saturdays and Sundays.
    pub weekends: bool,
}

impl ReminderPolicy {
    /// The Check Date to remind about at local time `local`, if reminders are due then.
    #[must_use]
    pub fn due_date(&self, local: NaiveDateTime) -> Option<NaiveDate> {
        let weekend = matches!(local.weekday(), Weekday::Sat | Weekday::Sun);
        (self.enabled && local.time() >= self.at && (self.weekends || !weekend))
            .then(|| local.date())
    }
}

/// How many Checks in the round are still unrecorded, per Check Group (`None`: ungrouped)
/// and recipient. A Check routed to an Analyst reminds that Analyst; one routed to a
/// Team reminds each member in `team_members`; one routed to nobody reminds nobody.
#[must_use]
pub fn unrecorded_by_recipient<'a>(
    round: impl IntoIterator<Item = (&'a Check, Option<&'a CheckGroup>)>,
    recorded: &BTreeSet<CheckId>,
    team_members: &BTreeMap<TeamId, BTreeSet<AnalystId>>,
) -> BTreeMap<(Option<CheckGroupId>, AnalystId), u32> {
    let mut out: BTreeMap<(Option<CheckGroupId>, AnalystId), u32> = BTreeMap::new();
    for (check, group) in round {
        if !check.in_round(group) || recorded.contains(&check.id) {
            continue;
        }
        let recipients: Vec<AnalystId> = match routing(check, group) {
            Routing::Analyst(a, _) => vec![a],
            Routing::Team(t) => team_members
                .get(&t)
                .map(|m| m.iter().copied().collect())
                .unwrap_or_default(),
            Routing::Nobody => Vec::new(),
        };
        for a in recipients {
            *out.entry((check.details.group_id, a)).or_default() += 1;
        }
    }
    out
}

// ----- Domain Events ----------------------------------------------------------------------

/// A Check Result was recorded or changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResultRecorded {
    /// The result.
    pub result_id: CheckResultId,
    /// Its Company.
    pub tenant_id: TenantId,
    /// The Check.
    pub check_id: CheckId,
    /// The Check Date.
    pub check_date: NaiveDate,
    /// The Check Status.
    pub status_id: Uuid,
    /// First result for that Check and date (`false`: a change).
    pub is_new: bool,
}

impl DomainEvent for CheckResultRecorded {
    const EVENT_TYPE: &'static str = "check_result.recorded";
    const AGGREGATE_TYPE: &'static str = "check_result";
    fn aggregate_id(&self) -> Uuid {
        self.result_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// A Check Result was cleared: the Check is unrecorded again for that date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResultCleared {
    /// The result.
    pub result_id: CheckResultId,
    /// Its Company.
    pub tenant_id: TenantId,
    /// The Check.
    pub check_id: CheckId,
    /// The Check Date.
    pub check_date: NaiveDate,
}

impl DomainEvent for CheckResultCleared {
    const EVENT_TYPE: &'static str = "check_result.cleared";
    const AGGREGATE_TYPE: &'static str = "check_result";
    fn aggregate_id(&self) -> Uuid {
        self.result_id.as_uuid()
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

/// Checks routed to an Analyst were still unrecorded at the reminder time (FR-MCK-006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksUnrecorded {
    /// The reminder (one per date, group and Analyst).
    pub reminder_id: Uuid,
    /// The Company.
    pub tenant_id: TenantId,
    /// The Check Date.
    pub check_date: NaiveDate,
    /// The Check Group (`None`: ungrouped Checks).
    pub check_group_id: Option<CheckGroupId>,
    /// Its name, for the Notification.
    pub check_group_name: Option<String>,
    /// Who to remind.
    pub analyst_id: AnalystId,
    /// How many of their Checks were unrecorded.
    pub unrecorded: u32,
}

impl DomainEvent for ChecksUnrecorded {
    const EVENT_TYPE: &'static str = "morning_checks.unrecorded";
    const AGGREGATE_TYPE: &'static str = "check_reminder";
    fn aggregate_id(&self) -> Uuid {
        self.reminder_id
    }
    fn tenant_id(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn now() -> DateTime<Utc> {
        at("2026-11-09T10:00:00Z")
    }

    fn day(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn status(name: &str, requires_notes: bool, active: bool) -> CheckStatus {
        CheckStatus::create(
            CheckStatusInput {
                name: name.into(),
                colour: "#28a745".into(),
                requires_notes,
                sort_order: 10,
                is_active: active,
            },
            now(),
        )
        .unwrap()
    }

    fn group(tenant: TenantId, assignee: Option<CheckAssignee>, active: bool) -> CheckGroup {
        CheckGroup::create(
            tenant,
            GroupDetails {
                name: "Infrastructure".into(),
                assignee,
                is_active: active,
                ..GroupDetails::default()
            },
            now(),
        )
        .unwrap()
    }

    fn check(
        tenant: TenantId,
        g: Option<&CheckGroup>,
        analyst: Option<AnalystId>,
        active: bool,
    ) -> Check {
        Check::create(
            tenant,
            CheckDetails {
                name: "Backups completed".into(),
                group_id: g.map(CheckGroup::id),
                assigned_analyst: analyst,
                is_active: active,
                ..CheckDetails::default()
            },
            g,
            now(),
        )
        .unwrap()
    }

    fn analyst() -> Actor {
        Actor::Analyst(AnalystId::new())
    }

    #[test]
    fn check_status_needs_a_name_and_a_hex_colour() {
        let mut input = CheckStatusInput {
            name: " ".into(),
            colour: "#ffc107".into(),
            ..CheckStatusInput::default()
        };
        assert!(CheckStatus::create(input.clone(), now()).is_err());
        input.name = "Amber".into();
        input.colour = "yellow".into();
        assert_eq!(
            CheckStatus::create(input.clone(), now()).unwrap_err(),
            MorningChecksError::invalid("colour", "must be #rrggbb")
        );
        input.colour = " #FFC107 ".into();
        assert_eq!(CheckStatus::create(input, now()).unwrap().colour, "#FFC107");
    }

    #[test]
    fn notes_are_required_when_the_status_requires_them() {
        let t = TenantId::new();
        let c = check(t, None, None, true);
        let amber = status("Amber", true, true);
        let err = CheckResult::record(
            &c,
            None,
            day("2026-11-09"),
            &amber,
            "   ",
            analyst(),
            day("2026-11-09"),
            now(),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            MorningChecksError::Invalid { field: "notes", .. }
        ));
        let r = CheckResult::record(
            &c,
            None,
            day("2026-11-09"),
            &amber,
            " Backup of SQL01 ran late ",
            analyst(),
            day("2026-11-09"),
            now(),
        )
        .unwrap();
        assert_eq!(r.notes(), "Backup of SQL01 ran late");
    }

    #[test]
    fn notes_are_optional_when_the_status_does_not_require_them() {
        let c = check(TenantId::new(), None, None, true);
        let green = status("Green", false, true);
        let r = CheckResult::record(
            &c,
            None,
            day("2026-11-09"),
            &green,
            "",
            analyst(),
            day("2026-11-09"),
            now(),
        )
        .unwrap();
        assert_eq!(r.status_id(), green.id);
        assert_eq!(r.recorded_by(), r.modified_by());
    }

    #[test]
    fn results_cannot_be_recorded_for_a_future_check_date() {
        let c = check(TenantId::new(), None, None, true);
        let err = CheckResult::record(
            &c,
            None,
            day("2026-11-10"),
            &status("Green", false, true),
            "",
            analyst(),
            day("2026-11-09"),
            now(),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            MorningChecksError::Invalid {
                field: "check_date",
                ..
            }
        ));
    }

    #[test]
    fn inactive_statuses_are_not_selectable() {
        let c = check(TenantId::new(), None, None, true);
        let err = CheckResult::record(
            &c,
            None,
            day("2026-11-09"),
            &status("Blue", false, false),
            "",
            analyst(),
            day("2026-11-09"),
            now(),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            MorningChecksError::Invalid {
                field: "status_id",
                ..
            }
        ));
    }

    #[test]
    fn checks_outside_the_round_are_not_recorded() {
        let t = TenantId::new();
        let off = check(t, None, None, false);
        let green = status("Green", false, true);
        let today = day("2026-11-09");
        assert!(matches!(
            CheckResult::record(&off, None, today, &green, "", analyst(), today, now()),
            Err(MorningChecksError::InvalidState(_))
        ));
        let mut g = group(t, None, true);
        let c = check(t, Some(&g), None, true);
        let details = GroupDetails {
            is_active: false,
            ..g.details().clone()
        };
        g.edit(details, now()).unwrap();
        assert!(
            !c.in_round(Some(&g)),
            "an inactive group takes its Checks out"
        );
        assert!(
            CheckResult::record(&c, Some(&g), today, &green, "", analyst(), today, now()).is_err()
        );
    }

    #[test]
    fn revising_a_result_credits_the_modifier_and_keeps_the_recorder() {
        let c = check(TenantId::new(), None, None, true);
        let (green, red) = (status("Green", false, true), status("Red", true, true));
        let today = day("2026-11-09");
        let (ann, bob) = (analyst(), analyst());
        let mut r = CheckResult::record(&c, None, today, &green, "", ann, today, now()).unwrap();
        let later = at("2026-11-09T11:00:00Z");
        assert!(
            r.revise(&c, None, &red, "", bob, today, later).is_err(),
            "Red needs notes"
        );
        assert!(
            r.revise(&c, None, &red, "Disk full", bob, today, later)
                .unwrap()
        );
        assert_eq!(r.recorded_by(), ann);
        assert_eq!(r.modified_by(), bob);
        assert_eq!(r.updated_at(), later);
        assert_eq!(r.recorded_at(), now());
        assert!(
            !r.revise(&c, None, &red, " Disk full ", ann, today, later)
                .unwrap(),
            "unchanged"
        );
        assert_eq!(r.modified_by(), bob, "a no-op is not a modification");
    }

    #[test]
    fn a_result_belongs_to_one_check() {
        let t = TenantId::new();
        let (c, other) = (check(t, None, None, true), check(t, None, None, true));
        let green = status("Green", false, true);
        let today = day("2026-11-09");
        let mut r =
            CheckResult::record(&c, None, today, &green, "", analyst(), today, now()).unwrap();
        assert!(
            r.revise(&other, None, &green, "x", analyst(), today, now())
                .is_err()
        );
    }

    #[test]
    fn a_check_group_must_be_of_the_checks_company() {
        let g = group(TenantId::new(), None, true);
        let details = CheckDetails {
            name: "Disk space".into(),
            group_id: Some(g.id()),
            is_active: true,
            ..CheckDetails::default()
        };
        assert!(Check::create(TenantId::new(), details.clone(), Some(&g), now()).is_err());
        assert!(
            Check::create(g.tenant_id(), details.clone(), None, now()).is_err(),
            "the named group must be given"
        );
        assert!(Check::create(g.tenant_id(), details, Some(&g), now()).is_ok());
    }

    #[test]
    fn checks_are_not_added_to_an_inactive_group_but_may_stay_in_one() {
        let t = TenantId::new();
        let mut g = group(t, None, true);
        let mut c = check(t, Some(&g), None, true);
        let off = GroupDetails {
            is_active: false,
            ..g.details().clone()
        };
        g.edit(off, now()).unwrap();
        let rename = CheckDetails {
            name: "Backups verified".into(),
            ..c.details().clone()
        };
        assert!(c.edit(rename, Some(&g), now()).is_ok(), "already in it");
        let details = CheckDetails {
            name: "New".into(),
            group_id: Some(g.id()),
            is_active: true,
            ..CheckDetails::default()
        };
        assert!(Check::create(t, details, Some(&g), now()).is_err());
    }

    #[test]
    fn routing_prefers_the_checks_analyst_then_the_groups_analyst_then_its_team() {
        let t = TenantId::new();
        let (ann, bob, team) = (AnalystId::new(), AnalystId::new(), TeamId::new());
        let by_team = group(t, Some(CheckAssignee::Team(team)), true);
        let by_bob = group(t, Some(CheckAssignee::Analyst(bob)), true);
        assert_eq!(
            routing(&check(t, Some(&by_team), Some(ann), true), Some(&by_team)),
            Routing::Analyst(ann, RoutingSource::Check)
        );
        assert_eq!(
            routing(&check(t, Some(&by_bob), None, true), Some(&by_bob)),
            Routing::Analyst(bob, RoutingSource::Group)
        );
        assert_eq!(
            routing(&check(t, Some(&by_team), None, true), Some(&by_team)),
            Routing::Team(team)
        );
        assert_eq!(routing(&check(t, None, None, true), None), Routing::Nobody);
        assert!(Routing::Team(team).concerns(ann, &BTreeSet::from([team])));
        assert!(!Routing::Nobody.concerns(ann, &BTreeSet::from([team])));
    }

    #[test]
    fn check_date_is_the_calendar_day_in_the_configured_time_zone() {
        let late = at("2026-11-09T23:30:00Z");
        assert_eq!(check_date_today(late, chrono_tz::UTC), day("2026-11-09"));
        assert_eq!(
            check_date_today(late, parse_time_zone("Europe/Berlin").unwrap()),
            day("2026-11-10")
        );
        assert_eq!(
            check_date_today(
                at("2026-11-09T03:00:00Z"),
                parse_time_zone("America/New_York").unwrap()
            ),
            day("2026-11-08")
        );
        assert!(parse_time_zone("Mars/Olympus").is_none());
    }

    #[test]
    fn date_ranges_are_ordered_and_bounded() {
        assert!(DateRange::new(day("2026-11-09"), day("2026-11-08"), 92).is_err());
        assert!(DateRange::new(day("2026-01-01"), day("2026-04-03"), 92).is_err());
        assert_eq!(
            DateRange::new(day("2026-01-01"), day("2026-04-02"), 92)
                .unwrap()
                .days()
                .len(),
            92
        );
        let r = DateRange::new(day("2026-01-01"), day("2026-04-01"), 92).unwrap();
        assert_eq!(r.days().len(), 91);
        let r = DateRange::ending(day("2026-11-09"), 30, 92).unwrap();
        assert_eq!(r.from(), day("2026-10-11"));
        assert_eq!(r.days().len(), 30);
        assert!(DateRange::ending(day("2026-11-09"), 0, 92).is_err());
    }

    #[test]
    fn reminder_times_are_hours_and_minutes() {
        assert_eq!(
            parse_reminder_time("09:30"),
            NaiveTime::from_hms_opt(9, 30, 0)
        );
        for bad in ["9:30", "24:00", "09:60", "", "0930", "09:30:00"] {
            assert_eq!(parse_reminder_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn reminders_are_due_after_the_time_on_weekdays_unless_weekends_are_on() {
        let policy = ReminderPolicy {
            enabled: true,
            at: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
            weekends: false,
        };
        let local = |s: &str| s.parse::<NaiveDateTime>().unwrap();
        assert_eq!(
            policy.due_date(local("2026-11-09T09:59:59")),
            None,
            "Monday, too early"
        );
        assert_eq!(
            policy.due_date(local("2026-11-09T10:00:00")),
            Some(day("2026-11-09"))
        );
        assert_eq!(
            policy.due_date(local("2026-11-08T11:00:00")),
            None,
            "Sunday"
        );
        let weekends = ReminderPolicy {
            weekends: true,
            ..policy
        };
        assert_eq!(
            weekends.due_date(local("2026-11-08T11:00:00")),
            Some(day("2026-11-08"))
        );
        let off = ReminderPolicy {
            enabled: false,
            ..policy
        };
        assert_eq!(off.due_date(local("2026-11-09T11:00:00")), None);
    }

    #[test]
    fn unrecorded_checks_remind_their_analyst_or_every_team_member_once_per_group() {
        let t = TenantId::new();
        let (ann, bob, cat, team) = (
            AnalystId::new(),
            AnalystId::new(),
            AnalystId::new(),
            TeamId::new(),
        );
        let infra = group(t, Some(CheckAssignee::Team(team)), true);
        let mut off = group(t, Some(CheckAssignee::Analyst(cat)), true);
        let c1 = check(t, Some(&infra), None, true); // team
        let c2 = check(t, Some(&infra), None, true); // team, recorded
        let c3 = check(t, Some(&infra), Some(ann), true); // Ann
        let c4 = check(t, None, Some(ann), true); // Ann, ungrouped
        let c5 = check(t, None, None, true); // nobody
        let c6 = check(t, Some(&off), None, true); // group deactivated below
        let deactivated = GroupDetails {
            is_active: false,
            ..off.details().clone()
        };
        off.edit(deactivated, now()).unwrap();
        let c7 = check(t, None, Some(cat), false); // inactive Check
        let round = [
            (&c1, Some(&infra)),
            (&c2, Some(&infra)),
            (&c3, Some(&infra)),
            (&c4, None),
            (&c5, None),
            (&c6, Some(&off)),
            (&c7, None),
        ];
        let members = BTreeMap::from([(team, BTreeSet::from([ann, bob]))]);
        let out = unrecorded_by_recipient(round, &BTreeSet::from([c2.id()]), &members);
        assert_eq!(
            out,
            BTreeMap::from([
                ((Some(infra.id()), ann), 2),
                ((Some(infra.id()), bob), 1),
                ((None, ann), 1),
            ])
        );
    }

    proptest! {
        #[test]
        fn notes_never_exceed_the_limit_and_are_trimmed(notes in ".{0,200}") {
            let c = check(TenantId::new(), None, None, true);
            let s = status("Amber", true, true);
            let today = day("2026-11-09");
            match CheckResult::record(&c, None, today, &s, &notes, analyst(), today, now()) {
                Ok(r) => {
                    prop_assert!(!r.notes().is_empty());
                    prop_assert_eq!(r.notes(), notes.trim());
                }
                Err(_) => prop_assert!(notes.trim().is_empty()),
            }
        }

        #[test]
        fn check_date_matches_the_local_calendar_day(secs in 0_i64..4_000_000_000) {
            let now = DateTime::<Utc>::from_timestamp(secs, 0).unwrap();
            for tz in [chrono_tz::UTC, chrono_tz::Pacific::Kiritimati, chrono_tz::Pacific::Pago_Pago] {
                let d = check_date_today(now, tz);
                let offset = (now.with_timezone(&tz).naive_local() - now.naive_utc()).num_hours();
                prop_assert!((-14..=14).contains(&offset));
                prop_assert!((d - now.date_naive()).num_days().abs() <= 1);
            }
        }
    }
}
