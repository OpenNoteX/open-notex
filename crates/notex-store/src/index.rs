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

pub fn upsert_note(
    conn: &mut Connection,
    note: &NoteFile,
    body: &str,
    now: i64,
    device_id: &str,
) -> Result<(), StoreError> {
    let path = note.path.to_string_lossy().to_string();
    let tags = serde_json::to_string(&note.tags).unwrap_or_else(|_| "[]".to_owned());

    let tx = conn.transaction()?;

    // A rename leaves the old path on a different id; that row is gone.
    tx.execute(
        "DELETE FROM note_index WHERE path = ?1 AND id <> ?2",
        params![path, note.id],
    )?;
    tx.execute("DELETE FROM notes_fts WHERE item_id = ?1", params![note.id])?;

    tx.execute(
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
        params![
            note.id,
            path,
            note.hash as i64,
            note.mtime_ns,
            note.size,
            note.title,
            tags,
            now
        ],
    )?;

    tx.execute(
        "INSERT INTO notes_fts(item_id, title, body) VALUES (?1, ?2, ?3)",
        params![note.id, note.title, body],
    )?;

    tx.execute(
        "DELETE FROM item_tags WHERE item_type = 'note' AND item_id = ?1",
        params![note.id],
    )?;
    for tag in &note.tags {
        let tag_id: String = match tx
            .query_row("SELECT id FROM tags WHERE name = ?1", params![tag], |row| {
                row.get(0)
            })
            .optional()?
        {
            Some(id) => id,
            None => {
                let id = ItemId::new().to_string();
                tx.execute(
                    "INSERT INTO tags(id, name) VALUES (?1, ?2)",
                    params![id, tag],
                )?;
                id
            }
        };
        tx.execute(
            "INSERT OR IGNORE INTO item_tags(tag_id, item_type, item_id) VALUES (?1, 'note', ?2)",
            params![tag_id, note.id],
        )?;
    }

    let rev: i64 = tx.query_row(
        "SELECT rev FROM note_index WHERE id = ?1",
        params![note.id],
        |row| row.get(0),
    )?;
    let payload = serde_json::json!({ "path": path, "hash": note.hash }).to_string();
    oplog::append(
        &tx,
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

    tx.commit()?;
    Ok(())
}

pub fn delete_note(
    conn: &mut Connection,
    id: &str,
    now: i64,
    device_id: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    let rev: i64 = tx
        .query_row(
            "SELECT rev FROM note_index WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);

    tx.execute("DELETE FROM note_index WHERE id = ?1", params![id])?;
    tx.execute("DELETE FROM notes_fts WHERE item_id = ?1", params![id])?;
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
}
