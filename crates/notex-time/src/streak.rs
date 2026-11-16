//! Streaks.
//!
//! A streak is derived from the set of completed local days, never stored. Only
//! *scheduled* days count: a habit set to Monday–Friday does not break over the
//! weekend, and today is "at risk" rather than "broken" until the day ends.

use std::collections::BTreeSet;

use chrono::{Datelike, Days, NaiveDate};

use crate::recurrence::weekday_bit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Streak {
    pub current: u32,
    pub longest: u32,
}

/// Whether a habit is due on this day. An empty mask means every day.
pub fn is_scheduled(scheduled_days: u8, day: NaiveDate) -> bool {
    scheduled_days == 0 || scheduled_days & weekday_bit(day.weekday()) != 0
}

pub fn compute_streak(
    done: &BTreeSet<NaiveDate>,
    scheduled_days: u8,
    today: NaiveDate,
    freezes: u32,
) -> Streak {
    Streak {
        current: current_streak(done, scheduled_days, today, freezes),
        longest: longest_streak(done, scheduled_days, today),
    }
}

fn current_streak(
    done: &BTreeSet<NaiveDate>,
    scheduled_days: u8,
    today: NaiveDate,
    freezes: u32,
) -> u32 {
    let mut current = 0;
    let mut remaining_freezes = freezes;
    let mut cursor = today;

    // A missing completion today is not a break; the day is still running.
    let mut today_is_grace = true;

    for _ in 0..(366 * 50) {
        if is_scheduled(scheduled_days, cursor) {
            if done.contains(&cursor) {
                current += 1;
            } else if today_is_grace && cursor == today {
                // no break yet
            } else if remaining_freezes > 0 {
                remaining_freezes -= 1;
            } else {
                break;
            }
        }

        today_is_grace = false;
        match cursor.checked_sub_days(Days::new(1)) {
            Some(previous) => cursor = previous,
            None => break,
        }
    }

    current
}

fn longest_streak(done: &BTreeSet<NaiveDate>, scheduled_days: u8, today: NaiveDate) -> u32 {
    let Some(start) = done.iter().next().copied() else {
        return 0;
    };

    let mut longest = 0;
    let mut run = 0;
    let mut cursor = start;

    while cursor <= today {
        if is_scheduled(scheduled_days, cursor) {
            if done.contains(&cursor) {
                run += 1;
                longest = longest.max(run);
            } else if cursor != today {
                run = 0;
            }
        }
        match cursor.checked_add_days(Days::new(1)) {
            Some(next) => cursor = next,
            None => break,
        }
    }

    longest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recurrence::{EVERY_DAY, WEEKDAYS};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn days(list: &[(i32, u32, u32)]) -> BTreeSet<NaiveDate> {
        list.iter().map(|(y, m, day)| d(*y, *m, *day)).collect()
    }

    #[test]
    fn counts_consecutive_days_and_grants_today() {
        let done = days(&[(2026, 11, 13), (2026, 11, 14), (2026, 11, 15)]);
        let streak = compute_streak(&done, EVERY_DAY, d(2026, 11, 16), 0);
        assert_eq!(streak.current, 3, "today is still open");

        let done = days(&[(2026, 11, 14), (2026, 11, 15), (2026, 11, 16)]);
        let streak = compute_streak(&done, EVERY_DAY, d(2026, 11, 16), 0);
        assert_eq!(streak.current, 3);
    }

    #[test]
    fn a_missed_day_breaks_the_streak() {
        let done = days(&[(2026, 11, 12), (2026, 11, 14), (2026, 11, 15)]);
        let streak = compute_streak(&done, EVERY_DAY, d(2026, 11, 16), 0);
        assert_eq!(streak.current, 2);
        assert_eq!(streak.longest, 2);
    }

    #[test]
    fn weekends_do_not_break_a_weekday_habit() {
        // Mon 9th .. Fri 13th done, then nothing over the weekend.
        let done = days(&[
            (2026, 11, 9),
            (2026, 11, 10),
            (2026, 11, 11),
            (2026, 11, 12),
            (2026, 11, 13),
        ]);
        // Monday the 16th, not done yet.
        let streak = compute_streak(&done, WEEKDAYS, d(2026, 11, 16), 0);
        assert_eq!(streak.current, 5);
        assert_eq!(streak.longest, 5);
    }

    #[test]
    fn a_freeze_absorbs_one_missed_day() {
        let done = days(&[(2026, 11, 12), (2026, 11, 14), (2026, 11, 15)]);
        let streak = compute_streak(&done, EVERY_DAY, d(2026, 11, 16), 1);
        assert_eq!(streak.current, 3);
    }

    #[test]
    fn empty_history_is_zero() {
        let streak = compute_streak(&BTreeSet::new(), EVERY_DAY, d(2026, 11, 16), 0);
        assert_eq!(streak, Streak::default());
    }
}
