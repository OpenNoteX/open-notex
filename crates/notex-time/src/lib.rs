//! Time logic for openNoteX: local day keys, recurrence expansion, streaks and
//! the scheduler's decisions.
//!
//! Everything here is pure. There is no clock, no I/O and no database, so the
//! whole crate is tested with table-driven cases and runs in milliseconds.

pub mod checklist;
pub mod day;
pub mod recurrence;
pub mod schedule;
pub mod streak;

pub use checklist::{Bucket, Due, add_days, bucket, day_label, due, parse_day};
pub use day::{
    DayPolicy, Stamp, TimeError, day_key, day_start_ms, local_day, local_stamp, parse_tz,
};
pub use recurrence::{Freq, Rule, weekday_bit};
pub use schedule::{Action, ReminderState, ScheduledReminder, next_action};
pub use streak::{Streak, compute_streak, is_scheduled};
