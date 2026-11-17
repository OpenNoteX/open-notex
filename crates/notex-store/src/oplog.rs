//! Append-only operation log.
//!
//! Every mutation appends here in the same transaction as the mutation itself.
//! This gives undo and export today, and multi-device sync later, without a
//! schema migration. The log is never rewritten or pruned in v1.

use rusqlite::{Connection, params};

use crate::db::StoreError;

pub struct Entry<'a> {
    pub entity: &'a str,
    pub entity_id: &'a str,
    pub op: &'a str,
    pub payload: &'a str,
    pub at: i64,
    pub device_id: &'a str,
    pub rev: i64,
}

impl<'a> Entry<'a> {
    pub fn new(entity: &'a str, entity_id: &'a str, op: &'a str) -> Self {
        Self {
            entity,
            entity_id,
            op,
            payload: "{}",
            at: 0,
            device_id: "",
            rev: 1,
        }
    }
}

pub fn append(conn: &Connection, entry: Entry<'_>) -> Result<i64, StoreError> {
    conn.prepare_cached(
        "INSERT INTO oplog(entity, entity_id, op, payload, at, device_id, rev)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![
        entry.entity,
        entry.entity_id,
        entry.op,
        entry.payload,
        entry.at,
        entry.device_id,
        entry.rev
    ])?;
    Ok(conn.last_insert_rowid())
}

pub fn len(conn: &Connection) -> Result<i64, StoreError> {
    let count = conn.query_row("SELECT count(*) FROM oplog", [], |row| row.get(0))?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn entries_append_in_order() {
        let conn = db::open_in_memory().unwrap();

        let first = append(
            &conn,
            Entry {
                at: 1,
                device_id: "dev",
                ..Entry::new("task", "t1", "create")
            },
        )
        .unwrap();
        let second = append(
            &conn,
            Entry {
                at: 2,
                device_id: "dev",
                rev: 2,
                ..Entry::new("task", "t1", "complete")
            },
        )
        .unwrap();

        assert!(second > first);
        assert_eq!(len(&conn).unwrap(), 2);

        let op: String = conn
            .query_row("SELECT op FROM oplog WHERE seq = ?1", [second], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(op, "complete");
    }
}
