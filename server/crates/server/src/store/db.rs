//! Opening connections with the pragmas every connection needs.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::Result;

const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);
const JOURNAL_SIZE_LIMIT: i64 = 64 * 1024 * 1024;

/// The single write connection. Creates the file on first use.
pub fn open_writer(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    // Only takes effect before the first table exists, which is exactly when it matters.
    conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(super::StoreError::Corrupt(format!("journal_mode is {mode}, not wal")));
    }
    shared_pragmas(&conn)?;
    Ok(conn)
}

/// A short-lived read connection (requests, `spawn_blocking`).
pub fn open_reader(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    shared_pragmas(&conn)?;
    Ok(conn)
}

/// A read-write connection used for one job (the nightly backup) beside the writer.
pub fn open_side(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    shared_pragmas(&conn)?;
    Ok(conn)
}

fn shared_pragmas(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_size_limit", JOURNAL_SIZE_LIMIT)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_gets_the_pragmas() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_writer(&dir.path().join("t.db")).unwrap();
        let get =
            |name: &str| conn.pragma_query_value(None, name, |row| row.get::<_, rusqlite::types::Value>(0)).unwrap();
        assert_eq!(get("journal_mode"), rusqlite::types::Value::Text("wal".into()));
        assert_eq!(get("synchronous"), rusqlite::types::Value::Integer(1));
        assert_eq!(get("foreign_keys"), rusqlite::types::Value::Integer(1));
        assert_eq!(get("auto_vacuum"), rusqlite::types::Value::Integer(2));
        assert_eq!(get("busy_timeout"), rusqlite::types::Value::Integer(5000));
        assert_eq!(get("journal_size_limit"), rusqlite::types::Value::Integer(JOURNAL_SIZE_LIMIT));
    }
}
