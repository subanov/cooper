//! Job schedules: every N seconds, daily at a UTC time, or off.

use chrono::{DateTime, Duration, NaiveTime, Utc};
use thiserror::Error;

use super::ScheduleColumns;

/// When a job runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Every `seconds` seconds, measured from the start of the previous run.
    Interval {
        /// Interval in seconds; at least 1.
        seconds: u32,
    },
    /// Once a day at this time (UTC).
    Daily {
        /// Time of day in UTC.
        at: NaiveTime,
    },
    /// Never.
    Off,
}

/// Stored schedule columns are inconsistent.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScheduleError {
    /// Unknown `schedule_kind`.
    #[error("unknown schedule kind {0:?}")]
    UnknownKind(String),
    /// The interval is missing or not positive.
    #[error("interval schedule needs a positive interval_seconds")]
    InvalidInterval,
    /// The daily time is missing.
    #[error("daily schedule needs daily_at")]
    MissingDailyTime,
}

impl Schedule {
    /// An interval schedule. Zero is rounded up to one second.
    #[must_use]
    pub const fn every_seconds(seconds: u32) -> Self {
        Self::Interval {
            seconds: if seconds == 0 { 1 } else { seconds },
        }
    }

    /// The next run strictly after `after`, or `None` when off.
    #[must_use]
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match *self {
            Self::Interval { seconds } => Some(after + Duration::seconds(i64::from(seconds))),
            Self::Daily { at } => {
                let today = after.date_naive().and_time(at).and_utc();
                Some(if today > after {
                    today
                } else {
                    today + Duration::days(1)
                })
            }
            Self::Off => None,
        }
    }

    /// The first run of a newly registered job: interval jobs run immediately, daily jobs
    /// at their next time of day.
    #[must_use]
    pub fn first_run(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Self::Interval { .. } => Some(now),
            Self::Daily { .. } | Self::Off => self.next_after(now),
        }
    }

    /// The `background_jobs` columns for this schedule.
    #[must_use]
    pub fn to_columns(&self) -> ScheduleColumns {
        match *self {
            Self::Interval { seconds } => (
                "interval",
                Some(i32::try_from(seconds).unwrap_or(i32::MAX)),
                None,
            ),
            Self::Daily { at } => ("daily", None, Some(at)),
            Self::Off => ("off", None, None),
        }
    }

    /// Rebuilds a schedule from `background_jobs` columns.
    ///
    /// # Errors
    ///
    /// Fails if the columns are inconsistent.
    pub fn from_columns(
        kind: &str,
        interval_seconds: Option<i32>,
        daily_at: Option<NaiveTime>,
    ) -> Result<Self, ScheduleError> {
        match kind {
            "interval" => interval_seconds
                .and_then(|s| u32::try_from(s).ok())
                .filter(|s| *s > 0)
                .map(|seconds| Self::Interval { seconds })
                .ok_or(ScheduleError::InvalidInterval),
            "daily" => daily_at
                .map(|at| Self::Daily { at })
                .ok_or(ScheduleError::MissingDailyTime),
            "off" => Ok(Self::Off),
            other => Err(ScheduleError::UnknownKind(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use proptest::prelude::*;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 10, h, m, 0).unwrap()
    }

    fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn daily_runs_today_if_time_not_reached_else_tomorrow() {
        let daily = Schedule::Daily { at: time(3, 0) };
        assert_eq!(daily.next_after(at(2, 59)), Some(at(3, 0)));
        assert_eq!(
            daily.next_after(at(3, 0)),
            Some(at(3, 0) + Duration::days(1))
        );
        assert_eq!(
            daily.next_after(at(23, 0)),
            Some(at(3, 0) + Duration::days(1))
        );
    }

    #[test]
    fn interval_jobs_run_immediately_when_registered() {
        let s = Schedule::every_seconds(60);
        assert_eq!(s.first_run(at(9, 0)), Some(at(9, 0)));
        assert_eq!(s.next_after(at(9, 0)), Some(at(9, 1)));
        assert_eq!(Schedule::Off.first_run(at(9, 0)), None);
    }

    #[test]
    fn rejects_inconsistent_columns() {
        assert_eq!(
            Schedule::from_columns("interval", Some(0), None),
            Err(ScheduleError::InvalidInterval)
        );
        assert_eq!(
            Schedule::from_columns("daily", None, None),
            Err(ScheduleError::MissingDailyTime)
        );
        assert!(matches!(
            Schedule::from_columns("weekly", None, None),
            Err(ScheduleError::UnknownKind(_))
        ));
    }

    proptest! {
        #[test]
        fn columns_round_trip(seconds in 1_u32..=i32::MAX as u32, h in 0_u32..24, m in 0_u32..60) {
            for s in [Schedule::every_seconds(seconds), Schedule::Daily { at: time(h, m) }, Schedule::Off] {
                let (kind, interval, daily) = s.to_columns();
                prop_assert_eq!(Schedule::from_columns(kind, interval, daily).unwrap(), s);
            }
        }

        #[test]
        fn next_run_is_after_and_within_a_day(secs in 0_i64..400 * 86_400, h in 0_u32..24, m in 0_u32..60) {
            let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::seconds(secs);
            let next = Schedule::Daily { at: time(h, m) }.next_after(now).unwrap();
            prop_assert!(next > now);
            prop_assert!(next <= now + Duration::days(1));
            prop_assert_eq!(next.time(), time(h, m));
        }
    }
}
