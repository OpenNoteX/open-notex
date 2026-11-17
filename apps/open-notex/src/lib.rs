//! The app's wiring: the shell, and the store behind it.
//!
//! [`notex_ui::State`] stays pure data. This module owns the one worker thread,
//! resolves the database path outside the vault, and turns worker events into
//! state changes. Rune calls [`setup`] once and [`poll`] once per frame; the view
//! never touches a file, a database or a thread.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use notex_store::worker::{self, Cmd, Evt, Notify, StoreHandle, Worker};
use notex_store::{StoreError, db};
use notex_ui::{Action, Load, NoteRow, State};
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
    let started = if session.is_fresh() {
        session.handle().send(reindex())
    } else {
        session
            .handle()
            .send(Cmd::ListNotes)
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
pub fn setup(state: &mut State, waker: Waker) {
    // Snapshots and test scripts are pure scenes: no vault, no database, no
    // worker thread. This is also what keeps `cargo xtask snap` off a real vault.
    if rune::is_headless() {
        return;
    }

    let vault = PathBuf::from(&state.vault);
    if let Ok(session) = start(state, &vault, &db_path(), Wake(waker)) {
        SESSION.with(|slot| *slot.borrow_mut() = Some(session));
    }
}

/// `AppSpec::poll`: advances the editor's clocks, tells Rune when to wake for an
/// autosave, and drains the worker. Returns whether the view changed, so an idle
/// app draws nothing.
pub fn poll(state: &mut State, now: f64, tx: &mut Tx) -> bool {
    let mut changed = state.tick(now);
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
    changed
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
        Evt::Notes(notes) => {
            state.set_notes(
                notes
                    .into_iter()
                    .map(|note| NoteRow {
                        id: note.id,
                        title: note.title,
                        tags: note.tags,
                    })
                    .collect(),
            );
            state.load = Load::Ready;
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
        Evt::Failed { op, err } => {
            state.load = Load::Failed(format!("{op}: {err}"));
            true
        }
    }
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
    // A switch queued behind the save must not run on the way out.
    state.pending_open = None;
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
pub fn db_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    app_support_dir(&home).join("notex.db")
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_database_is_not_in_the_default_vault() {
        let db = db_path();
        assert!(db.is_absolute(), "{}", db.display());
        assert!(db.ends_with("openNoteX/notex.db"), "{}", db.display());
        #[cfg(target_os = "macos")]
        assert!(db.to_string_lossy().contains("Application Support"));
    }
}
