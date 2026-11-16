-- 0001_init: the M0 schema.
--
-- Note bodies are NOT stored here. `note_index` and `notes_fts` are a derived
-- cache over the Markdown vault and can be rebuilt with --reindex.

CREATE TABLE note_index(
    id         TEXT PRIMARY KEY,
    path       TEXT NOT NULL,
    hash       INTEGER NOT NULL,
    mtime_ns   INTEGER NOT NULL,
    size       INTEGER NOT NULL,
    title      TEXT NOT NULL,
    tags       TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    rev        INTEGER NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX note_index_path ON note_index(path);

CREATE VIRTUAL TABLE notes_fts USING fts5(
    item_id UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE lists(
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    color      TEXT,
    position   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER,
    rev        INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE recurrences(
    id               TEXT PRIMARY KEY,
    freq             TEXT NOT NULL,
    interval         INTEGER NOT NULL DEFAULT 1,
    weekdays         INTEGER NOT NULL DEFAULT 0,
    month_day        INTEGER,
    anchor_local_date TEXT NOT NULL,
    tz               TEXT NOT NULL,
    until_at         INTEGER,
    count            INTEGER,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    deleted_at       INTEGER,
    rev              INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE tasks(
    id            TEXT PRIMARY KEY,
    list_id       TEXT REFERENCES lists(id),
    parent_id     TEXT REFERENCES tasks(id),
    title         TEXT NOT NULL,
    notes         TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL DEFAULT 'open',
    priority      INTEGER NOT NULL DEFAULT 0,
    due_at        INTEGER,
    due_all_day   INTEGER NOT NULL DEFAULT 0,
    recurrence_id TEXT REFERENCES recurrences(id),
    completed_at  INTEGER,
    sort_key      TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    rev           INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX tasks_open_due ON tasks(status, due_at) WHERE deleted_at IS NULL;

CREATE TABLE habits(
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    icon             TEXT,
    cadence          TEXT NOT NULL DEFAULT 'daily',
    scheduled_days   INTEGER NOT NULL DEFAULT 127,
    target           INTEGER NOT NULL DEFAULT 1,
    day_start_minute INTEGER NOT NULL DEFAULT 240,
    freeze_allowance INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    deleted_at       INTEGER,
    rev              INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE habit_days(
    habit_id   TEXT NOT NULL REFERENCES habits(id),
    local_date TEXT NOT NULL,
    done_count INTEGER NOT NULL DEFAULT 1,
    note       TEXT,
    tz         TEXT NOT NULL,
    PRIMARY KEY(habit_id, local_date)
);

CREATE TABLE reminders(
    id             TEXT PRIMARY KEY,
    source_type    TEXT NOT NULL,
    source_id      TEXT,
    state          TEXT NOT NULL DEFAULT 'pending',
    due_at         INTEGER NOT NULL,
    recurrence_id  TEXT REFERENCES recurrences(id),
    snoozed_until  INTEGER,
    last_fired_for INTEGER,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    deleted_at     INTEGER,
    rev            INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX reminders_pending_due ON reminders(due_at) WHERE state = 'pending';

CREATE TABLE reminder_fires(
    reminder_id   TEXT NOT NULL REFERENCES reminders(id),
    occurrence_at INTEGER NOT NULL,
    fired_at      INTEGER NOT NULL,
    outcome       TEXT NOT NULL,
    PRIMARY KEY(reminder_id, occurrence_at)
);

CREATE TABLE tags(
    id    TEXT PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE,
    color TEXT
);

CREATE TABLE item_tags(
    tag_id    TEXT NOT NULL REFERENCES tags(id),
    item_type TEXT NOT NULL,
    item_id   TEXT NOT NULL,
    PRIMARY KEY(tag_id, item_type, item_id)
);

CREATE TABLE links(
    from_type TEXT NOT NULL,
    from_id   TEXT NOT NULL,
    to_type   TEXT NOT NULL,
    to_id     TEXT NOT NULL,
    PRIMARY KEY(from_type, from_id, to_type, to_id)
);

CREATE TABLE oplog(
    seq       INTEGER PRIMARY KEY AUTOINCREMENT,
    entity    TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    op        TEXT NOT NULL,
    payload   TEXT NOT NULL,
    at        INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    rev       INTEGER NOT NULL
);

CREATE TABLE meta(
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO meta(key, value) VALUES ('schema_origin', '0001_init');
