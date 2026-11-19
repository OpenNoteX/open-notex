//! Local day arithmetic.
//!
//! A "day" is a local calendar date resolved through a timezone and a configurable
//! day start. The default start is 04:00, so work finished at 01:00 counts for the
//! previous day, which is how people talk about it.

use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimeError {
    #[error("unknown timezone: {0}")]
    Timezone(String),
    #[error("timestamp out of range: {0}")]
    Timestamp(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayPolicy {
    /// Minutes after local midnight at which a new day begins. 240 = 04:00.
    pub day_start_minute: u16,
}

impl Default for DayPolicy {
    fn default() -> Self {
        Self {
            day_start_minute: 240,
        }
    }
}

impl DayPolicy {
    pub const fn new(day_start_minute: u16) -> Self {
        Self { day_start_minute }
    }

    pub const fn midnight() -> Self {
        Self {
            day_start_minute: 0,
        }
    }
}

pub fn parse_tz(name: &str) -> Result<Tz, TimeError> {
    name.parse::<Tz>()
        .map_err(|_| TimeError::Timezone(name.to_owned()))
}

/// The clock and date a person reads on Home.
///
/// The app reads the wall clock and the system timezone; the view only draws
/// these strings, so `view()` stays a pure function of state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// `YYYY-MM-DD`, the day tasks are grouped against.
    pub day: String,
    /// `14:32`.
    pub time: String,
    /// `Thursday 19 November`.
    pub date: String,
}

/// The system's local clock at `ts_ms`.
pub fn local_stamp(ts_ms: i64) -> Result<Stamp, TimeError> {
    let utc = DateTime::<Utc>::from_timestamp_millis(ts_ms).ok_or(TimeError::Timestamp(ts_ms))?;
    let local = utc.with_timezone(&Local);
    Ok(Stamp {
        day: local.format("%Y-%m-%d").to_string(),
        time: local.format("%H:%M").to_string(),
        date: local.format("%A %-d %B").to_string(),
    })
}

/// The local calendar day `ts_ms` falls on, for the archive's grouping.
pub fn local_day(ts_ms: i64) -> Option<String> {
    local_stamp(ts_ms).ok().map(|stamp| stamp.day)
}

/// The local day a timestamp belongs to, given a timezone and day start.
pub fn day_key(ts_ms: i64, tz: Tz, policy: DayPolicy) -> Result<NaiveDate, TimeError> {
    let utc = DateTime::<Utc>::from_timestamp_millis(ts_ms).ok_or(TimeError::Timestamp(ts_ms))?;
    let local = utc.with_timezone(&tz);
    let shifted = local.naive_local() - Duration::minutes(i64::from(policy.day_start_minute));
    Ok(shifted.date())
}

/// The instant a local day begins, as UTC milliseconds.
///
/// Around a spring-forward gap the requested local time may not exist, so the
/// first valid instant after it is used.
pub fn day_start_ms(day: NaiveDate, tz: Tz, policy: DayPolicy) -> i64 {
    let midnight = day.and_hms_opt(0, 0, 0).expect("midnight is always valid");
    let naive = midnight + Duration::minutes(i64::from(policy.day_start_minute));
    local_to_utc_ms(naive, tz)
}

fn local_to_utc_ms(naive: NaiveDateTime, tz: Tz) -> i64 {
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.timestamp_millis(),
        LocalResult::Ambiguous(first, _second) => first.timestamp_millis(),
        LocalResult::None => {
            // A gap (spring forward). Walk forward to the first instant that exists.
            for minutes in 1..=180 {
                let shifted = naive + Duration::minutes(minutes);
                match tz.from_local_datetime(&shifted) {
                    LocalResult::Single(dt) => return dt.timestamp_millis(),
                    LocalResult::Ambiguous(first, _) => return first.timestamp_millis(),
                    LocalResult::None => {}
                }
            }
            // Unreachable for real zones; fall back to interpreting it as UTC.
            naive.and_utc().timestamp_millis()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0)
            .single()
            .expect("valid utc")
            .timestamp_millis()
    }

    #[test]
    fn early_morning_belongs_to_the_previous_day() {
        let ny = parse_tz("America/New_York").unwrap();
        // 01:30 local on 2026-11-17 is still 2026-11-16 with an 04:00 start.
        let early = ts(2026, 11, 17, 6, 30); // 01:30 EST
        assert_eq!(
            day_key(early, ny, DayPolicy::default()).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()
        );
        assert_eq!(
            day_key(early, ny, DayPolicy::midnight()).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 17).unwrap()
        );
    }

    #[test]
    fn day_keys_follow_dst_transitions() {
        let ny = parse_tz("America/New_York").unwrap();
        let policy = DayPolicy::midnight();
        // Spring forward: 2026-03-08 has 23 hours, but the date stays correct.
        let before = ts(2026, 3, 8, 4, 30); // 23:30 EST on the 7th
        let after = ts(2026, 3, 8, 5, 30); // 00:30 EST on the 8th
        assert_eq!(
            day_key(before, ny, policy).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap()
        );
        assert_eq!(
            day_key(after, ny, policy).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 8).unwrap()
        );

        let kolkata = parse_tz("Asia/Kolkata").unwrap();
        assert_eq!(
            day_key(ts(2026, 3, 8, 18, 45), kolkata, policy).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()
        );
    }

    #[test]
    fn day_start_is_stable_across_zones() {
        let day = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let policy = DayPolicy::default();
        for name in [
            "UTC",
            "Asia/Tokyo",
            "America/Los_Angeles",
            "Pacific/Kiritimati",
        ] {
            let tz = parse_tz(name).unwrap();
            let start = day_start_ms(day, tz, policy);
            assert_eq!(day_key(start, tz, policy).unwrap(), day);
        }
    }

    #[test]
    fn the_local_stamp_is_a_day_a_clock_and_a_date() {
        let ts = ts(2026, 11, 19, 14, 32);
        let stamp = local_stamp(ts).unwrap();
        assert_eq!(stamp.day.len(), 10, "{}", stamp.day);
        assert!(
            crate::checklist::parse_day(&stamp.day).is_some(),
            "{}",
            stamp.day
        );
        assert_eq!(stamp.time.len(), 5, "{}", stamp.time);
        assert!(stamp.date.contains(' '), "{}", stamp.date);
        assert_eq!(local_day(ts).as_deref(), Some(stamp.day.as_str()));
        assert!(local_stamp(i64::MAX).is_err());
        assert_eq!(local_day(i64::MAX), None);
    }

    #[test]
    fn rejects_bad_zone_and_timestamp() {
        assert_eq!(
            parse_tz("Mars/Olympus"),
            Err(TimeError::Timezone("Mars/Olympus".to_owned()))
        );
        assert!(day_key(i64::MAX, parse_tz("UTC").unwrap(), DayPolicy::default()).is_err());
    }
}
