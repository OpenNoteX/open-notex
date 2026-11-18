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
