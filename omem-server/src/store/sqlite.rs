use std::sync::Mutex;

use rusqlite::Connection;

use crate::domain::error::OmemError;

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn new(db_path: &str) -> Result<Self, OmemError> {
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| OmemError::Storage(e.to_string()))?;
        }
        let conn = Connection::open(db_path)
            .map_err(|e| OmemError::Storage(format!("Failed to open SQLite: {}", e)))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| OmemError::Storage(format!("Failed to set pragmas: {}", e)))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn new_in_memory() -> Result<Self, OmemError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| OmemError::Storage(format!("Failed to open in-memory SQLite: {}", e)))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn init_tables(&self) -> Result<(), OmemError> {
        Ok(())
    }

    pub fn conn(&self) -> &Mutex<Connection> {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_store_in_memory() {
        let store = SqliteStore::new_in_memory().unwrap();
        store.init_tables().unwrap();
    }

    #[test]
    fn test_sqlite_store_wal_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let store = SqliteStore::new(path.to_str().unwrap()).unwrap();
        let conn = store.conn().lock().unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }
}
