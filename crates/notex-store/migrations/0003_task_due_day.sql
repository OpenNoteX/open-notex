-- 0003_task_due_day: an all-day task's date is a local calendar date.
--
-- `due_at` is a UTC instant, which is right for a task that is due at 09:00.
-- An all-day task is due on a *day*, and a timestamp cannot hold one: set it in
-- IST, travel to PST, and local midnight is the previous day. Store the date as
-- `YYYY-MM-DD` and leave `due_at` NULL. Timed tasks keep using `due_at`.

ALTER TABLE tasks ADD COLUMN due_day TEXT;

CREATE INDEX tasks_open_day ON tasks(status, due_day) WHERE deleted_at IS NULL;
