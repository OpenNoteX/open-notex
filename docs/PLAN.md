# Plan

Status: `0.0.1` pre-alpha. M0 (context, workspace, store, time, CI), M0.5
(the shell), M1a (the store behind the shell), M1b (editing), M1's create,
delete, rename, vault watcher and first-launch vault choice, and M2a (Home and
one tasks checklist) are done. What remains of M1 is search and tags (`⌘K`, FTS
in the shell) and the Obsidian import; what remains of M2 is recurrence and
carried-days. M6a — a downloadable Mac app, the release pipeline and the update
check — is landing ahead of the milestone order so the app can be installed and
used.

One app, four primitives: **capture (notes) → plan (tasks) → repeat (habits) →
remember (reminders)**. SQLite and a Markdown vault are the source of truth.
Rune renders. No network unless a person asks for it: one check for a newer
release (decision 0014).

## Milestones

| # | Ships | Proven by |
| --- | --- | --- |
| M0 ✅ | Context docs, workspace, store contract, migrations, vault writer, time math, CI | `cargo test` green on pure layers; docs committed |
| M0.5 ✅ | Rune shell: window, three panes, theme, headless snapshot harness | `cargo run` opens a window; snapshot PNG + tree JSON asserted |
| M1a ✅ | Store in the app: DB worker wired to the UI, real notes list, note bodies read from Markdown, reindex on launch | Open the app on a vault and read a note; delete the DB, relaunch, the list comes back |
| M1b ✅ | Editing: the selected note is its Markdown file, edited in place; autosave after idle, `⌘S`, a hash conflict guard with Reload/Overwrite, frontmatter preserved | Type, wait, see the file change; edit the file in another editor and the save refuses rather than clobbering |
| M1 | Notes as files: create/rename/delete ✅, watcher ✅, tags, FTS search, Obsidian import, ⌘K | Edit a note in another editor and see it reflected; delete the DB, `--reindex`, search is identical |
| M1c ✅ | First launch: one dialog to create or choose the vault, remembered beside the database; every "no vault" surface reopens it | A fresh install creates `~/Documents/Notes` in one press and `⌘N` writes a `.md`; a vanished folder asks instead of switching |
| M2a ✅ | Home (clock, todo, reminders, next up, recent notes) and one tasks checklist: groups by due day, project chips with inline rename/delete, inline add/rename, due buttons, steps under a task, eight-second undo, archive | Add a task with `#project`, rename and delete the project without losing its tasks, undo the delete; complete a task, undo it, reopen it from the archive; break a task into steps, complete it and undo both; a restart shows the same list grouped against the local day |
| M2 | Recurring tasks and carried-over days on top of M2a | Recurrence correct across DST; a task missed yesterday is overdue today |
| M3 | Habits, streak engine, heatmap, freezes, at-risk states | Property tests across four timezones; hand-checked streak cases |
| M4 | Reminders, scheduler, notifications, tray, single instance, quick capture, snooze, click-through | Reminder fires with the window hidden; click focuses the item; missed-while-away digest |
| M5 | Undo, structured-data export/import, backups, settings, accessibility pass, onboarding | Export → fresh install → import round-trips |
| M6a | Packaging: `cargo xtask bundle`, release CI, installer, update check | A `.dmg` from Releases installs and launches on a clean Mac; the app names a newer release |
| M6b | Signing: Developer ID, hardened runtime, `notarytool`, stapling, `spctl` gate | The download opens with no detour, and an update keeps its permission grants. **Blocked on a paid Apple Developer account** |
| M6c | Updating: download, verify, swap in place, relaunch, roll back | An installed app updates itself from a release and a failed swap restores the old bundle. After M6b |
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
