//! Storage for openNoteX.
//!
//! Two responsibilities, deliberately separate:
//!
//! - [`vault`]: Markdown files are the source of truth for note bodies. This
//!   module writes them atomically, parses frontmatter and scans the tree.
//! - [`db`]: SQLite holds a rebuildable index of those files plus the structured
//!   data (tasks, habits, reminders) and an append-only oplog.
//!
//! [`worker`] is the only place that owns a `Connection`. The UI talks to
//! [`worker::StoreHandle`].

pub mod db;
pub mod index;
pub mod migrate;
pub mod oplog;
pub mod vault;
pub mod worker;

pub use db::{StoreError, assert_outside_vault, open, open_in_memory};
pub use index::{SearchHit, delete_note, search, upsert_note};
pub use vault::{Frontmatter, Note, NoteFile, read_note, scan_paths, write_atomic};
pub use worker::{Cmd, Evt, Notify, StoreHandle, spawn};
