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
pub enum Cmd {
    Ping, Reindex { root }, ListNotes, OpenNote { id },
    SaveNote { id, body, base_hash, token, force },
    CreateNote { title }, DeleteNote { id }, Shutdown,
}
pub enum Evt {
    Pong, Indexed { count }, Notes(Vec<NoteSummary>),
    Created { id, body, hash }, Deleted { id },
    CreateFailed { err }, DeleteFailed { id, err },
    Opened { id, body, hash }, OpenFailed { id, err },
    Saved { id, token, hash, title, tags },
    SaveConflict { id, token }, SaveFailed { id, token, err },
    Failed { op, err },
}
```

- One consumer thread owns the only `rusqlite::Connection`; commands are
  serialized by construction, so there is no `SQLITE_BUSY` and no lock ordering.
- Replies carry authoritative rows, so the UI replaces its cache instead of
  re-querying.
- A save names a note by id and carries the file hash it was read from. A
  different hash on disk is `SaveConflict` and the file is untouched; `force`
  is the user's explicit overwrite. The reply echoes the editor's `token`, so
  text typed while the save was in flight stays dirty.
- `Waker::wake()` tells the UI there is work; `poll()` drains the queue. No
  polling loops, no sleeps, no timers other than the scheduler's.
- The UI updates optimistically, then reconciles with the reply. A failed
  command reverts the optimistic row and raises a toast.
- Bulk jobs (`Reindex`, import) are one command that emits `Progress` and can
  be cancelled by request id.
- `CreateNote` writes the file (fresh id in frontmatter, `# Title` body) before
  indexing it and replies with the file as read back. `DeleteNote` moves the
  file to the trash beside the database, then removes the row; a file already
  missing is still removed from the index. A reindex prunes rows whose file the
  scan did not find, and each prune writes its oplog row, so an external delete
  leaves the list on the next launch.
- A save whose `# Title` line changed moves the file to that title's slug in the
  same save, keeping its id; a file named by hand keeps its name until its title
  changes. A slug already in use gets `-2`, never an overwrite.

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
- `SCENES` is the snapshot contract: Today, an empty Inbox, Notes (empty and
  with an open note), Projects, Settings, Search, loading and failed.
  `cargo xtask snap` renders each through the real binary in both themes, then
  asserts the tree still has the sidebar, the toolbar and a nav row.

### The store behind it

`apps/open-notex` is the only place that connects the shell to storage. `setup`
runs once on the UI thread: it resolves
`~/Library/Application Support/openNoteX/notex.db`, refuses a database inside
the vault, spawns the one worker thread with the `Waker` and sends `Reindex`.
`poll` drains `StoreHandle::drain()` into `State` and returns whether the view
changed, so an idle app draws nothing.

- The list is `Evt::Notes`: id, title and tags, alphabetical by title. Bodies
  are never in the index: `Cmd::OpenNote` names an id, the worker looks up the
  path and reads the Markdown file. `Evt::Opened` carries the file hash the
  editor sends back on save. For writes, file first, index second still holds.
- Handlers only mutate `State`. The editor queues `Action::Open`,
  `Action::Save`, `Action::Create` and `Action::Delete`; `poll` drains them into
  commands, so the view never talks to the worker. A body that arrives after the
  selection moved is dropped, not raced.
- New note (`⌘N`, the list's plus) and delete (`⌘⌫`, the detail pane's trash)
  are no different: they wait behind a dirty draft's save like a row switch, and
  a refused save drops them rather than taking the text with it. Delete asks in a
  dialog first, because the file leaves the vault.
- A missing vault, a failed reindex or a database inside the vault becomes
  `Load::Failed` with a dismissible message; the window still opens.
- Headless runs (`--snap`, `--scene`, the headless tests) skip `setup` entirely,
  so a snapshot never reads a real vault and `xtask snap` stays deterministic.
  The integration tests own a `Session` directly and drive `pump()` against a
  temp vault.

### The editor

The detail pane edits the Markdown file itself, frontmatter excluded, so the
`# Title` line is the rename control and there is no second preview mode.
Changing that line and saving renames the file to the title's slug, keeping the
note's id; a name already in use gets a `-2`, never a clobber.

- `Draft` holds the `TextEdit`, the base hash, the token and two clocks. An
  edit bumps the token; a save records the token it wrote. Dirty is
  `token > acked`, so a save that lands after more typing cannot mark newer text
  clean.
- Autosave is `poll`-driven: 700 ms after the last keystroke, or 5 s into a
  continuous run. `State::next_wake` tells `poll` when to sleep until; a clean or
  blocked draft schedules no wake, so an idle app stays idle.
- A save with a stale base hash becomes a conflict banner with Reload and
  Overwrite; autosave stops until the user chooses. Overwrite is `force: true`
  on one save, so a failed overwrite and any later save ask the guard again. A
  failed save keeps the draft and offers Try again.
- Selecting another note while dirty queues the open behind the save. `⌘S`, the
  Save button and Retry mean "write now"; a refusal does not block a later
  request, while a conflict waits for Reload or Overwrite.
- Closing the window keeps the app and its state (`keep_running`), releasing the
  GPU while it is closed; `⌘Q` ends the process. Both hooks flush a dirty draft
  and wait up to 750 ms for the file. Rune cannot cancel a quit, so a draft the
  disk still refuses is written to `~/Library/Application Support/openNoteX/
  recovery/` as a note with its id, and the path goes to stderr.

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

## Packaging

`cargo xtask bundle` is the only thing that turns a build into a `.app`, and it
uses the tools a Mac already has: `codesign` (ad-hoc until there is a Developer
ID), `hdiutil` for the disk image, `ditto` for the zip, `shasum` for the
checksums. The bundle id and the version in `Info.plist` come from
`xtask/src/bundle.rs` and the workspace version, so a release cannot disagree
with the crate it was built from, and `--version` answers before Rune starts so
a downloaded build says which commit it is.

The app is Apple Silicon only: the bundler reads the binary's Mach-O header and
refuses anything else with the command that fixes it, instead of shipping a
`.dmg` that dies on launch. Updates ship as a zip rather than a disk image,
because a running bundle can be replaced by an unpacked zip and a mounted image
cannot. `bundle --check` is the only proof that matters: it launches the app
from inside the bundle, with a `HOME` of its own, and waits for Rune's ready
receipt to name `com.opennote.opennotex`.

## No server

The process opens no listening socket, starts no child process and makes no
network call. Communication is in-process channels plus optional OS handoffs:
`rune::single_instance` (named pipe), the `opennotex://` URL scheme, and the
notification callback. `lsof` on a running build must show no listeners.
