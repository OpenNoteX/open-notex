//! Recurrence expansion.
//!
//! Rules are data and are expanded to a single occurrence at a time. The
//! scheduler stores the next instant, never a table of future rows.

use chrono::{Datelike, Days, Months, NaiveDate, Weekday};
use chrono_tz::Tz;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
}

impl Freq {
    pub const fn as_str(self) -> &'static str {
        match self {
            Freq::Daily => "daily",
            Freq::Weekly => "weekly",
            Freq::Monthly => "monthly",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "daily" => Some(Freq::Daily),
            "weekly" => Some(Freq::Weekly),
            "monthly" => Some(Freq::Monthly),
            _ => None,
        }
    }
}

/// Monday is bit 0, Sunday is bit 6.
pub fn weekday_bit(weekday: Weekday) -> u8 {
    1 << weekday.num_days_from_monday()
}

pub const EVERY_DAY: u8 = 0b0111_1111;
pub const WEEKDAYS: u8 = 0b0001_1111;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub freq: Freq,
    /// Every N days, weeks or months.
    pub interval: u32,
    /// Weekday mask for `Weekly`.
    pub weekdays: u8,
    /// Day of month for `Monthly`; `Some(-1)` means the last day.
    pub month_day: Option<i8>,
    pub anchor: NaiveDate,
    /// Inclusive last day on which the rule may fire.
    pub until: Option<NaiveDate>,
    pub tz: Tz,
}

impl Rule {
    pub fn daily(anchor: NaiveDate, interval: u32, tz: Tz) -> Self {
        Self {
            freq: Freq::Daily,
            interval: interval.max(1),
            weekdays: EVERY_DAY,
            month_day: None,
            anchor,
            until: None,
            tz,
        }
    }

    pub fn weekly(anchor: NaiveDate, weekdays: u8, tz: Tz) -> Self {
        Self {
            freq: Freq::Weekly,
            interval: 1,
            weekdays: if weekdays == 0 { EVERY_DAY } else { weekdays },
            month_day: None,
            anchor,
            until: None,
            tz,
        }
    }

    pub fn monthly(anchor: NaiveDate, month_day: i8, tz: Tz) -> Self {
        Self {
            freq: Freq::Monthly,
            interval: 1,
            weekdays: EVERY_DAY,
            month_day: Some(month_day),
            anchor,
            until: None,
            tz,
        }
    }

    pub fn with_until(mut self, until: NaiveDate) -> Self {
        self.until = Some(until);
        self
    }

    /// The first occurrence strictly after `after`, or `None` when the rule has
    /// ended. The search is bounded to four years, which covers every supported
    /// rule shape.
    pub fn next_after(&self, after: NaiveDate) -> Option<NaiveDate> {
        let mut candidate = after.checked_add_days(Days::new(1))?;
        for _ in 0..(366 * 4) {
            if let Some(until) = self.until
                && candidate > until
            {
                return None;
            }
            if self.matches(candidate) {
                return Some(candidate);
            }
            candidate = candidate.checked_add_days(Days::new(1))?;
        }
        None
    }

    fn matches(&self, day: NaiveDate) -> bool {
        if day < self.anchor {
            return false;
        }
        match self.freq {
            Freq::Daily => {
                let elapsed = day.signed_duration_since(self.anchor).num_days();
                elapsed % i64::from(self.interval) == 0
            }
            Freq::Weekly => {
                if self.weekdays & weekday_bit(day.weekday()) == 0 {
                    return false;
                }
                let weeks = week_start(day)
                    .signed_duration_since(week_start(self.anchor))
                    .num_days()
                    / 7;
                weeks % i64::from(self.interval) == 0
            }
            Freq::Monthly => {
                let months = month_index(day) - month_index(self.anchor);
                if months < 0 || months % (self.interval.max(1) as i32) != 0 {
                    return false;
                }
                match self.month_day {
                    Some(-1) => day == last_day_of_month(day),
                    Some(target) if target > 0 => day.day() == u32::from(target as u8),
                    _ => day.day() == self.anchor.day(),
                }
            }
        }
    }
}

fn week_start(day: NaiveDate) -> NaiveDate {
    day.checked_sub_days(Days::new(u64::from(day.weekday().num_days_from_monday())))
        .unwrap_or(day)
}

fn month_index(day: NaiveDate) -> i32 {
    day.year() * 12 + day.month0() as i32
}

pub fn last_day_of_month(day: NaiveDate) -> NaiveDate {
    let first_next = NaiveDate::from_ymd_opt(day.year(), day.month(), 1)
        .expect("first of month")
        .checked_add_months(Months::new(1))
        .expect("bounded months");
    first_next
        .checked_sub_days(Days::new(1))
        .expect("a day before the first")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn utc() -> Tz {
        chrono_tz::UTC
    }

    #[test]
    fn daily_respects_interval_and_is_strictly_after() {
        let rule = Rule::daily(d(2026, 11, 16), 2, utc());
        assert_eq!(rule.next_after(d(2026, 11, 15)), Some(d(2026, 11, 16)));
        assert_eq!(rule.next_after(d(2026, 11, 16)), Some(d(2026, 11, 18)));
        assert_eq!(rule.next_after(d(2026, 11, 17)), Some(d(2026, 11, 18)));
    }

    #[test]
    fn weekly_picks_the_next_selected_weekday() {
        let mon_wed_fri =
            weekday_bit(Weekday::Mon) | weekday_bit(Weekday::Wed) | weekday_bit(Weekday::Fri);
        let rule = Rule::weekly(d(2026, 11, 16), mon_wed_fri, utc());
        assert_eq!(rule.next_after(d(2026, 11, 17)), Some(d(2026, 11, 18)));
        assert_eq!(rule.next_after(d(2026, 11, 18)), Some(d(2026, 11, 20)));
        assert_eq!(rule.next_after(d(2026, 11, 20)), Some(d(2026, 11, 23)));
    }

    #[test]
    fn monthly_handles_short_months_and_last_day() {
        let jan31 = Rule::monthly(d(2026, 1, 31), 31, utc());
        // February has no 31st, so the next match is March.
        assert_eq!(jan31.next_after(d(2026, 1, 31)), Some(d(2026, 3, 31)));

        let last = Rule::monthly(d(2026, 1, 31), -1, utc());
        assert_eq!(last.next_after(d(2026, 1, 31)), Some(d(2026, 2, 28)));
        assert_eq!(last.next_after(d(2026, 2, 28)), Some(d(2026, 3, 31)));
    }

    #[test]
    fn until_stops_the_rule() {
        let rule = Rule::daily(d(2026, 11, 16), 1, utc()).with_until(d(2026, 11, 17));
        assert_eq!(rule.next_after(d(2026, 11, 16)), Some(d(2026, 11, 17)));
        assert_eq!(rule.next_after(d(2026, 11, 17)), None);
    }

    #[test]
    fn daily_survives_a_dst_transition() {
        let ny = chrono_tz::America::New_York;
        let rule = Rule::daily(d(2026, 3, 7), 1, ny);
        assert_eq!(rule.next_after(d(2026, 3, 7)), Some(d(2026, 3, 8)));
        assert_eq!(rule.next_after(d(2026, 3, 8)), Some(d(2026, 3, 9)));
    }
}
