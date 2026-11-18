//! The derived note index: metadata rows plus FTS5 content.
//!
//! Everything here can be rebuilt from the vault. Writes happen in one
//! transaction with their oplog row, so the index and the history cannot drift.

use notex_core::ItemId;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db::StoreError;
use crate::oplog;
use crate::vault::NoteFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub snippet: String,
}

/// One row of the notes list. The body is not here: it lives in the vault and is
/// read on open, so a list never holds a whole vault in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSummary {
    pub id: String,
    pub path: std::path::PathBuf,
    pub title: String,
    pub tags: Vec<String>,
}

pub fn upsert_note(
    conn: &mut Connection,
    note: &NoteFile,
    body: &str,
    now: i64,
    device_id: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    upsert_in(&tx, note, body, now, device_id)?;
    tx.commit()?;
    Ok(())
}

/// Many upserts in one transaction, for a reindex. One commit for the whole
/// vault instead of one per file turns a 10k-note rebuild from seconds into
/// frames; a crash still leaves the old index, because the transaction rolls back.
pub struct Batch<'a> {
    tx: rusqlite::Transaction<'a>,
    now: i64,
    device_id: &'a str,
    count: u64,
}

impl<'a> Batch<'a> {
    pub fn new(conn: &'a mut Connection, now: i64, device_id: &'a str) -> Result<Self, StoreError> {
        Ok(Self {
            tx: conn.transaction()?,
            now,
            device_id,
            count: 0,
        })
    }

    pub fn upsert(&mut self, note: &NoteFile, body: &str) -> Result<(), StoreError> {
        // A reindex runs at every launch. Reading a file is cheap; rewriting an
        // unchanged row is not, and would bump `rev` and append an oplog row for
        // every note each time the app opens.
        let unchanged: Option<(i64, String, Option<i64>)> = self
            .tx
            .prepare_cached("SELECT hash, path, fts_rowid FROM note_index WHERE id = ?1")?
            .query_row(params![note.id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?;
        let same_file = matches!(
            &unchanged,
            Some((hash, path, Some(_)))
                if *hash == note.hash as i64
                    && path.as_str() == note.path.to_string_lossy()
        );

        if !same_file {
            upsert_in(&self.tx, note, body, self.now, self.device_id)?;
        }
        self.count += 1;
        Ok(())
    }

    /// Drop rows whose file the scan did not find, which is how a note deleted
    /// or renamed outside the app leaves the list. Each removal writes its oplog
    /// row, like any other delete.
    pub fn prune(
        &mut self,
        keep: &std::collections::HashSet<std::path::PathBuf>,
    ) -> Result<u64, StoreError> {
        let rows: Vec<(String, String, Option<i64>, i64)> = self
            .tx
            .prepare("SELECT id, path, fts_rowid, rev FROM note_index")?
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut pruned = 0;
        for (id, path, fts_rowid, rev) in rows {
            if keep.contains(std::path::Path::new(&path)) {
                continue;
            }
            if let Some(rowid) = fts_rowid {
                self.tx
                    .prepare_cached("DELETE FROM notes_fts WHERE rowid = ?1")?
                    .execute(params![rowid])?;
            }
            self.tx
                .prepare_cached("DELETE FROM note_index WHERE id = ?1")?
                .execute(params![id])?;
            self.tx
                .prepare_cached("DELETE FROM item_tags WHERE item_type = 'note' AND item_id = ?1")?
                .execute(params![id])?;
            oplog::append(
                &self.tx,
                oplog::Entry {
                    entity: "note",
                    entity_id: &id,
                    op: "delete",
                    payload: "{}",
                    at: self.now,
                    device_id: self.device_id,
                    rev: rev + 1,
                },
            )?;
            pruned += 1;
        }
        Ok(pruned)
    }

    /// Commit the batch and report how many notes it wrote.
    pub fn commit(self) -> Result<u64, StoreError> {
        self.tx.commit()?;
        Ok(self.count)
    }
}

/// Rebuild the index from the vault: one transaction, one oplog row per changed
/// note, and rows whose file the scan did not find pruned. This is the whole
/// `--reindex` path, shared by the worker and `xtask bench`, so a budget
/// measures the code that ships.
pub fn reindex_vault(
    conn: &mut Connection,
    root: &std::path::Path,
    now: i64,
    device_id: &str,
) -> Result<u64, StoreError> {
    let paths = crate::vault::scan_paths(root)?;
    let mut batch = Batch::new(conn, now, device_id)?;
    let mut keep = std::collections::HashSet::with_capacity(paths.len());
    for path in paths {
        let note = crate::vault::read_note(&path)?;
        batch.upsert(&note.file, &note.body)?;
        keep.insert(note.file.path);
    }
    // A file deleted or renamed outside the app leaves the list here: the scan
    // is the truth, and a row it did not find is not a note any more.
    batch.prune(&keep)?;
    batch.commit()
}

fn upsert_in(
    tx: &rusqlite::Transaction<'_>,
    note: &NoteFile,
    body: &str,
    now: i64,
    device_id: &str,
) -> Result<(), StoreError> {
    let path = note.path.to_string_lossy().to_string();
    let tags = serde_json::to_string(&note.tags).unwrap_or_else(|_| "[]".to_owned());

    // Delete the old search row by its rowid: `item_id` is UNINDEXED, so
    // deleting by it would scan the whole FTS table for every note. Statements
    // are cached: a reindex runs this function once per file.
    let fts_rowid: Option<i64> = tx
        .prepare_cached("SELECT fts_rowid FROM note_index WHERE id = ?1")?
        .query_row(params![note.id], |row| row.get(0))
        .optional()?;
    if let Some(rowid) = fts_rowid {
        tx.prepare_cached("DELETE FROM notes_fts WHERE rowid = ?1")?
            .execute(params![rowid])?;
    }

    // A rename leaves the old path on a different id; that row is gone.
    tx.prepare_cached("DELETE FROM note_index WHERE path = ?1 AND id <> ?2")?
        .execute(params![path, note.id])?;

    tx.prepare_cached(
        "INSERT INTO note_index(id, path, hash, mtime_ns, size, title, tags,
                                created_at, updated_at, rev)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1)
         ON CONFLICT(id) DO UPDATE SET
             path = excluded.path,
             hash = excluded.hash,
             mtime_ns = excluded.mtime_ns,
             size = excluded.size,
             title = excluded.title,
             tags = excluded.tags,
             updated_at = excluded.updated_at,
             rev = note_index.rev + 1",
    )?
    .execute(params![
        note.id,
        path,
        note.hash as i64,
        note.mtime_ns,
        note.size,
        note.title,
        tags,
        now
    ])?;

    tx.prepare_cached("INSERT INTO notes_fts(item_id, title, body) VALUES (?1, ?2, ?3)")?
        .execute(params![note.id, note.title, body])?;
    let fts_rowid = tx.last_insert_rowid();
    tx.prepare_cached("UPDATE note_index SET fts_rowid = ?1 WHERE id = ?2")?
        .execute(params![fts_rowid, note.id])?;

    tx.prepare_cached("DELETE FROM item_tags WHERE item_type = 'note' AND item_id = ?1")?
        .execute(params![note.id])?;
    for tag in &note.tags {
        let tag_id: String = match tx
            .prepare_cached("SELECT id FROM tags WHERE name = ?1")?
            .query_row(params![tag], |row| row.get(0))
            .optional()?
        {
            Some(id) => id,
            None => {
                let id = ItemId::new().to_string();
                tx.prepare_cached("INSERT INTO tags(id, name) VALUES (?1, ?2)")?
                    .execute(params![id, tag])?;
                id
            }
        };
        tx.prepare_cached(
            "INSERT OR IGNORE INTO item_tags(tag_id, item_type, item_id)
             VALUES (?1, 'note', ?2)",
        )?
        .execute(params![tag_id, note.id])?;
    }

    let rev: i64 = tx
        .prepare_cached("SELECT rev FROM note_index WHERE id = ?1")?
        .query_row(params![note.id], |row| row.get(0))?;
    let payload = serde_json::json!({ "path": path, "hash": note.hash }).to_string();
    oplog::append(
        tx,
        oplog::Entry {
            entity: "note",
            entity_id: &note.id,
            op: "upsert",
            payload: &payload,
            at: now,
            device_id,
            rev,
        },
    )?;

    Ok(())
}

pub fn delete_note(
    conn: &mut Connection,
    id: &str,
    now: i64,
    device_id: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    let existing: Option<(i64, Option<i64>)> = tx
        .query_row(
            "SELECT rev, fts_rowid FROM note_index WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (rev, fts_rowid) = existing.unwrap_or((0, None));

    if let Some(rowid) = fts_rowid {
        tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", params![rowid])?;
    }
    tx.execute("DELETE FROM note_index WHERE id = ?1", params![id])?;
    tx.execute(
        "DELETE FROM item_tags WHERE item_type = 'note' AND item_id = ?1",
        params![id],
    )?;
    oplog::append(
        &tx,
        oplog::Entry {
            entity: "note",
            entity_id: id,
            op: "delete",
            payload: "{}",
            at: now,
            device_id,
            rev: rev + 1,
        },
    )?;
    tx.commit()?;
    Ok(())
}

/// Full-text search. Each token is quoted and turned into a prefix term, which
/// keeps user input from being parsed as FTS5 syntax.
pub fn search(conn: &Connection, raw: &str, limit: u32) -> Result<Vec<SearchHit>, StoreError> {
    let Some(query) = fts_query(raw) else {
        return Ok(Vec::new());
    };

    let mut statement = conn.prepare_cached(
        "SELECT item_id, title, snippet(notes_fts, 2, '[', ']', '…', 8)
         FROM notes_fts
         WHERE notes_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;

    let hits = statement
        .query_map(params![query, limit], |row| {
            Ok(SearchHit {
                id: row.get(0)?,
                title: row.get(1)?,
                snippet: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(hits)
}

pub fn fts_query(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split_whitespace()
        .filter_map(|token| {
            let cleaned: String = token
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if cleaned.is_empty() {
                None
            } else {
                Some(format!("\"{cleaned}\"*"))
            }
        })
        .collect();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

pub fn note_count(conn: &Connection) -> Result<i64, StoreError> {
    let count = conn.query_row("SELECT count(*) FROM note_index", [], |row| row.get(0))?;
    Ok(count)
}

/// Every indexed note, alphabetical by title. This is the list pane's read model:
/// small enough to hold, and cheap to rebuild from the vault.
pub fn notes(conn: &Connection) -> Result<Vec<NoteSummary>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT id, path, title, tags
         FROM note_index
         ORDER BY title COLLATE NOCASE, path",
    )?;

    let notes = statement
        .query_map([], |row| {
            let tags: String = row.get(3)?;
            Ok(NoteSummary {
                id: row.get(0)?,
                path: std::path::PathBuf::from(row.get::<_, String>(1)?),
                title: row.get(2)?,
                tags: serde_json::from_str(&tags).unwrap_or_default(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(notes)
}

/// The title the index has for a note. A save that changes it renames the file.
pub fn note_title(conn: &Connection, id: &str) -> Result<Option<String>, StoreError> {
    let title = conn
        .query_row(
            "SELECT title FROM note_index WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(title)
}

/// The file an indexed id names, or `None` when the note is gone from the index.
pub fn note_path(conn: &Connection, id: &str) -> Result<Option<std::path::PathBuf>, StoreError> {
    let path = conn
        .query_row(
            "SELECT path FROM note_index WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(path.map(std::path::PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::vault::content_hash;
    use std::path::PathBuf;

    fn note(id: &str, title: &str) -> NoteFile {
        NoteFile {
            path: PathBuf::from(format!("/vault/{id}.md")),
            id: id.to_owned(),
            title: title.to_owned(),
            tags: vec!["work".to_owned()],
            hash: content_hash(title.as_bytes()),
            mtime_ns: 1,
            size: 10,
        }
    }

    #[test]
    fn upsert_indexes_search_and_logs() {
        let mut conn = db::open_in_memory().unwrap();
        let n = note("01ARZ3NDEKTSV4RRFFQ69G5FAV", "Weekly review");
        upsert_note(&mut conn, &n, "plan the launch", 1_700_000_000_000, "dev").unwrap();

        let hits = search(&conn, "launch", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Weekly review");
        assert!(hits[0].snippet.contains("launch"));
        assert_eq!(note_count(&conn).unwrap(), 1);

        let logged: i64 = conn
            .query_row(
                "SELECT count(*) FROM oplog WHERE entity = 'note'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(logged, 1);

        let tag_rows: i64 = conn
            .query_row("SELECT count(*) FROM item_tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tag_rows, 1, "the note's tag is linked");
    }

    #[test]
    fn updating_a_note_replaces_its_search_text() {
        let mut conn = db::open_in_memory().unwrap();
        let n = note("01ARZ3NDEKTSV4RRFFQ69G5FAV", "Weekly review");
        upsert_note(&mut conn, &n, "plan the launch", 1, "dev").unwrap();
        upsert_note(&mut conn, &n, "shipping notes", 2, "dev").unwrap();

        assert!(search(&conn, "launch", 10).unwrap().is_empty());
        assert_eq!(search(&conn, "shipping", 10).unwrap().len(), 1);
        assert_eq!(note_count(&conn).unwrap(), 1, "upsert does not duplicate");

        let rev: i64 = conn
            .query_row("SELECT rev FROM note_index", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rev, 2);
    }

    #[test]
    fn deleting_removes_the_row_and_its_search_text() {
        let mut conn = db::open_in_memory().unwrap();
        let n = note("01ARZ3NDEKTSV4RRFFQ69G5FAV", "Weekly review");
        upsert_note(&mut conn, &n, "plan the launch", 1, "dev").unwrap();
        delete_note(&mut conn, &n.id, 2, "dev").unwrap();

        assert_eq!(note_count(&conn).unwrap(), 0);
        assert!(search(&conn, "launch", 10).unwrap().is_empty());
        let deletes: i64 = conn
            .query_row(
                "SELECT count(*) FROM oplog WHERE op = 'delete'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(deletes, 1);
    }

    #[test]
    fn a_reindex_prunes_the_rows_of_files_that_are_gone() {
        use std::collections::HashSet;

        let mut conn = db::open_in_memory().unwrap();
        let kept = note("01ARZ3NDEKTSV4RRFFQ69G5FAV", "Weekly review");
        let gone = NoteFile {
            path: PathBuf::from("/vault/gone.md"),
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAW".to_owned(),
            ..note("01ARZ3NDEKTSV4RRFFQ69G5FAW", "Gone")
        };
        let mut batch = Batch::new(&mut conn, 1, "dev").unwrap();
        batch.upsert(&kept, "still here").unwrap();
        batch.upsert(&gone, "deleted outside the app").unwrap();
        batch.commit().unwrap();

        let mut keep = HashSet::new();
        keep.insert(kept.path.clone());
        let mut batch = Batch::new(&mut conn, 2, "dev").unwrap();
        assert_eq!(batch.prune(&keep).unwrap(), 1);
        let written = batch.commit().unwrap();
        assert_eq!(written, 0);

        assert_eq!(note_count(&conn).unwrap(), 1);
        assert_eq!(
            note_title(&conn, &kept.id).unwrap().as_deref(),
            Some("Weekly review")
        );
        assert!(note_title(&conn, &gone.id).unwrap().is_none());
        assert_eq!(
            search(&conn, "deleted", 10).unwrap().len(),
            0,
            "the pruned note leaves the search index"
        );
        let deletes: i64 = conn
            .query_row(
                "SELECT count(*) FROM oplog WHERE op = 'delete'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(deletes, 1, "a prune is logged like any delete");
    }

    #[test]
    fn search_input_cannot_inject_fts_syntax() {
        let mut conn = db::open_in_memory().unwrap();
        let n = note("01ARZ3NDEKTSV4RRFFQ69G5FAV", "Weekly review");
        upsert_note(&mut conn, &n, "plan the launch", 1, "dev").unwrap();

        // These would be syntax errors or wildcards without quoting.
        for hostile in ["\"", "NEAR(", "*", "a OR b", "title:"] {
            let _ = search(&conn, hostile, 10).expect("no error");
        }
        assert_eq!(search(&conn, "   ", 10).unwrap().len(), 0);
    }

    #[test]
    fn renamed_notes_keep_one_row() {
        let mut conn = db::open_in_memory().unwrap();
        let mut n = note("01ARZ3NDEKTSV4RRFFQ69G5FAV", "Weekly review");
        upsert_note(&mut conn, &n, "body", 1, "dev").unwrap();
        n.path = PathBuf::from("/vault/renamed.md");
        upsert_note(&mut conn, &n, "body", 2, "dev").unwrap();

        assert_eq!(note_count(&conn).unwrap(), 1);
        let path: String = conn
            .query_row("SELECT path FROM note_index", [], |row| row.get(0))
            .unwrap();
        assert_eq!(path, "/vault/renamed.md");
    }

    #[test]
    fn the_note_list_is_alphabetical_and_carries_its_tags() {
        let mut conn = db::open_in_memory().unwrap();
        upsert_note(&mut conn, &note("01A", "zebra"), "z", 1, "dev").unwrap();
        upsert_note(&mut conn, &note("01B", "Apple"), "a", 2, "dev").unwrap();
        let mut tagged = note("01C", "mango");
        tagged.tags = vec!["work".to_owned(), "review".to_owned()];
        upsert_note(&mut conn, &tagged, "m", 3, "dev").unwrap();

        let notes = notes(&conn).unwrap();
        let titles: Vec<&str> = notes.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, vec!["Apple", "mango", "zebra"], "case-insensitive");
        let mango = notes.iter().find(|n| n.id == "01C").unwrap();
        assert_eq!(mango.tags, vec!["work", "review"]);
        assert_eq!(mango.path, PathBuf::from("/vault/01C.md"));
    }

    #[test]
    fn a_batch_commits_every_note_in_one_transaction() {
        let mut conn = db::open_in_memory().unwrap();
        let mut batch = Batch::new(&mut conn, 1, "dev").unwrap();
        for (id, title) in [("01A", "alpha"), ("01B", "beta"), ("01C", "gamma")] {
            let n = note(id, title);
            batch.upsert(&n, "launch").unwrap();
        }
        assert_eq!(batch.commit().unwrap(), 3);

        assert_eq!(note_count(&conn).unwrap(), 3);
        assert_eq!(search(&conn, "launch", 10).unwrap().len(), 3);
        let logged: i64 = conn
            .query_row(
                "SELECT count(*) FROM oplog WHERE entity = 'note'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(logged, 3, "every note logs its oplog row in the batch");
    }

    #[test]
    fn a_note_path_is_none_for_an_unknown_id() {
        let mut conn = db::open_in_memory().unwrap();
        upsert_note(&mut conn, &note("01A", "known"), "x", 1, "dev").unwrap();
        assert_eq!(
            note_path(&conn, "01A").unwrap(),
            Some(PathBuf::from("/vault/01A.md"))
        );
        assert!(note_path(&conn, "missing").unwrap().is_none());
    }
}
