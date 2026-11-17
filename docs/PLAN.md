# Plan

Status: `0.0.1` pre-alpha. M0 (context, workspace, store, time, CI), M0.5
(the shell), M1a (the store behind the shell) and M1b (editing) are done; the
rest of M1 — create, delete and the vault watcher — is next.

One app, four primitives: **capture (notes) → plan (tasks) → repeat (habits) →
remember (reminders)**. SQLite and a Markdown vault are the source of truth.
Rune renders. No network in v1.

## Milestones

| # | Ships | Proven by |
| --- | --- | --- |
| M0 ✅ | Context docs, workspace, store contract, migrations, vault writer, time math, CI | `cargo test` green on pure layers; docs committed |
| M0.5 ✅ | Rune shell: window, three panes, theme, headless snapshot harness | `cargo run` opens a window; snapshot PNG + tree JSON asserted |
| M1a ✅ | Store in the app: DB worker wired to the UI, real notes list, note bodies read from Markdown, reindex on launch | Open the app on a vault and read a note; delete the DB, relaunch, the list comes back |
| M1b ✅ | Editing: the selected note is its Markdown file, edited in place; autosave after idle, `⌘S`, a hash conflict guard with Reload/Overwrite, frontmatter preserved | Type, wait, see the file change; edit the file in another editor and the save refuses rather than clobbering |
| M1 | Notes as files: create/rename/delete, watcher, tags, FTS search, Obsidian import, ⌘K | Edit a note in another editor and see it reflected; delete the DB, `--reindex`, search is identical |
| M2 | Projects, tasks, Today, due dates, recurring tasks, reorder | Today matches reality after restart; recurrence correct across DST |
| M3 | Habits, streak engine, heatmap, freezes, at-risk states | Property tests across four timezones; hand-checked streak cases |
| M4 | Reminders, scheduler, notifications, tray, single instance, quick capture, snooze, click-through | Reminder fires with the window hidden; click focuses the item; missed-while-away digest |
| M5 | Undo, structured-data export/import, backups, settings, accessibility pass, onboarding | Export → fresh install → import round-trips |
| M6 | Packaging, signing, notarization, updater, release CI | Signed `.dmg` installs and updates on a clean Mac |
| M7 | Optional sync on the oplog, web build, mobile, ICS | — |

Private alpha after M4 (~7–8 focused weeks). Public 1.0 after M6.

## v1 non-goals

No accounts, network, telemetry, WYSIWYG, images or tables in preview, OS-wide
hotkey, delivery while quit, Linux notifications, mobile, web, or sync.

## Done means

- `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` green.
- Headless tree assertions cover the new flow; a snapshot scene exists.
- Budgets in `docs/PERFORMANCE.md` hold at the 10k-item fixture.
- `CHANGELOG.md` has an entry.
