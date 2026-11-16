//! Forward-only migrations tracked by `PRAGMA user_version`.
//!
//! Each file runs inside one transaction, so a failed migration leaves the
//! database at the version it started from.

use rusqlite::Connection;

use crate::db::StoreError;

struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "0001_init",
    sql: include_str!("../migrations/0001_init.sql"),
}];

pub const LATEST_VERSION: u32 = 1;

pub fn run(conn: &Connection) -> Result<u32, StoreError> {
    let mut current: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    for migration in MIGRATIONS {
        if migration.version <= current {
            continue;
        }

        conn.execute_batch("BEGIN IMMEDIATE;")?;
        match conn.execute_batch(migration.sql) {
            Ok(()) => {
                conn.execute_batch(&format!("PRAGMA user_version = {};", migration.version))?;
                conn.execute_batch("COMMIT;")?;
                current = migration.version;
            }
            Err(source) => {
                conn.execute_batch("ROLLBACK;")?;
                return Err(StoreError::Migration {
                    version: migration.version,
                    name: migration.name.to_owned(),
                    source,
                });
            }
        }
    }

    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn migrations_are_idempotent() {
        let conn = db::open_in_memory().unwrap();
        let first = run(&conn).unwrap();
        let second = run(&conn).unwrap();
        assert_eq!(first, LATEST_VERSION);
        assert_eq!(second, LATEST_VERSION);
    }

    #[test]
    fn migration_preserves_existing_rows() {
        let conn = db::open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO meta(key, value) VALUES ('device_id', 'test-device');
             INSERT INTO lists(id, name, position, created_at, updated_at)
               VALUES ('l1', 'Inbox', 0, 1, 1);",
        )
        .unwrap();

        run(&conn).unwrap();

        let name: String = conn
            .query_row("SELECT name FROM lists WHERE id = 'l1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(name, "Inbox");
    }

    #[test]
    fn a_failed_migration_rolls_back() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
        let result =
            conn.execute_batch("CREATE TABLE ok(id INTEGER); CREATE TABLE ok(id INTEGER);");
        assert!(result.is_err());
        conn.execute_batch("ROLLBACK;").unwrap();

        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0);
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'ok'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0, "the whole migration rolled back");
    }
}
