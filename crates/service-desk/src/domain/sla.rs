//! SLA Calendars and working-time arithmetic (FR-SLA-001 to FR-SLA-003, FR-SLA-007).
//!
//! SLA time counts only inside a calendar's working hours, in the calendar's time zone,
//! and skips Holidays. All arithmetic is pure and works on UTC instants; local times are
//! converted with an explicit `chrono_tz::Tz` (ADR-0002). Daylight-saving changes are
//! handled by converting each day's local window separately.

use std::collections::BTreeSet;

use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
};
use chrono_tz::Tz;
use cooper_kernel::TenantId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Audited, TicketError};

/// How far ahead the arithmetic looks for working time (ten years of days).
const MAX_DAYS: i64 = 3660;

/// Working hours of one weekday, in local time. `start < end`; overnight shifts are not
/// supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayHours {
    /// Start of work.
    pub start: NaiveTime,
    /// End of work.
    pub end: NaiveTime,
}

/// An SLA Calendar: working hours per weekday (Monday first), a time zone and Holidays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlaCalendar {
    id: Uuid,
    name: String,
    time_zone: Tz,
    hours: [Option<DayHours>; 7],
    holidays: BTreeSet<NaiveDate>,
    is_default: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Editable fields of a calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlaCalendarInput {
    /// Name.
    pub name: String,
    /// IANA time zone, for example `Europe/London`.
    pub time_zone: Tz,
    /// Working hours, Monday (index 0) to Sunday (index 6).
    pub hours: [Option<DayHours>; 7],
    /// Holidays (local dates).
    pub holidays: BTreeSet<NaiveDate>,
    /// Used for Priorities without their own calendar.
    pub is_default: bool,
}

fn validate(input: &SlaCalendarInput) -> Result<String, TicketError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(TicketError::invalid("name", "must be 1 to 100 characters"));
    }
    if input.hours.iter().flatten().any(|h| h.start >= h.end) {
        return Err(TicketError::invalid(
            "hours",
            "each day must start before it ends",
        ));
    }
    if input.hours.iter().all(Option::is_none) {
        return Err(TicketError::invalid(
            "hours",
            "at least one weekday needs working hours",
        ));
    }
    Ok(name.to_owned())
}

/// Converts a local wall-clock time to UTC; in a DST gap, the first valid instant after it.
fn to_utc(tz: Tz, local: NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(t) => t.with_timezone(&Utc),
        LocalResult::Ambiguous(earliest, _) => earliest.with_timezone(&Utc),
        LocalResult::None => to_utc(tz, local + Duration::minutes(30)),
    }
}

impl SlaCalendar {
    /// Creates a calendar.
    ///
    /// # Errors
    ///
    /// Invalid name or hours.
    pub fn create(input: SlaCalendarInput, now: DateTime<Utc>) -> Result<Self, TicketError> {
        let name = validate(&input)?;
        Ok(Self {
            id: Uuid::now_v7(),
            name,
            time_zone: input.time_zone,
            hours: input.hours,
            holidays: input.holidays,
            is_default: input.is_default,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a calendar from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: Uuid,
        name: String,
        time_zone: Tz,
        hours: [Option<DayHours>; 7],
        holidays: BTreeSet<NaiveDate>,
        is_default: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            time_zone,
            hours,
            holidays,
            is_default,
            created_at,
            updated_at,
        }
    }

    /// Replaces every editable field.
    ///
    /// # Errors
    ///
    /// Invalid name or hours.
    pub fn update(
        &mut self,
        input: SlaCalendarInput,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        self.name = validate(&input)?;
        self.time_zone = input.time_zone;
        self.hours = input.hours;
        self.holidays = input.holidays;
        self.is_default = input.is_default;
        self.updated_at = now;
        Ok(())
    }

    /// Clears the default flag.
    pub fn clear_default(&mut self, now: DateTime<Utc>) {
        if self.is_default {
            self.is_default = false;
            self.updated_at = now;
        }
    }

    /// The working window of a local date in UTC, if it is a working day.
    fn window(&self, date: NaiveDate) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        if self.holidays.contains(&date) {
            return None;
        }
        let hours = self.hours[date.weekday().num_days_from_monday() as usize]?;
        let start = to_utc(self.time_zone, date.and_time(hours.start));
        let end = to_utc(self.time_zone, date.and_time(hours.end));
        (start < end).then_some((start, end))
    }

    fn local_date(&self, at: DateTime<Utc>) -> NaiveDate {
        at.with_timezone(&self.time_zone).date_naive()
    }

    /// The instant `minutes` working minutes after `from`. Zero minutes gives `from`
    /// itself. Returns `None` only if no working time exists within ten years.
    #[must_use]
    pub fn add_working_minutes(&self, from: DateTime<Utc>, minutes: i64) -> Option<DateTime<Utc>> {
        let mut remaining = Duration::minutes(minutes.max(0));
        if remaining.is_zero() {
            return Some(from);
        }
        let mut cursor = from;
        let mut date = self.local_date(from) - Duration::days(1);
        for _ in 0..MAX_DAYS {
            if let Some((start, end)) = self.window(date) {
                let begin = cursor.max(start);
                if begin < end {
                    let available = end - begin;
                    if remaining <= available {
                        return Some(begin + remaining);
                    }
                    remaining -= available;
                    cursor = end;
                }
            }
            date += Duration::days(1);
        }
        None
    }

    /// Working minutes between two instants (0 when `to <= from`).
    #[must_use]
    pub fn working_minutes_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> i64 {
        if to <= from {
            return 0;
        }
        let mut total = Duration::zero();
        let mut date = self.local_date(from) - Duration::days(1);
        let last = self.local_date(to) + Duration::days(1);
        while date <= last {
            if let Some((start, end)) = self.window(date) {
                let (a, b) = (start.max(from), end.min(to));
                if a < b {
                    total += b - a;
                }
            }
            date += Duration::days(1);
        }
        total.num_minutes()
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Time zone.
    #[must_use]
    pub const fn time_zone(&self) -> Tz {
        self.time_zone
    }
    /// Working hours, Monday first.
    #[must_use]
    pub const fn hours(&self) -> &[Option<DayHours>; 7] {
        &self.hours
    }
    /// Holidays.
    #[must_use]
    pub const fn holidays(&self) -> &BTreeSet<NaiveDate> {
        &self.holidays
    }
    /// Default flag.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.is_default
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
}

impl Audited for SlaCalendar {
    fn audit_type(&self) -> &'static str {
        "sla_calendar"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.name)),
            ("time_zone", json!(self.time_zone.name())),
            ("hours", json!(self.hours)),
            ("holidays", json!(self.holidays)),
            ("is_default", json!(self.is_default)),
        ]
    }
}

/// Working-time arithmetic with an optional calendar: without one, every minute counts
/// (24×7).
#[derive(Debug, Clone, Copy)]
pub struct WorkingTime<'a>(pub Option<&'a SlaCalendar>);

impl WorkingTime<'_> {
    /// `minutes` working minutes after `from`.
    #[must_use]
    pub fn add(&self, from: DateTime<Utc>, minutes: i64) -> Option<DateTime<Utc>> {
        match self.0 {
            Some(c) => c.add_working_minutes(from, minutes),
            None => Some(from + Duration::minutes(minutes.max(0))),
        }
    }

    /// Working minutes between two instants.
    #[must_use]
    pub fn between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> i64 {
        match self.0 {
            Some(c) => c.working_minutes_between(from, to),
            None => (to - from).num_minutes().max(0),
        }
    }
}

/// An SLA target: response or resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlaTarget {
    /// First response.
    Response,
    /// Resolution.
    Resolution,
}

impl SlaTarget {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Response => "response",
            Self::Resolution => "resolution",
        }
    }
}

/// An SLA trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlaTrigger {
    /// The target is about to be missed.
    Warning,
    /// The target was missed.
    Breach,
}

impl SlaTrigger {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Breach => "breach",
        }
    }
}

/// The state of one target, shown in ticket lists (FR-SLA-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetState {
    /// No target configured.
    None,
    /// Satisfied in time.
    Met,
    /// Satisfied late.
    MetLate,
    /// The ticket is in a status that pauses the SLA.
    Paused,
    /// Running, before the warning point.
    OnTrack,
    /// Running, past the warning point.
    Warning,
    /// Past due.
    Breached,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::London;
    use proptest::prelude::*;

    fn nine_to_five_weekdays(holidays: &[NaiveDate]) -> SlaCalendar {
        let day = Some(DayHours {
            start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        });
        SlaCalendar::create(
            SlaCalendarInput {
                name: "Office".into(),
                time_zone: London,
                hours: [day, day, day, day, day, None, None],
                holidays: holidays.iter().copied().collect(),
                is_default: true,
            },
            Utc::now(),
        )
        .unwrap()
    }

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn counts_only_working_hours() {
        let c = nine_to_five_weekdays(&[]);
        // Friday 16:00 London (winter: UTC) + 2h → Monday 10:00.
        assert_eq!(
            c.add_working_minutes(utc("2026-01-09T16:00:00Z"), 120),
            Some(utc("2026-01-12T10:00:00Z"))
        );
        // Saturday start → Monday 09:00 + 30 min.
        assert_eq!(
            c.add_working_minutes(utc("2026-01-10T12:00:00Z"), 30),
            Some(utc("2026-01-12T09:30:00Z"))
        );
        assert_eq!(
            c.working_minutes_between(utc("2026-01-09T16:00:00Z"), utc("2026-01-12T10:00:00Z")),
            120
        );
        assert_eq!(
            c.add_working_minutes(utc("2026-01-10T12:00:00Z"), 0),
            Some(utc("2026-01-10T12:00:00Z"))
        );
    }

    #[test]
    fn skips_holidays() {
        let monday = NaiveDate::from_ymd_opt(2026, 1, 12).unwrap();
        let c = nine_to_five_weekdays(&[monday]);
        assert_eq!(
            c.add_working_minutes(utc("2026-01-09T16:00:00Z"), 120),
            Some(utc("2026-01-13T10:00:00Z"))
        );
    }

    #[test]
    fn follows_daylight_saving_time() {
        let c = nine_to_five_weekdays(&[]);
        // 2026-03-27 is a Friday in GMT; Monday 2026-03-30 is in BST (UTC+1):
        // 09:00 local = 08:00 UTC.
        assert_eq!(
            c.add_working_minutes(utc("2026-03-27T16:00:00Z"), 120),
            Some(utc("2026-03-30T09:00:00Z"))
        );
    }

    #[test]
    fn rejects_invalid_hours() {
        let bad = Some(DayHours {
            start: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        });
        let input = SlaCalendarInput {
            name: "x".into(),
            time_zone: London,
            hours: [bad, None, None, None, None, None, None],
            holidays: BTreeSet::new(),
            is_default: false,
        };
        assert!(SlaCalendar::create(input.clone(), Utc::now()).is_err());
        let none = SlaCalendarInput {
            hours: [None; 7],
            ..input
        };
        assert!(SlaCalendar::create(none, Utc::now()).is_err());
    }

    #[test]
    fn without_a_calendar_every_minute_counts() {
        let w = WorkingTime(None);
        assert_eq!(
            w.add(utc("2026-01-10T12:00:00Z"), 90),
            Some(utc("2026-01-10T13:30:00Z"))
        );
        assert_eq!(
            w.between(utc("2026-01-10T12:00:00Z"), utc("2026-01-10T13:30:00Z")),
            90
        );
    }

    proptest! {
        /// Adding n working minutes and measuring back gives n, whatever the start
        /// (weekends, nights, DST changes).
        #[test]
        fn add_and_measure_round_trip(start_minutes in 0i64..(2 * 365 * 24 * 60), minutes in 0i64..(20 * 8 * 60)) {
            let c = nine_to_five_weekdays(&[NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()]);
            let start = utc("2026-01-01T00:00:00Z") + Duration::minutes(start_minutes);
            let due = c.add_working_minutes(start, minutes).unwrap();
            prop_assert!(due >= start);
            prop_assert_eq!(c.working_minutes_between(start, due), minutes);
        }

        /// More working minutes never give an earlier due time.
        #[test]
        fn monotonic(start_minutes in 0i64..(365 * 24 * 60), a in 0i64..5000, b in 0i64..5000) {
            let c = nine_to_five_weekdays(&[]);
            let start = utc("2026-01-01T00:00:00Z") + Duration::minutes(start_minutes);
            let (lo, hi) = (a.min(b), a.max(b));
            prop_assert!(c.add_working_minutes(start, lo).unwrap() <= c.add_working_minutes(start, hi).unwrap());
        }
    }
}

/// Which targets an SLA Notification Rule covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleTarget {
    /// Response only.
    Response,
    /// Resolution only.
    Resolution,
    /// Both targets.
    Both,
}

impl RuleTarget {
    /// Storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Response => "response",
            Self::Resolution => "resolution",
            Self::Both => "both",
        }
    }

    /// Whether the rule covers `target`.
    #[must_use]
    pub fn covers(self, target: SlaTarget) -> bool {
        matches!(
            (self, target),
            (Self::Both, _)
                | (Self::Response, SlaTarget::Response)
                | (Self::Resolution, SlaTarget::Resolution)
        )
    }
}

/// Editable fields of an SLA Notification Rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlaRuleInput {
    /// Name.
    pub name: String,
    /// Warning or Breach.
    pub trigger: SlaTrigger,
    /// Response, resolution or both.
    pub target: RuleTarget,
    /// Notify the Assignee.
    pub notify_assignee: bool,
    /// Notify the members of the Queue.
    pub notify_team: bool,
    /// Also notify these Analysts.
    pub analyst_ids: BTreeSet<cooper_kernel::AnalystId>,
    /// Active flag.
    pub is_active: bool,
}

/// Who is notified for an SLA Warning or Breach (FR-SLA-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlaNotificationRule {
    id: Uuid,
    input: SlaRuleInput,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn validate_rule(mut input: SlaRuleInput) -> Result<SlaRuleInput, TicketError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(TicketError::invalid("name", "must be 1 to 100 characters"));
    }
    if !input.notify_assignee && !input.notify_team && input.analyst_ids.is_empty() {
        return Err(TicketError::invalid(
            "recipients",
            "a rule must notify someone",
        ));
    }
    input.name = name.to_owned();
    Ok(input)
}

impl SlaNotificationRule {
    /// Creates a rule.
    ///
    /// # Errors
    ///
    /// Invalid name or no recipients.
    pub fn create(input: SlaRuleInput, now: DateTime<Utc>) -> Result<Self, TicketError> {
        Ok(Self {
            id: Uuid::now_v7(),
            input: validate_rule(input)?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a rule from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        input: SlaRuleInput,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            input,
            created_at,
            updated_at,
        }
    }

    /// Replaces the rule.
    ///
    /// # Errors
    ///
    /// Invalid name or no recipients.
    pub fn update(&mut self, input: SlaRuleInput, now: DateTime<Utc>) -> Result<(), TicketError> {
        self.input = validate_rule(input)?;
        self.updated_at = now;
        Ok(())
    }

    /// The Analysts to notify for `trigger` on `target`, given the ticket's Assignee and
    /// Queue members. Empty when the rule does not apply.
    #[must_use]
    pub fn recipients(
        &self,
        trigger: SlaTrigger,
        target: SlaTarget,
        assignee: Option<cooper_kernel::AnalystId>,
        team_members: &BTreeSet<cooper_kernel::AnalystId>,
    ) -> BTreeSet<cooper_kernel::AnalystId> {
        let r = &self.input;
        if !r.is_active || r.trigger != trigger || !r.target.covers(target) {
            return BTreeSet::new();
        }
        let mut to = r.analyst_ids.clone();
        if r.notify_assignee {
            to.extend(assignee);
        }
        if r.notify_team {
            to.extend(team_members.iter().copied());
        }
        to
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// The rule.
    #[must_use]
    pub const fn rule(&self) -> &SlaRuleInput {
        &self.input
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
}

impl Audited for SlaNotificationRule {
    fn audit_type(&self) -> &'static str {
        "sla_notification_rule"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let r = &self.input;
        vec![
            ("name", json!(r.name)),
            ("trigger", json!(r.trigger)),
            ("target", json!(r.target)),
            ("notify_assignee", json!(r.notify_assignee)),
            ("notify_team", json!(r.notify_team)),
            ("analyst_ids", json!(r.analyst_ids)),
            ("is_active", json!(r.is_active)),
        ]
    }
}

#[cfg(test)]
mod rule_tests {
    use super::*;
    use cooper_kernel::AnalystId;

    #[test]
    fn recipients_follow_trigger_target_and_flags() {
        let manager = AnalystId::new();
        let rule = SlaNotificationRule::create(
            SlaRuleInput {
                name: "Breaches".into(),
                trigger: SlaTrigger::Breach,
                target: RuleTarget::Resolution,
                notify_assignee: true,
                notify_team: true,
                analyst_ids: BTreeSet::from([manager]),
                is_active: true,
            },
            Utc::now(),
        )
        .unwrap();
        let (assignee, member) = (AnalystId::new(), AnalystId::new());
        let team = BTreeSet::from([member]);
        assert_eq!(
            rule.recipients(
                SlaTrigger::Breach,
                SlaTarget::Resolution,
                Some(assignee),
                &team
            ),
            BTreeSet::from([manager, assignee, member])
        );
        assert!(
            rule.recipients(
                SlaTrigger::Warning,
                SlaTarget::Resolution,
                Some(assignee),
                &team
            )
            .is_empty()
        );
        assert!(
            rule.recipients(
                SlaTrigger::Breach,
                SlaTarget::Response,
                Some(assignee),
                &team
            )
            .is_empty()
        );
        let nobody = SlaRuleInput {
            notify_assignee: false,
            notify_team: false,
            analyst_ids: BTreeSet::new(),
            ..rule.rule().clone()
        };
        assert!(SlaNotificationRule::create(nobody, Utc::now()).is_err());
    }
}
