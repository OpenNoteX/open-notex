# Testing

One principle: push logic out of the UI into pure functions, then test the UI
only for wiring and appearance.

## Layer 1 — pure logic (largest share, no I/O, no clock)

| Module | Covered by |
| --- | --- |
| `notex-time::day` | day keys across day-start offsets, DST, timezones |
| `notex-time::recurrence` | daily/weekly/monthly, last-day, DST gap and overlap |
| `notex-time::streak` | scheduled vs skipped days, freezes, today-at-risk |
| `notex-time::schedule` | `next_action` picks the minimum due, honours snooze |
| `notex-core` | id and entity invariants |

## Layer 2 — store and vault (real SQLite, temp dirs)

- Migrations: open at version 0 and at every historical version, upgrade to
  HEAD, assert `user_version` and data preservation. `0002_fts_rowid` backfills
  the rowid of every existing search row, so a v1 database keeps its search.
- FTS: prefix, phrase, unicode, delete and update remove stale matches.
- Oplog: replay produces the same tables; `rev` strictly increases.
- Notes list: `notes()` sorts by title and parses tags; `note_path()` resolves an
  id and returns `None` for an unknown one.
- Reindex: `Batch` writes every note in one transaction, and a repeat scan of an
  unchanged vault writes nothing — same rows, no new oplog entries, same `rev`.
- Vault: atomic write leaves either the old or the new file, never a partial;
  frontmatter round-trips unknown keys; external edits are detected; duplicate
  ids surface as conflicts; `--reindex` restores identical index rows.
- Saving: `write_body` keeps `id`, tags and unknown frontmatter keys and adds
  none to a bare file; a save through the worker replaces the body, updates
  FTS, and keeps the id; a save from a stale hash is a conflict that writes
  nothing; `force` overwrites; a missing id or file is a typed `SaveFailed` and
  the worker still answers afterwards.
- Creating: a new note is a file with an id in its frontmatter and a `# Title`
  body before it is listed; a title already used gets `-2`, never a clobber.
- Deleting: the file leaves the vault for `trash/` with its body intact and the
  row goes; deleting a note whose file is already gone still removes the row;
  a reindex prunes rows whose file the scan did not find and logs each prune.
- Renaming: a save that changes the `# Title` moves the file to the title's slug,
  keeps the id, and the reply names the file that now holds the text.
- Watching: an edit made by another program is picked up by the worker's own
  timer without a command; a pass reports only the notes whose text changed, and
  names none when a file was only renamed; a file added outside the app is
  indexed with its frontmatter tags; a file deleted outside the app leaves the
  list; a quiet vault publishes nothing; a vault that is not there does not empty
  the index.
- Crash safety: kill the process during a write loop, reopen, `integrity_check`
  is `ok`.

## Layer 3 — headless UI (Rune `Runtime`)

```rust
let mut rt = Runtime::new(state, view, (1100.0, 720.0), env);
rt.frame(0.0);
let (x, y) = rt.find("complete-task").unwrap();
rt.pointer_down(x, y); rt.pointer_up(x, y);
rt.frame(1.0 / 60.0);
assert!(rt.state.tasks[0].done);
```

- Element-tree assertions (`--tree`): structure, roles, labels, focus order.
  These block merges.
- The Notes list asserts one row per note with its title and tags; clicking
  `list-note-{id}` selects it and opens it through the worker; the editor holds
  the file's Markdown; a failed read shows the message and can be dismissed.
- The editor: typing marks the draft `Edited` and offers Save; `⌘S` asks for a
  save now; autosave fires after the idle delay and not before; text typed while
  a save is in flight stays dirty; a dirty switch waits for the ack; a conflict
  shows the banner, blocks the automatic retry and its Overwrite sends
  `force: true` for that one save only; a failed save keeps the text and Try again sends it again; the
  list row carries a dirty dot, and a clean one does not. New note (`⌘N` and the
  list's plus) and delete (`⌘⌫`, the trash button) queue behind a dirty draft and
  are dropped if that save is refused; delete asks in a dialog and does nothing
  until `delete-confirm`; a refused create is explained under the list header.
  These assert `Action`s from `take_action()`; nothing in the test talks to a
  worker.
- The vault dialog: the status line and primary label for a missing, empty,
  counted and unusable folder; `Not now` leaves the shell open with "No vault
  yet", and both its button and Settings → Vault reopen the same dialog; `⌘N`
  without a vault opens it instead of raising an error; a scripted folder answer
  becomes `VaultIntent::Use` without a second press; a refusal keeps the dialog
  with the reason and the path. `take_vault_intent()` is asserted, not
  `take_action()`, because opening a vault replaces the session.
- Home and Tasks: the clock block reads the scene's fixed time and date; the
  Todo block holds overdue and today's rows while Next up holds later ones;
  Recent notes is ordered by file mtime; the sidebar counts open tasks and has
  no Inbox/Today/Upcoming/Projects row. The checklist groups `Overdue / Today /
  Upcoming / No date`, the chips filter and add into the project they show,
  `Return` in the add row asks for a task and keeps the field, clicking a title
  renames in place (Escape cancels), completing shows the undo toast and its
  Undo reopens the task, the eight-second expiry is counted on the virtual
  clock, deleting is one press and a restore, the expanded row sets a due day
  and a project, and the archive groups done tasks by the day they were done.
  A project chip's `⋯` opens Rename… and Delete project; a rename is a field on
  the chip (a refused name stays open with the reason), and a delete releases
  its tasks to No project, drops the filter to All and offers the same undo.
- PNG snapshots (`--snap`): attached for human review, not pixel-diffed.
- Every interactive element must have a role and a non-empty label; a test fails
  otherwise.
- Reminder timing uses the virtual clock, so a 09:00 reminder test runs in
  microseconds and a three-day-old reminder yields a digest, not three
  notifications.

## Layer 4 — process level

- Launch the real binary with `--script` and `--snap`; assert first-frame
  readiness and persistence across restart.
- `apps/open-notex/tests/notes.rs` drives a real `Session` against a temp vault:
  reindex lists both notes, selecting one returns its body, clicking it again
  does not re-read the file, an edit passes the idle delay and the Markdown file
  changes on disk with its frontmatter intact, a file changed underneath the
  editor is a conflict that leaves the other edit alone until Overwrite, the
  close hook writes a dirty draft without waiting for the idle delay, a file
  deleted under the editor fails the close with "no longer in the vault" and
  keeps the text, a missing vault becomes `Load::Failed`, and a database inside
  the vault is refused. A recovery file is written as a valid note with its id.
  A new note is a real `.md` before its row exists; deleting the open note moves
  the file to the trash and a reindex agrees; renaming the title moves the file
  and keeps the id and the frontmatter. A file edited, added or deleted by
  another program reaches the open editor and the list while the app runs, with
  no command from the test: the worker's timer is what notices.
- `apps/open-notex/tests/tasks.rs` covers tasks through the real worker and
  database: a line in the add row becomes a row and a project; `#Launch` and
  `#launch` are one project; a due day is written as a local date and its group
  follows it; completing moves the task to the archive and a second launch on
  the same database reads it back as done; a delete and its undo restore the
  row, and letting the toast pass leaves it gone; a rename, a move to no
  project and a task note all come back from a relist; renaming a project
  renames its tasks' filing, and deleting it keeps the tasks, unfiles them, and
  undo refiles them into the restored project.
- `apps/open-notex/tests/vault.rs` covers the launch and the dialog without a
  window: `--vault` aside, the saved folder wins over the suggested one, a saved
  folder that is gone asks and is never replaced by the default, a broken config
  asks instead of guessing, the choice round-trips through `config.json` beside
  the database, `inspect` names missing/empty/counted/file/read-only/inside-app
  folders, `prepare` creates only when asked, and `open_vault` creates and opens
  a folder end to end (the first `⌘N` writes a real `.md`), indexes an existing
  folder's notes, and refuses a read-only or app-data folder without remembering
  it.
- `lsof -iTCP -sTCP:LISTEN` on a running build must be empty.
- Notification permission denied: the app degrades to in-app toasts and still
  records the fire.
- Clock jump forward three hours: reminders fire once, no duplicates.

## Layer 5 — budgets

Measured with `cargo xtask bench` at 1k / 10k / 50k items, gated by
`docs/PERFORMANCE.md`. Soft-fail locally, tracked in CI.

## Commands

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p notex-time -- --nocapture
cargo test -p notex-ui            # headless shell tests
cargo xtask snap                  # PNG + element tree for every scene
cargo xtask seed --count 10000 --dir /tmp/notex-bench
cargo xtask bench --db /tmp/notex-bench/bench.db
```

## Storyboards

A still strip shows a sequence of states without a video encoder. `cargo run -p
open-notex -- …` has Rune's tooling in a debug build:

```sh
cargo run -p open-notex -- --script docs/storyboards/nav.json \
    --strip artifacts/nav-light.png --every 24 --cols 2 --fps 30 \
    --duration 6 --theme light --scale 1
```

`docs/storyboards/nav-light.png` is that command's output: a sidebar row, Search,
Settings, then the sidebar hidden, one beat apart. `--every 24` samples it at
0.8 s at 30 fps, so no tile lands mid-animation. Change the script to show a
different sequence; the PNGs and trees from `xtask snap` remain the review set.

## CI

macOS for the app, Linux for the pure crates. `fmt`, `clippy -D warnings`,
tests, and snapshot artifacts on every pull request.
