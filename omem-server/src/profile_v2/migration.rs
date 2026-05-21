use rusqlite::Connection;

use crate::domain::error::OmemError;

const CREATE_PROFILE_TABLES_SQL: &str = "
CREATE TABLE IF NOT EXISTS preferences (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    slot TEXT NOT NULL,
    value TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.5,
    scope TEXT NOT NULL DEFAULT 'project',
    project_path TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'observed',
    status TEXT NOT NULL DEFAULT 'active',
    last_reinforced_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(tenant_id, slot, value, project_path)
);
CREATE INDEX IF NOT EXISTS idx_prefs_tenant_slot ON preferences(tenant_id, slot);
CREATE INDEX IF NOT EXISTS idx_prefs_tenant_status ON preferences(tenant_id, status);

CREATE TABLE IF NOT EXISTS profile_versions (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    snapshot TEXT NOT NULL,
    preference_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_versions_tenant ON profile_versions(tenant_id);

CREATE TABLE IF NOT EXISTS profile_changelog (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    preference_id TEXT NOT NULL,
    action TEXT NOT NULL,
    old_value TEXT,
    new_value TEXT,
    source TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_changelog_tenant ON profile_changelog(tenant_id);

CREATE TABLE IF NOT EXISTS induction_runs (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    candidate_count INTEGER NOT NULL DEFAULT 0,
    extracted_count INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_induction_tenant ON induction_runs(tenant_id);

CREATE TABLE IF NOT EXISTS induction_locks (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    ttl_secs INTEGER NOT NULL DEFAULT 600
);
CREATE INDEX IF NOT EXISTS idx_locks_tenant ON induction_locks(tenant_id);
";

pub fn create_profile_tables(conn: &Connection) -> Result<(), OmemError> {
    conn.execute_batch(CREATE_PROFILE_TABLES_SQL)
        .map_err(|e| OmemError::Storage(format!("failed to create profile tables: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn creates_all_five_tables() {
        let conn = Connection::open_in_memory().unwrap();
        create_profile_tables(&conn).unwrap();

        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('preferences','profile_versions','profile_changelog','induction_runs','induction_locks')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 5);
    }

    #[test]
    fn idempotent_creation() {
        let conn = Connection::open_in_memory().unwrap();
        create_profile_tables(&conn).unwrap();
        create_profile_tables(&conn).unwrap();

        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='preferences'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1);
    }
}
