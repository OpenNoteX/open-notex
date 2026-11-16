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
