//! Opening and configuring the database.

use std::path::Path;

use rusqlite::Connection;
use thiserror::Error;

use crate::migrate;
use crate::vault::VaultError;

/// WAL for crash safety and one reader/one writer, an 8 MiB page cache so a
/// desktop app stays light, and a bounded journal so the `.db-wal` file cannot
/// grow without limit.
pub const PRAGMAS: &str = "\
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -8000;
PRAGMA temp_store = MEMORY;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA journal_size_limit = 8388608;
";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("migration {version} ({name}) failed: {source}")]
    Migration {
        version: u32,
        name: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error("vault: {0}")]
    Vault(#[from] VaultError),
    #[error("the database must not live inside the vault: {0}")]
    DatabaseInsideVault(String),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("worker stopped")]
    WorkerStopped,
}

/// Open a database, apply pragmas and run migrations.
pub fn open(path: impl AsRef<Path>) -> Result<Connection, StoreError> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate::run(&conn)?;
    Ok(conn)
}

/// In-memory database with the same pragmas and schema, for tests.
pub fn open_in_memory() -> Result<Connection, StoreError> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate::run(&conn)?;
    Ok(conn)
}

pub fn configure(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(PRAGMAS)?;
    Ok(())
}

/// Guards the invariant that the vault is the user's folder and the database is
/// ours: a database inside the vault would be synced, backed up and indexed.
pub fn assert_outside_vault(db: &Path, vault: &Path) -> Result<(), StoreError> {
    let db = db
        .canonicalize()
        .map_err(|_| StoreError::InvalidPath(db.display().to_string()))?;
    let vault = vault
        .canonicalize()
        .map_err(|_| StoreError::InvalidPath(vault.display().to_string()))?;
    if db.starts_with(&vault) {
        return Err(StoreError::DatabaseInsideVault(db.display().to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn pragmas_and_migrations_apply() {
        let conn = open_in_memory().expect("open");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal mode");
        assert!(mode.eq_ignore_ascii_case("memory") || mode.eq_ignore_ascii_case("wal"));

        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user_version");
        assert_eq!(version, migrate::LATEST_VERSION);

        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'note_index'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(tables, 1);
    }

    #[test]
    fn fts5_is_available() {
        let conn = open_in_memory().expect("open");
        conn.execute_batch("CREATE VIRTUAL TABLE probe USING fts5(body);")
            .expect("fts5 is compiled in");
    }

    #[test]
    fn rejects_a_database_inside_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        fs::create_dir(&vault).unwrap();
        let db = vault.join("notex.db");
        fs::write(&db, b"").unwrap();
        assert!(matches!(
            assert_outside_vault(&db, &vault),
            Err(StoreError::DatabaseInsideVault(_))
        ));

        let outside = dir.path().join("notex.db");
        fs::write(&outside, b"").unwrap();
        assert!(assert_outside_vault(&outside, &vault).is_ok());
    }
}
