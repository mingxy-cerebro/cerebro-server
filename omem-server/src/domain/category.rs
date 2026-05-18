use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use crate::domain::error::OmemError;
use crate::store::sqlite::SqliteStore;
use crate::store::sqlite_schema;

/// Category newtype — transparent String wrapper preserved for backward compat.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Hash, Eq)]
#[serde(transparent)]
pub struct Category(String);

impl Category {
    pub fn new(s: &str) -> Self {
        Category(s.to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Category {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Category(s.to_lowercase()))
    }
}

/// Full category configuration from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryConfig {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub decision_rule: Option<String>,
    pub always_merge: bool,
    pub append_only: bool,
    pub temporal_versioned: bool,
    pub merge_supported: bool,
    pub admission_weight: f32,
    pub importance_base: f32,
    pub prompt_format: Option<String>,
    pub default_visibility: String,
    pub default_scope: String,
    pub default_ttl_days: Option<i32>,
    pub sort_order: i32,
    pub is_active: bool,
}

/// Per-tenant in-memory cache of categories + aliases, backed by SQLite.
pub struct CategoryRegistry {
    /// tenant_id → Vec<CategoryConfig>
    categories: DashMap<String, Vec<CategoryConfig>>,
    /// tenant_id → HashMap<alias, target>
    aliases: DashMap<String, HashMap<String, String>>,
    sqlite: Arc<SqliteStore>,
}

impl CategoryRegistry {
    pub fn new(sqlite: Arc<SqliteStore>) -> Self {
        Self {
            categories: DashMap::new(),
            aliases: DashMap::new(),
            sqlite,
        }
    }

    /// Load categories for a tenant from SQLite into cache.
    fn load_for_tenant(&self, tenant_id: &str) -> Result<(), OmemError> {
        let conn = self
            .sqlite
            .conn()
            .lock()
            .map_err(|e| OmemError::Storage(format!("SQLite lock error: {}", e)))?;

        let mut stmt = conn
            .prepare(
                "SELECT name, display_name, description, decision_rule, always_merge, \
                 append_only, temporal_versioned, merge_supported, admission_weight, \
                 importance_base, prompt_format, default_visibility, default_scope, \
                 default_ttl_days, sort_order, is_active \
                 FROM categories WHERE tenant_id = ?1 ORDER BY sort_order",
            )
            .map_err(|e| OmemError::Storage(e.to_string()))?;

        let cats = stmt
            .query_map(rusqlite::params![tenant_id], |row| {
                Ok(CategoryConfig {
                    name: row.get(0)?,
                    display_name: row.get(1)?,
                    description: row.get(2)?,
                    decision_rule: row.get(3)?,
                    always_merge: row.get(4)?,
                    append_only: row.get(5)?,
                    temporal_versioned: row.get(6)?,
                    merge_supported: row.get(7)?,
                    admission_weight: row.get(8)?,
                    importance_base: row.get(9)?,
                    prompt_format: row.get(10)?,
                    default_visibility: row.get(11)?,
                    default_scope: row.get(12)?,
                    default_ttl_days: row.get(13)?,
                    sort_order: row.get(14)?,
                    is_active: row.get(15)?,
                })
            })
            .map_err(|e| OmemError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| OmemError::Storage(e.to_string()))?;

        // Load aliases
        let mut alias_stmt = conn
            .prepare("SELECT alias, target FROM category_aliases WHERE tenant_id = ?1")
            .map_err(|e| OmemError::Storage(e.to_string()))?;

        let alias_map: HashMap<String, String> = alias_stmt
            .query_map(rusqlite::params![tenant_id], |row| {
                let alias: String = row.get(0)?;
                let target: String = row.get(1)?;
                Ok((alias, target))
            })
            .map_err(|e| OmemError::Storage(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        drop(alias_stmt);
        drop(stmt);
        drop(conn);

        self.categories.insert(tenant_id.to_string(), cats);
        self.aliases.insert(tenant_id.to_string(), alias_map);
        Ok(())
    }

    /// Ensure cache is loaded for a tenant (lazy load).
    fn ensure_loaded(&self, tenant_id: &str) -> Result<(), OmemError> {
        if !self.categories.contains_key(tenant_id) {
            self.load_for_tenant(tenant_id)?;
        }
        Ok(())
    }

    /// Get all categories for a tenant (loads from DB on first access).
    pub fn get_categories(&self, tenant_id: &str) -> Result<Vec<CategoryConfig>, OmemError> {
        self.ensure_loaded(tenant_id)?;
        Ok(self
            .categories
            .get(tenant_id)
            .map(|r| r.value().clone())
            .unwrap_or_default())
    }

    /// Get only active categories.
    pub fn get_active_categories(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<CategoryConfig>, OmemError> {
        let all = self.get_categories(tenant_id)?;
        Ok(all.into_iter().filter(|c| c.is_active).collect())
    }

    /// Find a category by name.
    pub fn find_by_name(
        &self,
        tenant_id: &str,
        name: &str,
    ) -> Result<Option<CategoryConfig>, OmemError> {
        let cats = self.get_categories(tenant_id)?;
        Ok(cats.into_iter().find(|c| c.name == name))
    }

    /// Normalize a raw category string: resolve aliases, validate against known categories.
    /// Returns Some(normalized_name) if valid, None if unknown.
    pub fn normalize(&self, tenant_id: &str, raw: &str) -> Result<Option<String>, OmemError> {
        let lower = raw.to_lowercase();

        // Fast path: check cache first
        if let Some(cats) = self.categories.get(tenant_id) {
            if cats.iter().any(|c| c.name == lower) {
                return Ok(Some(lower));
            }
        }
        if let Some(aliases) = self.aliases.get(tenant_id) {
            if let Some(target) = aliases.get(&lower) {
                return Ok(Some(target.clone()));
            }
        }

        // Not found — ensure loaded from DB, then re-check
        self.ensure_loaded(tenant_id)?;

        if let Some(cats) = self.categories.get(tenant_id) {
            if cats.iter().any(|c| c.name == lower) {
                return Ok(Some(lower));
            }
        }
        if let Some(aliases) = self.aliases.get(tenant_id) {
            if let Some(target) = aliases.get(&lower) {
                return Ok(Some(target.clone()));
            }
        }

        Ok(None)
    }

    /// Get admission weight for a category. Returns 0.50 as default.
    pub fn get_prior(&self, tenant_id: &str, name: &str) -> Result<f32, OmemError> {
        Ok(self
            .find_by_name(tenant_id, name)?
            .map(|c| c.admission_weight)
            .unwrap_or(0.50))
    }

    /// Get importance base for a category. Returns 0.50 as default.
    pub fn get_importance(&self, tenant_id: &str, name: &str) -> Result<f32, OmemError> {
        Ok(self
            .find_by_name(tenant_id, name)?
            .map(|c| c.importance_base)
            .unwrap_or(0.50))
    }

    /// Seed default categories for a new tenant.
    pub fn seed_tenant(&self, tenant_id: &str) -> Result<(), OmemError> {
        let conn = self
            .sqlite
            .conn()
            .lock()
            .map_err(|e| OmemError::Storage(format!("SQLite lock error: {}", e)))?;
        sqlite_schema::seed_default_categories(&conn, tenant_id)?;
        drop(conn);
        self.invalidate(tenant_id);
        Ok(())
    }

    /// Create a new category. Invalidates cache.
    pub fn create_category(
        &self,
        tenant_id: &str,
        config: &CategoryConfig,
    ) -> Result<(), OmemError> {
        let conn = self
            .sqlite
            .conn()
            .lock()
            .map_err(|e| OmemError::Storage(format!("SQLite lock error: {}", e)))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO categories (name, tenant_id, display_name, description, decision_rule, \
             always_merge, append_only, temporal_versioned, merge_supported, admission_weight, \
             importance_base, prompt_format, default_visibility, default_scope, \
             default_ttl_days, sort_order, is_active, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            rusqlite::params![
                config.name,
                tenant_id,
                config.display_name,
                config.description,
                config.decision_rule,
                config.always_merge,
                config.append_only,
                config.temporal_versioned,
                config.merge_supported,
                config.admission_weight,
                config.importance_base,
                config.prompt_format,
                config.default_visibility,
                config.default_scope,
                config.default_ttl_days,
                config.sort_order,
                config.is_active,
                now,
                now
            ],
        )
        .map_err(|e| OmemError::Storage(format!("Failed to create category: {}", e)))?;
        drop(conn);
        self.invalidate(tenant_id);
        Ok(())
    }

    /// Update an existing category. Invalidates cache.
    pub fn update_category(
        &self,
        tenant_id: &str,
        name: &str,
        updates: &CategoryUpdate,
    ) -> Result<(), OmemError> {
        let conn = self
            .sqlite
            .conn()
            .lock()
            .map_err(|e| OmemError::Storage(format!("SQLite lock error: {}", e)))?;
        let now = chrono::Utc::now().to_rfc3339();

        let mut set_clauses = Vec::new();
        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(ref v) = updates.display_name {
            set_clauses.push("display_name = ?");
            params.push(rusqlite::types::Value::Text(v.clone()));
        }
        if let Some(ref v) = updates.description {
            set_clauses.push("description = ?");
            params.push(rusqlite::types::Value::Text(v.clone()));
        }
        if let Some(ref v) = updates.decision_rule {
            set_clauses.push("decision_rule = ?");
            params.push(rusqlite::types::Value::Text(v.clone()));
        }
        if let Some(v) = updates.always_merge {
            set_clauses.push("always_merge = ?");
            params.push(rusqlite::types::Value::Integer(if v { 1 } else { 0 }));
        }
        if let Some(v) = updates.append_only {
            set_clauses.push("append_only = ?");
            params.push(rusqlite::types::Value::Integer(if v { 1 } else { 0 }));
        }
        if let Some(v) = updates.temporal_versioned {
            set_clauses.push("temporal_versioned = ?");
            params.push(rusqlite::types::Value::Integer(if v { 1 } else { 0 }));
        }
        if let Some(v) = updates.merge_supported {
            set_clauses.push("merge_supported = ?");
            params.push(rusqlite::types::Value::Integer(if v { 1 } else { 0 }));
        }
        if let Some(v) = updates.admission_weight {
            set_clauses.push("admission_weight = ?");
            params.push(rusqlite::types::Value::Real(v as f64));
        }
        if let Some(v) = updates.importance_base {
            set_clauses.push("importance_base = ?");
            params.push(rusqlite::types::Value::Real(v as f64));
        }
        if let Some(ref v) = updates.prompt_format {
            set_clauses.push("prompt_format = ?");
            params.push(rusqlite::types::Value::Text(v.clone()));
        }
        if let Some(v) = updates.is_active {
            set_clauses.push("is_active = ?");
            params.push(rusqlite::types::Value::Integer(if v { 1 } else { 0 }));
        }

        set_clauses.push("updated_at = ?");
        params.push(rusqlite::types::Value::Text(now));

        if set_clauses.len() == 1 {
            return Ok(());
        }

        let sql = format!(
            "UPDATE categories SET {} WHERE name = ? AND tenant_id = ?",
            set_clauses.join(", ")
        );
        params.push(rusqlite::types::Value::Text(name.to_string()));
        params.push(rusqlite::types::Value::Text(tenant_id.to_string()));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
        conn.execute(&sql, param_refs.as_slice())
            .map_err(|e| OmemError::Storage(format!("Failed to update category: {}", e)))?;
        drop(conn);
        self.invalidate(tenant_id);
        Ok(())
    }

    /// Delete a category (hard delete). Invalidates cache.
    pub fn delete_category(&self, tenant_id: &str, name: &str) -> Result<(), OmemError> {
        let conn = self
            .sqlite
            .conn()
            .lock()
            .map_err(|e| OmemError::Storage(format!("SQLite lock error: {}", e)))?;
        conn.execute(
            "DELETE FROM categories WHERE name = ?1 AND tenant_id = ?2",
            rusqlite::params![name, tenant_id],
        )
        .map_err(|e| OmemError::Storage(format!("Failed to delete category: {}", e)))?;
        drop(conn);
        self.invalidate(tenant_id);
        Ok(())
    }

    /// Invalidate cache for a tenant (next access reloads from DB).
    pub fn invalidate(&self, tenant_id: &str) {
        self.categories.remove(tenant_id);
        self.aliases.remove(tenant_id);
    }

    /// Create a category alias. Invalidates cache.
    pub fn create_alias(
        &self,
        tenant_id: &str,
        alias: &str,
        target: &str,
    ) -> Result<(), OmemError> {
        let conn = self
            .sqlite
            .conn()
            .lock()
            .map_err(|e| OmemError::Storage(format!("SQLite lock error: {}", e)))?;
        conn.execute(
            "INSERT INTO category_aliases (alias, tenant_id, target) VALUES (?1, ?2, ?3)",
            rusqlite::params![alias, tenant_id, target],
        )
        .map_err(|e| OmemError::Storage(format!("Failed to create alias: {}", e)))?;
        drop(conn);
        self.invalidate(tenant_id);
        Ok(())
    }

    /// Delete a category alias. Invalidates cache.
    pub fn delete_alias(&self, tenant_id: &str, alias: &str) -> Result<(), OmemError> {
        let conn = self
            .sqlite
            .conn()
            .lock()
            .map_err(|e| OmemError::Storage(format!("SQLite lock error: {}", e)))?;
        conn.execute(
            "DELETE FROM category_aliases WHERE alias = ?1 AND tenant_id = ?2",
            rusqlite::params![alias, tenant_id],
        )
        .map_err(|e| OmemError::Storage(format!("Failed to delete alias: {}", e)))?;
        drop(conn);
        self.invalidate(tenant_id);
        Ok(())
    }

    /// Get aliases for a tenant.
    pub fn get_aliases(
        &self,
        tenant_id: &str,
    ) -> Result<HashMap<String, String>, OmemError> {
        self.ensure_loaded(tenant_id)?;
        Ok(self
            .aliases
            .get(tenant_id)
            .map(|r| r.value().clone())
            .unwrap_or_default())
    }
}

/// Partial update payload for categories.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CategoryUpdate {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub decision_rule: Option<String>,
    pub always_merge: Option<bool>,
    pub append_only: Option<bool>,
    pub temporal_versioned: Option<bool>,
    pub merge_supported: Option<bool>,
    pub admission_weight: Option<f32>,
    pub importance_base: Option<f32>,
    pub prompt_format: Option<String>,
    pub is_active: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_registry() -> CategoryRegistry {
        let sqlite = SqliteStore::new_in_memory().unwrap();
        let conn = sqlite.conn().lock().unwrap();
        sqlite_schema::create_tables(&conn).unwrap();
        drop(conn);
        CategoryRegistry::new(Arc::new(sqlite))
    }

    // ── Category newtype tests (preserved behavior) ──

    #[test]
    fn category_new_lowercases() {
        let c = Category::new("Profile");
        assert_eq!(c.as_str(), "profile");
    }

    #[test]
    fn category_display() {
        assert_eq!(Category::new("preferences").to_string(), "preferences");
        assert_eq!(Category::new("EVENTS").to_string(), "events");
    }

    #[test]
    fn category_from_str() {
        let c: Category = "Profile".parse().unwrap();
        assert_eq!(c.as_str(), "profile");
        let unknown: Category = "unknown".parse().unwrap();
        assert_eq!(unknown.as_str(), "unknown");
    }

    #[test]
    fn category_serde_roundtrip() {
        let c = Category::new("cases");
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "\"cases\"");
        let parsed: Category = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn category_equality_and_hash() {
        let a = Category::new("profile");
        let b = Category::new("PROFILE");
        assert_eq!(a, b);
        let mut set = std::collections::HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b));
    }

    // ── CategoryRegistry tests ──

    #[test]
    fn seed_and_get_categories() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();
        let cats = reg.get_categories("t1").unwrap();
        assert_eq!(cats.len(), 9);
        // First category by sort_order
        assert_eq!(cats[0].name, "preferences");
    }

    #[test]
    fn get_active_categories() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();
        let active = reg.get_active_categories("t1").unwrap();
        assert_eq!(active.len(), 9); // All seeded are active
    }

    #[test]
    fn find_by_name() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();
        let cat = reg.find_by_name("t1", "identity").unwrap();
        assert!(cat.is_some());
        let cat = cat.unwrap();
        assert!(cat.always_merge);
        assert!((cat.admission_weight - 0.75).abs() < 0.01);

        let missing = reg.find_by_name("t1", "nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn normalize_direct_and_alias() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();

        // Direct name
        let result = reg.normalize("t1", "preferences").unwrap();
        assert_eq!(result, Some("preferences".to_string()));

        // Alias
        let result = reg.normalize("t1", "likes").unwrap();
        assert_eq!(result, Some("preferences".to_string()));

        // Unknown
        let result = reg.normalize("t1", "totally_unknown").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn get_prior_and_importance() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();

        let prior = reg.get_prior("t1", "preferences").unwrap();
        assert!((prior - 0.90).abs() < 0.01);

        let importance = reg.get_importance("t1", "identity").unwrap();
        assert!((importance - 0.80).abs() < 0.01);

        // Unknown category → default 0.50
        let default = reg.get_prior("t1", "nonexistent").unwrap();
        assert!((default - 0.50).abs() < 0.01);
    }

    #[test]
    fn create_and_delete_category() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();

        let new_cat = CategoryConfig {
            name: "custom".to_string(),
            display_name: "Custom".to_string(),
            description: "A custom category".to_string(),
            decision_rule: None,
            always_merge: false,
            append_only: false,
            temporal_versioned: false,
            merge_supported: false,
            admission_weight: 0.60,
            importance_base: 0.50,
            prompt_format: None,
            default_visibility: "global".to_string(),
            default_scope: "global".to_string(),
            default_ttl_days: None,
            sort_order: 100,
            is_active: true,
        };

        reg.create_category("t1", &new_cat).unwrap();
        let found = reg.find_by_name("t1", "custom").unwrap().unwrap();
        assert_eq!(found.display_name, "Custom");

        reg.delete_category("t1", "custom").unwrap();
        let gone = reg.find_by_name("t1", "custom").unwrap();
        assert!(gone.is_none());
    }

    #[test]
    fn update_category() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();

        let update = CategoryUpdate {
            display_name: Some("Updated Display".to_string()),
            admission_weight: Some(0.99),
            ..Default::default()
        };

        reg.update_category("t1", "preferences", &update).unwrap();
        let cat = reg.find_by_name("t1", "preferences").unwrap().unwrap();
        assert_eq!(cat.display_name, "Updated Display");
        assert!((cat.admission_weight - 0.99).abs() < 0.01);
    }

    #[test]
    fn alias_crud() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();

        reg.create_alias("t1", "my_alias", "preferences").unwrap();
        let result = reg.normalize("t1", "my_alias").unwrap();
        assert_eq!(result, Some("preferences".to_string()));

        reg.delete_alias("t1", "my_alias").unwrap();
        let result = reg.normalize("t1", "my_alias").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn tenant_isolation() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();
        reg.seed_tenant("t2").unwrap();

        let t1_cats = reg.get_categories("t1").unwrap();
        let t2_cats = reg.get_categories("t2").unwrap();
        assert_eq!(t1_cats.len(), 9);
        assert_eq!(t2_cats.len(), 9);

        // Deleting from t1 doesn't affect t2
        reg.delete_category("t1", "preferences").unwrap();
        assert!(reg.find_by_name("t1", "preferences").unwrap().is_none());
        assert!(reg.find_by_name("t2", "preferences").unwrap().is_some());
    }

    #[test]
    fn invalidate_clears_cache() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();
        assert!(reg.categories.contains_key("t1"));
        reg.invalidate("t1");
        assert!(!reg.categories.contains_key("t1"));
    }

    #[test]
    fn update_with_no_fields_is_noop() {
        let reg = setup_registry();
        reg.seed_tenant("t1").unwrap();
        let update = CategoryUpdate::default();
        // Should succeed without error (only updated_at clause, which is len==1 → early return)
        reg.update_category("t1", "preferences", &update).unwrap();
    }
}
