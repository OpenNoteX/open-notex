# Architecture

Two threads, one writer, one renderer.

```
┌─────────────────────────────── apps/open-notex ────────────────────────────────┐
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

## The shell

`crates/notex-ui` owns every pixel. `State` holds the route, the two pane
widths, the load state and the vault path; `view()` turns that into a tree and
does nothing else — no I/O, no clock, no history. `apps/open-notex` supplies the
`AppSpec` and answers the keys the shell owns: `⌘K` (Search), `⌘,` (Settings)
and the sidebar toggle, which stays Rune's so it matches its own button and
tooltip.

- `Route` is one enum of ten screens, and it is the only source of the sidebar
  row, the toolbar title, the empty-state copy, the icon and the `--route`
  value. A screen cannot exist in one place and not the others.
- Search and Settings are single-pane; every other route is sidebar, list and
  detail, decided by `Route::has_detail`.
- The palette is three seeds — background, ink, accent — resolved by
  `Kit::from_palette_focus`. Nothing else is hand-picked, so light and dark
  cannot drift, and tests assert the accent and secondary text clear 3:1.
- `SCENES` is the snapshot contract: Today, an empty Inbox, Notes, Projects,
  Settings, Search, loading and failed. `cargo xtask snap` renders each through
  the real binary in both themes, then asserts the tree still has the sidebar,
  the toolbar and a nav row.

There is no store yet, so every route is `Load::Ready` with nothing in it. The
loading and failed states exist now because they are the hardest thing to
retrofit once a screen has content.

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
