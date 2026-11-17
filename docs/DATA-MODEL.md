# Data model

## Note bodies are files

```
<vault>/
  Weekly review.md
  Ideas/Product notes.md
```

Frontmatter carries identity; everything else is derived from the file.

```markdown
---
id: 01J9Z6Q8K3V4Y7M2N5P8R1T6WX
tags: [work, review]
---
# Weekly review
```

- `id` is the only required key. Unknown keys are preserved byte-for-byte on
  save. Missing `id` means "adopt this file": generate one and write it back.
- The title is the first `# H1`, else the filename stem.
- Filename and folder are presentation; the `id` survives renames and moves.
- Duplicate `id` (a sync conflict copy) is a conflict to resolve, never a
  second note.

## SQLite

```sql
-- derived from the vault
CREATE TABLE note_index(
  id TEXT PRIMARY KEY,
  path TEXT NOT NULL,
  hash INTEGER NOT NULL,
  mtime_ns INTEGER NOT NULL,
  size INTEGER NOT NULL,
  title TEXT NOT NULL,
  tags TEXT NOT NULL,            -- JSON array
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  rev INTEGER NOT NULL DEFAULT 1,
  fts_rowid INTEGER              -- migration 0002; the note's notes_fts rowid
);
CREATE UNIQUE INDEX note_index_path ON note_index(path);

CREATE VIRTUAL TABLE notes_fts USING fts5(
  item_id UNINDEXED, title, body, tokenize = 'unicode61'
);
```

`item_id` is `UNINDEXED`, so it cannot be used to find a row: deleting a note's
search row by `item_id` scans the whole FTS table. `fts_rowid` remembers FTS5's
own key and every delete goes through it. Migration `0002` backfills it from the
existing search rows.

-- structured, authoritative in SQLite
CREATE TABLE lists(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, color TEXT,
  position INTEGER NOT NULL, created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL, deleted_at INTEGER, rev INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE tasks(
  id TEXT PRIMARY KEY,
  list_id TEXT REFERENCES lists(id),
  parent_id TEXT REFERENCES tasks(id),
  title TEXT NOT NULL,
  notes TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'open',      -- open | done | cancelled
  priority INTEGER NOT NULL DEFAULT 0,
  due_at INTEGER,
  due_all_day INTEGER NOT NULL DEFAULT 0,
  recurrence_id TEXT REFERENCES recurrences(id),
  completed_at INTEGER,
  sort_key TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  deleted_at INTEGER, rev INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX tasks_open_due ON tasks(status, due_at) WHERE deleted_at IS NULL;

CREATE TABLE habits(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, icon TEXT,
  cadence TEXT NOT NULL DEFAULT 'daily',    -- daily | weekly
  scheduled_days INTEGER NOT NULL DEFAULT 127,  -- bitmask, Mon = 1
  target INTEGER NOT NULL DEFAULT 1,
  day_start_minute INTEGER NOT NULL DEFAULT 240,
  freeze_allowance INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  deleted_at INTEGER, rev INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE habit_days(
  habit_id TEXT NOT NULL REFERENCES habits(id),
  local_date TEXT NOT NULL,                  -- YYYY-MM-DD
  done_count INTEGER NOT NULL DEFAULT 1,
  note TEXT,
  tz TEXT NOT NULL,                          -- zone used when recording
  PRIMARY KEY(habit_id, local_date)
);

CREATE TABLE recurrences(
  id TEXT PRIMARY KEY,
  freq TEXT NOT NULL,                        -- daily | weekly | monthly
  interval INTEGER NOT NULL DEFAULT 1,
  weekdays INTEGER NOT NULL DEFAULT 0,       -- bitmask for weekly
  month_day INTEGER,                         -- for monthly; -1 = last day
  anchor_local_date TEXT NOT NULL,
  tz TEXT NOT NULL,
  until_at INTEGER, count INTEGER,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  deleted_at INTEGER, rev INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE reminders(
  id TEXT PRIMARY KEY,
  source_type TEXT NOT NULL,                 -- task | habit | note | standalone
  source_id TEXT,
  state TEXT NOT NULL DEFAULT 'pending',     -- pending | fired | acked | missed | cancelled
  due_at INTEGER NOT NULL,
  recurrence_id TEXT REFERENCES recurrences(id),
  snoozed_until INTEGER,
  last_fired_for INTEGER,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  deleted_at INTEGER, rev INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX reminders_pending_due ON reminders(due_at) WHERE state = 'pending';

-- idempotency ledger: one row per scheduled occurrence
CREATE TABLE reminder_fires(
  reminder_id TEXT NOT NULL REFERENCES reminders(id),
  occurrence_at INTEGER NOT NULL,
  fired_at INTEGER NOT NULL,
  outcome TEXT NOT NULL,                     -- notified | missed | suppressed | clicked
  PRIMARY KEY(reminder_id, occurrence_at)
);

CREATE TABLE tags(id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, color TEXT);
CREATE TABLE item_tags(
  tag_id TEXT NOT NULL REFERENCES tags(id),
  item_type TEXT NOT NULL, item_id TEXT NOT NULL,
  PRIMARY KEY(tag_id, item_type, item_id)
);

CREATE TABLE links(
  from_type TEXT NOT NULL, from_id TEXT NOT NULL,
  to_type TEXT NOT NULL, to_id TEXT NOT NULL,
  PRIMARY KEY(from_type, from_id, to_type, to_id)
);

CREATE TABLE oplog(
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  entity TEXT NOT NULL, entity_id TEXT NOT NULL,
  op TEXT NOT NULL, payload TEXT NOT NULL,
  at INTEGER NOT NULL, device_id TEXT NOT NULL, rev INTEGER NOT NULL
);

CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
```

## Invariants

1. Every mutation appends an `oplog` row **in the same transaction**.
2. Soft delete only (`deleted_at`); every read filters it.
3. `rev` increases by exactly one per mutation of that row.
4. FTS rows are written in the same transaction as the index row.
5. A note body is never written only to SQLite.
6. `habit_days.primary key` makes checking the same habit twice in one day
   idempotent; a second check increments `done_count`.
7. `reminder_fires.primary key` makes firing idempotent across restarts, clock
   jumps and duplicate polls.

## Pragmas

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -8000;        -- 8 MiB
PRAGMA temp_store = MEMORY;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA journal_size_limit = 8388608;
```

Migrations are forward-only, embedded with `include_str!`, numbered
`NNNN_name.sql`, and applied inside one transaction each, tracked by
`PRAGMA user_version`.

## Backups and recovery

- Daily `VACUUM INTO` snapshot, seven kept.
- Notes are already recoverable from the vault; `--reindex` reconstructs search
  and metadata.
- A corrupt database is a typed error that offers a rebuild, never a panic.
