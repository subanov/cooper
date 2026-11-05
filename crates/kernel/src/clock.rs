//! Time source abstraction.
//!
//! Domain code never reads the system clock. It takes a `&dyn Clock` (or a timestamp)
//! so tests can use a [`FixedClock`]. All timestamps are UTC.

use std::sync::Mutex;

use chrono::{DateTime, Duration, SubsecRound as _, Utc};

/// Provides the current time.
pub trait Clock: Send + Sync {
    /// The current instant in UTC.
    fn now(&self) -> DateTime<Utc>;
}

/// The real system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    /// Microsecond precision, like PostgreSQL `timestamptz`, so a value returned to a
    /// client equals the value stored.
    fn now(&self) -> DateTime<Utc> {
        Utc::now().trunc_subsecs(6)
    }
}

/// A clock that only moves when told to. Use it in tests.
#[derive(Debug)]
pub struct FixedClock {
    now: Mutex<DateTime<Utc>>,
}

impl FixedClock {
    /// A clock frozen at `now`.
    #[must_use]
    pub const fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    /// Moves the clock to `now`.
    pub fn set(&self, now: DateTime<Utc>) {
        *self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = now;
    }

    /// Moves the clock forward by `by`.
    pub fn advance(&self, by: Duration) {
        let mut now = self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *now += by;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn fixed_clock_only_moves_when_told() {
        let start = Utc.with_ymd_and_hms(2026, 1, 5, 9, 0, 0).unwrap();
        let clock = FixedClock::new(start);
        assert_eq!(clock.now(), start);

        clock.advance(Duration::minutes(90));
        assert_eq!(clock.now(), start + Duration::minutes(90));

        clock.set(start);
        assert_eq!(clock.now(), start);
    }
}
