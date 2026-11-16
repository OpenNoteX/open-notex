//! The scheduler's decision function.
//!
//! Given "now" and the pending reminders, return the one thing to do next:
//! fire something that is due, or arm a single timer. This is deliberately pure
//! so the whole scheduling model is testable without a clock or a window.

use std::cmp::min;

/// How far ahead the timer may be armed. Bounds the damage from clock jumps,
/// timezone changes and long sleeps.
pub const MAX_ARM_HORIZON_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderState {
    Pending,
    Fired,
    Acked,
    Missed,
    Cancelled,
}

impl ReminderState {
    pub const fn as_str(self) -> &'static str {
        match self {
            ReminderState::Pending => "pending",
            ReminderState::Fired => "fired",
            ReminderState::Acked => "acked",
            ReminderState::Missed => "missed",
            ReminderState::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(ReminderState::Pending),
            "fired" => Some(ReminderState::Fired),
            "acked" => Some(ReminderState::Acked),
            "missed" => Some(ReminderState::Missed),
            "cancelled" => Some(ReminderState::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledReminder {
    pub id: String,
    pub due_at: i64,
    pub snoozed_until: Option<i64>,
    pub state: ReminderState,
}

impl ScheduledReminder {
    pub fn pending(id: impl Into<String>, due_at: i64) -> Self {
        Self {
            id: id.into(),
            due_at,
            snoozed_until: None,
            state: ReminderState::Pending,
        }
    }

    pub fn effective_due(&self) -> i64 {
        self.snoozed_until.unwrap_or(self.due_at)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Persist a fire for this reminder, then notify.
    Fire { id: String, at: i64 },
    /// Arm the single timer for this instant.
    Arm { at: i64 },
}

/// The next thing to do, or `None` when nothing is pending.
///
/// Overdue reminders win; the caller loops until this returns `Arm` or `None`.
pub fn next_action(now: i64, reminders: &[ScheduledReminder]) -> Option<Action> {
    let mut next_due: Option<(i64, &ScheduledReminder)> = None;

    for reminder in reminders {
        if reminder.state != ReminderState::Pending {
            continue;
        }
        let due = reminder.effective_due();
        if due <= now {
            return Some(Action::Fire {
                id: reminder.id.clone(),
                at: due,
            });
        }
        match next_due {
            Some((best, _)) if best <= due => {}
            _ => next_due = Some((due, reminder)),
        }
    }

    next_due.map(|(due, _)| Action::Arm {
        at: min(due, now.saturating_add(MAX_ARM_HORIZON_MS)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> i64 {
        minutes * 60_000
    }

    #[test]
    fn nothing_pending_means_no_timer() {
        assert_eq!(next_action(at(0), &[]), None);
        let cancelled = ScheduledReminder {
            state: ReminderState::Cancelled,
            ..ScheduledReminder::pending("r1", at(10))
        };
        assert_eq!(next_action(at(0), &[cancelled]), None);
    }

    #[test]
    fn overdue_fires_and_future_arms() {
        let reminders = [
            ScheduledReminder::pending("late", at(-5)),
            ScheduledReminder::pending("soon", at(10)),
        ];
        assert_eq!(
            next_action(at(0), &reminders),
            Some(Action::Fire {
                id: "late".to_owned(),
                at: at(-5)
            })
        );

        let reminders = [ScheduledReminder::pending("soon", at(10))];
        assert_eq!(
            next_action(at(0), &reminders),
            Some(Action::Arm { at: at(10) })
        );
    }

    #[test]
    fn picks_the_earliest_and_honours_snooze() {
        let mut snoozed = ScheduledReminder::pending("snoozed", at(5));
        snoozed.snoozed_until = Some(at(30));
        let reminders = [snoozed, ScheduledReminder::pending("plain", at(20))];
        assert_eq!(
            next_action(at(0), &reminders),
            Some(Action::Arm { at: at(20) })
        );
    }

    #[test]
    fn the_horizon_bounds_how_far_ahead_the_timer_goes() {
        let reminders = [ScheduledReminder::pending("far", at(60 * 24 * 7))];
        assert_eq!(
            next_action(at(0), &reminders),
            Some(Action::Arm {
                at: MAX_ARM_HORIZON_MS
            })
        );
    }

    #[test]
    fn a_snooze_into_the_past_fires_immediately() {
        let mut reminder = ScheduledReminder::pending("r", at(0));
        reminder.snoozed_until = Some(at(-1));
        assert!(matches!(
            next_action(at(0), &[reminder]),
            Some(Action::Fire { .. })
        ));
    }
}
