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
    SaveNote {
        path: PathBuf,
        body: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evt {
    Pong,
    Indexed { count: u64 },
    Saved { id: String, hash: u64 },
    Failed { op: &'static str, err: String },
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
        Cmd::SaveNote { path, body } => match save_note(conn, &path, &body, device) {
            Ok(note) => vec![Evt::Saved {
                id: note.file.id.clone(),
                hash: note.file.hash,
            }],
            Err(error) => vec![Evt::Failed {
                op: "save_note",
                err: error.to_string(),
            }],
        },
        Cmd::Shutdown => Vec::new(),
    }
}

fn reindex(conn: &mut Connection, root: &std::path::Path, device: &str) -> Result<u64, StoreError> {
    let paths = vault::scan_paths(root)?;
    let mut count = 0_u64;
    for path in paths {
        let note = vault::read_note(&path)?;
        index::upsert_note(conn, &note.file, &note.body, now_ms(), device)?;
        count += 1;
    }
    Ok(count)
}

fn save_note(
    conn: &mut Connection,
    path: &std::path::Path,
    body: &str,
    device: &str,
) -> Result<Note, StoreError> {
    vault::write_atomic(path, body)?;
    let note = vault::read_note(path)?;
    index::upsert_note(conn, &note.file, &note.body, now_ms(), device)?;
    Ok(note)
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

        handle
            .send(Cmd::SaveNote {
                path: vault_dir.join("b.md"),
                body: "# B\n\ntask list\n".to_owned(),
            })
            .unwrap();
        let saved = wait(&handle, &rx);
        assert!(matches!(saved.as_slice(), [Evt::Saved { .. }]));

        worker.shutdown();

        let conn = db::open(&db_path).unwrap();
        assert_eq!(index::note_count(&conn).unwrap(), 2);
        assert_eq!(index::search(&conn, "task", 10).unwrap().len(), 1);
        assert_eq!(index::search(&conn, "plan", 10).unwrap().len(), 1);
    }

    #[test]
    fn a_bad_save_reports_an_error_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("notex.db");
        let (tx, rx) = mpsc::channel();
        let worker = spawn(&db_path, Chan(tx)).unwrap();
        let handle = worker.handle();

        // A path whose parent is a file cannot be created.
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "not a directory").unwrap();

        handle
            .send(Cmd::SaveNote {
                path: blocker.join("note.md"),
                body: "# Nope\n".to_owned(),
            })
            .unwrap();
        let events = wait(&handle, &rx);
        assert!(matches!(
            events.as_slice(),
            [Evt::Failed {
                op: "save_note",
                ..
            }]
        ));

        // The worker still answers after a failure.
        handle.send(Cmd::Ping).unwrap();
        assert_eq!(wait(&handle, &rx), vec![Evt::Pong]);
        worker.shutdown();
    }
}
