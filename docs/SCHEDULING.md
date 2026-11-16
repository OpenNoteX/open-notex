# Scheduling

Reminders are not cron. Cron is a daemon that matches wall-clock minutes against
a spec. We need per-item state, catch-up, snooze, timezones and dedupe, so the
rule is data and the scheduler is one timer.

## Model

- Each reminder stores `due_at`: the **next** occurrence, materialized as UTC
  milliseconds. Recurrence rules are expanded one occurrence at a time; we never
  pre-insert future rows.
- `reminder_fires(reminder_id, occurrence_at)` is the idempotency ledger. The
  primary key is what makes firing exactly-once across restarts.
- `snoozed_until` overrides `due_at` for scheduling without disturbing the rule.

## Invariant: one timer

```
effective_due(r) = r.snoozed_until ?? r.due_at

poll(now):
    while let Some(r) = due(now):        # catch-up: everything overdue
        fire(r)
    arm(min(next_due(now), now + 1h))
```

`arm` is Rune's `tx.wake_after`. Deadlines coalesce, the shell sleeps without a
timer thread, and only due work wakes the app. The one-hour clamp bounds the
damage from clock jumps, timezone changes and long sleeps.

## Firing

```
persist fire row  →  notify  →  advance due_at to the next occurrence  →  arm
```

Persist first, notify second. A crash in between leaves a `fired` row with no
click, which the launch reconciliation shows as part of the "while you were
away" digest. Notifying first could double-notify.

## States

```
                 ┌──────── snooze ────────┐
                 ▼                        │
 pending ──due──▶ fired ──ack──▶ acked    │
    │               │                     │
    │               └──grace elapsed──▶ missed
    └──deleted──▶ cancelled
```

## Edge cases

| Situation | Behavior |
| --- | --- |
| Asleep at 09:00, wakes at 11:00 | Fires with a "2 h late" marker, then advances |
| App quit at 09:00, opened at 14:00 | No notification; digest of missed items |
| Spring-forward gap (02:30 does not exist) | Shift to the first valid instant; tested |
| Fall-back overlap (02:30 twice) | Fire once at the first; ledger blocks the second |
| Timezone change | Rule keeps its local wall-clock time; next occurrence recomputed |
| System clock jump | One-hour clamp forces re-evaluation |
| Notification permission denied | In-app toast plus badge; the fire is still recorded |
| Duplicate poll | Ledger rejects the second insert; notify is never reached |

## Testing

`next_action(now, reminders)` is pure and covers most cases without a clock.
Headless runs use Rune's virtual clock, so "fires at 09:00" is a microsecond
test rather than a sleep.
