# Changelog

All notable changes to this project. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Home is the first screen: a full-width clock and date, then one block with
  Todo (overdue first, then due today), Reminders (honest that they are a later
  milestone) and Next up, then Recent notes ordered by the file's modification
  time. Every block is a way into the screen that owns it.
- Tasks is one checklist instead of four lists. Open tasks are grouped
  `Overdue / Today / Upcoming / No date` by their local due day, with projects
  as a filter chip row (`All`, each project, `+ Project`). There is no Inbox,
  Today, Upcoming or Projects row in the sidebar; `Home` and `Tasks (n)` are.
- Adding is one borderless row: `Return` writes the task and leaves the caret
  there for the next one, and `#project` files it under a project, creating the
  project when the name is new. A filtered list adds into the project it shows.
- A task can be broken into **steps**: the chevron's row opens a STEPS section
  where a line becomes a step and Return keeps the field open for the next one.
  A step is a task row with a parent, so it has its own checkbox, in-place
  rename and delete; it is filed with its task (moving the task moves its
  steps) and never takes a date of its own. The collapsed row shows `1/3`.
  Completing a task closes its open steps in the same write, and the toast's
  Undo reopens the task and the steps that check closed. Deleting a task takes
  its steps with it; undo restores them.
- `notex-store`: `migrations/0004` with a partial index on `parent_id`;
  `Cmd::CreateSubtask`, `tasks::create_subtask`, `Completion` and `Restored`,
  and `Task.parent_id`. `set_done` closes open steps in one transaction (one
  oplog row each), `delete` answers with every id it removed, `restore` brings
  back the steps deleted by that same delete, and `set_list` refiles the steps
  with the task.
- `notex-ui`: `TaskRow.parent_id`, `AddTarget`, `StepInput`, `steps_of`,
  `open_steps_of`, `begin_step`, `add_step_now` and the `tasks-steps` scene.
- A row's title renames in place (`Return` saves, `Escape` puts it back). The
  due chip reads as a person would say it — `2d late`, `Today`, `Tomorrow`,
  `Fri`, `23 Nov` — and the chevron opens due buttons (Today, Tomorrow, Next
  week, Clear), the project picker and a notes field.
- A project is maintainable from its chip: the `⋯` on hover (or a secondary
  click) offers **Rename…** and **Delete project**. Rename is inline on the chip;
  an empty name, or one another project holds, is refused with the field left
  open to correct. Delete never deletes a task: its tasks move to **No project**,
  the filter falls back to **All**, and the eight-second toast restores the
  project and each task's filing.
- Completing a task shows an eight-second toast whose **Undo** reopens it, and
  deleting is one press with the same way back. Done tasks move to an archive
  grouped by the day they were completed, where they can be reopened.
- `notex-store`: a `tasks` table and `lists` (projects), `migrations/0003` with
  `due_day` and a partial index for open tasks, and `Cmd::ListTasks`,
  `ListLists`, `CreateTask`, `SetTaskTitle`, `SetTaskNotes`, `SetTaskDue`,
  `SetTaskList`, `SetTaskDone`, `DeleteTask`, `RestoreTask`, `CreateList`,
  `SetListName`, `DeleteList` and `RestoreList`; a refused create is
  `Evt::TaskCreateFailed`, separate from another task write's `TaskFailed` so
  the add row can give the typed line back.
  Every write appends its oplog row in the same transaction. `NoteSummary`
  carries `mtime_ns`, so Home can order notes by recency.
- `notex-time`: `Bucket`, `bucket`, `due`, `day_label` and `add_days` for due
  days, plus `local_stamp`/`local_day`, so the shell draws a clock it never
  reads. `notex-core`: `parse_task_input`, which splits a `#project` from a line.
- First launch asks where notes live instead of showing an error when the
  folder is missing. One dialog offers the suggested `~/Documents/Notes`,
  **Choose…** (the macOS folder sheet) and **Not now**; **Create folder** makes
  the folder and opens it. The choice is remembered in
  `~/Library/Application Support/openNoteX/config.json`, beside the database and
  never in the vault. The same dialog is one press away from the empty Notes
  list, Settings → Vault, and `⌘N` while no vault is open. A folder that is
  missing, read-only, a file, or inside the app's own data is named in the
  dialog and not opened. `--vault` still outranks the saved choice for one run
  and is never stored.
- Settings → Updates: **Check now** asks GitHub for the latest release and says
  whether a newer one exists; a newer one becomes a banner naming the version,
  with a button that opens its release page. Nothing is asked on launch — the
  check is a `curl` started when pressed — and a failed check is a sentence in
  the row, not an error state. Comparison is numeric, so `0.10.0` is newer than
  `0.9.0`.
- Watching the vault: an edit made in another editor shows up in the app within
  two seconds — the open note is re-read, and a changed `# Title`, tags or a new
  or deleted file reaches the list. A draft with unsaved text is never thrown
  away by this: its next save reports the conflict, as before. The pass reads no
  note and costs nothing when the vault has not moved.
- Creating: `⌘N` and a plus in the note list write a real Markdown file with a
  fresh id and `# Untitled`, then open it. A title already in use gets `-2`.
- Deleting: `⌘⌫` and the trash button in the detail pane, confirmed in a dialog.
  The file moves to `~/Library/Application Support/openNoteX/trash/` and the
  note leaves the list; the body stays recoverable. A file deleted outside the
  app leaves the list at the next launch (`--reindex` included).
- Renaming: saving a changed `# Title` renames the file to the title's slug and
  keeps the note's id and frontmatter.
- Editing: the Notes detail pane is the note's Markdown file, edited in place
  with no separate preview. The `# Title` line is the rename control.
- Autosave: 700 ms after the last keystroke, or 5 s into continuous typing.
  `⌘S` and a Save button write immediately; closing the window or quitting
  flushes a dirty draft and waits for the file.
- A save is guarded by the hash the file had when it was opened. A file changed
  on disk becomes a conflict banner with Reload and Overwrite instead of a
  silent clobber; a failed write keeps the draft and offers Try again.
- Selecting another note while the editor is dirty queues the switch behind the
  save, so typing is never dropped.
- A dot on a note's list row while its editor holds unsaved text.
- Closing the window keeps the app running, as on macOS, so an unsaved draft
  survives a failed close-time save; the closed window releases its GPU memory
  and reopening recreates it. `⌘Q` still quits.
- A draft the disk refuses while the app is quitting is kept in
  `~/Library/Application Support/openNoteX/recovery/` as a valid note with its
  id, instead of disappearing with the process.
- `notex-store`: `vault::write_body` (replaces a body and keeps frontmatter, so
  ids and tags survive an edit), `vault::file_hash`, `vault::create_note`,
  `vault::file_name_for` with `unique_path_in`, and `vault::move_file`.
- `notex-store`: `Cmd::CreateNote { title }` and `Cmd::DeleteNote { id }`, with
  `Evt::Created`, `Evt::Deleted`, `Evt::CreateFailed` and `Evt::DeleteFailed`.
- `notex-store`: `Cmd::SaveNote { id, body, base_hash, token, force }` and
  `Evt::Saved`/`SaveConflict`/`SaveFailed`, all echoed with the editor's token.
- `notex-ui`: `Draft`, `DraftStatus`, `SaveRequest` and `Action`; the
  `notes-edit` and `notes-conflict` scenes.
- `cargo xtask bench` measures `SaveNote` and splits it into file and index
  time.
- The Notes screen shows the real vault: `apps/open-notex` opens
  `~/Library/Application Support/openNoteX/notex.db`, starts the one DB worker,
  lists every note by title with its tags, and renders the selected note's
  Markdown body. Selecting a row reads its `.md` file through the worker; bodies
  never enter the index.
- `notex-store`: `NoteSummary`, `notes()` (alphabetical, tags parsed),
  `note_path()` and `Batch`, which indexes a whole vault in one transaction.
- `notex-store`: `Cmd::ListNotes`, `Cmd::OpenNote`, `Evt::Notes`, `Evt::Opened`
  and `Evt::OpenFailed` on the worker.
- `notex-ui`: `NoteRow`, `NoteBody` and the `notes-list` scene; the list has
  loading, empty and failed states, the detail pane has loading, failed and
  "no note selected" states.
- A warm launch lists the existing index first and reindexes behind it; a fresh
  database reindexes first and keeps the list's "opening" state.
- Reindex skips a file whose hash and path already match, so a repeat scan
  writes no rows, appends no oplog entries and does not bump `rev`.
- The shell: a window with a sidebar, a list and a detail pane, ten routes
  (Inbox, Today, Upcoming, Projects, Notes, Tags, Habits, Reminders, Search,
  Settings), `⌘K` for search, `⌘,` for settings and the sidebar toggle.
- `crates/notex-ui`: `Route` as the single source of labels, icons and empty
  states; a three-seed palette (warm paper, near-black, teal) for light and
  dark; loading, failed and empty states; `SCENES` for snapshots.
- `apps/open-notex`: the `AppSpec`, traffic-light and drag-region chrome, and
  `--vault` / `--route`.
- `cargo xtask snap` renders every scene in both themes through the real binary,
  writes a PNG and an element tree, and asserts the tree is not blank.
- Rune and rune-kit pinned by revision (`631e8a6`), with the three dependencies
  Rune patches copied into `vendor/`.
- A `.cargo/config.toml` alias so `cargo xtask` works as documented.
- CI renders the snapshots on macOS and uploads them with the other artifacts.
- Project context: `AGENTS.md`, `README.md`, and `docs/` covering plan,
  architecture, data model, scheduling, decisions, testing and budgets.
- Cargo workspace on Rust 1.99.1 with `notex-core`, `notex-time`, `notex-store`
  and `xtask`.
- `notex-time`: day keys with a configurable day start, recurrence expansion
  (daily/weekly/monthly), streak calculation, and the scheduler's `next_action`.
- `notex-store`: WAL pragmas, forward-only migrations, note index with FTS5,
  oplog writer, and a Markdown vault writer with frontmatter handling and atomic
  replacement.
- `xtask seed` to generate a fixture vault and database.
- CI: formatting, clippy with `-D warnings`, and the workspace test suite.
- Packaging: `cargo xtask bundle` writes `OpenNoteX.app` — icon, `Info.plist`
  with the workspace version, ad-hoc signed — and, with `--dmg --zip`,
  `openNoteX-<version>-arm64.dmg` (drag to Applications), a `.zip` for updates
  and `SHA256SUMS.txt`. A binary built for anything but arm64 is refused with
  the command that fixes it, rather than shipped and failing to launch, and
  `--check` launches the bundle with a home of its own and waits for its first
  window.
- `--version` prints `openNoteX 0.1.0 (<revision>)` and exits before a window,
  a vault or a GPU exist; the release workflow stamps the revision.
- `assets/icon.svg` and `scripts/make-icon.sh`: one vector source, and the
  `.iconset`/`.icns` built from it rather than exported by hand.
- Release workflow: tagging `v*` builds the arm64 bundle, refuses a tag that
  disagrees with the workspace version or a runner that is not arm64, launches
  the build, installs it from the disk image and launches that too, then
  publishes a **draft** release with the `.dmg`, the `.zip`, `SHA256SUMS.txt`
  and `scripts/install.sh`.
- `scripts/install.sh`: one command installs or updates openNoteX — verifies the
  download against the release checksums, quits a running copy, swaps the bundle
  with two renames on one volume, restores it if the swap fails, clears the
  quarantine flag and opens the app.
- `docs/RELEASING.md`: cutting a release, verifying the draft, and what ad-hoc
  signing means for the person installing it. `docs/PLAN.md` splits M6 into
  M6a (this), M6b (signing and notarization, blocked on an Apple Developer
  account) and M6c (in-place updates, after M6b).

### Changed

- The database lives in the platform data directory, never in the vault; startup
  refuses a path inside it.
- `xtask seed` indexes through `Batch`, so its timings match the app's reindex.

### Fixed

- A task typed in the add row did not reach the store until the next pointer or
  key event: `poll` sends what the handlers queued, and it runs before the
  handler that queues, so a command typed in a field had no frame of its own.
  Every handler that queues work now asks for the next frame, and the typed row
  is in the checklist before the store answers — replaced in place when the real
  id arrives, and given back to the field with the text if the create is refused.
- A note deleted or renamed outside the app stayed in the list after a reindex,
  and opening it failed. A reindex now prunes rows whose file the scan did not
  find, and logs the delete.
- Try again, `⌘S` and the Save button did nothing after a failed save: the
  refusal blocked every later attempt. Asking again now clears it, while a
  conflict still waits for Reload or Overwrite.
- Overwrite stayed armed after a failed save, so the next save would clobber a
  file that had changed again without asking. It now covers the version the
  user saw, and the hash guard runs again after it.
- Deleting a note's file while it was open reported `io: No such file or
  directory`; a save now says the note is no longer in the vault.
- `DELETE FROM notes_fts WHERE item_id = ?` scanned the whole search table for
  every note (`item_id` is `UNINDEXED`), which made a 10k-note reindex
  quadratic. Migration `0002_fts_rowid` stores each note's FTS rowid and deletes
  use it: cold full index of 10k notes went from 8.4 s to 4.2 s.
