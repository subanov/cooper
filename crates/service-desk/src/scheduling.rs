//! Scheduled work on tickets (FR-TKT-031) and the rota (FR-SLA-008; ADR-0042). Pure Rust.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use cooper_kernel::AnalystId;
use thiserror::Error;
use uuid::Uuid;

/// A scheduling problem.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchedulingError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
}

fn invalid(field: &'static str, reason: impl Into<String>) -> SchedulingError {
    SchedulingError::Invalid { field, reason: reason.into() }
}

/// When scheduled work happens: whole days (inclusive) or a time range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// Whole days, first and last day inclusive.
    AllDay {
        /// First day.
        starts_on: NaiveDate,
        /// Last day.
        ends_on: NaiveDate,
    },
    /// A time range.
    Timed {
        /// Start.
        starts_at: DateTime<Utc>,
        /// End (after the start).
        ends_at: DateTime<Utc>,
    },
}

impl Span {
    /// Validates a span.
    ///
    /// # Errors
    ///
    /// A last day before the first, or an end not after the start.
    pub fn new(all_day: bool, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Self, SchedulingError> {
        if all_day {
            let (a, b) = (from.date_naive(), to.date_naive());
            if b < a {
                return Err(invalid("ends_on", "is before the start"));
            }
            Ok(Self::AllDay { starts_on: a, ends_on: b })
        } else {
            if to <= from {
                return Err(invalid("ends_at", "must be after the start"));
            }
            Ok(Self::Timed { starts_at: from, ends_at: to })
        }
    }

    /// The day the work starts (its calendar day).
    #[must_use]
    pub fn first_day(&self) -> NaiveDate {
        match self {
            Self::AllDay { starts_on, .. } => *starts_on,
            Self::Timed { starts_at, .. } => starts_at.date_naive(),
        }
    }

    /// The day the work ends (its calendar day).
    #[must_use]
    pub fn last_day(&self) -> NaiveDate {
        match self {
            Self::AllDay { ends_on, .. } => *ends_on,
            Self::Timed { ends_at, .. } => ends_at.date_naive(),
        }
    }

    /// Whether it is whole days.
    #[must_use]
    pub const fn all_day(&self) -> bool {
        matches!(self, Self::AllDay { .. })
    }
}

/// Work on a ticket for an Owner (an Analyst).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledWork {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: Uuid,
    /// The ticket.
    pub ticket_id: Uuid,
    /// The Owner.
    pub owner: AnalystId,
    /// When.
    pub span: Span,
    /// Notes (at most 2000 characters).
    pub notes: String,
    /// Whether it still stands (cancelled work is kept for the history).
    pub active: bool,
}

impl ScheduledWork {
    /// Validates new work.
    ///
    /// # Errors
    ///
    /// Notes that are too long, or a bad span.
    pub fn new(tenant_id: Uuid, ticket_id: Uuid, owner: AnalystId, span: Span, notes: &str) -> Result<Self, SchedulingError> {
        let notes = notes.trim().to_owned();
        if notes.chars().count() > 2000 {
            return Err(invalid("notes", "must be at most 2000 characters"));
        }
        Ok(Self { id: Uuid::now_v7(), tenant_id, ticket_id, owner, span, notes, active: true })
    }
}

/// A rota shift: a named time of day (it may run over midnight).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shift {
    /// ID.
    pub id: Uuid,
    /// Name (1 to 100 characters).
    pub name: String,
    /// Start time.
    pub starts_at: NaiveTime,
    /// End time (not the start).
    pub ends_at: NaiveTime,
    /// Active.
    pub active: bool,
}

impl Shift {
    /// Validates a shift.
    ///
    /// # Errors
    ///
    /// An empty name or a shift with the same start and end.
    pub fn new(name: &str, starts_at: NaiveTime, ends_at: NaiveTime) -> Result<Self, SchedulingError> {
        let name = clean_name(name)?;
        if starts_at == ends_at {
            return Err(invalid("ends_at", "must differ from the start"));
        }
        Ok(Self { id: Uuid::now_v7(), name, starts_at, ends_at, active: true })
    }
}

/// A rota location, such as Office or WFH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotaLocation {
    /// ID.
    pub id: Uuid,
    /// Name (1 to 100 characters).
    pub name: String,
    /// Active.
    pub active: bool,
}

impl RotaLocation {
    /// Validates a location.
    ///
    /// # Errors
    ///
    /// An empty or too long name.
    pub fn new(name: &str) -> Result<Self, SchedulingError> {
        Ok(Self { id: Uuid::now_v7(), name: clean_name(name)?, active: true })
    }
}

fn clean_name(name: &str) -> Result<String, SchedulingError> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(invalid("name", "must be 1 to 100 characters"));
    }
    Ok(name)
}

/// One Analyst scheduled on a day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotaEntry {
    /// ID.
    pub id: Uuid,
    /// The Analyst.
    pub analyst: AnalystId,
    /// The day.
    pub work_date: NaiveDate,
    /// The shift.
    pub shift_id: Uuid,
    /// The location.
    pub location_id: Uuid,
    /// On call that day and shift.
    pub on_call: bool,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test code

    use super::*;
    use chrono::TimeZone;

    fn at(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 12, 2, h, 0, 0).unwrap()
    }

    #[test]
    fn all_day_work_runs_from_its_first_to_its_last_day_inclusive() {
        let span = Span::new(true, at(0), at(0) + chrono::Duration::days(2)).unwrap();
        assert_eq!(span.first_day(), NaiveDate::from_ymd_opt(2026, 12, 2).unwrap());
        assert_eq!(span.last_day(), NaiveDate::from_ymd_opt(2026, 12, 4).unwrap());
        assert!(Span::new(true, at(0) + chrono::Duration::days(2), at(0)).is_err());
    }

    #[test]
    fn timed_work_must_end_after_it_starts() {
        assert!(Span::new(false, at(9), at(9)).is_err());
        assert!(Span::new(false, at(9), at(11)).is_ok());
    }

    #[test]
    fn work_notes_are_limited() {
        let span = Span::new(false, at(9), at(10)).unwrap();
        let ok = ScheduledWork::new(Uuid::now_v7(), Uuid::now_v7(), AnalystId::new(), span, " Replace disk ");
        assert_eq!(ok.unwrap().notes, "Replace disk");
        assert!(ScheduledWork::new(Uuid::now_v7(), Uuid::now_v7(), AnalystId::new(), span, &"x".repeat(2001)).is_err());
    }

    #[test]
    fn shifts_run_between_different_times_and_names_are_required() {
        let t = |h| NaiveTime::from_hms_opt(h, 0, 0).unwrap();
        assert!(Shift::new("Early", t(7), t(15)).is_ok());
        assert!(Shift::new("Night", t(22), t(6)).is_ok(), "shifts may run over midnight");
        assert!(Shift::new("Same", t(9), t(9)).is_err());
        assert!(RotaLocation::new("  ").is_err());
    }
}
