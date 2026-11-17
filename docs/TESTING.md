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
  list row carries a dirty dot, and a clean one does not. These assert `Action`s
  from `take_action()`; nothing in the test talks to a worker.
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
