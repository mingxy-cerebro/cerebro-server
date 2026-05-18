use rusqlite::Connection;
use crate::domain::error::OmemError;

/// DDL for categories and category_aliases tables.
pub const CREATE_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS categories (
    name TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL,
    decision_rule TEXT,
    always_merge BOOLEAN NOT NULL DEFAULT FALSE,
    append_only BOOLEAN NOT NULL DEFAULT FALSE,
    temporal_versioned BOOLEAN NOT NULL DEFAULT FALSE,
    merge_supported BOOLEAN NOT NULL DEFAULT FALSE,
    admission_weight REAL NOT NULL DEFAULT 0.50,
    importance_base REAL NOT NULL DEFAULT 0.50,
    prompt_format TEXT,
    default_visibility TEXT NOT NULL DEFAULT 'global',
    default_scope TEXT NOT NULL DEFAULT 'global',
    default_ttl_days INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (name, tenant_id)
);

CREATE INDEX IF NOT EXISTS idx_categories_tenant ON categories(tenant_id);
CREATE INDEX IF NOT EXISTS idx_categories_tenant_active ON categories(tenant_id, is_active);

CREATE TABLE IF NOT EXISTS category_aliases (
    alias TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    target TEXT NOT NULL,
    PRIMARY KEY (alias, tenant_id)
);

CREATE INDEX IF NOT EXISTS idx_category_aliases_tenant ON category_aliases(tenant_id);
"#;

/// Execute all DDL statements to create tables.
pub fn create_tables(conn: &Connection) -> Result<(), OmemError> {
    conn.execute_batch(CREATE_TABLES_SQL)
        .map_err(|e| OmemError::Storage(format!("Failed to create tables: {}", e)))
}

/// Seed the 9 design-doc categories for a given tenant.
/// Uses INSERT OR IGNORE to be idempotent.
pub fn seed_default_categories(conn: &Connection, tenant_id: &str) -> Result<(), OmemError> {
    let now = chrono::Utc::now().to_rfc3339();

    let categories: &[(&str, &str, &str, Option<&str>, bool, bool, bool, bool, f32, f32, Option<&str>, &str, &str, Option<i32>, i32)] = &[
        // (name, display_name, description, decision_rule, always_merge, append_only, temporal_versioned, merge_supported, admission_weight, importance_base, prompt_format, default_visibility, default_scope, default_ttl_days, sort_order)
        ("preferences", "偏好", "User likes/dislikes/tool choices and preferences", None, false, false, true, true, 0.90, 0.70, Some("preference"), "global", "global", None, 0),
        ("identity", "身份规则", "Stable identity traits and repeated characteristics", None, true, false, false, false, 0.75, 0.80, None, "global", "global", None, 1),
        ("emotional", "感情记忆", "Emotional states, feelings, and sentiment", None, false, true, false, false, 0.65, 0.55, None, "global", "global", None, 2),
        ("project", "项目上下文", "Project-specific context and status", None, false, false, true, true, 0.70, 0.60, None, "global", "global", None, 3),
        ("work", "工作记忆", "Work-related memories with decay", None, false, true, false, false, 0.55, 0.45, Some("work"), "global", "global", Some(90), 4),
        ("lessons_learned", "经验教训", "Lessons learned from experience", None, false, false, false, true, 0.85, 0.70, None, "global", "global", None, 5),
        ("decisions", "重要决策", "Important decisions made and reasoning", None, false, true, false, false, 0.80, 0.75, None, "global", "global", None, 6),
        ("success_patterns", "成功方案", "Successful patterns and solutions", None, false, false, false, true, 0.85, 0.65, None, "global", "global", None, 7),
        ("mistakes", "犯过的错", "Mistakes, failures, and what went wrong", None, false, false, false, true, 0.80, 0.60, None, "global", "global", None, 8),
    ];

    for (name, display_name, description, decision_rule, always_merge, append_only, temporal_versioned, merge_supported, admission_weight, importance_base, prompt_format, default_visibility, default_scope, default_ttl_days, sort_order) in categories {
        conn.execute(
            "INSERT OR IGNORE INTO categories (name, tenant_id, display_name, description, decision_rule, always_merge, append_only, temporal_versioned, merge_supported, admission_weight, importance_base, prompt_format, default_visibility, default_scope, default_ttl_days, sort_order, is_active, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, TRUE, ?17, ?18)",
            rusqlite::params![
                name, tenant_id, display_name, description, decision_rule,
                always_merge, append_only, temporal_versioned, merge_supported,
                admission_weight, importance_base, prompt_format,
                default_visibility, default_scope, default_ttl_days,
                sort_order, now, now
            ],
        ).map_err(|e| OmemError::Storage(format!("Failed to seed category '{}': {}", name, e)))?;
    }

    let aliases: &[(&str, &str)] = &[
        ("likes", "preferences"),
        ("dislikes", "preferences"),
        ("hobbies", "preferences"),
        ("identity_rules", "identity"),
        ("feelings", "emotional"),
        ("emotions", "emotional"),
        ("projects", "project"),
        ("project_context", "project"),
        ("work_memory", "work"),
        ("working", "work"),
        ("lessons", "lessons_learned"),
        ("lesson", "lessons_learned"),
        ("decision", "decisions"),
        ("choices", "decisions"),
        ("success", "success_patterns"),
        ("successful_patterns", "success_patterns"),
        ("mistake", "mistakes"),
        ("errors", "mistakes"),
        ("failures", "mistakes"),
    ];

    for (alias, target) in aliases {
        conn.execute(
            "INSERT OR IGNORE INTO category_aliases (alias, tenant_id, target) VALUES (?1, ?2, ?3)",
            rusqlite::params![alias, tenant_id, target],
        ).map_err(|e| OmemError::Storage(format!("Failed to seed alias '{}': {}", alias, e)))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_create_tables() {
        let conn = setup_db();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='categories'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1);

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='category_aliases'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_seed_inserts_9_categories() {
        let conn = setup_db();
        seed_default_categories(&conn, "tenant-1").unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM categories WHERE tenant_id = 'tenant-1'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 9);
    }

    #[test]
    fn test_seed_is_idempotent() {
        let conn = setup_db();
        seed_default_categories(&conn, "tenant-1").unwrap();
        seed_default_categories(&conn, "tenant-1").unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM categories WHERE tenant_id = 'tenant-1'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 9);
    }

    #[test]
    fn test_seed_aliases() {
        let conn = setup_db();
        seed_default_categories(&conn, "tenant-1").unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM category_aliases WHERE tenant_id = 'tenant-1'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(count > 0, "Should have seed aliases");
    }

    #[test]
    fn test_tenant_isolation() {
        let conn = setup_db();
        seed_default_categories(&conn, "tenant-1").unwrap();
        seed_default_categories(&conn, "tenant-2").unwrap();
        let t1: i64 = conn.query_row(
            "SELECT COUNT(*) FROM categories WHERE tenant_id = 'tenant-1'",
            [],
            |row| row.get(0),
        ).unwrap();
        let t2: i64 = conn.query_row(
            "SELECT COUNT(*) FROM categories WHERE tenant_id = 'tenant-2'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(t1, 9);
        assert_eq!(t2, 9);
    }

    #[test]
    fn test_seed_category_weights() {
        let conn = setup_db();
        seed_default_categories(&conn, "tenant-1").unwrap();
        let (weight, importance): (f32, f32) = conn.query_row(
            "SELECT admission_weight, importance_base FROM categories WHERE name = 'identity' AND tenant_id = 'tenant-1'",
            [],
            |row| Ok((row.get(0).unwrap(), row.get(1).unwrap())),
        ).unwrap();
        assert!((weight - 0.75).abs() < 0.01);
        assert!((importance - 0.80).abs() < 0.01);
    }
}
