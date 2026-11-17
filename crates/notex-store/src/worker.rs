//! The DB worker: one thread, one connection, commands in, events out.
//!
//! This is the only place a `rusqlite::Connection` is opened. The UI holds a
//! [`StoreHandle`], which is `Send + Sync` and never blocks on disk.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use chrono::Utc;
use notex_core::ItemId;
use rusqlite::{Connection, OptionalExtension};

use crate::db::{self, StoreError};
use crate::index;
use crate::vault::{self, Note};

#[derive(Debug)]
pub enum Cmd {
    Ping,
    /// Rebuild the whole index from `root`.
    Reindex {
        root: PathBuf,
    },
    /// Write a note body to disk, then index it. File first, index second.
    ///
    /// `base_hash` is the hash the note had when it was opened; a different file
    /// on disk is a conflict, not something to overwrite. `token` is the UI's
    /// edit revision and comes back on the reply. `force` writes anyway.
    SaveNote {
        id: String,
        body: String,
        base_hash: u64,
        token: u64,
        force: bool,
    },
    /// The list pane's read model.
    ListNotes,
    /// Read one note's body from its file (never from SQLite).
    OpenNote {
        id: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evt {
    Pong,
    Indexed {
        count: u64,
    },
    Saved {
        id: String,
        token: u64,
        hash: u64,
        title: String,
        tags: Vec<String>,
    },
    /// The file is not the one the editor read. Nothing is written; the user
    /// chooses to reload or to overwrite.
    SaveConflict {
        id: String,
        token: u64,
    },
    SaveFailed {
        id: String,
        token: u64,
        err: String,
    },
    Notes(Vec<crate::index::NoteSummary>),
    Opened {
        id: String,
        body: String,
        hash: u64,
    },
    OpenFailed {
        id: String,
        err: String,
    },
    Failed {
        op: &'static str,
        err: String,
    },
}

/// How the worker tells the UI loop there is work. In the app this wraps Rune's
/// `Waker`; in tests it is an mpsc sender.
pub trait Notify: Send + 'static {
    fn notify(&self);
}

#[derive(Clone)]
pub struct StoreHandle {
    tx: Arc<Mutex<Sender<Cmd>>>,
    inbox: Arc<Mutex<VecDeque<Evt>>>,
}

impl StoreHandle {
    pub fn send(&self, cmd: Cmd) -> Result<(), StoreError> {
        let sender = self.tx.lock().map_err(|_| StoreError::WorkerStopped)?;
        sender.send(cmd).map_err(|_| StoreError::WorkerStopped)
    }

    /// Drain pending events. Never blocks; safe to call from `poll()`.
    pub fn drain(&self) -> Vec<Evt> {
        let mut inbox = match self.inbox.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        inbox.drain(..).collect()
    }
}

/// Owns the worker thread. Dropping it shuts the thread down cleanly.
pub struct Worker {
    handle: StoreHandle,
    join: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn handle(&self) -> StoreHandle {
        self.handle.clone()
    }

    pub fn shutdown(mut self) {
        let _ = self.handle.send(Cmd::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = self.handle.send(Cmd::Shutdown);
            let _ = join.join();
        }
    }
}

pub fn spawn(db_path: impl Into<PathBuf>, notify: impl Notify) -> Result<Worker, StoreError> {
    let db_path = db_path.into();
    let (tx, rx) = mpsc::channel();
    let inbox = Arc::new(Mutex::new(VecDeque::new()));

    let handle = StoreHandle {
        tx: Arc::new(Mutex::new(tx)),
        inbox: Arc::clone(&inbox),
    };

    let notify: Box<dyn Notify> = Box::new(notify);
    let join = thread::Builder::new()
        .name("notex-store".to_owned())
        .spawn(move || run(db_path, rx, inbox, notify))?;

    Ok(Worker {
        handle,
        join: Some(join),
    })
}

fn run(
    db_path: PathBuf,
    rx: Receiver<Cmd>,
    inbox: Arc<Mutex<VecDeque<Evt>>>,
    notify: Box<dyn Notify>,
) {
    let mut conn = match db::open(&db_path) {
        Ok(conn) => conn,
        Err(error) => {
            publish(
                &inbox,
                notify.as_ref(),
                Evt::Failed {
                    op: "open",
                    err: error.to_string(),
                },
            );
            return;
        }
    };

    let device = device_id(&conn).unwrap_or_else(|_| "unknown".to_owned());

    while let Ok(cmd) = rx.recv() {
        if matches!(cmd, Cmd::Shutdown) {
            break;
        }
        for event in execute(&mut conn, cmd, &device) {
            publish(&inbox, notify.as_ref(), event);
        }
    }

    // Leave a tidy database behind for the next launch.
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
}

fn execute(conn: &mut Connection, cmd: Cmd, device: &str) -> Vec<Evt> {
    match cmd {
        Cmd::Ping => vec![Evt::Pong],
        Cmd::Reindex { root } => match reindex(conn, &root, device) {
            Ok(count) => vec![Evt::Indexed { count }],
            Err(error) => vec![Evt::Failed {
                op: "reindex",
                err: error.to_string(),
            }],
        },
        Cmd::SaveNote {
            id,
            body,
            base_hash,
            token,
            force,
        } => match save_note(conn, &id, &body, base_hash, force, device) {
            Ok(SaveOutcome::Saved(note)) => vec![Evt::Saved {
                id,
                token,
                hash: note.file.hash,
                title: note.file.title,
                tags: note.file.tags,
            }],
            Ok(SaveOutcome::Missing) => vec![Evt::SaveFailed {
                id,
                token,
                err: "the note is no longer in the vault".to_owned(),
            }],
            Ok(SaveOutcome::Conflict) => vec![Evt::SaveConflict { id, token }],
            Err(error) => vec![Evt::SaveFailed {
                id,
                token,
                err: error.to_string(),
            }],
        },
        Cmd::ListNotes => match index::notes(conn) {
            Ok(notes) => vec![Evt::Notes(notes)],
            Err(error) => vec![Evt::Failed {
                op: "list_notes",
                err: error.to_string(),
            }],
        },
        Cmd::OpenNote { id } => match open_note(conn, &id) {
            Ok(Some((body, hash))) => vec![Evt::Opened { id, body, hash }],
            Ok(None) => vec![Evt::OpenFailed {
                id,
                err: "the note is no longer in the index".to_owned(),
            }],
            Err(error) => vec![Evt::OpenFailed {
                id,
                err: error.to_string(),
            }],
        },
        Cmd::Shutdown => Vec::new(),
    }
}

fn reindex(conn: &mut Connection, root: &std::path::Path, device: &str) -> Result<u64, StoreError> {
    let paths = vault::scan_paths(root)?;
    let mut batch = index::Batch::new(conn, now_ms(), device)?;
    for path in paths {
        let note = vault::read_note(&path)?;
        batch.upsert(&note.file, &note.body)?;
    }
    batch.commit()
}

enum SaveOutcome {
    Saved(Note),
    /// The id is not in the index: the file was deleted or renamed.
    Missing,
    /// The file changed on disk since it was opened.
    Conflict,
}

fn save_note(
    conn: &mut Connection,
    id: &str,
    body: &str,
    base_hash: u64,
    force: bool,
    device: &str,
) -> Result<SaveOutcome, StoreError> {
    let Some(path) = index::note_path(conn, id)? else {
        return Ok(SaveOutcome::Missing);
    };

    // The file is the source of truth: if it moved under us, refuse to clobber
    // it. `force` is the user's explicit "overwrite" after a conflict.
    if !force {
        let disk_hash = match vault::file_hash(&path) {
            // The file itself is gone, not just the index row: a deleted or
            // renamed note, which the editor should hear as "no longer in the
            // vault" rather than as an io error.
            Err(vault::VaultError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SaveOutcome::Missing);
            }
            other => other?,
        };
        if disk_hash != base_hash {
            return Ok(SaveOutcome::Conflict);
        }
    }

    match vault::write_body(&path, body) {
        Ok(()) => {}
        // Removed between the check and the write.
        Err(vault::VaultError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SaveOutcome::Missing);
        }
        Err(error) => return Err(error.into()),
    }
    let note = vault::read_note(&path)?;
    index::upsert_note(conn, &note.file, &note.body, now_ms(), device)?;
    Ok(SaveOutcome::Saved(note))
}

/// Read a body from the vault through the index. `None` means the id is no
/// longer indexed (deleted or renamed since the list was read). The hash is the
/// one the editor sends back on save.
fn open_note(conn: &Connection, id: &str) -> Result<Option<(String, u64)>, StoreError> {
    let Some(path) = index::note_path(conn, id)? else {
        return Ok(None);
    };
    let note = vault::read_note(&path)?;
    Ok(Some((note.body, note.file.hash)))
}

fn device_id(conn: &Connection) -> Result<String, StoreError> {
    if let Some(value) = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'device_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(value);
    }

    let id = ItemId::new().to_string();
    conn.execute(
        "INSERT INTO meta(key, value) VALUES ('device_id', ?1)",
        [&id],
    )?;
    Ok(id)
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn publish(inbox: &Arc<Mutex<VecDeque<Evt>>>, notify: &dyn Notify, event: Evt) {
    let mut queue = match inbox.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    queue.push_back(event);
    drop(queue);
    notify.notify();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    struct Chan(Sender<()>);

    impl Notify for Chan {
        fn notify(&self) {
            let _ = self.0.send(());
        }
    }

    fn wait(handle: &StoreHandle, rx: &Receiver<()>) -> Vec<Evt> {
        for _ in 0..200 {
            let events = handle.drain();
            if !events.is_empty() {
                return events;
            }
            let _ = rx.recv_timeout(Duration::from_millis(25));
        }
        panic!("worker produced no events");
    }

    #[test]
    fn indexes_a_vault_and_saves_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        fs::create_dir_all(&vault_dir).unwrap();
        fs::write(vault_dir.join("a.md"), "# A\n\nhello plan\n").unwrap();
        let db_path = dir.path().join("notex.db");

        let (tx, rx) = mpsc::channel();
        let worker = spawn(&db_path, Chan(tx)).unwrap();
        let handle = worker.handle();

        handle.send(Cmd::Ping).unwrap();
        assert_eq!(wait(&handle, &rx), vec![Evt::Pong]);

        handle
            .send(Cmd::Reindex {
                root: vault_dir.clone(),
            })
            .unwrap();
        assert_eq!(wait(&handle, &rx), vec![Evt::Indexed { count: 1 }]);

        handle.send(Cmd::ListNotes).unwrap();
        let listed = wait(&handle, &rx);
        let [Evt::Notes(notes)] = listed.as_slice() else {
            panic!("expected a note list, got {listed:?}");
        };
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "A");
        let id = notes[0].id.clone();

        handle.send(Cmd::OpenNote { id: id.clone() }).unwrap();
        let opened = wait(&handle, &rx);
        let [
            Evt::Opened {
                id: opened_id,
                body,
                hash,
            },
        ] = opened.as_slice()
        else {
            panic!("expected an opened note, got {opened:?}");
        };
        assert_eq!(opened_id, &id);
        assert_eq!(body, "# A\n\nhello plan\n");

        handle
            .send(Cmd::OpenNote {
                id: "missing".to_owned(),
            })
            .unwrap();
        assert!(matches!(
            wait(&handle, &rx).as_slice(),
            [Evt::OpenFailed { .. }]
        ));

        handle
            .send(Cmd::SaveNote {
                id: id.clone(),
                body: "# A\n\ntask list\n".to_owned(),
                base_hash: *hash,
                token: 7,
                force: false,
            })
            .unwrap();
        let saved = wait(&handle, &rx);
        let [
            Evt::Saved {
                token,
                title,
                hash: saved_hash,
                ..
            },
        ] = saved.as_slice()
        else {
            panic!("expected a save, got {saved:?}");
        };
        assert_eq!(*token, 7);
        assert_eq!(title, "A");
        assert_ne!(*saved_hash, *hash, "the content changed");

        // The body was replaced; the frontmatter the vault adopted was kept.
        let on_disk = fs::read_to_string(vault_dir.join("a.md")).unwrap();
        assert!(
            on_disk.contains("id:"),
            "the id survived the save: {on_disk}"
        );
        assert!(on_disk.contains("task list"), "{on_disk}");

        worker.shutdown();

        let conn = db::open(&db_path).unwrap();
        assert_eq!(index::note_count(&conn).unwrap(), 1);
        assert_eq!(index::search(&conn, "task", 10).unwrap().len(), 1);
        assert_eq!(index::search(&conn, "plan", 10).unwrap().len(), 0);
    }

    #[test]
    fn a_save_from_a_stale_hash_is_a_conflict_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        fs::create_dir_all(&vault_dir).unwrap();
        let path = vault_dir.join("a.md");
        fs::write(&path, "# A\n\nfirst\n").unwrap();
        let db_path = dir.path().join("notex.db");

        let (tx, rx) = mpsc::channel();
        let worker = spawn(&db_path, Chan(tx)).unwrap();
        let handle = worker.handle();

        handle
            .send(Cmd::Reindex {
                root: vault_dir.clone(),
            })
            .unwrap();
        wait(&handle, &rx);
        handle.send(Cmd::ListNotes).unwrap();
        let listed = wait(&handle, &rx);
        let [Evt::Notes(notes)] = listed.as_slice() else {
            panic!("expected notes");
        };
        let id = notes[0].id.clone();

        // Someone else edits the file after the editor opened it.
        fs::write(&path, "# A\n\nfrom another editor\n").unwrap();
        let before = fs::read_to_string(&path).unwrap();

        handle
            .send(Cmd::SaveNote {
                id: id.clone(),
                body: "# A\n\nmy draft\n".to_owned(),
                base_hash: 0,
                token: 1,
                force: false,
            })
            .unwrap();
        assert!(matches!(
            wait(&handle, &rx).as_slice(),
            [Evt::SaveConflict { id: conflicted, .. }] if conflicted == &id
        ));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before,
            "a conflict never touches the file"
        );

        // Explicit overwrite goes through.
        handle
            .send(Cmd::SaveNote {
                id,
                body: "# A\n\nmy draft\n".to_owned(),
                base_hash: 0,
                token: 2,
                force: true,
            })
            .unwrap();
        assert!(matches!(
            wait(&handle, &rx).as_slice(),
            [Evt::Saved { token: 2, .. }]
        ));
        assert!(fs::read_to_string(&path).unwrap().contains("my draft"));
        worker.shutdown();
    }

    #[test]
    fn reindexing_an_unchanged_vault_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        fs::create_dir_all(&vault_dir).unwrap();
        fs::write(vault_dir.join("a.md"), "# A\n\nsteady\n").unwrap();
        let db_path = dir.path().join("notex.db");

        let (tx, rx) = mpsc::channel();
        let worker = spawn(&db_path, Chan(tx)).unwrap();
        let handle = worker.handle();

        for _ in 0..2 {
            handle
                .send(Cmd::Reindex {
                    root: vault_dir.clone(),
                })
                .unwrap();
            assert_eq!(wait(&handle, &rx), vec![Evt::Indexed { count: 1 }]);
        }
        worker.shutdown();

        let conn = db::open(&db_path).unwrap();
        let logged: i64 = conn
            .query_row(
                "SELECT count(*) FROM oplog WHERE entity = 'note'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(logged, 1, "the second scan wrote no oplog rows");
        let rev: i64 = conn
            .query_row("SELECT rev FROM note_index", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rev, 1, "an unchanged note keeps its revision");
    }

    #[test]
    fn a_save_of_a_missing_note_fails_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        fs::create_dir_all(&vault_dir).unwrap();
        fs::write(vault_dir.join("a.md"), "# A\n\nhere\n").unwrap();
        let db_path = dir.path().join("notex.db");
        let (tx, rx) = mpsc::channel();
        let worker = spawn(&db_path, Chan(tx)).unwrap();
        let handle = worker.handle();

        handle
            .send(Cmd::Reindex {
                root: vault_dir.clone(),
            })
            .unwrap();
        wait(&handle, &rx);
        handle.send(Cmd::ListNotes).unwrap();
        let listed = wait(&handle, &rx);
        let [Evt::Notes(notes)] = listed.as_slice() else {
            panic!("expected notes");
        };
        let id = notes[0].id.clone();

        // The file disappears between the list and the save.
        fs::remove_file(vault_dir.join("a.md")).unwrap();
        handle
            .send(Cmd::SaveNote {
                id,
                body: "# A\n\nlost\n".to_owned(),
                base_hash: 0,
                token: 1,
                force: false,
            })
            .unwrap();
        let saved = wait(&handle, &rx);
        let [Evt::SaveFailed { token, err, .. }] = saved.as_slice() else {
            panic!("expected a failed save, got {saved:?}");
        };
        assert_eq!(*token, 1);
        assert!(
            err.contains("no longer in the vault"),
            "a deleted file reads as a missing note, not an io error: {err}"
        );

        // An id the index never had is a typed failure, not a panic.
        handle
            .send(Cmd::SaveNote {
                id: "missing".to_owned(),
                body: "x".to_owned(),
                base_hash: 0,
                token: 2,
                force: false,
            })
            .unwrap();
        assert!(matches!(
            wait(&handle, &rx).as_slice(),
            [Evt::SaveFailed { token: 2, .. }]
        ));

        // The worker still answers after a failure.
        handle.send(Cmd::Ping).unwrap();
        assert_eq!(wait(&handle, &rx), vec![Evt::Pong]);
        worker.shutdown();
    }
}
