//! Task Recurrence (FR-TSK-006): the date engine. Pure: dates in, dates out.
//!
//! All arithmetic is on calendar dates, never instants: "the 1st" is the 1st wherever
//! the reader is.

use std::collections::BTreeSet;

use chrono::{Datelike, Days, Months, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use crate::domain::TaskError;

/// Which day of a month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum MonthlyOn {
    /// A day of the month, 1–31 (clamped to the month's last day), or -1 for the last.
    DayOfMonth {
        /// The day.
        day: i8,
    },
    /// The nth weekday, 1–5 or -1 for the last (months without a 5th are skipped).
    NthWeekday {
        /// Which one.
        nth: i8,
        /// ISO weekday, 1 (Monday) to 7 (Sunday).
        weekday: u8,
    },
}

/// How often.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "frequency", rename_all = "snake_case")]
pub enum Frequency {
    /// Every `interval` days.
    Daily,
    /// On these ISO weekdays (1–7) every `interval` weeks; none means the weekday of
    /// the date it counts from.
    Weekly {
        /// ISO weekdays.
        #[serde(default)]
        weekdays: BTreeSet<u8>,
    },
    /// Every `interval` months.
    Monthly {
        /// Which day.
        #[serde(flatten)]
        on: MonthlyOn,
    },
    /// Every `interval` years, on a month and day (clamped, or -1 for the last day).
    Yearly {
        /// Month, 1–12.
        month: u8,
        /// Day, 1–31 or -1.
        day: i8,
    },
}

/// When the next occurrence appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    /// When this one is completed, counted from the completion date (default).
    OnCompletion,
    /// On fixed dates, whether or not the previous one was completed.
    Fixed,
}

/// When the series ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ends", rename_all = "snake_case")]
pub enum Ends {
    /// Never.
    Never,
    /// No occurrence after this date.
    OnDate {
        /// The last allowed date.
        date: NaiveDate,
    },
    /// After this many occurrences in total (the first Task counts).
    AfterCount {
        /// Total occurrences.
        count: u32,
    },
}

/// A repeat rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurrenceRule {
    /// How often.
    #[serde(flatten)]
    pub frequency: Frequency,
    /// Every n days, weeks, months or years (1–999).
    pub interval: u32,
    /// When the next one appears.
    pub mode: RepeatMode,
    /// When the series ends.
    #[serde(flatten)]
    pub ends: Ends,
}

fn iso(weekday: u8) -> Option<Weekday> {
    match weekday {
        1 => Some(Weekday::Mon),
        2 => Some(Weekday::Tue),
        3 => Some(Weekday::Wed),
        4 => Some(Weekday::Thu),
        5 => Some(Weekday::Fri),
        6 => Some(Weekday::Sat),
        7 => Some(Weekday::Sun),
        _ => None,
    }
}

fn last_day(year: i32, month: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, 1)?
        .checked_add_months(Months::new(1))?
        .pred_opt()
}

/// The day `day` (1–31 clamped, or -1 for the last) of a month.
fn day_in_month(year: i32, month: u32, day: i8) -> Option<NaiveDate> {
    let last = last_day(year, month)?;
    if day == -1 {
        return Some(last);
    }
    let d = u32::try_from(day).ok()?.min(last.day());
    NaiveDate::from_ymd_opt(year, month, d)
}

/// The `nth` (1–5, or -1 for the last) `weekday` of a month, if the month has one.
fn nth_weekday(year: i32, month: u32, nth: i8, weekday: Weekday) -> Option<NaiveDate> {
    if nth == -1 {
        let last = last_day(year, month)?;
        let back = (7 + last.weekday().num_days_from_monday() - weekday.num_days_from_monday()) % 7;
        return last.checked_sub_days(Days::new(u64::from(back)));
    }
    NaiveDate::from_weekday_of_month_opt(year, month, weekday, u8::try_from(nth).ok()?)
}

impl RecurrenceRule {
    /// Checks the rule.
    ///
    /// # Errors
    ///
    /// An interval outside 1–999, unknown weekdays, impossible days or months, or an
    /// end count of zero.
    pub fn validate(&self) -> Result<(), TaskError> {
        let bad = |reason: &str| Err(TaskError::invalid("recurrence", reason));
        if !(1..=999).contains(&self.interval) {
            return bad("interval must be 1 to 999");
        }
        match &self.frequency {
            Frequency::Daily => {}
            Frequency::Weekly { weekdays } => {
                if weekdays.iter().any(|d| iso(*d).is_none()) {
                    return bad("weekdays are 1 (Monday) to 7 (Sunday)");
                }
            }
            Frequency::Monthly {
                on: MonthlyOn::DayOfMonth { day },
            } => {
                if !(*day == -1 || (1..=31).contains(day)) {
                    return bad("day must be 1 to 31, or -1 for the last day");
                }
            }
            Frequency::Monthly {
                on: MonthlyOn::NthWeekday { nth, weekday },
            } => {
                if !(*nth == -1 || (1..=5).contains(nth)) || iso(*weekday).is_none() {
                    return bad("nth must be 1 to 5 or -1, weekday 1 to 7");
                }
            }
            Frequency::Yearly { month, day } => {
                if !(1..=12).contains(month) || !(*day == -1 || (1..=31).contains(day)) {
                    return bad("month must be 1 to 12, day 1 to 31 or -1");
                }
            }
        }
        if self.ends == (Ends::AfterCount { count: 0 }) {
            return bad("a series has at least one occurrence");
        }
        Ok(())
    }

    /// The first date strictly after `after` that the rule produces, counting
    /// intervals from `after`. `None` when no such date exists in the calendar.
    #[must_use]
    pub fn next_after(&self, after: NaiveDate) -> Option<NaiveDate> {
        let n = self.interval.max(1);
        match &self.frequency {
            Frequency::Daily => after.checked_add_days(Days::new(u64::from(n))),
            Frequency::Weekly { weekdays } => {
                let days: BTreeSet<u32> = if weekdays.is_empty() {
                    BTreeSet::from([after.weekday().number_from_monday()])
                } else {
                    weekdays.iter().map(|d| u32::from(*d)).collect()
                };
                let monday = after.checked_sub_days(Days::new(u64::from(
                    after.weekday().num_days_from_monday(),
                )))?;
                (1..=(7 * u64::from(n) + 7)).find_map(|k| {
                    let d = after.checked_add_days(Days::new(k))?;
                    let week = u32::try_from((d - monday).num_days() / 7).ok()?;
                    (week % n == 0 && days.contains(&d.weekday().number_from_monday())).then_some(d)
                })
            }
            Frequency::Monthly { on } => (0..=(12 * 30)).find_map(|k| {
                let first = after.with_day(1)?.checked_add_months(Months::new(k * n))?;
                let d = match on {
                    MonthlyOn::DayOfMonth { day } => {
                        day_in_month(first.year(), first.month(), *day)
                    }
                    MonthlyOn::NthWeekday { nth, weekday } => {
                        nth_weekday(first.year(), first.month(), *nth, iso(*weekday)?)
                    }
                }?;
                (d > after).then_some(d)
            }),
            Frequency::Yearly { month, day } => (0..=50).find_map(|k| {
                let year = after.year() + i32::try_from(k * n).ok()?;
                let d = day_in_month(year, u32::from(*month), *day)?;
                (d > after).then_some(d)
            }),
        }
    }

    /// Whether the series may make another occurrence on `date`, given how many it has
    /// made.
    #[must_use]
    pub fn allows(&self, date: NaiveDate, created: u32) -> bool {
        match self.ends {
            Ends::Never => true,
            Ends::OnDate { date: last } => date <= last,
            Ends::AfterCount { count } => created < count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn rule(frequency: Frequency, interval: u32) -> RecurrenceRule {
        RecurrenceRule {
            frequency,
            interval,
            mode: RepeatMode::OnCompletion,
            ends: Ends::Never,
        }
    }

    #[test]
    fn worked_examples() {
        assert_eq!(
            rule(Frequency::Daily, 3).next_after(d("2026-12-30")),
            Some(d("2027-01-02"))
        );
        let mon_thu = Frequency::Weekly {
            weekdays: BTreeSet::from([1, 4]),
        };
        assert_eq!(
            rule(mon_thu.clone(), 1).next_after(d("2026-11-09")),
            Some(d("2026-11-12")),
            "Mon -> Thu"
        );
        assert_eq!(
            rule(mon_thu.clone(), 1).next_after(d("2026-11-12")),
            Some(d("2026-11-16")),
            "Thu -> next Mon"
        );
        assert_eq!(
            rule(mon_thu, 2).next_after(d("2026-11-12")),
            Some(d("2026-11-23")),
            "every other week"
        );
        let weekly = Frequency::Weekly {
            weekdays: BTreeSet::new(),
        };
        assert_eq!(
            rule(weekly, 1).next_after(d("2026-11-11")),
            Some(d("2026-11-18")),
            "same weekday"
        );
        let the_31st = Frequency::Monthly {
            on: MonthlyOn::DayOfMonth { day: 31 },
        };
        assert_eq!(
            rule(the_31st.clone(), 1).next_after(d("2027-01-31")),
            Some(d("2027-02-28")),
            "clamped"
        );
        assert_eq!(
            rule(the_31st, 1).next_after(d("2027-02-28")),
            Some(d("2027-03-31"))
        );
        let last = Frequency::Monthly {
            on: MonthlyOn::DayOfMonth { day: -1 },
        };
        assert_eq!(
            rule(last, 1).next_after(d("2028-01-31")),
            Some(d("2028-02-29")),
            "leap year"
        );
        let second_tue = Frequency::Monthly {
            on: MonthlyOn::NthWeekday { nth: 2, weekday: 2 },
        };
        assert_eq!(
            rule(second_tue, 1).next_after(d("2026-11-10")),
            Some(d("2026-12-08"))
        );
        let last_fri = Frequency::Monthly {
            on: MonthlyOn::NthWeekday {
                nth: -1,
                weekday: 5,
            },
        };
        assert_eq!(
            rule(last_fri, 1).next_after(d("2026-11-01")),
            Some(d("2026-11-27"))
        );
        let fifth_mon = Frequency::Monthly {
            on: MonthlyOn::NthWeekday { nth: 5, weekday: 1 },
        };
        assert_eq!(
            rule(fifth_mon, 1).next_after(d("2026-11-30")),
            Some(d("2027-03-29")),
            "months without one are skipped"
        );
        let quarterly = Frequency::Monthly {
            on: MonthlyOn::DayOfMonth { day: 1 },
        };
        assert_eq!(
            rule(quarterly, 3).next_after(d("2026-11-15")),
            Some(d("2027-02-01"))
        );
        let leap_day = Frequency::Yearly { month: 2, day: 29 };
        assert_eq!(
            rule(leap_day, 1).next_after(d("2028-02-29")),
            Some(d("2029-02-28")),
            "clamped in common years"
        );
    }

    #[test]
    fn rules_are_validated_and_end_conditions_hold() {
        assert!(rule(Frequency::Daily, 0).validate().is_err());
        assert!(
            rule(
                Frequency::Weekly {
                    weekdays: BTreeSet::from([8])
                },
                1
            )
            .validate()
            .is_err()
        );
        assert!(
            rule(
                Frequency::Monthly {
                    on: MonthlyOn::DayOfMonth { day: 0 }
                },
                1
            )
            .validate()
            .is_err()
        );
        assert!(
            rule(Frequency::Yearly { month: 13, day: 1 }, 1)
                .validate()
                .is_err()
        );
        let mut r = rule(Frequency::Daily, 1);
        r.ends = Ends::AfterCount { count: 0 };
        assert!(r.validate().is_err());
        r.ends = Ends::AfterCount { count: 3 };
        assert!(r.allows(d("2030-01-01"), 2) && !r.allows(d("2030-01-01"), 3));
        r.ends = Ends::OnDate {
            date: d("2026-12-31"),
        };
        assert!(r.allows(d("2026-12-31"), 99) && !r.allows(d("2027-01-01"), 1));
        let json = serde_json::json!({ "frequency": "monthly", "on": "nth_weekday", "nth": -1, "weekday": 5,
                                       "interval": 1, "mode": "fixed", "ends": "after_count", "count": 12 });
        let parsed: RecurrenceRule = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json, "round-trips");
    }

    /// Whether `date` fits the rule's shape when the interval is 1 (no anchor needed).
    fn fits(f: &Frequency, anchor: NaiveDate, date: NaiveDate) -> bool {
        match f {
            Frequency::Daily => true,
            Frequency::Weekly { weekdays } if weekdays.is_empty() => {
                date.weekday() == anchor.weekday()
            }
            Frequency::Weekly { weekdays } => {
                weekdays.iter().any(|w| iso(*w) == Some(date.weekday()))
            }
            Frequency::Monthly {
                on: MonthlyOn::DayOfMonth { day },
            } => day_in_month(date.year(), date.month(), *day) == Some(date),
            Frequency::Monthly {
                on: MonthlyOn::NthWeekday { nth, weekday },
            } => {
                iso(*weekday).and_then(|w| nth_weekday(date.year(), date.month(), *nth, w))
                    == Some(date)
            }
            Frequency::Yearly { month, day } => {
                day_in_month(date.year(), u32::from(*month), *day) == Some(date)
            }
        }
    }

    fn frequency() -> impl Strategy<Value = Frequency> {
        prop_oneof![
            Just(Frequency::Daily),
            proptest::collection::btree_set(1u8..=7, 0..=7)
                .prop_map(|weekdays| Frequency::Weekly { weekdays }),
            prop_oneof![Just(-1i8), 1i8..=31].prop_map(|day| Frequency::Monthly {
                on: MonthlyOn::DayOfMonth { day }
            }),
            (prop_oneof![Just(-1i8), 1i8..=5], 1u8..=7).prop_map(|(nth, weekday)| {
                Frequency::Monthly {
                    on: MonthlyOn::NthWeekday { nth, weekday },
                }
            }),
            (1u8..=12, prop_oneof![Just(-1i8), 1i8..=31])
                .prop_map(|(month, day)| Frequency::Yearly { month, day }),
        ]
    }

    fn date() -> impl Strategy<Value = NaiveDate> {
        (0u64..36_500).prop_map(|n| NaiveDate::from_ymd_opt(2000, 1, 1).unwrap() + Days::new(n))
    }

    proptest! {
        #[test]
        fn the_next_date_is_later_valid_and_the_first_one(f in frequency(), after in date()) {
            let r = rule(f.clone(), 1);
            prop_assert!(r.validate().is_ok());
            let next = r.next_after(after).expect("a next date exists");
            prop_assert!(next > after);
            prop_assert!(fits(&f, after, next), "{next} does not fit {f:?}");
            let mut day = after.succ_opt().unwrap();
            while day < next {
                prop_assert!(!fits(&f, after, day), "{day} fits {f:?} before {next}");
                day = day.succ_opt().unwrap();
            }
        }

        #[test]
        fn intervals_never_come_sooner_than_one(f in frequency(), after in date(), n in 1u32..=12) {
            let one = rule(f.clone(), 1).next_after(after).unwrap();
            let many = rule(f.clone(), n).next_after(after).unwrap();
            prop_assert!(many >= one);
            prop_assert!(fits(&f, after, many));
        }
    }
}
