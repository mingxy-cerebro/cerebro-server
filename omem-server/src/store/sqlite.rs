use std::sync::Mutex;

use rusqlite::Connection;

use crate::domain::error::OmemError;
use crate::store::sqlite_schema;

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
        let conn = self.conn.lock()
            .map_err(|e| OmemError::Storage(format!("SQLite lock error: {}", e)))?;
        sqlite_schema::create_tables(&conn)?;
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

    #[test]
    fn test_init_tables_with_schema() {
        let store = SqliteStore::new_in_memory().unwrap();
        store.init_tables().unwrap();

        let conn = store.conn().lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('categories', 'category_aliases')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "init_tables should create both categories and category_aliases tables");
    }

    #[test]
    fn test_crud_operations() {
        let store = SqliteStore::new_in_memory().unwrap();
        let conn = store.conn().lock().unwrap();
        sqlite_schema::create_tables(&conn).unwrap();
        drop(conn);

        let conn = store.conn().lock().unwrap();

        conn.execute(
            "INSERT INTO categories (name, tenant_id, display_name, description, always_merge, append_only, temporal_versioned, merge_supported, admission_weight, importance_base, default_visibility, default_scope, sort_order, is_active, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params!["test_cat", "t1", "Test", "A test category", false, false, false, false, 0.5f32, 0.5f32, "global", "global", 0, true, "2025-01-01T00:00:00Z", "2025-01-01T00:00:00Z"],
        ).unwrap();

        let name: String = conn
            .query_row(
                "SELECT name FROM categories WHERE tenant_id = 't1' AND name = 'test_cat'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "test_cat");

        conn.execute(
            "UPDATE categories SET display_name = 'Updated' WHERE name = 'test_cat' AND tenant_id = 't1'",
            [],
        )
        .unwrap();
        let display: String = conn
            .query_row(
                "SELECT display_name FROM categories WHERE name = 'test_cat' AND tenant_id = 't1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(display, "Updated");

        conn.execute(
            "DELETE FROM categories WHERE name = 'test_cat' AND tenant_id = 't1'",
            [],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = 'test_cat' AND tenant_id = 't1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
