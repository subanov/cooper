//! Reporting domain (ADR-0032): report periods and calendar buckets in the instance time
//! zone, the dimensions reports group by, the rates they show, and the Watchtower Items
//! that decide what Watchtower tiles count. Pure Rust: no I/O.

use std::collections::BTreeSet;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use thiserror::Error;
use uuid::Uuid;

/// A business rule of a report was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReportingError {
    /// Invalid input.
    #[error("{field}: {message}")]
    Invalid {
        /// The offending field.
        field: &'static str,
        /// Why.
        message: String,
    },
}

impl ReportingError {
    /// An invalid field.
    #[must_use]
    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            message: message.into(),
        }
    }
}

// ----- Time zone and periods ----------------------------------------------------------------

/// Parses an IANA time zone name (`Europe/London`).
#[must_use]
pub fn parse_time_zone(name: &str) -> Option<Tz> {
    name.trim().parse().ok()
}

/// Today in `tz`: the calendar day of `now` there, never the server's local day.
#[must_use]
pub fn today_in(now: DateTime<Utc>, tz: Tz) -> NaiveDate {
    now.with_timezone(&tz).date_naive()
}

/// The instant a calendar day starts in `tz`: local midnight, or, where a
/// daylight-saving change skips midnight, the first local time that exists that day.
/// When midnight happens twice, the earlier one.
#[must_use]
pub fn start_of_day(date: NaiveDate, tz: Tz) -> DateTime<Utc> {
    let midnight = date.and_hms_opt(0, 0, 0).unwrap_or_default();
    // Gaps last at most a few hours; step forward a quarter of an hour at a time.
    for step in 0..=(4 * 24) {
        let local = midnight + Duration::minutes(15 * step);
        match tz.from_local_datetime(&local) {
            LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => {
                return t.with_timezone(&Utc);
            }
            LocalResult::None => {}
        }
    }
    midnight.and_utc()
}

/// The calendar bucket of a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interval {
    /// Calendar days.
    #[default]
    Day,
    /// Calendar months, keyed by their first day.
    Month,
}

impl Interval {
    /// The API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
        }
    }

    /// The bucket a date falls in.
    #[must_use]
    pub fn bucket_of(self, date: NaiveDate) -> NaiveDate {
        match self {
            Self::Day => date,
            Self::Month => date.with_day(1).unwrap_or(date),
        }
    }

    /// The bucket after `bucket`.
    #[must_use]
    pub fn next(self, bucket: NaiveDate) -> Option<NaiveDate> {
        match self {
            Self::Day => bucket.succ_opt(),
            Self::Month => bucket.checked_add_months(chrono::Months::new(1)),
        }
    }
}

impl FromStr for Interval {
    type Err = ReportingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "day" => Ok(Self::Day),
            "month" => Ok(Self::Month),
            _ => Err(ReportingError::invalid("interval", "must be day or month")),
        }
    }
}

/// The longest period of a report by day.
pub const MAX_DAYS_BY_DAY: i64 = 366;
/// The longest period of any other report (about five years).
pub const MAX_DAYS: i64 = 1_830;

/// An inclusive range of calendar days in the instance time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    from: NaiveDate,
    to: NaiveDate,
}

impl Period {
    /// A period of `from..=to`, at most `max_days` long.
    ///
    /// # Errors
    ///
    /// `from` after `to`, or too long.
    pub fn new(from: NaiveDate, to: NaiveDate, max_days: i64) -> Result<Self, ReportingError> {
        if from > to {
            return Err(ReportingError::invalid("from", "must not be after to"));
        }
        if (to - from).num_days() + 1 > max_days {
            return Err(ReportingError::invalid(
                "to",
                format!("the period can be at most {max_days} days"),
            ));
        }
        Ok(Self { from, to })
    }

    /// The period a report asked for. Without `to`, it ends today; without `from`, it
    /// covers the 30 days to `to` (by day) or the 12 calendar months to `to` (by month).
    /// By day it may be at most [`MAX_DAYS_BY_DAY`] long, otherwise [`MAX_DAYS`].
    ///
    /// # Errors
    ///
    /// See [`Period::new`].
    pub fn resolve(
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        today: NaiveDate,
        interval: Interval,
    ) -> Result<Self, ReportingError> {
        let to = to.unwrap_or(today);
        let from = from.unwrap_or_else(|| match interval {
            Interval::Day => to - Duration::days(29),
            Interval::Month => Interval::Month
                .bucket_of(to)
                .checked_sub_months(chrono::Months::new(11))
                .unwrap_or(to),
        });
        let max = match interval {
            Interval::Day => MAX_DAYS_BY_DAY,
            Interval::Month => MAX_DAYS,
        };
        Self::new(from, to, max)
    }

    /// The period of a report that is not split into calendar buckets. Without `to`, it
    /// ends today; without `from`, it covers the 30 days to `to`. At most [`MAX_DAYS`].
    ///
    /// # Errors
    ///
    /// See [`Period::new`].
    pub fn resolve_span(
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        today: NaiveDate,
    ) -> Result<Self, ReportingError> {
        let to = to.unwrap_or(today);
        let from = from.unwrap_or(to - Duration::days(29));
        Self::new(from, to, MAX_DAYS)
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

    /// The half-open range of instants `[start of from, start of the day after to)` in
    /// `tz`, for filtering `timestamptz` columns.
    #[must_use]
    pub fn instants(&self, tz: Tz) -> (DateTime<Utc>, DateTime<Utc>) {
        let end = self.to.succ_opt().unwrap_or(self.to);
        (start_of_day(self.from, tz), start_of_day(end, tz))
    }

    /// Every bucket of `interval` that overlaps the period, in order.
    #[must_use]
    pub fn buckets(&self, interval: Interval) -> Vec<NaiveDate> {
        let mut out = Vec::new();
        let mut b = Some(interval.bucket_of(self.from));
        while let Some(bucket) = b {
            if bucket > self.to {
                break;
            }
            out.push(bucket);
            b = interval.next(bucket);
        }
        out
    }
}

// ----- Dimensions -------------------------------------------------------------------------

/// What ticket counts are grouped by (FR-RPT-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketDimension {
    /// Ticket Status.
    Status,
    /// Priority.
    Priority,
    /// Department.
    Department,
    /// Ticket Type.
    Type,
    /// Origin.
    Origin,
    /// Queue.
    Team,
    /// Assignee.
    Analyst,
    /// Company.
    Company,
}

impl TicketDimension {
    /// The API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Priority => "priority",
            Self::Department => "department",
            Self::Type => "type",
            Self::Origin => "origin",
            Self::Team => "team",
            Self::Analyst => "analyst",
            Self::Company => "company",
        }
    }

    /// Where the names of its values come from.
    #[must_use]
    pub const fn label_kind(self) -> LabelKind {
        match self {
            Self::Status => LabelKind::TicketStatus,
            Self::Priority => LabelKind::TicketPriority,
            Self::Department => LabelKind::Department,
            Self::Type => LabelKind::TicketType,
            Self::Origin => LabelKind::TicketOrigin,
            Self::Team => LabelKind::Team,
            Self::Analyst => LabelKind::Analyst,
            Self::Company => LabelKind::Company,
        }
    }
}

impl FromStr for TicketDimension {
    type Err = ReportingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "status" => Self::Status,
            "priority" => Self::Priority,
            "department" => Self::Department,
            "type" => Self::Type,
            "origin" => Self::Origin,
            "team" => Self::Team,
            "analyst" => Self::Analyst,
            "company" => Self::Company,
            _ => {
                return Err(ReportingError::invalid(
                    "by",
                    "must be status, priority, department, type, origin, team, analyst or company",
                ));
            }
        })
    }
}

/// What the SLA compliance report is grouped by (FR-RPT-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlaDimension {
    /// Priority.
    Priority,
    /// Queue.
    Team,
    /// Company.
    Company,
    /// Day or month of creation.
    Period,
}

impl FromStr for SlaDimension {
    type Err = ReportingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "priority" => Self::Priority,
            "team" => Self::Team,
            "company" => Self::Company,
            "period" => Self::Period,
            _ => {
                return Err(ReportingError::invalid(
                    "by",
                    "must be priority, team, company or period",
                ));
            }
        })
    }
}

/// What the time-entry report is grouped by (FR-RPT-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeDimension {
    /// The Analyst who spent the time.
    Analyst,
    /// The ticket's Queue or the Task's Team.
    Team,
    /// Company.
    Company,
    /// Day or month of the work date.
    Period,
}

impl FromStr for TimeDimension {
    type Err = ReportingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "analyst" => Self::Analyst,
            "team" => Self::Team,
            "company" => Self::Company,
            "period" => Self::Period,
            _ => {
                return Err(ReportingError::invalid(
                    "by",
                    "must be analyst, team, company or period",
                ));
            }
        })
    }
}

/// Where the names of group values come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LabelKind {
    /// Ticket Statuses.
    TicketStatus,
    /// Ticket Priorities.
    TicketPriority,
    /// Ticket Types.
    TicketType,
    /// Origins.
    TicketOrigin,
    /// Departments.
    Department,
    /// Teams.
    Team,
    /// Analysts.
    Analyst,
    /// Companies.
    Company,
    /// Task Statuses.
    TaskStatus,
    /// Check Statuses.
    CheckStatus,
}

impl LabelKind {
    /// Whether the values are reference data with an admin-defined order.
    #[must_use]
    pub const fn is_lookup(self) -> bool {
        matches!(
            self,
            Self::TicketStatus
                | Self::TicketPriority
                | Self::TicketType
                | Self::TicketOrigin
                | Self::TaskStatus
                | Self::CheckStatus
        )
    }
}

/// The value a report row stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    /// No value (no Department, no Queue, unassigned, …).
    None,
    /// A reference.
    Id(Uuid),
    /// A day, or the first day of a month.
    Date(NaiveDate),
}

// ----- Rates ------------------------------------------------------------------------------

/// `part` as a percentage of `whole`, rounded to one decimal; `None` when `whole` is zero
/// (no tickets: there is no rate, rather than a rate of 0 % or 100 %).
#[must_use]
#[allow(clippy::cast_precision_loss)] // counts far below 2^52
pub fn percentage(part: u64, whole: u64) -> Option<f64> {
    if whole == 0 {
        return None;
    }
    let p = (part as f64) * 100.0 / (whole as f64);
    Some((p * 10.0).round() / 10.0)
}

/// The First Time Fix rate: the share of tickets closed in a period that were flagged
/// First Time Fix. Open tickets are not counted: they have not been fixed yet.
#[must_use]
pub fn first_time_fix_rate(closed: u64, first_time_fix: u64) -> Option<f64> {
    percentage(first_time_fix.min(closed), closed)
}

/// Where tickets stand against one SLA target: the states the service desk shows on
/// each ticket (FR-SLA-007), counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TargetResult {
    /// Satisfied in time.
    pub met: u64,
    /// Satisfied, but after the due time (breached).
    pub met_late: u64,
    /// Not satisfied and past due (breached).
    pub breached: u64,
    /// Not satisfied and not past due yet, or paused.
    pub running: u64,
    /// No target.
    pub no_target: u64,
}

impl TargetResult {
    /// Adds another group's counts.
    pub const fn add(&mut self, other: &Self) {
        self.met += other.met;
        self.met_late += other.met_late;
        self.breached += other.breached;
        self.running += other.running;
        self.no_target += other.no_target;
    }

    /// Breached in total: met late, or not met and past due.
    #[must_use]
    pub const fn breached_total(&self) -> u64 {
        self.met_late + self.breached
    }

    /// Tickets whose outcome is known: met or breached.
    #[must_use]
    pub const fn decided(&self) -> u64 {
        self.met + self.breached_total()
    }

    /// Compliance: met as a percentage of met and breached. Running tickets and tickets
    /// without a target do not count. `None` when nothing is decided.
    #[must_use]
    pub fn compliance(&self) -> Option<f64> {
        percentage(self.met, self.decided())
    }
}

// ----- Watchtower -------------------------------------------------------------------------

/// A kind of Watchtower Item: which values a Watchtower count includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WatchtowerItemKind {
    /// Open Ticket Statuses that get their own count.
    TicketStatuses,
    /// Priorities counted as high priority.
    TicketPriorities,
    /// Open Task Statuses that get their own count.
    TaskStatuses,
    /// Check Statuses that need attention.
    CheckStatuses,
}

impl WatchtowerItemKind {
    /// Every kind.
    pub const ALL: [Self; 4] = [
        Self::TicketStatuses,
        Self::TicketPriorities,
        Self::TaskStatuses,
        Self::CheckStatuses,
    ];

    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TicketStatuses => "ticket_statuses",
            Self::TicketPriorities => "ticket_priorities",
            Self::TaskStatuses => "task_statuses",
            Self::CheckStatuses => "check_statuses",
        }
    }
}

impl FromStr for WatchtowerItemKind {
    type Err = ReportingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| ReportingError::invalid("item", "is not a Watchtower Item"))
    }
}

/// The Watchtower Items: per kind, the values chosen. An empty selection means the
/// default rule of that kind (see [`select_open`], [`select_high_priorities`] and
/// [`select_attention`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchtowerItems {
    /// Open Ticket Statuses counted one by one.
    pub ticket_statuses: BTreeSet<Uuid>,
    /// Priorities counted as high priority.
    pub ticket_priorities: BTreeSet<Uuid>,
    /// Open Task Statuses counted one by one.
    pub task_statuses: BTreeSet<Uuid>,
    /// Check Statuses that need attention.
    pub check_statuses: BTreeSet<Uuid>,
}

impl WatchtowerItems {
    /// The selection of one kind.
    #[must_use]
    pub const fn of(&self, kind: WatchtowerItemKind) -> &BTreeSet<Uuid> {
        match kind {
            WatchtowerItemKind::TicketStatuses => &self.ticket_statuses,
            WatchtowerItemKind::TicketPriorities => &self.ticket_priorities,
            WatchtowerItemKind::TaskStatuses => &self.task_statuses,
            WatchtowerItemKind::CheckStatuses => &self.check_statuses,
        }
    }

    /// The selection of one kind, to change.
    pub const fn of_mut(&mut self, kind: WatchtowerItemKind) -> &mut BTreeSet<Uuid> {
        match kind {
            WatchtowerItemKind::TicketStatuses => &mut self.ticket_statuses,
            WatchtowerItemKind::TicketPriorities => &mut self.ticket_priorities,
            WatchtowerItemKind::TaskStatuses => &mut self.task_statuses,
            WatchtowerItemKind::CheckStatuses => &mut self.check_statuses,
        }
    }
}

/// A reference-data row (a status or Priority of another context), as Reporting sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupInfo {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Admin-defined order.
    pub sort_order: i32,
    /// Still offered.
    pub is_active: bool,
    /// A closed status.
    pub is_closed: bool,
    /// The default row.
    pub is_default: bool,
    /// `#rrggbb`, where the context has colours.
    pub colour: Option<String>,
}

fn in_order<'a>(rows: impl Iterator<Item = &'a LookupInfo>) -> Vec<&'a LookupInfo> {
    let mut v: Vec<&LookupInfo> = rows.collect();
    v.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
    v
}

/// The open statuses a Watchtower tile counts one by one: the chosen ones, or every
/// active open status when none is chosen. Closed statuses never count (an open-work
/// tile), and the tile's total is the sum of exactly these rows.
#[must_use]
pub fn select_open<'a>(statuses: &'a [LookupInfo], chosen: &BTreeSet<Uuid>) -> Vec<&'a LookupInfo> {
    in_order(statuses.iter().filter(|s| {
        !s.is_closed
            && if chosen.is_empty() {
                s.is_active
            } else {
                chosen.contains(&s.id)
            }
    }))
}

/// The Priorities counted as high priority: the chosen ones, or every active Priority
/// ranked after the default one when none is chosen (so a Priority added above the
/// default is counted without configuration).
#[must_use]
pub fn select_high_priorities<'a>(
    priorities: &'a [LookupInfo],
    chosen: &BTreeSet<Uuid>,
) -> Vec<&'a LookupInfo> {
    if !chosen.is_empty() {
        return in_order(priorities.iter().filter(|p| chosen.contains(&p.id)));
    }
    let Some(default) = priorities
        .iter()
        .find(|p| p.is_default)
        .or_else(|| in_order(priorities.iter()).into_iter().next())
    else {
        return Vec::new();
    };
    in_order(
        priorities
            .iter()
            .filter(|p| p.is_active && p.sort_order > default.sort_order),
    )
}

/// The Check Statuses that need attention: the chosen ones, or every status except the
/// first active one in order (the most favourable) when none is chosen.
#[must_use]
pub fn select_attention<'a>(
    statuses: &'a [LookupInfo],
    chosen: &BTreeSet<Uuid>,
) -> Vec<&'a LookupInfo> {
    if !chosen.is_empty() {
        return in_order(statuses.iter().filter(|s| chosen.contains(&s.id)));
    }
    let best = in_order(statuses.iter().filter(|s| s.is_active))
        .first()
        .map(|s| s.id);
    in_order(statuses.iter().filter(|s| Some(s.id) != best))
}

/// Checks a selection of Watchtower Items against the reference data it names: every
/// value must exist, and status selections must name open statuses.
///
/// # Errors
///
/// An unknown value, or a closed status.
pub fn validate_selection(
    kind: WatchtowerItemKind,
    chosen: &BTreeSet<Uuid>,
    rows: &[LookupInfo],
) -> Result<(), ReportingError> {
    for id in chosen {
        let Some(row) = rows.iter().find(|r| r.id == *id) else {
            return Err(ReportingError::invalid(
                kind_field(kind),
                format!("{id} does not exist"),
            ));
        };
        let needs_open = matches!(
            kind,
            WatchtowerItemKind::TicketStatuses | WatchtowerItemKind::TaskStatuses
        );
        if needs_open && row.is_closed {
            return Err(ReportingError::invalid(
                kind_field(kind),
                format!("{} is a closed status", row.name),
            ));
        }
    }
    Ok(())
}

const fn kind_field(kind: WatchtowerItemKind) -> &'static str {
    kind.as_str()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use chrono_tz::{America, Europe};
    use proptest::prelude::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn days_start_at_local_midnight() {
        // Winter in London is UTC, summer is UTC+1.
        assert_eq!(
            start_of_day(d(2026, 1, 15), Europe::London),
            utc("2026-01-15T00:00:00Z")
        );
        assert_eq!(
            start_of_day(d(2026, 7, 15), Europe::London),
            utc("2026-07-14T23:00:00Z")
        );
        assert_eq!(
            start_of_day(d(2026, 7, 15), chrono_tz::UTC),
            utc("2026-07-15T00:00:00Z")
        );
    }

    #[test]
    fn a_day_with_a_daylight_saving_change_has_23_or_25_hours() {
        let tz = Europe::London;
        // 2026-03-29: clocks go forward at 01:00 GMT.
        let spring = Period::new(d(2026, 3, 29), d(2026, 3, 29), 1).unwrap();
        let (a, b) = spring.instants(tz);
        assert_eq!((b - a).num_hours(), 23);
        // 2026-10-25: clocks go back at 02:00 BST.
        let autumn = Period::new(d(2026, 10, 25), d(2026, 10, 25), 1).unwrap();
        let (a, b) = autumn.instants(tz);
        assert_eq!((b - a).num_hours(), 25);
    }

    #[test]
    fn a_skipped_midnight_starts_the_day_at_the_first_local_time() {
        // Santiago moved clocks forward at 00:00 on 2026-09-06 (local midnight did not
        // exist); the day starts at 01:00 local, 04:00 UTC.
        let start = start_of_day(d(2026, 9, 6), America::Santiago);
        assert_eq!(start, utc("2026-09-06T04:00:00Z"));
        assert_eq!(
            start.with_timezone(&America::Santiago).date_naive(),
            d(2026, 9, 6)
        );
    }

    #[test]
    fn today_is_the_day_in_the_instance_time_zone() {
        // 23:30 UTC on 30 June is already 1 July in London (BST).
        assert_eq!(
            today_in(utc("2026-06-30T23:30:00Z"), Europe::London),
            d(2026, 7, 1)
        );
        assert_eq!(
            today_in(utc("2026-06-30T23:30:00Z"), chrono_tz::UTC),
            d(2026, 6, 30)
        );
    }

    #[test]
    fn month_buckets_cover_partial_months() {
        let p = Period::new(d(2026, 1, 31), d(2026, 3, 1), 100).unwrap();
        assert_eq!(
            p.buckets(Interval::Month),
            vec![d(2026, 1, 1), d(2026, 2, 1), d(2026, 3, 1)]
        );
        let p = Period::new(d(2026, 2, 27), d(2026, 3, 2), 100).unwrap();
        assert_eq!(
            p.buckets(Interval::Day),
            vec![d(2026, 2, 27), d(2026, 2, 28), d(2026, 3, 1), d(2026, 3, 2)]
        );
        assert_eq!(Interval::Month.bucket_of(d(2024, 2, 29)), d(2024, 2, 1));
        assert_eq!(Interval::Month.next(d(2026, 12, 1)), Some(d(2027, 1, 1)));
    }

    #[test]
    fn default_periods_end_today() {
        let today = d(2026, 11, 9);
        let p = Period::resolve(None, None, today, Interval::Day).unwrap();
        assert_eq!((p.from(), p.to()), (d(2026, 10, 11), today));
        assert_eq!(p.buckets(Interval::Day).len(), 30);
        let p = Period::resolve(None, None, today, Interval::Month).unwrap();
        assert_eq!((p.from(), p.to()), (d(2025, 12, 1), today));
        assert_eq!(p.buckets(Interval::Month).len(), 12);
    }

    #[test]
    fn periods_are_bounded_and_ordered() {
        assert!(Period::new(d(2026, 2, 1), d(2026, 1, 1), 10).is_err());
        assert!(Period::new(d(2026, 1, 1), d(2026, 1, 10), 10).is_ok());
        assert!(Period::new(d(2026, 1, 1), d(2026, 1, 11), 10).is_err());
        let today = d(2026, 11, 9);
        assert!(Period::resolve(Some(d(2025, 1, 1)), None, today, Interval::Day).is_err());
        assert!(Period::resolve(Some(d(2025, 1, 1)), None, today, Interval::Month).is_ok());
    }

    #[test]
    fn rates_have_no_value_without_a_denominator() {
        assert_eq!(percentage(0, 0), None);
        assert_eq!(percentage(1, 3), Some(33.3));
        assert_eq!(percentage(2, 3), Some(66.7));
        assert_eq!(percentage(3, 3), Some(100.0));
        assert_eq!(first_time_fix_rate(0, 0), None);
        assert_eq!(first_time_fix_rate(4, 1), Some(25.0));
    }

    #[test]
    fn compliance_counts_only_decided_tickets() {
        let r = TargetResult {
            met: 3,
            met_late: 1,
            breached: 1,
            running: 10,
            no_target: 7,
        };
        assert_eq!(r.breached_total(), 2);
        assert_eq!(r.decided(), 5);
        assert_eq!(r.compliance(), Some(60.0));
        let nothing_decided = TargetResult {
            running: 4,
            no_target: 2,
            ..TargetResult::default()
        };
        assert_eq!(nothing_decided.compliance(), None);
        let mut total = TargetResult::default();
        total.add(&r);
        total.add(&nothing_decided);
        assert_eq!(total.running, 14);
        assert_eq!(total.no_target, 9);
    }

    fn lookup(name: &str, sort: i32, closed: bool, default: bool) -> LookupInfo {
        LookupInfo {
            id: Uuid::now_v7(),
            name: name.into(),
            sort_order: sort,
            is_active: true,
            is_closed: closed,
            is_default: default,
            colour: None,
        }
    }

    #[test]
    fn open_status_tiles_default_to_every_active_open_status() {
        let mut on_hold = lookup("On Hold", 30, false, false);
        on_hold.is_active = false;
        let rows = vec![
            lookup("In Progress", 20, false, false),
            lookup("Open", 10, false, true),
            on_hold,
            lookup("Closed", 50, true, false),
        ];
        let names = |v: Vec<&LookupInfo>| v.into_iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(select_open(&rows, &BTreeSet::new())),
            ["Open", "In Progress"]
        );
        let chosen = BTreeSet::from([rows[1].id, rows[3].id]);
        assert_eq!(
            names(select_open(&rows, &chosen)),
            ["Open"],
            "closed never counts"
        );
    }

    #[test]
    fn high_priority_defaults_to_priorities_ranked_after_the_default() {
        let rows = vec![
            lookup("Low", 10, false, false),
            lookup("Normal", 20, false, true),
            lookup("High", 30, false, false),
            lookup("Critical", 40, false, false),
        ];
        let names = |v: Vec<&LookupInfo>| v.into_iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(select_high_priorities(&rows, &BTreeSet::new())),
            ["High", "Critical"]
        );
        let chosen = BTreeSet::from([rows[3].id]);
        assert_eq!(names(select_high_priorities(&rows, &chosen)), ["Critical"]);
        assert!(select_high_priorities(&[], &BTreeSet::new()).is_empty());
    }

    #[test]
    fn attention_defaults_to_every_status_but_the_first() {
        let rows = vec![
            lookup("Red", 30, false, false),
            lookup("Green", 10, false, false),
            lookup("Amber", 20, false, false),
        ];
        let names = |v: Vec<&LookupInfo>| v.into_iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(select_attention(&rows, &BTreeSet::new())),
            ["Amber", "Red"]
        );
        let chosen = BTreeSet::from([rows[0].id]);
        assert_eq!(names(select_attention(&rows, &chosen)), ["Red"]);
    }

    #[test]
    fn selections_must_name_existing_open_statuses() {
        let rows = vec![
            lookup("Open", 10, false, true),
            lookup("Closed", 50, true, false),
        ];
        let kind = WatchtowerItemKind::TicketStatuses;
        assert!(validate_selection(kind, &BTreeSet::from([rows[0].id]), &rows).is_ok());
        assert!(validate_selection(kind, &BTreeSet::from([rows[1].id]), &rows).is_err());
        assert!(validate_selection(kind, &BTreeSet::from([Uuid::now_v7()]), &rows).is_err());
        // Priorities and Check Statuses have no closed rows to refuse.
        let p = WatchtowerItemKind::CheckStatuses;
        assert!(validate_selection(p, &BTreeSet::from([rows[1].id]), &rows).is_ok());
    }

    #[test]
    fn names_parse() {
        assert_eq!("month".parse::<Interval>().unwrap(), Interval::Month);
        assert!("week".parse::<Interval>().is_err());
        assert_eq!(
            "origin".parse::<TicketDimension>().unwrap(),
            TicketDimension::Origin
        );
        assert!("requester".parse::<TicketDimension>().is_err());
        assert_eq!(
            "period".parse::<SlaDimension>().unwrap(),
            SlaDimension::Period
        );
        assert_eq!(
            "analyst".parse::<TimeDimension>().unwrap(),
            TimeDimension::Analyst
        );
        for k in WatchtowerItemKind::ALL {
            assert_eq!(k.as_str().parse::<WatchtowerItemKind>().unwrap(), k);
        }
    }

    proptest! {
        /// Every instant of a period's range falls on a day of the period in the time
        /// zone, and the range is contiguous: day ranges of consecutive days touch.
        #[test]
        fn day_ranges_tile_the_timeline(offset in 0i64..(3 * 365), zone in 0usize..4) {
            let tz = [Europe::London, America::Santiago, America::New_York, chrono_tz::Australia::Lord_Howe][zone];
            let day = d(2025, 1, 1) + Duration::days(offset);
            let (a, b) = Period::new(day, day, 1).unwrap().instants(tz);
            let (b2, _) = Period::new(day.succ_opt().unwrap(), day.succ_opt().unwrap(), 1).unwrap().instants(tz);
            prop_assert_eq!(b, b2);
            prop_assert!(a < b);
            prop_assert_eq!(a.with_timezone(&tz).date_naive(), day);
            prop_assert_eq!((b - Duration::seconds(1)).with_timezone(&tz).date_naive(), day);
        }
    }
}
