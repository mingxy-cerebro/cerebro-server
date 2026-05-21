# Plan: 自动迁移老数据 project_path

## 背景

Phase 3 完成后，新记忆会自动携带 `project_path`，但存量数据（`project_path = NULL`）没有归属。
需要根据记忆内容自动判断归属项目，在服务启动时一次性完成迁移。

## 项目映射规则

| 关键词匹配 | → project_path |
|-----------|---------------|
| 包含 cerebro/omem/lancedb/vector/memory-server 等关键词 | `/mnt/d/dev/github/project/omem-server-source` |
| 包含 nfqw/mall4j/nf/农服/java/spring-boot 等关键词 | `/mnt/d/dev/project/nf` |
| category = preferences | 保持 NULL（全局偏好） |
| 匹配不上的 | 保持 NULL（全局可见） |

## 方案选择

### 方案 A：Rust 服务端启动时自动迁移（推荐）

在 `main.rs` 启动流程中，仿照已有的 seed_categories 迁移，增加一个 `migrate_project_path` 模块。

**优势：**
- 无需外部脚本，服务升级自动完成
- 每个 tenant 只跑一次（幂等）
- 纯关键词匹配，不依赖 LLM

**工作流：**
1. 启动时遍历所有 tenant
2. 对每个 tenant 的 personal space，`list()` 拿所有 `project_path IS NULL` 的记忆
3. 遍历每条记忆，检查 tags + content 关键词
4. 匹配到项目的 → 用 `batch_update_project_path()` 批量更新
5. 匹配不上的 → 保持 NULL

**实现步骤：**

### T1: 创建迁移模块 `omem-server/src/store/migration.rs`

新建模块，包含：
- `PROJECT_RULES: &[(&[&str], &str)]` — 关键词→路径映射表
- `classify_memory(tags, content) -> Option<String>` — 分类函数
- `migrate_project_paths(store: &LanceStore) -> Result<MigrationStats>` — 主逻辑

```rust
// 关键词规则
const PROJECT_RULES: &[(&[&str], &str)] = &[
    // (关键词列表, project_path)
    (&["cerebro", "omem", "lancedb", "omem-server", "vector-db", "memory-server"], 
     "/mnt/d/dev/github/project/omem-server-source"),
    (&["nfqw", "mall4j", "nf-project", "农服", "spring-boot-mall"], 
     "/mnt/d/dev/project/nf"),
];

fn classify_memory(tags: &[String], content: &str, category: &str) -> Option<String> {
    // preferences 类别 → 全局
    if category == "preferences" { return None; }
    
    let content_lower = content.to_lowercase();
    for (keywords, path) in PROJECT_RULES {
        for kw in *keywords {
            if tags.iter().any(|t| t.to_lowercase().contains(kw)) 
               || content_lower.contains(kw) {
                return Some(path.to_string());
            }
        }
    }
    None // 匹配不上 → 全局
}
```

### T2: 在 `main.rs` 启动流程调用迁移

在 seed_categories 迁移之后，加入 project_path 迁移：

```rust
// Migration: auto-assign project_path for existing memories
match tenant_store.list_all().await {
    Ok(tenants) => {
        for tenant in &tenants {
            let space_id = personal_space_id(&tenant.id);
            match state.store_manager.get_store(&space_id).await {
                Ok(store) => {
                    match omem_server::store::migration::migrate_project_paths(&store).await {
                        Ok(stats) => {
                            if stats.updated > 0 {
                                tracing::info!(
                                    tenant = %tenant.id,
                                    total = stats.total,
                                    updated = stats.updated,
                                    skipped = stats.skipped,
                                    "project_path migration completed"
                                );
                            }
                        }
                        Err(e) => tracing::warn!("project_path migration failed for {}: {}", tenant.id, e),
                    }
                }
                Err(e) => tracing::warn!("Failed to get store for {}: {}", tenant.id, e),
            }
        }
    }
    Err(e) => tracing::warn!("Failed to list tenants: {}", e),
}
```

### T3: 编写测试

在 `migration.rs` 中写单元测试：
- 测试关键词匹配逻辑（cerebro → omem路径，nfqw → nf路径）
- 测试 preferences 类别跳过
- 测试匹配不上返回 None
- 测试大小写不敏感

### T4: 更新 backfill API

已有的 `POST /v1/memories/backfill-project-path` filter 加入 `category != 'preferences'`（**已完成** ✓）。

## 幂等性保证

- `batch_update_project_path` 底层用 `only_if("project_path IS NULL AND ...")` 
- 已有 project_path 的记忆不会被覆盖
- 迁移完成后再次启动，`project_path IS NULL` 的记忆数为 0，直接跳过

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| 关键词误判 | 规则保守，宁可漏判（保持全局）不可误判（绑定错误项目） |
| 启动时间变长 | 只处理 project_path IS NULL 的记忆，已迁移的跳过 |
| tags 可能不够 | 同时检查 content 和 tags，双保险 |
| 未来新增项目 | 规则集中维护在 PROJECT_RULES 常量，易于扩展 |

## 验收标准

- [ ] `migration.rs` 模块创建，分类逻辑正确
- [ ] `main.rs` 启动流程调用迁移
- [ ] 单元测试覆盖关键词匹配、preferences跳过、无匹配返回None
- [ ] 幂等：重复运行不改变已有 project_path
- [ ] `cargo check` + `cargo test` 通过
- [ ] `cargo clippy` 无 warning
