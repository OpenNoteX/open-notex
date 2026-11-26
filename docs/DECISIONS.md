# Decisions

Numbered, dated, with the reason and the condition that would reopen them.
Add an entry before adding a dependency.

## 0001 — Rune as the UI renderer

Native GPU rendering, no webview, no JS runtime, no IPC to a second process.
Rune is pre-release, so it is pinned by git revision and upgraded in its own
commit with snapshots regenerated. *Reopen if:* Rune stalls on the platforms we
need, or an upgrade blocks us for more than a milestone.

## 0002 — Markdown files are the source of truth for note bodies

A single `.db` is a poor cloud-sync unit: WAL pages, binary conflicts, no diffs.
One file per note syncs independently, is readable by any editor, and gives
history through git. SQLite becomes a rebuildable index. *Reopen if:* the
watcher/indexer proves unable to stay consistent, or a feature needs a document
format files cannot express.

## 0003 — SQLite for structured data, with oplog scaffolding

Tasks, habits, reminders and settings are small and relational, and need
transactions. `rev`, `deleted_at` and an append-only `oplog` cost almost nothing
now and make future sync a new module rather than a migration. No sync code
ships in v1. *Reopen if:* multi-device becomes a v1 requirement.

## 0004 — One DB worker thread

The UI state must be `Send + Sync`; `rusqlite::Connection` is not `Sync`. SQLite
is single-writer anyway, and file I/O must never sit between `view()` and draw.
Commands in, events out, `Waker` to wake the loop. *Reopen if:* the command
surface becomes a bottleneck, not before.

## 0005 — No server, no async runtime, no network

The process opens no listening socket and makes no outbound connection. Rune is
event-driven, so async buys nothing; threads and channels are simpler and leaner.
*Reopen if:* sync ships, and then as an explicit, optional module.

## 0006 — macOS first

One platform at a time: `.app` bundle, `UNUserNotificationCenter`, notarized
`.dmg`. Windows and Linux follow after M6. *Reopen at* M6.

## 0007 — Tray-resident, not a background daemon

`keep_running()` plus a tray item keeps reminders working while the window is
closed. Delivery while the process is quit is out of scope; missed reminders
produce a digest at next launch. *Reopen if:* users report missed reminders as
the top complaint.

## 0008 — chrono + chrono-tz for time

Ivy League timezone database, correct DST handling, stable API. All day and
recurrence math stays in `notex-time` so it can be property-tested without a
clock. *Reopen if:* we need features chrono cannot express.

## 0009 — Day boundary at 04:00, configurable

A completion at 01:00 belongs to the previous day, matching how people talk
about "yesterday". Stored per habit so it can be changed later without rewriting
history. *Reopen if:* users want a midnight boundary; it is a setting.

## 0010 — Budgets are merge gates

Numbers in `PERFORMANCE.md` are enforced in CI, not aspirations. *Reopen only by*
editing the budget table in its own commit with a measurement attached.

## 0011 — Rune is pinned by revision, and its patched dependencies are vendored

Rune has no release cadence, so both `rune` and `rune-kit` come from git at
`631e8a6` (`dev`) and are upgraded in a single commit with the snapshots
regenerated. Rune's own `[patch.crates-io]` covers `parley_engine` (bidi rule
L1, cached shaping), `winit` (Windows taskbar and window placement) and
`wgpu-hal` (adapter exposure, partial presents); a dependency's patch does not
apply to a workspace that depends on it, so those three sources are copied
verbatim into `vendor/` and patched here too. `vendor/README.md` records the
provenance and the rule against mixing revisions. *Reopen if:* Rune publishes
crates with the patches merged, or its toolchain requirement moves past our
pinned Rust.

Rune depends on `tokio` directly (with `wasm-bindgen-futures` and `zbus` behind
it), so it is in the binary. Our code builds no runtime: threads and channels
remain the only concurrency, per 0005.

## 0012 — A deleted note's file goes to the app's trash, not a vault folder

Deleting a note moves its Markdown file to `~/Library/Application Support/
openNoteX/trash/` (beside the database) and then drops its index row. A trash
folder inside the vault would break the rule that every `.md` under the vault is
a note: the next reindex would adopt the deleted files back into the list. The
move happens before the index write, so a crash between them costs a reindex
(the row is pruned), never the body; a file already gone from the disk is
treated as deleted. *Reopen if:* we ship an in-app trash with restore, which
would need the index to keep the row and the reason.

## 0013 — A download is ad-hoc signed, and the installer is the supported path

Notarization needs a Developer ID certificate, which needs a paid Apple
Developer account we do not have yet. Apple Silicon refuses an unsigned binary
outright, so every release is at least ad-hoc signed (`codesign -s -`); the
alternative — shipping nothing installable until the account exists — costs
more than it buys. Ad-hoc has two consequences, both handled rather than
hidden: macOS refuses an app that arrived with `com.apple.quarantine`, and each
build has a different ad-hoc identity, so anything bound to code signing starts
over on an update. `scripts/install.sh` verifies the checksum and clears the
quarantine flag, which is why it, not the `.dmg`, is what `RELEASING.md` tells
people to run; nothing today depends on a stable signing identity.
`cargo xtask bundle --sign` already produces a hardened, timestamped Developer
ID build, so M6b is a secrets change plus notarization, not a rewrite. *Reopen
when* the account exists, which is also the condition for M6c (in-place update
keeps permission grants only if the identity is stable).

## 0014 — The update check is asked for, and it is one child process

A notes app that phones home on launch is not local-first, so nothing leaves the
machine until someone presses **Check now** in Settings. Then the app runs `curl`
once against `api.github.com/repos/OpenNoteX/open-notex/releases/latest`, reads
`tag_name`, and compares it with the version it was built with — numerically, in
`notex_core::update`, because `0.10.0` is newer than `0.9.0` and a string compare
hides every tenth minor release. No HTTP crate, no async runtime, no telemetry: a
refused or failed check is a sentence in a row and the app is otherwise
unchanged. `curl` and `open` (the release page) are the only child processes this
process starts, both are on every macOS, and neither is a dependency in
`Cargo.toml`. Drafts are invisible to the releases API, so an unpublished build
is never offered to anyone. *Reopen when* M6c installs updates: that makes a
check automatic enough to be useful, and a downloaded artifact has to be
verified rather than handed to the browser.
## 0015 — The vault is watched by polling the worker, not by FSEvents

External edits have to reach the index, and macOS has FSEvents for exactly that.
It is not used here: FSEvents needs `libc` calls and `unsafe` (which this
workspace forbids) or a watcher crate, and a crate is a dependency with a
maintenance cost for a feature that is one metadata comparison. So the worker,
which already owns the index and the vault path, looks at the tree on an absolute
two-second deadline: a walk and a `stat` per file, compared with `path`,
`mtime_ns` and `size` in `note_index`. A quiet vault reads nothing and publishes
nothing; a vault that moved is reindexed in the same transaction as before. 36 ms
per pass at 10k notes, on the worker thread, is the price (see
`PERFORMANCE.md`). The trade is latency for a dependency: an outside edit shows
up within two seconds, not within milliseconds, and the app wakes at most twice a
second while a vault is open. *Reopen when* the filesystem is the bottleneck —
a 50k vault, or a pass that competes with a save — at which point FSEvents earns
its dependency and its `unsafe` exemption.

## 0016 — The vault is chosen once and remembered beside the database

A notes app whose first frame is an error is not usable, so launch resolves a
vault before the window draws and asks in one dialog when there is none. The
choice is written to `~/Library/Application Support/openNoteX/config.json` —
with the database and the trash, never inside the vault, where it would be
indexed and would leave with the folder. `--vault` outranks the file for one run
and is never persisted: a developer override must not become the next launch's
vault. A saved path that is gone is not replaced by the default; the dialog
keeps it and names the likely cause, because an unplugged drive must not look
like a folder someone moved. Markdown files stay the source of truth: the config
holds one path, and losing it costs one choice, not a note. *Reopen when*
changing a working vault lands: that flow also has to reindex the new folder and
prune the old rows, and it belongs with its own tests.

## 0017 — The folder picker is the system panel, through Rune's native-file-dialog

"Choose…" has to be the real macOS folder sheet: a hand-drawn file browser would
be a second, worse Finder and would need its own permission handling. Rune
already wraps `rfd`, so the app enables the `native-file-dialog` feature instead
of adding a dialog crate of its own — that puts `rfd` (and `dbus` on Linux) in
the lockfile, which is why this is an entry and not a Cargo line. The system
panel brings its own **New Folder** button, so there is no save-panel flow and no
second way to create a vault. Headless runs never touch it: the runtime answers
with a scripted path, so tests and snapshots stay display-free. *Reopen when* the
app targets Windows or Linux: `rfd` covers both, but the sheet's behaviour and
permissions are then a platform test, not a macOS one.

## 0018 — Tasks are one checklist, and a due day is a date

The first sketch had four screens (Inbox, Today, Upcoming, Projects) and a board
with stages. It made a person choose which inbox a task lived in before writing
it, and it made "what do I do now" a tour of four lists. It is replaced by one
checklist grouped by when a thing is due — `Overdue`, `Today`, `Upcoming`, `No
date` — with projects demoted to a filter chip. There is no stage, no drag order
and no carried-days field: a task missed on Monday is in `Overdue` on Tuesday
because its due day is in the past, not because anything rewrote the row.

A due date is `due_day`, a local `YYYY-MM-DD`, not a timestamp. An all-day task
is a calendar fact, and storing midnight as a UTC instant makes it move when a
person travels or the clock changes. Sorting, grouping and the words on the chip
("2d late", "Fri", "23 Nov") all live in `notex_time::checklist`, which is pure
and takes `today` as an argument, so tests and snapshots never read a clock.
`due_at` stays NULL until a task has a time of day; that milestone will add the
hours, not reinterpret this column.

Task rows are structured data in SQLite, like habits and reminders will be —
migrations and the oplog, not Markdown. Note *bodies* stay Markdown files, which
is the invariant that matters: losing the database costs an index, not a note.
Tasks are the app's own records and are exported from the oplog rather than
scanned from the vault. *Reopen when* sync lands: the oplog is then the source
for task rows on another machine, and this decision should name the export shape.

## 0019 — A project is a grouping: deleting one never deletes its tasks

A project (`lists`) does not own a task; `tasks.list_id` is a nullable filing.
Deleting a project soft-deletes the project row and clears `list_id` on its
tasks in one transaction, with an oplog row for each task it released. Undo
calls `restore_list` with the ids the delete released, and refiles only tasks
that are still unfiled — a task moved somewhere else in the meantime stays where
the person put it. Names are unique case-insensitively among live projects, so a
rename or a restore that would collide is a typed refusal, never two `Launch`
chips. A delete is therefore one press behind the usual eight-second toast: the
blast radius is a label, not data. *Reopen when* projects gain colour choices or
merging: those change a project's identity, not a task's filing.

## 0020 — A queued action asks for its own frame; a new task shows before the store answers

`poll` runs at the start of a frame and sends what `take_action` returns; the
key and pointer handlers that queue those actions run after it. Rune cannot tell
the app that input is still waiting (`Tx::input_waiting` is crate-private), so a
command that only queued from a handler waited for the next event: pressing
Return in the add row drew the row but did not write it until the next click or
keypress. Every handler that can queue work now calls `State::flush(tx)`, which
asks for a zero-delay wake when something is queued. The command leaves in the
frame it was asked for, and the app still sleeps when it has nothing to do.

The row itself is optimistic for the same reason: a person judges "did that
work?" from the checklist, not from the store. `add_task_now` puts the parsed
title in the list under a temporary id (`new-N`) and remembers the typed line.
The store's `TaskChanged` for that create replaces the row in place, so the
checklist never shows it twice, and the typed line is kept only until then. A
refusal (`Evt::TaskCreateFailed`, separate from `TaskFailed` so the UI can tell
which request failed) takes the row away and puts the whole line back in the
field, `#project` and all. A row the store has not answered refuses checkbox,
title, notes, due and project writes: its temporary id must never reach the
database. Create events arrive in the order the commands were queued, so the
oldest unanswered create owns the next unknown id. *Reopen when* actions can
come from outside the window (a menu bar item, a notification): those need the
same wake, and the store should then echo the caller's tag instead of relying on
order.

## 0021 — A step is a task with a parent

A step is a row in `tasks` with `parent_id` set, not a second table. It keeps the
whole model it already has: title, notes, done, `completed_at`, `rev`, a soft
delete and an oplog row for every write. A separate `steps` table would have had
to duplicate all of that, and every query, export and future sync would need to
know two shapes for "a thing you check off". A tree stops at three levels — a
task, its steps, and their steps (`notex_core::MAX_TASK_DEPTH`) — and a finished
task takes no new step. Three is enough to say "break this down"; a fourth would
need a tree view and a way to move a branch, which is a later milestone.

A step is filed where its task is. `create_subtask` copies the task's project,
and `set_list` moves every open row below the task in the same transaction, one
oplog row each, so a project filter, an export and the join never disagree. A
step never takes a due day of its own: it belongs to its task's day. The
checklist leaves steps out of its top level, its open count and its archive, and
draws them under the task instead, indented one level per step. A row that can
still take a step carries a `+`; the last level has none, rather than one that
refuses. There is no STEPS heading or panel: the steps are part of the
checklist, visible whether or not the row's notes and dates are open. The row
carries a `1/3` count of the steps below it.

Checking a task checks every open step below it: `set_done` reads the open ids
with a recursive query first, then closes the task and each step in one
transaction. A step that was already done is left alone, so the write changes
exactly what it reports. Reopening a task does **not** reopen its steps — a step
someone finished stays finished — but the toast's Undo is not a plain reopen:
the UI remembers every step that check closed and asks to reopen each, so undo
takes the whole check back. Deleting a task soft-deletes the whole branch in the
same transaction and answers with every id, so no step is left on screen under a
task that is gone. Undo restores the steps whose `deleted_at` equals the task's,
which is the delete being undone; a step deleted on its own earlier keeps its own
timestamp and stays gone.

The add-step field is optimistic and stays open, for the same reasons as the add
row (0020): the step shows under its task in the frame Return is handled, and a
run of steps is a run of lines. `PendingAdd` carries an `AddTarget` so a refusal
puts the line back in the field it came from — the step's field, not the task add
row — and the queue is still FIFO, so the oldest unanswered create owns the next
unknown id.
