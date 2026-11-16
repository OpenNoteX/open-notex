# Architecture

Two threads, one writer, one renderer.

```
┌──────────────────────────── applications/open-notex ────────────────────────────┐
│ AppSpec::setup / poll / on_key / on_open_url / on_reopen                        │
│                                                                                 │
│   ┌──────────────┐   Cmd (mpsc)   ┌───────────────────────────────────────┐     │
│   │ notex-ui     │ ─────────────▶ │ DB worker thread                      │     │
│   │ view(s, cx)  │ ◀───────────── │ rusqlite (WAL) + vault file I/O       │     │
│   └──────┬───────┘  Evt + Waker   └───────────────────────────────────────┘     │
│          │                                                                      │
│          ▼                                                                      │
│   diff → Taffy layout → scene → one GPU draw call                               │
└─────────────────────────────────────────────────────────────────────────────────┘
        ▲                                        ▲
        │ OS events (winit)                      │ notify (FSEvents) → indexer
```

## The render loop

```
OS event → handler mutates &mut State → view(&State) → diff → layout → draw
```

- The UI is a pure function of state. Handlers never touch widgets.
- Rune re-runs `view()` on every event, so `view()` must not perform I/O, query
  the database, read files, or call the clock in a loop.
- Hover and press retarget animation tracks without re-running the view.
- When nothing changes, `poll()` returns `false` and Rune draws nothing: idle CPU
  is zero.

## Data ownership

| Data | Owner | Rebuildable |
| --- | --- | --- |
| Note body | `.md` file in the vault | never (it is the original) |
| Note index (path, hash, title, tags, mtime) | SQLite | yes, `--reindex` |
| Full-text search | SQLite FTS5 | yes, `--reindex` |
| Tasks, habits, habit days, reminders, recurrences, tags, links | SQLite | from oplog/export only |
| Oplog | SQLite | append-only, never rewritten |
| Settings | SQLite `meta` | defaults |

The database lives in the platform data directory, never inside the vault.

## The worker contract

```rust
pub enum Cmd { SaveNote(..), CompleteTask(..), CheckHabit(..), LoadToday(..), ReindexAll(..) }
pub enum Evt { NoteSaved(..), Today(..), Progress { .. }, Failed { .. } }
```

- One consumer thread owns the only `rusqlite::Connection`; commands are
  serialized by construction, so there is no `SQLITE_BUSY` and no lock ordering.
- Replies carry authoritative rows, so the UI replaces its cache instead of
  re-querying.
- `Waker::wake()` tells the UI there is work; `poll()` drains the queue. No
  polling loops, no sleeps, no timers other than the scheduler's.
- The UI updates optimistically, then reconciles with the reply. A failed
  command reverts the optimistic row and raises a toast.
- Bulk jobs (`ReindexAll`, import) are one command that emits `Progress` and can
  be cancelled by request id.

## Time

- Timestamps are UTC epoch milliseconds.
- A "day" is a local `YYYY-MM-DD` computed with `chrono` + `chrono-tz` from a
  timestamp, a timezone and a configurable day start (default 04:00) so a 1 a.m.
  completion still belongs to the previous day.
- All day and recurrence logic is pure and lives in `notex-time`.

## Scheduler

One timer at a time, armed with Rune's `tx.wake_after`. See
[`SCHEDULING.md`](SCHEDULING.md). Deadlines are on Rune's clock, which is virtual
in headless runs, so reminder tests need no sleeping.

## No server

The process opens no listening socket, starts no child process and makes no
network call. Communication is in-process channels plus optional OS handoffs:
`rune::single_instance` (named pipe), the `opennotex://` URL scheme, and the
notification callback. `lsof` on a running build must show no listeners.
