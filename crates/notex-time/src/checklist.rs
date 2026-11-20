//! How a checklist reads a due day.
//!
//! A task's due day is a local calendar date (`YYYY-MM-DD`), never a timestamp.
//! These helpers turn that string into the one line a row shows ("2d late",
//! "Tomorrow", "Fri", "23 Nov") and into the group it belongs to. They are pure:
//! `today` is passed in, so a test and a screenshot never depend on the clock.

use chrono::{Datelike, Duration, NaiveDate};

/// The four groups the Tasks list is drawn in, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    Overdue,
    Today,
    Upcoming,
    Someday,
}

impl Bucket {
    pub const ALL: [Bucket; 4] = [
        Bucket::Overdue,
        Bucket::Today,
        Bucket::Upcoming,
        Bucket::Someday,
    ];

    /// The heading above the group.
    pub const fn title(self) -> &'static str {
        match self {
            Bucket::Overdue => "Overdue",
            Bucket::Today => "Today",
            Bucket::Upcoming => "Upcoming",
            Bucket::Someday => "No date",
        }
    }

    /// A stable fragment for element keys and scene names.
    pub const fn key(self) -> &'static str {
        match self {
            Bucket::Overdue => "overdue",
            Bucket::Today => "today",
            Bucket::Upcoming => "upcoming",
            Bucket::Someday => "someday",
        }
    }
}

/// A due day, read for one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    /// What the chip says.
    pub text: String,
    /// Past its day: the chip uses the danger colour.
    pub late: bool,
    pub bucket: Bucket,
}

/// Parse the stored `YYYY-MM-DD`.
pub fn parse_day(day: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

/// Which group a task belongs to. No date, or a date this cannot read, is
/// `Someday`: a malformed row must not be hidden in "Overdue".
pub fn bucket(due_day: Option<&str>, today: &str) -> Bucket {
    let Some(due) = due_day.and_then(parse_day) else {
        return Bucket::Someday;
    };
    let Some(today) = parse_day(today) else {
        return Bucket::Someday;
    };
    match (due - today).num_days() {
        days if days < 0 => Bucket::Overdue,
        0 => Bucket::Today,
        _ => Bucket::Upcoming,
    }
}

/// How a due day reads on its chip.
pub fn due(due_day: &str, today: &str) -> Due {
    let Some(due) = parse_day(due_day) else {
        // The value is still the truth about the row, even when this cannot
        // read it as a date.
        return Due {
            text: due_day.to_owned(),
            late: false,
            bucket: Bucket::Someday,
        };
    };
    let days = parse_day(today).map_or(0, |today| (due - today).num_days());
    let late = days < 0;
    let text = match days {
        // "1d late" rather than "Yesterday": a missed date is a count, not a day.
        days if days < 0 => format!("{}d late", -days),
        0 => "Today".to_owned(),
        1 => "Tomorrow".to_owned(),
        2..=6 => due.format("%a").to_string(),
        _ => due.format("%-d %b").to_string(),
    };
    Due {
        text,
        late,
        bucket: if late {
            Bucket::Overdue
        } else if days == 0 {
            Bucket::Today
        } else {
            Bucket::Upcoming
        },
    }
}

/// A past or future day, as the archive and the due buttons name it.
///
/// "Today" and "Yesterday" are words; anything else is "23 Nov", with the year
/// only when it is not this one, so a row from last December still says which
/// December.
pub fn day_label(day: &str, today: &str) -> String {
    let Some(date) = parse_day(day) else {
        return day.to_owned();
    };
    let Some(today) = parse_day(today) else {
        return day.to_owned();
    };
    match (date - today).num_days() {
        0 => "Today".to_owned(),
        -1 => "Yesterday".to_owned(),
        _ if date.year() == today.year() => date.format("%-d %b").to_string(),
        _ => date.format("%-d %b %Y").to_string(),
    }
}

/// The day `days` after `day`, for the "Today", "Tomorrow" and "Next week"
/// due buttons.
pub fn add_days(day: &str, days: i64) -> Option<String> {
    parse_day(day)
        .and_then(|date| date.checked_add_signed(Duration::days(days)))
        .map(|date| date.format("%Y-%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-11-19"; // a Thursday

    #[test]
    fn a_missed_day_counts_the_days_it_is_late() {
        assert_eq!(
            due("2026-11-18", TODAY),
            Due {
                text: "1d late".to_owned(),
                late: true,
                bucket: Bucket::Overdue,
            }
        );
        assert_eq!(due("2026-11-14", TODAY).text, "5d late");
    }

    #[test]
    fn the_next_week_reads_as_a_word_then_a_weekday_then_a_date() {
        assert_eq!(due(TODAY, TODAY).text, "Today");
        assert_eq!(due("2026-11-20", TODAY).text, "Tomorrow");
        assert_eq!(due("2026-11-22", TODAY).text, "Sun");
        assert_eq!(due("2026-11-26", TODAY).text, "26 Nov");
    }

    #[test]
    fn no_date_and_a_bad_date_are_both_no_date() {
        assert_eq!(bucket(None, TODAY), Bucket::Someday);
        assert_eq!(bucket(Some("soon"), TODAY), Bucket::Someday);
        assert_eq!(bucket(Some("2026-11-19T09:00"), TODAY), Bucket::Someday);
        assert_eq!(bucket(Some(TODAY), TODAY), Bucket::Today);
        assert_eq!(bucket(Some("2026-11-20"), TODAY), Bucket::Upcoming);
        assert_eq!(bucket(Some("2026-11-01"), TODAY), Bucket::Overdue);
    }

    #[test]
    fn the_groups_have_titles_and_stable_keys() {
        let keys: std::collections::HashSet<_> = Bucket::ALL.iter().map(|b| b.key()).collect();
        assert_eq!(keys.len(), Bucket::ALL.len());
        for bucket in Bucket::ALL {
            assert!(!bucket.title().is_empty());
        }
    }

    #[test]
    fn the_archive_names_today_yesterday_and_older_days() {
        assert_eq!(day_label(TODAY, TODAY), "Today");
        assert_eq!(day_label("2026-11-18", TODAY), "Yesterday");
        assert_eq!(day_label("2026-11-03", TODAY), "3 Nov");
        assert_eq!(day_label("2025-12-31", TODAY), "31 Dec 2025");
        assert_eq!(day_label("not a day", TODAY), "not a day");
    }

    #[test]
    fn due_buttons_walk_forward_from_today() {
        assert_eq!(add_days(TODAY, 0).as_deref(), Some(TODAY));
        assert_eq!(add_days(TODAY, 1).as_deref(), Some("2026-11-20"));
        assert_eq!(add_days(TODAY, 7).as_deref(), Some("2026-11-26"));
        assert_eq!(add_days("2026-12-31", 1).as_deref(), Some("2027-01-01"));
        assert_eq!(add_days("nope", 1), None);
    }
}
