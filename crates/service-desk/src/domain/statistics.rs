//! Ticket statistics that reports are built from (FR-RPT-001, FR-RPT-002, FR-RPT-007,
//! ADR-0032).
//!
//! Counting happens in the database: the repository groups tickets and Time Entries by
//! one dimension and returns one tally per group. The same tenant, deletion and
//! Confidential predicates as ticket lists apply, so a figure never counts a ticket the
//! caller could not open.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use cooper_kernel::{AnalystId, TenantId, TenantScope};
use uuid::Uuid;

use super::{RepoError, TargetState, Visibility};

/// A calendar bucket: a day or a month in the instance time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Interval {
    /// Calendar days.
    Day,
    /// Calendar months (keyed by their first day).
    Month,
}

/// What tickets are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TicketGrouping {
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
    /// Queue (assigned Team).
    Team,
    /// Assignee.
    Analyst,
    /// Company.
    Company,
    /// Day or month of creation, in the given time zone.
    Created(Interval),
    /// Day or month of closure, in the given time zone (closed tickets only).
    Closed(Interval),
}

/// What Time Entries are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeGrouping {
    /// The Analyst who spent the time.
    Analyst,
    /// The ticket's Queue.
    Team,
    /// Company.
    Company,
    /// Work date, by day or month.
    WorkDate(Interval),
}

/// The value a group stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupValue {
    /// No value (no Department, unassigned, …).
    None,
    /// A reference (status, Priority, Analyst, Company, …).
    Id(Uuid),
    /// A day, or the first day of a month.
    Date(NaiveDate),
}

impl GroupValue {
    /// From the two nullable columns a statistics query returns.
    #[must_use]
    pub const fn from_columns(id: Option<Uuid>, date: Option<NaiveDate>) -> Self {
        match (id, date) {
            (Some(id), _) => Self::Id(id),
            (None, Some(d)) => Self::Date(d),
            (None, None) => Self::None,
        }
    }
}

/// Which tickets a statistic covers. Instants are half-open ranges `[from, to)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatisticsFilter {
    /// Only this Company (must be within the caller's scope).
    pub tenant: Option<TenantId>,
    /// Only tickets created in this range.
    pub created: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Only tickets closed in this range.
    pub closed: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// `Some(true)`: open tickets only; `Some(false)`: closed only.
    pub open: Option<bool>,
    /// Only tickets assigned to this Analyst.
    pub analyst: Option<AnalystId>,
    /// Only tickets assigned to nobody.
    pub unassigned: bool,
}

/// How many tickets fall in one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TicketTally {
    /// The group.
    pub group: GroupValue,
    /// Tickets.
    pub tickets: u64,
    /// Of which flagged First Time Fix.
    pub first_time_fix: u64,
}

/// Where tickets stand against one SLA target, using the state the service desk shows
/// on each ticket ([`TargetState`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TargetTally {
    /// Satisfied in time ([`TargetState::Met`]).
    pub met: u64,
    /// Satisfied after the due time ([`TargetState::MetLate`]).
    pub met_late: u64,
    /// Not satisfied and past due ([`TargetState::Breached`]).
    pub breached: u64,
    /// Not satisfied, not past due or paused ([`TargetState::OnTrack`],
    /// [`TargetState::Warning`], [`TargetState::Paused`]).
    pub running: u64,
    /// No target configured ([`TargetState::None`]).
    pub no_target: u64,
}

impl TargetTally {
    /// Counts one ticket in the bucket of its state.
    pub const fn count(&mut self, state: TargetState) {
        match state {
            TargetState::Met => self.met += 1,
            TargetState::MetLate => self.met_late += 1,
            TargetState::Breached => self.breached += 1,
            TargetState::OnTrack | TargetState::Warning | TargetState::Paused => {
                self.running += 1;
            }
            TargetState::None => self.no_target += 1,
        }
    }
}

/// SLA results of one group of tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlaTally {
    /// The group.
    pub group: GroupValue,
    /// The Response target.
    pub response: TargetTally,
    /// The Resolution target.
    pub resolution: TargetTally,
}

/// Time recorded in one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeTally {
    /// The group.
    pub group: GroupValue,
    /// Total minutes.
    pub minutes: u64,
    /// Number of Time Entries.
    pub entries: u64,
}

/// Aggregate queries over tickets and Time Entries. Every query filters by `scope` and
/// `visibility` exactly like ticket lists, and leaves out soft-deleted tickets and
/// tickets merged into another (their work continues on the target).
#[async_trait]
pub trait TicketStatistics: Send + Sync {
    /// Tickets per group. Date groups are calendar days or months in `tz`.
    async fn tally(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        group: TicketGrouping,
        filter: &StatisticsFilter,
        tz: Tz,
    ) -> Result<Vec<TicketTally>, RepoError>;

    /// SLA results per group, classified at `now` like [`super::Ticket::sla_state`].
    async fn sla_tally(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        group: TicketGrouping,
        filter: &StatisticsFilter,
        tz: Tz,
        now: DateTime<Utc>,
    ) -> Result<Vec<SlaTally>, RepoError>;

    /// Time Entries with a work date in `from..=to` per group.
    async fn time_tally(
        &self,
        scope: &TenantScope,
        visibility: &Visibility,
        group: TimeGrouping,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<TimeTally>, RepoError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_states_fall_in_one_bucket_each() {
        let mut t = TargetTally::default();
        for s in [
            TargetState::Met,
            TargetState::MetLate,
            TargetState::Breached,
            TargetState::OnTrack,
            TargetState::Warning,
            TargetState::Paused,
            TargetState::None,
        ] {
            t.count(s);
        }
        assert_eq!(
            t,
            TargetTally {
                met: 1,
                met_late: 1,
                breached: 1,
                running: 3,
                no_target: 1
            }
        );
    }

    #[test]
    fn group_values_prefer_the_id_column() {
        let id = Uuid::now_v7();
        let d = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap_or_default();
        assert_eq!(GroupValue::from_columns(Some(id), None), GroupValue::Id(id));
        assert_eq!(GroupValue::from_columns(None, Some(d)), GroupValue::Date(d));
        assert_eq!(GroupValue::from_columns(None, None), GroupValue::None);
    }
}
