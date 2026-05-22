use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rusqlite::params;

use crate::domain::error::OmemError;
fn sqlite_err(e: rusqlite::Error) -> OmemError {
    OmemError::Storage(format!("sqlite error: {e}"))
}

fn prepare_stmt<'a>(conn: &'a rusqlite::Connection, sql: &str) -> Result<rusqlite::Statement<'a>, OmemError> {
    conn.prepare(sql).map_err(sqlite_err)
}

use crate::store::sqlite::SqliteStore;

use super::migration;
use super::types::{InductionLock, InductionRun, Preference, PreferenceScope, PreferenceStatus, ProfileChangelog, ProfileVersion};

struct CachedPreferences {
    prefs: Vec<Preference>,
    cached_at: Instant,
}

pub struct ProfileStore {
    sqlite: Arc<SqliteStore>,
    cache: DashMap<String, CachedPreferences>,
}

const CACHE_TTL_SECS: u64 = 1800;

impl ProfileStore {
    pub fn new(sqlite: Arc<SqliteStore>) -> Self {
        Self {
            sqlite,
            cache: DashMap::new(),
        }
    }

    /// Invalidate all cached preference lists for a tenant.
    /// Must be called after any write operation (upsert, delete, update).
    pub fn invalidate_cache(&self, tenant_id: &str) {
        let prefix = format!("{}:", tenant_id);
        let keys_to_remove: Vec<String> = self
            .cache
            .iter()
            .filter(|entry| entry.key().starts_with(&prefix) || entry.key() == tenant_id)
            .map(|entry| entry.key().clone())
            .collect();
        for key in keys_to_remove {
            self.cache.remove(&key);
        }
    }

    pub fn init(&self) -> Result<(), OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;
        migration::create_profile_tables(&conn)
    }

    pub fn get_preferences(
        &self,
        tenant_id: &str,
        project_path: Option<&str>,
    ) -> Result<Vec<Preference>, OmemError> {
        let cache_key = format!("{}:{}", tenant_id, project_path.unwrap_or(""));
        if let Some(cached) = self.cache.get(&cache_key) {
            if cached.cached_at.elapsed().as_secs() < CACHE_TTL_SECS {
                return Ok(cached.prefs.clone());
            }
        }

        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        let mut stmt = if let Some(_pp) = project_path {
            prepare_stmt(&conn, 
                "SELECT id, tenant_id, slot, value, confidence, scope, project_path, source, status, last_reinforced_at, created_at, updated_at FROM preferences WHERE tenant_id=?1 AND status!='deleted' AND (scope='global' OR project_path=?2) ORDER BY confidence DESC"
            )?
        } else {
            prepare_stmt(&conn, 
                "SELECT id, tenant_id, slot, value, confidence, scope, project_path, source, status, last_reinforced_at, created_at, updated_at FROM preferences WHERE tenant_id=?1 AND status!='deleted' ORDER BY confidence DESC"
            )?
        };

        let pp_owned = project_path.map(|s| s.to_string());
        let rows: Vec<Result<Preference, _>> = if let Some(ref pp) = pp_owned {
            stmt.query_map(params![tenant_id, pp], Self::row_to_preference).map_err(sqlite_err)?.collect()
        } else {
            stmt.query_map(params![tenant_id], Self::row_to_preference).map_err(sqlite_err)?.collect()
        };

        let mut prefs = Vec::new();
        for row in rows {
            prefs.push(row.map_err(|e| OmemError::Storage(format!("row parse error: {e}")))?);
        }
        drop(stmt);
        drop(conn);

        self.cache.insert(cache_key, CachedPreferences {
            prefs: prefs.clone(),
            cached_at: Instant::now(),
        });

        Ok(prefs)
    }

    pub fn get_preference_by_id(&self, id: &str) -> Result<Option<Preference>, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        let mut stmt = prepare_stmt(&conn, 
            "SELECT id, tenant_id, slot, value, confidence, scope, project_path, source, status, last_reinforced_at, created_at, updated_at FROM preferences WHERE id=?1"
        )?;

        let result = stmt.query_row(params![id], Self::row_to_preference);
        match result {
            Ok(pref) => Ok(Some(pref)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(OmemError::Storage(format!("query error: {e}"))),
        }
    }

    pub fn upsert_preference(&self, pref: &Preference) -> Result<Preference, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        conn.execute(
            "INSERT OR REPLACE INTO preferences (id, tenant_id, slot, value, confidence, scope, project_path, source, status, last_reinforced_at, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                pref.id, pref.tenant_id, pref.slot, pref.value, pref.confidence,
                pref.scope.as_str(), pref.project_path, pref.source, pref.status.as_str(),
                pref.last_reinforced_at.to_rfc3339(), pref.created_at.to_rfc3339(), pref.updated_at.to_rfc3339()
            ],
        ).map_err(|e| OmemError::Storage(format!("upsert error: {e}")))?;

        self.invalidate_cache(&pref.tenant_id);
        Ok(pref.clone())
    }

    pub fn delete_preference(&self, id: &str) -> Result<bool, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        let rows = conn.execute("DELETE FROM preferences WHERE id=?1", params![id])
            .map_err(|e| OmemError::Storage(format!("delete error: {e}")))?;

        Ok(rows > 0)
    }

    pub fn update_confidence(&self, id: &str, delta: f32) -> Result<(), OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        conn.execute(
            "UPDATE preferences SET confidence = MIN(0.95, MAX(0.0, confidence + ?1)), updated_at=?2 WHERE id=?3",
            params![delta, Utc::now().to_rfc3339(), id],
        ).map_err(|e| OmemError::Storage(format!("update confidence error: {e}")))?;

        Ok(())
    }

    pub fn update_status(&self, id: &str, status: &str) -> Result<(), OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        conn.execute(
            "UPDATE preferences SET status=?1, updated_at=?2 WHERE id=?3",
            params![status, Utc::now().to_rfc3339(), id],
        ).map_err(|e| OmemError::Storage(format!("update status error: {e}")))?;

        Ok(())
    }

    pub fn get_induction_lock(&self, tenant_id: &str) -> Result<Option<InductionLock>, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        self.cleanup_expired_locks_inner(&conn)?;

        let mut stmt = prepare_stmt(&conn, 
            "SELECT id, tenant_id, created_at, ttl_secs FROM induction_locks WHERE tenant_id=?1 LIMIT 1"
        )?;

        let result = stmt.query_row(params![tenant_id], |row: &rusqlite::Row<'_>| {
            let created_at_str: String = row.get(2)?;
            Ok(InductionLock {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                created_at: parse_datetime(&created_at_str),
                ttl_secs: row.get(3)?,
            })
        });

        match result {
            Ok(lock) => Ok(Some(lock)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(OmemError::Storage(format!("lock query error: {e}"))),
        }
    }

    pub fn acquire_induction_lock(&self, tenant_id: &str, ttl_secs: i32) -> Result<bool, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        self.cleanup_expired_locks_inner(&conn)?;

        let existing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM induction_locks WHERE tenant_id=?1",
            params![tenant_id],
            |row: &rusqlite::Row<'_>| row.get(0),
        ).unwrap_or(0);

        if existing > 0 {
            return Ok(false);
        }

        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO induction_locks (id, tenant_id, created_at, ttl_secs) VALUES (?1,?2,?3,?4)",
            params![id, tenant_id, Utc::now().to_rfc3339(), ttl_secs],
        ).map_err(|e| OmemError::Storage(format!("acquire lock error: {e}")))?;

        Ok(true)
    }

    pub fn release_induction_lock(&self, id: &str) -> Result<(), OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        conn.execute("DELETE FROM induction_locks WHERE id=?1", params![id])
            .map_err(|e| OmemError::Storage(format!("release lock error: {e}")))?;

        Ok(())
    }

    pub fn cleanup_expired_locks(&self) -> Result<usize, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;
        self.cleanup_expired_locks_inner(&conn)
    }

    fn cleanup_expired_locks_inner(&self, conn: &rusqlite::Connection) -> Result<usize, OmemError> {
        let now = Utc::now().to_rfc3339();
        let rows = conn.execute(
            "DELETE FROM induction_locks WHERE datetime(created_at, '+' || ttl_secs || ' seconds') < datetime(?1)",
            params![now],
        ).map_err(|e| OmemError::Storage(format!("cleanup locks error: {e}")))?;
        Ok(rows)
    }

    pub fn create_induction_run(&self, run: &InductionRun) -> Result<(), OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        conn.execute(
            "INSERT INTO induction_runs (id, tenant_id, status, candidate_count, extracted_count, error, started_at, completed_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                run.id, run.tenant_id, run.status, run.candidate_count, run.extracted_count,
                run.error, run.started_at.to_rfc3339(),
                run.completed_at.map(|t| t.to_rfc3339())
            ],
        ).map_err(|e| OmemError::Storage(format!("create run error: {e}")))?;

        Ok(())
    }

    pub fn update_induction_run(&self, id: &str, status: &str, extracted: i32, error: Option<&str>) -> Result<(), OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        conn.execute(
            "UPDATE induction_runs SET status=?1, extracted_count=?2, error=?3, completed_at=?4 WHERE id=?5",
            params![status, extracted, error, Utc::now().to_rfc3339(), id],
        ).map_err(|e| OmemError::Storage(format!("update run error: {e}")))?;

        Ok(())
    }

    pub fn get_induction_runs(&self, tenant_id: &str, limit: i32) -> Result<Vec<InductionRun>, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        let mut stmt = prepare_stmt(&conn, 
            "SELECT id, tenant_id, status, candidate_count, extracted_count, error, started_at, completed_at FROM induction_runs WHERE tenant_id=?1 ORDER BY started_at DESC LIMIT ?2"
        )?;

        let rows: Vec<Result<InductionRun, _>> = stmt.query_map(params![tenant_id, limit], |row: &rusqlite::Row<'_>| {
            let started_at_str: String = row.get(6)?;
            let completed_at_str: Option<String> = row.get(7)?;
            Ok(InductionRun {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                status: row.get(2)?,
                candidate_count: row.get(3)?,
                extracted_count: row.get(4)?,
                error: row.get(5)?,
                started_at: parse_datetime(&started_at_str),
                completed_at: completed_at_str.as_ref().map(|s| parse_datetime(s)),
            })
        }).map_err(|e| OmemError::Storage(format!("query runs error: {e}")))?.collect();

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row.map_err(|e| OmemError::Storage(format!("row parse: {e}")))?);
        }
        Ok(runs)
    }

    pub fn record_changelog(&self, entry: &ProfileChangelog) -> Result<(), OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        conn.execute(
            "INSERT INTO profile_changelog (id, tenant_id, preference_id, action, old_value, new_value, source, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                entry.id, entry.tenant_id, entry.preference_id, entry.action,
                entry.old_value, entry.new_value, entry.source, entry.created_at.to_rfc3339()
            ],
        ).map_err(|e| OmemError::Storage(format!("changelog error: {e}")))?;

        Ok(())
    }

    pub fn get_changelog(&self, tenant_id: &str, limit: i32) -> Result<Vec<ProfileChangelog>, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        let mut stmt = prepare_stmt(&conn, 
            "SELECT id, tenant_id, preference_id, action, old_value, new_value, source, created_at FROM profile_changelog WHERE tenant_id=?1 ORDER BY created_at DESC LIMIT ?2"
        )?;

        let rows: Vec<Result<ProfileChangelog, _>> = stmt.query_map(params![tenant_id, limit], |row: &rusqlite::Row<'_>| {
            let created_at_str: String = row.get(7)?;
            Ok(ProfileChangelog {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                preference_id: row.get(2)?,
                action: row.get(3)?,
                old_value: row.get(4)?,
                new_value: row.get(5)?,
                source: row.get(6)?,
                created_at: parse_datetime(&created_at_str),
            })
        }).map_err(|e| OmemError::Storage(format!("changelog query error: {e}")))?.collect();

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|e| OmemError::Storage(format!("row parse: {e}")))?);
        }
        Ok(entries)
    }

    pub fn save_version(&self, version: &ProfileVersion) -> Result<(), OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        conn.execute(
            "INSERT INTO profile_versions (id, tenant_id, snapshot, preference_count, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![
                version.id, version.tenant_id, version.snapshot,
                version.preference_count, version.created_at.to_rfc3339()
            ],
        ).map_err(|e| OmemError::Storage(format!("save version error: {e}")))?;

        Ok(())
    }

    pub fn get_versions(&self, tenant_id: &str, limit: i32) -> Result<Vec<ProfileVersion>, OmemError> {
        let conn = self.sqlite.conn().lock().map_err(|_| {
            OmemError::Storage("sqlite lock poisoned".to_string())
        })?;

        let mut stmt = prepare_stmt(&conn, 
            "SELECT id, tenant_id, snapshot, preference_count, created_at FROM profile_versions WHERE tenant_id=?1 ORDER BY created_at DESC LIMIT ?2"
        )?;

        let rows: Vec<Result<ProfileVersion, _>> = stmt.query_map(params![tenant_id, limit], |row: &rusqlite::Row<'_>| {
            let created_at_str: String = row.get(4)?;
            Ok(ProfileVersion {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                snapshot: row.get(2)?,
                preference_count: row.get(3)?,
                created_at: parse_datetime(&created_at_str),
            })
        }).map_err(|e| OmemError::Storage(format!("versions query error: {e}")))?.collect();

        let mut versions = Vec::new();
        for row in rows {
            versions.push(row.map_err(|e| OmemError::Storage(format!("row parse: {e}")))?);
        }
        Ok(versions)
    }

    fn row_to_preference(row: &rusqlite::Row) -> rusqlite::Result<Preference> {
        let scope_str: String = row.get(5)?;
        let status_str: String = row.get(8)?;
        let last_reinforced_at_str: String = row.get(9)?;
        let created_at_str: String = row.get(10)?;
        let updated_at_str: String = row.get(11)?;

        Ok(Preference {
            id: row.get(0)?,
            tenant_id: row.get(1)?,
            slot: row.get(2)?,
            value: row.get(3)?,
            confidence: row.get(4)?,
            scope: match scope_str.as_str() {
                "global" => PreferenceScope::Global,
                _ => PreferenceScope::Project,
            },
            project_path: row.get(6)?,
            source: row.get(7)?,
            status: match status_str.as_str() {
                "reinforce" => PreferenceStatus::Reinforce,
                "dormant" => PreferenceStatus::Dormant,
                "deleted" => PreferenceStatus::Deleted,
                _ => PreferenceStatus::Active,
            },
            last_reinforced_at: parse_datetime(&last_reinforced_at_str),
            created_at: parse_datetime(&created_at_str),
            updated_at: parse_datetime(&updated_at_str),
        })
    }
}

fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).map(|dt| dt.to_utc()).unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> ProfileStore {
        let sqlite = Arc::new(SqliteStore::new_in_memory().unwrap());
        let store = ProfileStore::new(sqlite);
        store.init().unwrap();
        store
    }

    fn make_pref(tenant: &str, slot: &str, value: &str) -> Preference {
        Preference {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            slot: slot.to_string(),
            value: value.to_string(),
            confidence: 0.5,
            scope: PreferenceScope::Global,
            project_path: None,
            source: "observed".to_string(),
            status: PreferenceStatus::Active,
            last_reinforced_at: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn crud_roundtrip() {
        let store = test_store();

        let pref = make_pref("t1", "language", "Rust");
        store.upsert_preference(&pref).unwrap();

        let prefs = store.get_preferences("t1", None).unwrap();
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].slot, "language");
        assert_eq!(prefs[0].value, "Rust");

        let fetched = store.get_preference_by_id(&pref.id).unwrap().unwrap();
        assert_eq!(fetched.value, "Rust");

        let deleted = store.delete_preference(&pref.id).unwrap();
        assert!(deleted);

        let gone = store.get_preference_by_id(&pref.id).unwrap();
        assert!(gone.is_none());
    }

    #[test]
    fn update_confidence_clamps() {
        let store = test_store();
        let pref = make_pref("t1", "tone", "casual");
        store.upsert_preference(&pref).unwrap();

        store.update_confidence(&pref.id, 0.4).unwrap();
        let updated = store.get_preference_by_id(&pref.id).unwrap().unwrap();
        assert!((updated.confidence - 0.9).abs() < 0.01);

        store.update_confidence(&pref.id, 0.2).unwrap();
        let updated = store.get_preference_by_id(&pref.id).unwrap().unwrap();
        assert!((updated.confidence - 0.95).abs() < 0.01);
    }

    #[test]
    fn update_status_changes() {
        let store = test_store();
        let pref = make_pref("t1", "code_style", "terse");
        store.upsert_preference(&pref).unwrap();

        store.update_status(&pref.id, "dormant").unwrap();
        let updated = store.get_preference_by_id(&pref.id).unwrap().unwrap();
        assert_eq!(updated.status, PreferenceStatus::Dormant);
    }

    #[test]
    fn induction_lock_mutex() {
        let store = test_store();

        let acquired = store.acquire_induction_lock("t1", 600).unwrap();
        assert!(acquired);

        let again = store.acquire_induction_lock("t1", 600).unwrap();
        assert!(!again);

        let lock = store.get_induction_lock("t1").unwrap().unwrap();
        store.release_induction_lock(&lock.id).unwrap();

        let reacquired = store.acquire_induction_lock("t1", 600).unwrap();
        assert!(reacquired);
    }

    #[test]
    fn expired_locks_are_cleaned_on_next_acquire() {
        let store = test_store();

        let acquired1 = store.acquire_induction_lock("t1", 1).unwrap();
        assert!(acquired1);

        std::thread::sleep(std::time::Duration::from_secs(2));

        let acquired2 = store.acquire_induction_lock("t1", 600).unwrap();
        assert!(acquired2, "should acquire after previous lock expired");
    }

    #[test]
    fn induction_run_lifecycle() {
        let store = test_store();

        let run = InductionRun {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id: "t1".to_string(),
            status: "running".to_string(),
            candidate_count: 10,
            extracted_count: 0,
            error: None,
            started_at: Utc::now(),
            completed_at: None,
        };
        store.create_induction_run(&run).unwrap();

        store.update_induction_run(&run.id, "completed", 5, None).unwrap();

        let runs = store.get_induction_runs("t1", 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "completed");
        assert_eq!(runs[0].extracted_count, 5);
    }

    #[test]
    fn changelog_and_versions() {
        let store = test_store();

        let entry = ProfileChangelog {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id: "t1".to_string(),
            preference_id: "p1".to_string(),
            action: "created".to_string(),
            old_value: None,
            new_value: Some("Rust".to_string()),
            source: "induction".to_string(),
            created_at: Utc::now(),
        };
        store.record_changelog(&entry).unwrap();

        let log = store.get_changelog("t1", 10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].action, "created");

        let version = ProfileVersion {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id: "t1".to_string(),
            snapshot: "{}".to_string(),
            preference_count: 3,
            created_at: Utc::now(),
        };
        store.save_version(&version).unwrap();

        let versions = store.get_versions("t1", 10).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].preference_count, 3);
    }

    #[test]
    fn project_scoped_preferences() {
        let store = test_store();

        let global = Preference {
            scope: PreferenceScope::Global,
            ..make_pref("t1", "tone", "formal")
        };
        let project = Preference {
            scope: PreferenceScope::Project,
            project_path: Some("my-project".to_string()),
            ..make_pref("t1", "code_style", "terse")
        };
        store.upsert_preference(&global).unwrap();
        store.upsert_preference(&project).unwrap();

        let all = store.get_preferences("t1", None).unwrap();
        assert_eq!(all.len(), 2);

        let project_only = store.get_preferences("t1", Some("my-project")).unwrap();
        assert_eq!(project_only.len(), 2);

        let other_project = store.get_preferences("t1", Some("other")).unwrap();
        assert_eq!(other_project.len(), 1);
        assert_eq!(other_project[0].scope, PreferenceScope::Global);
    }
}
