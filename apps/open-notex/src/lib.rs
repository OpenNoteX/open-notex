//! The app's wiring: the shell, and the store behind it.
//!
//! [`notex_ui::State`] stays pure data. This module owns the one worker thread,
//! resolves the database path outside the vault, and turns worker events into
//! state changes. Rune calls [`setup`] once and [`poll`] once per frame; the view
//! never touches a file, a database or a thread.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

pub mod update;
pub mod vault;

use notex_core::Availability;
use notex_store::worker::{self, Cmd, Evt, Notify, StoreHandle, Worker};
use notex_store::{StoreError, db};
use notex_ui::{Action, Load, NoteRow, Now, ProjectRow, State, TaskRow, VaultIntent};
use rune::{Tx, Waker};
use thiserror::Error;

/// The store session the UI thread talks to. One per app, or per test.
pub struct Session {
    handle: StoreHandle,
    /// The database did not exist before this session created it.
    fresh: bool,
    /// Owned so dropping the session join-checkpoints the database.
    _worker: Worker,
}

impl Session {
    /// Spawn the worker for `db`, guarding that it is outside `vault`. The caller
    /// sends the first command (see [`start`], which rebuilds the index).
    pub fn open(db: &Path, vault: &Path, notify: impl Notify) -> Result<Self, ConnectError> {
        if !vault.is_dir() {
            return Err(ConnectError::MissingVault(vault.display().to_string()));
        }

        let fresh = !db.exists();
        if let Some(parent) = db.parent() {
            fs::create_dir_all(parent)?;
        }
        if fresh {
            fs::File::create(db)?;
        }
        db::assert_outside_vault(db, vault)?;

        let worker = worker::spawn(db, notify)?;
        Ok(Self {
            handle: worker.handle(),
            fresh,
            _worker: worker,
        })
    }

    pub fn handle(&self) -> &StoreHandle {
        &self.handle
    }

    /// True when there was no index to show, so the first frame must wait for
    /// the reindex rather than flash an empty list.
    pub fn is_fresh(&self) -> bool {
        self.fresh
    }
}

/// Why the store could not start. Every variant is shown in the list pane, in
/// place of the notes, so it has to read like a sentence.
#[derive(Debug, Error)]
pub enum ConnectError {
    #[error("The vault at {0} is missing.")]
    MissingVault(String),
    /// The folder the dialog asked for cannot be used, in the dialog's words.
    #[error("{0}")]
    Refused(String),
    #[error("The store could not use its database: {0}")]
    Store(#[from] StoreError),
    #[error("The store could not prepare its database: {0}")]
    Io(#[from] std::io::Error),
}

/// Start the store for `vault`, rebuild the index, and leave the list pane in
/// `Loading`. On failure the reason goes into `state.load`; the app still runs.
pub fn start(
    state: &mut State,
    vault: &Path,
    db: &Path,
    notify: impl Notify,
) -> Result<Session, ConnectError> {
    state.load = Load::Loading;

    let session = match Session::open(db, vault, notify) {
        Ok(session) => session,
        Err(error) => {
            state.load = Load::Failed(error.to_string());
            return Err(error);
        }
    };

    // A warm database already has a list to show: answer it first, then scan the
    // vault behind it. A fresh one has nothing to show, so the reindex comes first
    // and the list pane keeps its "opening" state until the notes exist.
    let reindex = || Cmd::Reindex {
        root: vault.to_path_buf(),
    };
    // Tasks are structured data: they come from SQLite, not the vault scan, so
    // one read at launch is enough and the reindex does not touch them.
    let tasks = || {
        let handle = session.handle();
        handle
            .send(Cmd::ListTasks)
            .and_then(|()| handle.send(Cmd::ListLists))
    };
    let started = if session.is_fresh() {
        session.handle().send(reindex()).and_then(|()| tasks())
    } else {
        session
            .handle()
            .send(Cmd::ListNotes)
            .and_then(|()| tasks())
            .and_then(|()| session.handle().send(reindex()))
    };

    if let Err(error) = started {
        let error = ConnectError::from(error);
        state.load = Load::Failed(error.to_string());
        return Err(error);
    }

    Ok(session)
}

/// `AppSpec::setup`: runs once, on the UI thread, with the waker the worker uses
/// to ask `poll` for a frame.
///
/// Launch order: `--vault` (strict, never remembered) → the saved folder → a
/// `~/Documents/Notes` that already exists → ask, with `~/Documents/Notes`
/// pre-filled.
pub fn setup(state: &mut State, waker: Waker) {
    // Snapshots and test scripts are pure scenes: no vault, no database, no
    // worker thread. This is also what keeps `cargo xtask snap` off a real vault.
    if rune::is_headless() {
        return;
    }

    // Kept for the update check: it answers on its own thread and has to ask for
    // a frame, or the settings row would wait for the next keypress.
    WAKER.with(|slot| *slot.borrow_mut() = Some(waker.clone()));

    let home = home_dir();
    let app_support = app_support_dir(&home);
    let db = app_support.join("notex.db");

    // `--vault` is an override for this run, and it stays strict: asking for a
    // path gets that path or the list pane's loud failure, never a dialog. It is
    // deliberately not written to config.json.
    if let Some(flag) = notex_ui::arg(rune::extra_args(), "vault") {
        state.vault = flag.clone();
        state.vault_open = true;
        if let Ok(session) = start(state, &PathBuf::from(flag), &db, Wake(waker)) {
            SESSION.with(|slot| *slot.borrow_mut() = Some(session));
        }
        return;
    }

    match vault::resolve(&home, &app_support) {
        vault::Launch::Open(vault) => {
            if let Err(error) = vault::remember(&app_support, &vault) {
                // Not being able to remember does not stop this run.
                eprintln!("openNoteX: the vault could not be remembered: {error}");
            }
            state.vault = vault.display().to_string();
            state.vault_open = true;
            if let Ok(session) = start(state, &vault, &db, Wake(waker)) {
                SESSION.with(|slot| *slot.borrow_mut() = Some(session));
            }
        }
        vault::Launch::Ask { path, notice } => {
            state.ask_for_vault(path.display().to_string(), notice);
        }
    }
}

/// Open the folder the dialog asked for: validate, remember, then start the one
/// store. A refusal leaves `state` in the dialog with the reason, so the person
/// can pick again without losing the path they chose.
pub fn open_vault(
    state: &mut State,
    intent: VaultIntent,
    app_support: &Path,
    db: &Path,
    notify: impl Notify,
) -> Result<Session, ConnectError> {
    let (path, create) = match intent {
        VaultIntent::Create(path) => (PathBuf::from(path), true),
        VaultIntent::Use(path) => (PathBuf::from(path), false),
    };
    if let Err(reason) = vault::prepare(&path, app_support, create) {
        state.vault_failed(reason.clone());
        return Err(ConnectError::Refused(reason));
    }
    if let Err(error) = vault::remember(app_support, &path) {
        let reason = format!("openNoteX could not remember that folder: {error}");
        state.vault_failed(reason.clone());
        return Err(ConnectError::Refused(reason));
    }
    // A failure from an earlier attempt is not this folder's failure.
    state.load = Load::Ready;
    match start(state, &path, db, notify) {
        Ok(session) => {
            state.vault_opened(path.display().to_string());
            Ok(session)
        }
        Err(error) => {
            state.load = Load::Ready;
            state.vault_failed(error.to_string());
            Err(error)
        }
    }
}

/// Answer the dialog: look at the folder once when it changed, then run the one
/// intent. Split out from [`poll`] so a test never needs the thread-local
/// session, and so `poll` stays a drain.
fn drive_vault(state: &mut State) -> bool {
    let home = home_dir();
    let app_support = app_support_dir(&home);
    let mut changed = false;

    if state.vault_setup.needs_check {
        let path = state.vault_setup.path.clone();
        let status = vault::inspect(&path, &app_support);
        state.vault_checked(&path, status);
        changed = true;
    }

    let Some(intent) = state.take_vault_intent() else {
        return changed;
    };
    let db = app_support.join("notex.db");
    let wake = WAKER.with(|slot| slot.borrow().clone());
    let Some(wake) = wake else {
        state.vault_failed("Opening a folder needs a window.");
        return true;
    };
    if let Ok(session) = open_vault(state, intent, &app_support, &db, Wake(wake)) {
        SESSION.with(|slot| *slot.borrow_mut() = Some(session));
    }
    true
}

/// `AppSpec::poll`: advances the editor's clocks, tells Rune when to wake for an
/// autosave, and drains the worker. Returns whether the view changed, so an idle
/// app draws nothing.
pub fn poll(state: &mut State, now: f64, tx: &mut Tx) -> bool {
    // A headless run is a fixed scene: its clock is part of it, so the wall
    // clock must not replace it and make a snapshot depend on the hour.
    let mut changed = if rune::is_headless() {
        false
    } else {
        refresh_clock(state)
    };
    changed |= state.tick(now);
    // The dialog is answered here, before the worker drains, so a folder chosen
    // this frame has its first reply pumped in the same frame.
    if state.vault_pending() {
        changed |= drive_vault(state);
    }
    // A dirty draft that is waiting out its idle delay needs one wake to save
    // it; a clean or blocked one schedules nothing, so the app can sleep.
    if let Some(wait) = state.next_wake() {
        tx.wake_after(std::time::Duration::from_secs_f64(wait.max(0.001)));
    }
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow().as_ref() {
            changed |= pump(state, session.handle());
        }
    });
    UPDATER.with(|slot| {
        if let Some(updater) = slot.borrow().as_ref() {
            while let Some(checked) = updater.poll() {
                changed |= apply_check(state, checked);
            }
        }
    });
    changed
}

/// Turn one answer into the Settings row. Split out from [`poll`] so the mapping
/// is tested without a thread, a process or a network.
fn apply_check(state: &mut State, checked: update::Checked) -> bool {
    match checked {
        update::Checked::Compared(availability) => match availability {
            Availability::Newer(version) => state.update_available(version.to_string()),
            Availability::UpToDate => state.update_current(),
            // A development build is newer than the release. Saying "up to date"
            // would be the wrong sentence.
            Availability::Older(_) => state.update_ahead(),
            // A tag this cannot read is a tag it must not offer.
            Availability::Unknown => state.update_failed("GitHub's release tag is not a version."),
        },
        update::Checked::Failed(message) => state.update_failed(message),
    }
    true
}

/// Send the commands the editor and the list asked for, then apply every event
/// the worker queued. Split out from [`poll`] so tests can drive a session they
/// own.
pub fn pump(state: &mut State, handle: &StoreHandle) -> bool {
    let mut changed = false;

    while let Some(action) = state.take_action() {
        match action {
            Action::Open(id) => {
                if handle.send(Cmd::OpenNote { id: id.clone() }).is_err() {
                    state.fail_body(&id, "The store is no longer running.".to_owned());
                }
            }
            Action::Create => {
                if handle
                    .send(Cmd::CreateNote {
                        title: "Untitled".to_owned(),
                    })
                    .is_err()
                {
                    state.create_failed("The store is no longer running.");
                }
            }
            Action::Delete(id) => {
                if handle.send(Cmd::DeleteNote { id: id.clone() }).is_err() {
                    state.delete_failed(&id, "The store is no longer running.");
                }
            }
            Action::CheckForUpdates => {
                let wake = WAKER.with(|slot| slot.borrow().clone());
                let Some(wake) = wake else {
                    // No window means no frame to draw the answer in.
                    state.update_failed("The update check needs a window.");
                    changed = true;
                    continue;
                };
                UPDATER.with(|slot| {
                    *slot.borrow_mut() = Some(update::Updater::start(
                        env!("CARGO_PKG_VERSION"),
                        move || wake(),
                    ));
                });
            }
            Action::ShowRelease(version) => {
                update::open_page(&update::release_page(&version));
            }
            Action::CreateTask {
                title,
                project,
                due_day,
            } => {
                if handle
                    .send(Cmd::CreateTask {
                        title,
                        project,
                        due_day,
                    })
                    .is_err()
                {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::SetTaskTitle { id, title } => {
                if handle.send(Cmd::SetTaskTitle { id, title }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::SetTaskNotes { id, notes } => {
                if handle.send(Cmd::SetTaskNotes { id, notes }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::SetTaskDue { id, due_day } => {
                if handle.send(Cmd::SetTaskDue { id, due_day }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::SetTaskProject { id, list_id } => {
                if handle.send(Cmd::SetTaskList { id, list_id }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::SetTaskDone { id, done } => {
                if handle.send(Cmd::SetTaskDone { id, done }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::DeleteTask(id) => {
                if handle.send(Cmd::DeleteTask { id }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::RestoreTask(id) => {
                if handle.send(Cmd::RestoreTask { id }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::CreateProject(name) => {
                if handle.send(Cmd::CreateList { name }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::SetProjectName { id, name } => {
                if handle.send(Cmd::SetListName { id, name }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::DeleteProject(id) => {
                if handle.send(Cmd::DeleteList { id }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::RestoreProject { id, task_ids } => {
                if handle.send(Cmd::RestoreList { id, task_ids }).is_err() {
                    state.task_failed("The store is no longer running.");
                }
            }
            Action::Save(request) => {
                let id = request.id.clone();
                let sent = handle.send(Cmd::SaveNote {
                    id: id.clone(),
                    body: request.body,
                    base_hash: request.base_hash,
                    token: request.token,
                    force: request.force,
                });
                if sent.is_err() {
                    state.save_failed(&id, "The store is no longer running.");
                }
            }
        }
        changed = true;
    }

    for event in handle.drain() {
        changed |= apply(state, handle, event);
    }
    changed
}

fn apply(state: &mut State, handle: &StoreHandle, event: Evt) -> bool {
    match event {
        // Pong is a health check.
        Evt::Pong => false,
        // The index is fresh: ask for the list it now backs.
        Evt::Indexed { .. } => {
            let _ = handle.send(Cmd::ListNotes);
            false
        }
        // Another program changed the vault. The index is caught up, so the
        // list is re-asked for, and an open note is re-read unless the draft has
        // something unsaved in it.
        Evt::Synced { notes } => {
            let _ = handle.send(Cmd::ListNotes);
            state.refresh_from_disk(&notes);
            true
        }
        Evt::Notes(notes) => {
            state.set_notes(
                notes
                    .into_iter()
                    .map(|note| NoteRow {
                        id: note.id,
                        title: note.title,
                        tags: note.tags,
                        modified: note.mtime_ns,
                    })
                    .collect(),
            );
            state.load = Load::Ready;
            true
        }
        Evt::Created { id, body, hash } => {
            state.created(&id, body, hash);
            // The row comes from the index, like every other row.
            let _ = handle.send(Cmd::ListNotes);
            true
        }
        Evt::Deleted { id } => {
            state.deleted(&id);
            let _ = handle.send(Cmd::ListNotes);
            true
        }
        Evt::CreateFailed { err } => {
            state.create_failed(err);
            true
        }
        Evt::DeleteFailed { id, err } => {
            state.delete_failed(&id, err);
            true
        }
        Evt::Opened { id, body, hash } => {
            state.show_body(&id, body, hash);
            true
        }
        Evt::OpenFailed { id, err } => {
            state.fail_body(&id, err);
            true
        }
        Evt::Saved {
            id,
            token,
            hash,
            title,
            tags,
        } => {
            state.saved(&id, token, hash, title, tags);
            true
        }
        Evt::SaveConflict { id, .. } => {
            state.save_conflict(&id, "This file changed on disk since you opened it.");
            true
        }
        Evt::SaveFailed { id, err, .. } => {
            state.save_failed(&id, err);
            true
        }
        Evt::Tasks(tasks) => {
            state.set_tasks(tasks.into_iter().map(task_row).collect());
            true
        }
        Evt::Lists(lists) => {
            state.set_projects(lists.into_iter().map(project_row).collect());
            true
        }
        Evt::TaskChanged(task) => {
            state.task_changed(task_row(task));
            true
        }
        Evt::TaskGone { id } => {
            state.task_gone(&id);
            true
        }
        Evt::ListChanged(list) => {
            state.list_changed(project_row(list));
            true
        }
        Evt::ListGone { id, .. } => {
            state.project_gone(&id);
            true
        }
        Evt::ListRestored { list, task_ids } => {
            state.list_restored(project_row(list), &task_ids);
            true
        }
        Evt::TaskFailed { err } => {
            state.task_failed(err);
            true
        }
        Evt::Failed { op, err } => {
            state.load = Load::Failed(format!("{op}: {err}"));
            true
        }
    }
}

/// Map the store's row into the shell's. `completed_day` is resolved here, in
/// the layer that owns a clock and a timezone.
fn task_row(task: notex_store::Task) -> TaskRow {
    TaskRow {
        completed_day: task.completed_at.and_then(notex_time::local_day),
        id: task.id,
        title: task.title,
        notes: task.notes,
        done: task.done,
        list_id: task.list_id,
        list_name: task.list_name,
        list_color: task.list_color,
        due_day: task.due_day,
        completed_at: task.completed_at,
    }
}

fn project_row(list: notex_store::List) -> ProjectRow {
    ProjectRow {
        id: list.id,
        name: list.name,
        color: list.color,
    }
}

thread_local! {
    /// The minute the Home clock was last built for, so `poll` formats three
    /// strings once a minute rather than once a frame.
    static CLOCK_MINUTE: std::cell::Cell<i64> = const { std::cell::Cell::new(i64::MIN) };
}

/// Fill Home's clock. The view never asks the system what time it is.
fn refresh_clock(state: &mut State) -> bool {
    let minute = now_ms() / 60_000;
    let due = CLOCK_MINUTE.with(|cell| {
        if cell.get() == minute {
            false
        } else {
            cell.set(minute);
            true
        }
    });
    if !due {
        return false;
    }
    match notex_time::local_stamp(minute * 60_000) {
        Ok(stamp) => state.set_clock(Now {
            day: stamp.day,
            time: stamp.time,
            date: stamp.date,
        }),
        Err(_) => false,
    }
}

/// The wall clock, in UTC epoch milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// How long a close or a quit waits for a draft to reach the file.
pub const FLUSH_WAIT: std::time::Duration = std::time::Duration::from_millis(750);

/// Close and quit hook: write a dirty draft and wait for the file, so closing
/// the window never drops the last edit.
///
/// Rune cannot cancel a quit, so a draft the disk still refuses is kept in the
/// recovery folder rather than disappearing with the process. Closing the window
/// alone keeps it in memory, since the app stays alive (see `main`).
pub fn flush(state: &mut State, _tx: &mut Tx) {
    SESSION.with(|slot| {
        let session = slot.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        if !flush_with(state, session.handle()) && rune::quitting() {
            keep_draft(state);
        }
    });
}

/// Write a dirty draft and wait for the answer. True when the draft is clean (or
/// there was none). Bounded, so a dead worker cannot hang the close, and split
/// out from [`flush`] so a test can drive a session it owns.
pub fn flush_with(state: &mut State, handle: &StoreHandle) -> bool {
    // Anything queued behind the save must not run on the way out.
    state.cancel_pending();
    state.request_save();
    let started = std::time::Instant::now();
    loop {
        pump(state, handle);
        match &state.draft {
            None => return true,
            Some(draft) if !draft.is_dirty() => return true,
            // A conflict or a refusal does not resolve by waiting.
            Some(draft) if draft.conflict.is_some() || draft.error.is_some() => return false,
            Some(_) => {}
        }
        if started.elapsed() >= FLUSH_WAIT {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Where drafts that could not be written are kept, outside the vault so the
/// index never sees them.
pub fn recovery_dir() -> PathBuf {
    db_path()
        .parent()
        .map(|parent| parent.join("recovery"))
        .unwrap_or_else(|| PathBuf::from("recovery"))
}

/// The last resort when the process is ending: the body, with the note's id so
/// the file is a valid note if it is ever copied back into the vault.
fn keep_draft(state: &State) {
    let Some(draft) = &state.draft else {
        return;
    };
    match keep_draft_in(&recovery_dir(), &draft.id, &draft.edit.text) {
        Ok(path) => eprintln!(
            "openNoteX: the last save failed; the draft is kept at {}",
            path.display()
        ),
        Err(error) => {
            eprintln!("openNoteX: the last save failed and the draft could not be kept: {error}")
        }
    }
}

fn keep_draft_in(dir: &Path, id: &str, body: &str) -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{id}-{stamp}.md"));
    fs::write(&path, format!("---\nid: {id}\n---\n{body}"))?;
    Ok(path)
}

/// The database lives with the app's other data, never in the vault (rule 8).
/// The version line: `openNoteX 0.1.0 (abc1234)`. The revision is stamped by the
/// release workflow; a local build says `dev`, so a bug report says which binary
/// it came from.
pub fn version_line() -> String {
    format!(
        "{} {} ({})",
        notex_ui::TITLE,
        env!("CARGO_PKG_VERSION"),
        option_env!("NOTEX_REVISION").unwrap_or("dev")
    )
}

/// `--version` is ours: Rune owns the rest of the command line, so the answer
/// has to happen before it starts. `--scene`, `--snap` and `--tree` are
/// untouched and still reach Rune.
pub fn wants_version(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--version" || arg == "-V")
}

pub fn db_path() -> PathBuf {
    app_support_dir(&home_dir()).join("notex.db")
}

/// `$HOME`, or the current directory when it is unset. One place, so the
/// database and the default vault never disagree about which home they mean.
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
fn app_support_dir(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("openNoteX")
}

#[cfg(not(target_os = "macos"))]
fn app_support_dir(home: &Path) -> PathBuf {
    home.join(".local").join("share").join("openNoteX")
}

/// Wakes Rune's loop from the store worker. `Waker` is `Send + Sync`, which is
/// all [`Notify`] needs.
struct Wake(Waker);

impl Notify for Wake {
    fn notify(&self) {
        (self.0)();
    }
}

thread_local! {
    /// The one session for this window. Setup runs on the UI thread, poll reads
    /// it there, and it drops (joining the worker) when that thread ends.
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    /// The waker the update check uses to ask for a frame once it has an answer.
    static WAKER: RefCell<Option<Waker>> = const { RefCell::new(None) };
    /// The check in flight, if someone asked for one.
    static UPDATER: RefCell<Option<update::Updater>> = const { RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> notex_core::Version {
        notex_core::Version::parse(text).unwrap()
    }

    #[test]
    fn each_answer_from_the_check_reads_as_the_right_row() {
        let mut state = State::with_vault("/tmp/v");

        apply_check(
            &mut state,
            update::Checked::Compared(Availability::Newer(version("0.1.0"))),
        );
        assert_eq!(
            state.update,
            notex_ui::Update::Available("0.1.0".to_owned())
        );
        assert_eq!(state.update.offered(), Some("0.1.0"));

        apply_check(
            &mut state,
            update::Checked::Compared(Availability::UpToDate),
        );
        assert_eq!(state.update, notex_ui::Update::Current);

        // A development build is newer than the last release, and it is not
        // "up to date": there is nothing to install and nothing to hide.
        apply_check(
            &mut state,
            update::Checked::Compared(Availability::Older(version("0.9.0"))),
        );
        assert_eq!(state.update, notex_ui::Update::Ahead);

        // A tag that is not a version is not offered.
        apply_check(&mut state, update::Checked::Compared(Availability::Unknown));
        assert!(matches!(state.update, notex_ui::Update::Failed(_)));
        assert_eq!(state.update.offered(), None);

        apply_check(
            &mut state,
            update::Checked::Failed("GitHub did not answer.".to_owned()),
        );
        assert_eq!(
            state.update,
            notex_ui::Update::Failed("GitHub did not answer.".to_owned())
        );
    }

    #[test]
    fn an_unwritable_draft_is_kept_as_a_valid_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = keep_draft_in(dir.path(), "01ARZ3NDEKTSV4RRFFQ69G5FAV", "# Kept\n\nmine\n")
            .expect("the recovery file is written");

        let kept = std::fs::read_to_string(&path).unwrap();
        assert!(kept.contains("id: 01ARZ3NDEKTSV4RRFFQ69G5FAV"), "{kept}");
        assert!(kept.ends_with("# Kept\n\nmine\n"), "{kept}");
        let (_, body) = notex_store::vault::parse_note_source(&kept).unwrap();
        assert_eq!(body, "# Kept\n\nmine\n", "the body survives verbatim");
    }

    #[test]
    fn the_version_line_names_the_app_the_version_and_the_build() {
        let line = version_line();
        assert!(line.starts_with("openNoteX 0.0.1 ("), "{line}");
        assert!(line.ends_with(')'), "{line}");
    }

    #[test]
    fn only_version_flags_are_answered_before_rune_sees_the_line() {
        let args = |line: &[&str]| line.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert!(wants_version(&args(&["--version"])));
        assert!(wants_version(&args(&["-V"])));
        assert!(wants_version(&args(&["--vault", "/tmp/v", "--version"])));
        assert!(!wants_version(&args(&["--snap", "out.png"])));
        assert!(!wants_version(&args(&["--scene", "notes-list"])));
        assert!(!wants_version(&args(&[])));
    }

    #[test]
    fn the_database_is_not_in_the_default_vault() {
        let db = db_path();
        assert!(db.is_absolute(), "{}", db.display());
        assert!(db.ends_with("openNoteX/notex.db"), "{}", db.display());
        #[cfg(target_os = "macos")]
        assert!(db.to_string_lossy().contains("Application Support"));
    }
}
