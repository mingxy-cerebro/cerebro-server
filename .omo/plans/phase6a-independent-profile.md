# Phase 6a — 独立画像系统 (Profile V2)

## TL;DR

> **Quick Summary**: 将偏好/画像从Memory管线剥离，建立独立的SQLite偏好存储、LLM驱动的归纳引擎和精炼注入协议。新建 `profile_v2/` 模块，14个API端点，5张SQLite表。
> 
> **Deliverables**:
> - `profile_v2/` 新模块（7个Rust源文件）
> - 5张SQLite表（preferences, profile_versions, profile_changelog, induction_runs, induction_locks）
> - 归纳引擎（4种触发机制，deepseek-v4-flash LLM驱动）
> - 注入协议（`<cerebro-profile>` XML，~500 token预算）
> - 14个 `/v2/profile/` API端点
> - Lifecycle集成（dormant检查+清理定时任务）
> - Pipeline集成（ingest末尾异步触发归纳）
> - 12个新增配置项（OMEM_PROFILE_*）
> 
> **Estimated Effort**: Large
> **Parallel Execution**: YES - 3 waves + Final
> **Critical Path**: T1+T2+T3 → T5 → T10 → F1-F4

> ⚠️ **执行前置条件**: Phase 2（Categories Dict）必须全部完成。Phase 2计划: `.omo/plans/phase2-categories-dict.md`
> Phase 2完成后代码库将包含: `store/sqlite.rs`(SqliteStore), `store/sqlite_schema.rs`(DDL), `domain/category.rs`(CategoryRegistry+DashMap缓存), Cargo.toml(rusqlite 0.32 bundled), AppState(sqlite_store+category_registry字段)

---

## Context

### Original Request
从设计文档 `docs/superpowers/specs/2026-05-15-memory-system-rewrite-design.md` L339-753 提取的Phase 6a规格。将偏好/画像从Memory管线中彻底分离，建立独立的两阶段模型：举证(Memory管线) → 归纳(Profile V2引擎) → 注入(精炼结论)。

### Interview Summary
**Key Discussions**:
- 范围：全量6a（CRUD + 归纳引擎 + 注入协议 + lifecycle集成）
- SQLite策略：先共享omem.db（复用Phase 2 SqliteStore），后期按需拆per-tenant
- 归纳LLM：独立配置 deepseek-v4-flash via OpenCode Zen
- 测试：Tests-after
- 旧profile模块不碰，Phase 6b切换

**Research Findings**:
- 现有ProfileService(783行)基于LanceDB无LLM，注入格式为flat bullet list
- Phase 2计划产出: SqliteStore(Mutex<Connection> + WAL + busy_timeout=5000) — **尚未commit到代码库，依赖Phase 2完成**
- Phase 2计划产出: CategoryRegistry(Arc<SqliteStore> + DashMap + lazy loading) — **同上**
- 注入实际只取static_facts中tag=preferences的，dynamic_context丢弃

### Metis Review
**Identified Gaps** (addressed):
- 存储策略矛盾 → 师尊决策先共享omem.db
- 依赖状态需验证Phase 2 Task 4完成（不阻塞规划）
- 归纳prompt模板 → 延迟到T5实现时定义
- 注入tag冲突 → Phase 6b统一迁移
- OpenCode Zen thinking参数 → 运行时测试
- 失败模式/重试策略 → 在plan guardrails中明确
- 注入缓存key → `tenant_id:project_path`
- custom slot验证 → `[a-z_]+` 格式限制

---

## Work Objectives

### Core Objective
建立独立于Memory管线的偏好画像系统，实现"举证→归纳→晋升"两阶段模型，通过精炼注入协议为LLM提供高质量用户偏好上下文。

### Concrete Deliverables
- `omem-server/src/profile_v2/mod.rs` — 模块入口
- `omem-server/src/profile_v2/types.rs` — 偏好类型定义 + 偏好生命周期状态机
- `omem-server/src/profile_v2/slots.rs` — 14个预定义slot + custom:* 自动创建
- `omem-server/src/profile_v2/store.rs` — ProfileStore (Arc<SqliteStore> + DashMap cache)
- `omem-server/src/profile_v2/migration.rs` — 5张表DDL + seed data
- `omem-server/src/profile_v2/induction.rs` — 归纳引擎（LLM驱动）
- `omem-server/src/profile_v2/injection.rs` — 注入协议（token预算+格式+缓存）
- `omem-server/src/profile_v2/service.rs` — ProfileV2Service（统一门面）
- `omem-server/src/api/handlers/profile_v2.rs` — 14个API handler
- 修改: `api/server.rs`, `api/router.rs`, `main.rs`, `config.rs`, `llm/mod.rs`, `lifecycle/scheduler.rs`, `ingest/pipeline.rs`

### Definition of Done
- [ ] `cargo check` 无错误
- [ ] `cargo test` 现有370个测试不回归
- [ ] `cargo clippy` 无新warning
- [ ] 所有14个API端点可通过curl调用
- [ ] 归纳引擎可手动触发并产生偏好
- [ ] 注入协议输出 `<cerebro-profile>` 格式，≤600字符
- [ ] Lifecycle scheduler包含dormant检查

### Must Have
- 5张SQLite表（preferences, profile_versions, profile_changelog, induction_runs, induction_locks）
- 14个预定义偏好slot（11单值+3多值）
- custom:* slot自动创建
- 偏好生命周期状态机（active → reinforce → dormant(90d) → deleted(180d)）
- 归纳引擎4种触发（session结束/阈值/跨项目/手动）
- 归纳并发锁（per-tenant互斥，TTL 600s）
- 注入token预算~500 tokens（全局20+项目10条）
- Profile LLM独立配置（deepseek-v4-flash via OpenCode Zen）
- 12个OMEM_PROFILE_*环境变量
- 与ingest pipeline末尾的异步触发集成
- 与lifecycle scheduler的dormant检查集成

### Must NOT Have (Guardrails)
- ❌ 不修改旧profile模块（profile/service.rs, domain/profile.rs, api/handlers/profile.rs, ingest/preference_slots.rs）
- ❌ 不import旧profile模块的任何类型
- ❌ 不在ingest pipeline热路径上await归纳结果
- ❌ 不引入Weibull/线性confidence衰减（reinforce-only）
- ❌ 不做v1→v2数据迁移（Phase 6b）
- ❌ 不做批量导入/导出API
- ❌ 不做Admin UI/WebSocket/实时推送
- ❌ 不做偏好推荐/suggestion
- ❌ 不做跨tenant共享
- ❌ 不做偏好版本回滚API
- ❌ LLM调用不设置超过60s超时
- ❌ 归纳不在pipeline热路径上同步执行
- ❌ 不使用placeholder/模糊测试数据
- ❌ 不要求"用户手动测试"任何功能

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test, 370 inline tests)
- **Automated tests**: Tests-after
- **Framework**: cargo test (Rust inline tests)
- **New module tests**: 每个profile_v2文件包含 `#[cfg(test)] mod tests`

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **API/Backend**: Use Bash (curl) - Send requests, assert status + response fields
- **Rust module**: Use Bash (cargo test) - Run inline tests, check compilation
- **Integration**: Use Bash (cargo check + cargo test) - Verify no regressions

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - foundation):
├── Task 1: 类型定义 + Slot定义 (types.rs + slots.rs) [quick]
├── Task 2: SQLite DDL + ProfileStore基础 (migration.rs + store.rs) [quick]
├── Task 3: Profile LLM配置 + 工厂 (config.rs + llm/) [quick]
└── Task 4: ProfileV2Service核心 + 缓存 (service.rs + mod.rs) [quick]

Wave 2 (After Wave 1 - core logic, MAX PARALLEL):
├── Task 5: 归纳引擎 (induction.rs) (depends: T1,T2,T3) [deep]
├── Task 6: 注入协议 (injection.rs) (depends: T1,T2) [unspecified-high]
├── Task 7: API handlers + router (depends: T1,T2,T4) [unspecified-high]
└── Task 8: Lifecycle集成 (depends: T2) [unspecified-high]

Wave 3 (After Wave 2 - integration):
├── Task 9: Main.rs + AppState集成 (depends: T4,T7,T8) [quick]
└── Task 10: Pipeline集成 (depends: T5,T9) [quick]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: T1/T2/T3 → T5 → T10 → F1-F4
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 4 (Wave 1)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| T1   | - | T5, T6, T7 | 1 |
| T2   | - | T5, T6, T7, T8 | 1 |
| T3   | - | T5 | 1 |
| T4   | T2 | T7, T9 | 1 |
| T5   | T1, T2, T3 | T10 | 2 |
| T6   | T1, T2 | T9 | 2 |
| T7   | T1, T2, T4 | T9 | 2 |
| T8   | T2 | T9 | 2 |
| T9   | T4, T7, T8 | T10 | 3 |
| T10  | T5, T9 | F1-F4 | 3 |
| F1-F4 | ALL | - | FINAL |

### Agent Dispatch Summary

- **Wave 1**: 4 tasks — T1→`quick`, T2→`quick`, T3→`quick`, T4→`quick`
- **Wave 2**: 4 tasks — T5→`deep`, T6→`unspecified-high`, T7→`unspecified-high`, T8→`unspecified-high`
- **Wave 3**: 2 tasks — T9→`quick`, T10→`quick`
- **FINAL**: 4 tasks — F1→`oracle`, F2→`unspecified-high`, F3→`unspecified-high`, F4→`deep`

---

## TODOs

- [ ] 1. 类型定义 + Slot系统 (types.rs + slots.rs)

  **What to do**:
  - 创建 `omem-server/src/profile_v2/types.rs`:
    - `PreferenceStatus` enum: `Active`, `Reinforce`, `Dormant`, `Deleted`
    - `PreferenceScope` enum: `Project`, `Global`
    - `Preference` struct: id(Uuid), tenant_id(String), slot(String), value(String), confidence(f32), scope(PreferenceScope), project_path(Option<String>), source(String: "observed"|"explicit"), status(PreferenceStatus), last_reinforced_at(DateTime<Utc>), created_at, updated_at
    - `InductionRun` struct: id, tenant_id, status(String: "running"|"completed"|"failed"), candidate_count(i32), extracted_count(i32), error(Option<String>), started_at, completed_at
    - `InductionLock` struct: id, tenant_id, created_at, ttl_secs(i32)
    - `ProfileVersion` struct: id, tenant_id, snapshot(Json<String>), preference_count(i32), created_at
    - `ProfileChangelog` struct: id, tenant_id, preference_id, action(String: "created"|"updated"|"reinforced"|"dormant"|"deleted"|"promoted"), old_value(Option), new_value(Option), source(String), created_at
    - `InjectionRequest` struct: tenant_id, project_path(Option<String>)
    - `InjectionResult` struct: content(String), preference_count(i32), estimated_tokens(i32)
    - 所有struct derive `Debug, Clone, Serialize, Deserialize`
  - 创建 `omem-server/src/profile_v2/slots.rs`:
    - `SlotDefinition` struct: name(String), display_name(String), is_multi(bool), description(String)
    - `BUILTIN_SLOTS: &[SlotDefinition]` 常量: 14个预定义slot
      - 单值(11): communication_style, tone, code_style, error_handling, naming_convention, testing_strategy, workflow_preference, commit_style, emoji_preference, self_reference, address_style
      - 多值(3): language, framework_preference, preferred_tools
    - `fn is_valid_slot_name(name: &str) -> bool` — 验证格式: `custom:` 前缀 + `[a-z0-9_]+`
    - `fn get_slot_definition(name: &str) -> Option<&SlotDefinition>` — 查找预定义slot
    - `fn is_multi_slot(name: &str) -> bool` — 判断是否多值slot
  - 创建 `omem-server/src/profile_v2/mod.rs` — 模块入口，pub mod所有子模块

  **Must NOT do**:
  - 不import `profile/service.rs` 或 `domain/profile.rs` 的任何类型
  - 不使用 `as any` / `unwrap()` 在非测试代码中

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 纯类型定义+常量，无业务逻辑
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2, T3, T4)
  - **Blocks**: T5, T6, T7
  - **Blocked By**: None

  **References**:
  **Pattern References**:
  - `omem-server/src/domain/profile.rs` — 现有Profile类型（仅参考结构，不import）
  - `omem-server/src/domain/category.rs:CategoryConfig` — struct derive风格参考（Debug, Clone, Serialize, Deserialize）
  - `omem-server/src/store/sqlite_schema.rs:1-30` — DDL中字段类型参考

  **Design Doc References**:
  - `docs/superpowers/specs/2026-05-15-memory-system-rewrite-design.md:380-420` — 偏好数据模型定义
  - `docs/superpowers/specs/2026-05-15-memory-system-rewrite-design.md:420-450` — Slot定义（14预定义）

  **WHY Each Reference Matters**:
  - `domain/profile.rs`: 理解现有UserProfile结构以便设计不冲突的新类型
  - `domain/category.rs:CategoryConfig`: 复制struct derive + field命名风格保持一致性
  - `sqlite_schema.rs`: DDL字段类型必须与Rust struct field类型对应

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 类型定义编译通过
    Tool: Bash (cargo check)
    Preconditions: profile_v2/ 目录存在，包含 types.rs, slots.rs, mod.rs
    Steps:
      1. cargo check -p omem-server
      2. 确认无编译错误
    Expected Result: "Checking omem-server ... Finished" 无error
    Failure Indicators: "error[E....]" 编译错误
    Evidence: .sisyphus/evidence/task-1-compile-check.txt

  Scenario: Slot定义完整性
    Tool: Bash (cargo test)
    Preconditions: slots.rs 包含测试
    Steps:
      1. 在 slots.rs 添加 #[cfg(test)] mod tests { #[test] fn builtin_slots_count() { assert_eq!(BUILTIN_SLOTS.len(), 14); } }
      2. cargo test -p omem-server builtin_slots_count
    Expected Result: "test builtin_slots_count ... ok"
    Failure Indicators: "assertion failed" 或编译错误
    Evidence: .sisyphus/evidence/task-1-slot-test.txt

  Scenario: custom slot名称验证
    Tool: Bash (cargo test)
    Preconditions: is_valid_slot_name 函数实现
    Steps:
      1. 测试有效: is_valid_slot_name("custom:foo_bar") == true, is_valid_slot_name("language") == true
      2. 测试无效: is_valid_slot_name("CUSTOM:FOO") == false, is_valid_slot_name("custom:") == false, is_valid_slot_name("bad slot!") == false
    Expected Result: 所有断言通过
    Failure Indicators: 任何断言失败
    Evidence: .sisyphus/evidence/task-1-slot-validation.txt
  ```

  **Commit**: YES (groups with T2, T3, T4)
  - Message: `feat(profile_v2): add type definitions, slot system, and SQLite store`
  - Files: `omem-server/src/profile_v2/{types,slots,mod}.rs`
  - Pre-commit: `cargo check`

- [ ] 2. SQLite DDL迁移 + ProfileStore基础 (migration.rs + store.rs)

  **What to do**:
  - 创建 `omem-server/src/profile_v2/migration.rs`:
    - `CREATE_PROFILE_TABLES_SQL` 常量: 5张表的DDL
      ```sql
      CREATE TABLE IF NOT EXISTS preferences (
        id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, slot TEXT NOT NULL,
        value TEXT NOT NULL, confidence REAL NOT NULL DEFAULT 0.5,
        scope TEXT NOT NULL DEFAULT 'project', project_path TEXT,
        source TEXT NOT NULL DEFAULT 'observed', status TEXT NOT NULL DEFAULT 'active',
        last_reinforced_at TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
        UNIQUE(tenant_id, slot, value, COALESCE(project_path, ''))
      );
      CREATE INDEX IF NOT EXISTS idx_prefs_tenant_slot ON preferences(tenant_id, slot);
      CREATE INDEX IF NOT EXISTS idx_prefs_tenant_status ON preferences(tenant_id, status);
      -- profile_versions, profile_changelog, induction_runs, induction_locks 类似
      ```
    - `fn create_profile_tables(conn: &Connection) -> Result<()>` — 执行DDL
  - 修改 `omem-server/src/store/sqlite_schema.rs`:
    - 在 `create_tables()` 末尾追加 `migration::create_profile_tables(conn)` 调用（或独立调用点）
  - 创建 `omem-server/src/profile_v2/store.rs`:
    - `pub struct ProfileStore { sqlite: Arc<SqliteStore>, cache: Arc<DashMap<String, CachedPreferences>> }`
    - `CachedPreferences` { prefs: Vec<Preference>, cached_at: Instant }
    - `impl ProfileStore`:
      - `pub fn new(sqlite: Arc<SqliteStore>) -> Self`
      - `pub fn init(&self) -> Result<()>` — 调用migration创建表
      - `pub fn get_preferences(&self, tenant_id: &str, project_path: Option<&str>) -> Result<Vec<Preference>>` — 带缓存
      - `pub fn get_preference_by_id(&self, id: &str) -> Result<Option<Preference>>`
      - `pub fn upsert_preference(&self, pref: &Preference) -> Result<Preference>` — INSERT OR REPLACE
      - `pub fn delete_preference(&self, id: &str) -> Result<bool>`
      - `pub fn update_confidence(&self, id: &str, delta: f32) -> Result<()>`
      - `pub fn update_status(&self, id: &str, status: &str) -> Result<()>`
      - `pub fn get_induction_lock(&self, tenant_id: &str) -> Result<Option<InductionLock>>`
      - `pub fn acquire_induction_lock(&self, tenant_id: &str, ttl_secs: i32) -> Result<bool>`
      - `pub fn release_induction_lock(&self, id: &str) -> Result<()>`
      - `pub fn cleanup_expired_locks(&self) -> Result<usize>`
      - `pub fn create_induction_run(&self, run: &InductionRun) -> Result<()>`
      - `pub fn update_induction_run(&self, id: &str, status: &str, extracted: i32, error: Option<&str>) -> Result<()>`
      - `pub fn get_induction_runs(&self, tenant_id: &str, limit: i32) -> Result<Vec<InductionRun>>`
      - `pub fn record_changelog(&self, entry: &ProfileChangelog) -> Result<()>`
      - `pub fn get_changelog(&self, tenant_id: &str, limit: i32) -> Result<Vec<ProfileChangelog>>`
      - `pub fn save_version(&self, version: &ProfileVersion) -> Result<()>`
      - `pub fn get_versions(&self, tenant_id: &str, limit: i32) -> Result<Vec<ProfileVersion>>`
      - `pub fn invalidate_cache(&self, tenant_id: &str)` — 清除缓存
    - 所有SQLite操作通过 `sqlite.conn().lock()` + `spawn_blocking` 模式
    - 缓存TTL 1800s (30min), stale-while-revalidate到3600s (1hr)

  **Must NOT do**:
  - 不修改现有 `SqliteStore` 的 `init_tables()` 实现（那是Phase 2的scope）
  - 不创建新的SQLite文件（使用共享omem.db）
  - 不在非测试代码中使用 `unwrap()`

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: DDL+CRUD样板代码，模式清晰（copy CategoryRegistry）
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T3, T4)
  - **Blocks**: T5, T6, T7, T8
  - **Blocked By**: None

  **References**:
  **Pattern References**:
  - `.omo/plans/phase2-categories-dict.md` — Phase 2计划中描述的SqliteStore/CategoryRegistry/DDL模式（**Phase 2完成后这些文件才会出现在代码库中**）
  - `omem-server/src/profile/service.rs:70-116` — 缓存模式参考（stale-while-revalidate，当前已存在）

  **API/Type References**:
  - Phase 2计划描述的SqliteStore API: `new(db_path)`, `conn() → &Mutex<Connection>`，用法: `let conn = sqlite.conn().lock().map_err(|e| ...)?; ... drop(conn);`
  - Phase 2计划描述的CategoryRegistry缓存模式: `ensure_loaded()` + `invalidate()` + DashMap

  **Design Doc References**:
  - `docs/superpowers/specs/2026-05-15-memory-system-rewrite-design.md:360-380` — 5张表结构定义
  - `docs/superpowers/specs/2026-05-15-memory-system-rewrite-design.md:450-475` — 偏好生命周期状态机

  **WHY Each Reference Matters**:
  - Phase 2计划: ProfileStore的架构直接复制CategoryRegistry模式（Arc<SqliteStore> + DashMap + ensure_loaded + invalidate），DDL复制sqlite_schema模式。执行时需确认Phase 2已完成再开始本任务。
  - `profile/service.rs:70-116`: 缓存TTL和stale-while-revalidate的具体实现参考

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: ProfileStore编译通过
    Tool: Bash (cargo check)
    Preconditions: store.rs, migration.rs 已创建
    Steps:
      1. cargo check -p omem-server
    Expected Result: "Finished" 无error
    Evidence: .sisyphus/evidence/task-2-compile-check.txt

  Scenario: SQLite DDL执行成功
    Tool: Bash (cargo test)
    Preconditions: migration.rs 包含 create_profile_tables 函数
    Steps:
      1. 在 migration.rs 添加测试: 创建in_memory SQLite，调用create_profile_tables，查询 sqlite_master 验证5张表存在
      2. cargo test -p omem-server profile_tables
    Expected Result: 5张表全部创建成功 (preferences, profile_versions, profile_changelog, induction_runs, induction_locks)
    Evidence: .sisyphus/evidence/task-2-ddl-test.txt

  Scenario: ProfileStore CRUD操作
    Tool: Bash (cargo test)
    Preconditions: store.rs 包含测试
    Steps:
      1. 创建in_memory SqliteStore + ProfileStore
      2. 插入一条偏好: slot="language", value="中文", tenant_id="test-tenant"
      3. 查询验证: get_preferences("test-tenant", None) → 返回1条
      4. 更新confidence: update_confidence(id, 0.15) → confidence变为0.65
      5. 删除: delete_preference(id) → 再次查询返回0条
    Expected Result: 所有CRUD操作正确
    Evidence: .sisyphus/evidence/task-2-crud-test.txt

  Scenario: 归纳锁互斥
    Tool: Bash (cargo test)
    Preconditions: acquire_induction_lock 实现完成
    Steps:
      1. 对tenant "test" acquire_induction_lock(ttl=600) → Ok(true)
      2. 再次 acquire_induction_lock → Ok(false) (已有锁)
      3. release_induction_lock(id)
      4. 再次 acquire_induction_lock → Ok(true)
    Expected Result: 锁的acquire/release正确互斥
    Evidence: .sisyphus/evidence/task-2-lock-test.txt
  ```

  **Commit**: YES (groups with T1, T3, T4)
  - Message: `feat(profile_v2): add type definitions, slot system, and SQLite store`
  - Files: `omem-server/src/profile_v2/{migration,store}.rs`
  - Pre-commit: `cargo check`

- [ ] 3. Profile LLM配置 + 工厂 (config.rs + llm/)

  **What to do**:
  - 修改 `omem-server/src/config.rs`:
    - 新增 `OMEM_PROFILE_LLM_PROVIDER` (default: "openai")
    - 新增 `OMEM_PROFILE_LLM_API_KEY` (default: "")
    - 新增 `OMEM_PROFILE_LLM_MODEL` (default: "deepseek-v4-flash")
    - 新增 `OMEM_PROFILE_LLM_BASE_URL` (default: "https://opencode.ai/zen/v1")
    - 新增 `OMEM_PROFILE_ENABLED` (default: "true")
    - 新增 `OMEM_PROFILE_CACHE_TTL_SECS` (default: "1800")
    - 新增 `OMEM_PROFILE_INDUCTION_COOLDOWN_SECS` (default: "600")
    - 新增 `OMEM_PROFILE_INDUCTION_THRESHOLD` (default: "5")
    - 新增 `OMEM_PROFILE_INJECTION_BUDGET_TOKENS` (default: "500")
    - 新增 `OMEM_PROFILE_MAX_GLOBAL_PREFERENCES` (default: "20")
    - 新增 `OMEM_PROFILE_MAX_PROJECT_PREFERENCES` (default: "10")
    - 新增 `OMEM_PROFILE_DORMANT_DAYS` (default: "90")
    - 在 `OmemConfig` struct 中新增对应字段 + `from_env()` 读取
  - 修改 `omem-server/src/llm/service.rs`:
    - 新增 `pub fn create_profile_llm_service(config: &OmemConfig) -> Result<Arc<dyn LlmService>>` 工厂函数
    - 逻辑: 如果 `OMEM_PROFILE_LLM_API_KEY` 非空则创建实例，否则返回noop
  - 修改 `omem-server/src/llm/openai_compat.rs`:
    - 确认 `new()` 构造器可接受自定义 base_url（用于OpenCode Zen端点）
    - 归纳引擎调用时需传 `thinking: {"type": "disabled"}` 关闭thinking mode（如果端点支持）

  **Must NOT do**:
  - 不修改现有 `OMEM_LLM_*` 或 `OMEM_RECALL_LLM_*` 配置
  - 不修改 `create_llm_service()` 或 `create_recall_llm_service()` 的现有签名
  - 不修改 Cargo.toml（rusqlite已在Phase 2添加）

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 配置项+工厂函数，模式清晰（copy现有LLM配置模式）
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2, T4)
  - **Blocks**: T5
  - **Blocked By**: None

  **References**:
  **Pattern References**:
  - `omem-server/src/config.rs:OmemConfig` — 现有配置struct，照此模式新增字段
  - `omem-server/src/llm/service.rs:create_recall_llm_service()` — LLM工厂函数模式，**照此写 create_profile_llm_service**
  - `omem-server/src/llm/openai_compat.rs` — OpenAI兼容客户端构造器

  **External References**:
  - OpenCode Zen endpoint: `https://opencode.ai/zen/v1` (OpenAI-compatible /chat/completions)
  - Model ID: `deepseek-v4-flash`

  **WHY Each Reference Matters**:
  - `config.rs`: 需要了解OmemConfig的from_env()模式，每个配置项如何声明默认值
  - `llm/service.rs`: create_recall_llm_service是独立的recall LLM工厂，profile LLM工厂完全复制此模式
  - `openai_compat.rs`: 需确认base_url可自定义（OpenCode Zen不是api.openai.com）

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 配置项编译通过
    Tool: Bash (cargo check)
    Preconditions: config.rs 和 llm/service.rs 已修改
    Steps:
      1. cargo check -p omem-server
    Expected Result: "Finished" 无error
    Evidence: .sisyphus/evidence/task-3-compile-check.txt

  Scenario: Profile LLM工厂创建
    Tool: Bash (cargo test)
    Preconditions: create_profile_llm_service 实现完成
    Steps:
      1. 测试: 使用空API key → 返回noop LLM实例
      2. 测试: 使用mock配置 → 返回openai_compat实例，base_url正确
    Expected Result: 工厂函数根据配置返回正确实例
    Evidence: .sisyphus/evidence/task-3-llm-factory.txt

  Scenario: 环境变量默认值
    Tool: Bash (cargo test)
    Preconditions: OmemConfig 包含 profile 字段
    Steps:
      1. 不设置任何OMEM_PROFILE_*环境变量
      2. OmemConfig::from_env() → 验证默认值: enabled=true, model="deepseek-v4-flash", base_url="https://opencode.ai/zen/v1", dormant_days=90
    Expected Result: 所有默认值正确
    Evidence: .sisyphus/evidence/task-3-config-defaults.txt
  ```

  **Commit**: YES (groups with T1, T2, T4)
  - Message: `feat(profile_v2): add type definitions, slot system, and SQLite store`
  - Files: `omem-server/src/config.rs`, `omem-server/src/llm/service.rs`
  - Pre-commit: `cargo check`

- [ ] 4. ProfileV2Service核心结构 + 缓存 (service.rs)

  **What to do**:
  - 创建 `omem-server/src/profile_v2/service.rs`:
    - `pub struct ProfileV2Service { store: Arc<ProfileStore>, llm: Option<Arc<dyn LlmService>> }`
    - `impl ProfileV2Service`:
      - `pub fn new(store: Arc<ProfileStore>, llm: Option<Arc<dyn LlmService>>) -> Self`
      - `pub fn store(&self) -> &Arc<ProfileStore>` — 给handler/injection访问store
      - `pub fn llm(&self) -> Option<&Arc<dyn LlmService>>` — 给归纳引擎访问LLM
    - 缓存管理方法委托给ProfileStore
  - 更新 `omem-server/src/profile_v2/mod.rs`:
    - 确认 `pub mod service;` 已注册

  **Must NOT do**:
  - 不在这里实现归纳引擎逻辑（T5的scope）
  - 不在这里实现注入协议（T6的scope）

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 轻量级service层，主要组装store+llm
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2, T3)
  - **Blocks**: T7, T9
  - **Blocked By**: T2 (需要ProfileStore)

  **References**:
  **Pattern References**:
  - `omem-server/src/profile/service.rs:44-49` — 现有ProfileService结构（store + llm + cache），参考但不复制
  - `omem-server/src/domain/category.rs:CategoryRegistry` — Service包装SqliteStore的模式

  **WHY Each Reference Matters**:
  - `profile/service.rs`: 了解现有service如何组织store+llm+cache，但profile_v2用更简单的结构
  - `CategoryRegistry`: 参考service如何暴露store方法给外部调用者

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Service编译通过
    Tool: Bash (cargo check)
    Preconditions: service.rs 已创建
    Steps:
      1. cargo check -p omem-server
    Expected Result: "Finished" 无error
    Evidence: .sisyphus/evidence/task-4-compile-check.txt

  Scenario: Service组装正确
    Tool: Bash (cargo test)
    Preconditions: service.rs 包含测试
    Steps:
      1. 创建in_memory ProfileStore
      2. ProfileV2Service::new(store, None)
      3. 验证 store() 和 llm() 返回正确引用
    Expected Result: service正确组装store和llm
    Evidence: .sisyphus/evidence/task-4-service-assembly.txt
  ```

  **Commit**: YES (groups with T1, T2, T3)
  - Message: `feat(profile_v2): add type definitions, slot system, and SQLite store`
  - Files: `omem-server/src/profile_v2/service.rs`
  - Pre-commit: `cargo check`

- [ ] 5. 归纳引擎 (induction.rs)

  **What to do**:
  - 创建 `profile_v2/induction.rs`:
    - `InductionEngine { service: Arc<ProfileV2Service> }`
    - `trigger_induction(tenant_id, trigger_reason)`:
      1. 检查归纳锁（锁存在且未过期→skip）
      2. 检查冷却期（最近run + cooldown > now → skip）
      3. acquire_induction_lock(ttl=600s) + 创建induction_run(status="running")
      4. 查询候选记忆：LanceStore查询 category in [preferences,patterns,events], importance≥0.3, 最近7天, ≤50条
      5. 调用归纳LLM（不可用→failed返回）: System prompt="你是偏好归纳引擎。从行为记忆中提取偏好。每条=一个slot一个值=15-30字。仅从提供的记忆提取，不编造。输出JSON数组: [{\"slot\":\"xxx\",\"value\":\"xxx\",\"confidence\":0.0-1.0,\"scope\":\"project|global\"}]" + 超时60s
      6. 解析JSON, 验证slot合法性+confidence范围+value长度
      7. 冲突解决: explicit > observed, 新近 > 陈旧, conf高 > 低
      8. Reinforce: 同slot+value → confidence += 0.15 (上限0.95)
      9. 跨项目晋升: 同slot+value在≥2项目 → scope=global, confidence += 0.2
      10. 写入preferences + changelog + 释放锁 + 更新run + 保存version
    - 错误: LLM无效JSON重试1次skip, 超时/429标记failed, SQLite失败回滚
    - 异步: spawn detached tokio task, LLM调用async
  **Must NOT do**: 不在热路径同步调用, 不引入confidence衰减, 不调用旧preference_slots
  **Category**: `deep` | **Skills**: `[]` | **Wave**: 2 | **Blocks**: T10 | **Blocked By**: T1,T2,T3
  **References**: `profile_v2/store.rs`(T2), `profile_v2/types.rs`(T1), `llm/service.rs:LlmService`, `store/lancedb.rs:LanceStore`, 设计文档:475-620
  **QA**: 锁互斥(同tenant互斥不同独立), LLM→DB写入, reinforce +0.15, LLM超时→failed不panic

- [ ] 6. 注入协议 (injection.rs)

  **What to do**:
  - 创建 `profile_v2/injection.rs`:
    - `InjectionBuilder { store: Arc<ProfileStore> }`
    - `build_injection(tenant_id, project_path)`:
      1. 全局偏好(global,active) 按confidence降序 ≤20条
      2. 项目偏好(project, project_path匹配, active) ≤10条
      3. Token预算: 总字符≤600, 超出从最低confidence裁剪
      4. 格式化: `<cerebro-profile>\n  · 偏好1\n  · 偏好2\n</cerebro-profile>` (flat bullet list)
      5. 返回 InjectionResult { content, preference_count, estimated_tokens }
    - 缓存: DashMap key=`tenant_id:project_path` TTL=1800s, 写入时invalidate
  **Must NOT do**: 不用JSON格式, 不超600字符
  **Category**: `unspecified-high` | **Skills**: `[]` | **Wave**: 2 | **Blocks**: T9 | **Blocked By**: T1,T2
  **References**: `profile/service.rs`(了解画像输出), `hooks.ts:378-418`(注入格式化,不能破坏), 设计文档:620-700
  **QA**: 格式兼容<cerebro-profile>, 25条偏好裁剪≤600字符, 缓存写入后失效

- [ ] 7. API handlers + 路由 (handlers/profile_v2.rs + router.rs)

  **What to do**:
  - 创建 `api/handlers/profile_v2.rs`: 14个Axum handler
    - CRUD: get_preferences, get_preference, create_preference, update_preference, delete_preference
    - 注入: get_injection
    - 归纳: trigger_induction, get_induction_runs
    - 画像: get_profile, get_profile_versions, get_changelog, get_stats
    - 每个handler: `State(state): State<Arc<AppState>>` + `Extension(auth)`
    - 显式偏好: source="explicit", confidence=0.9, scope=global
  - 修改 `api/router.rs`: 新增 `/v2/profile` 路由组
  - 修改 `api/handlers/mod.rs`: 新增 `pub mod profile_v2;`
  **Must NOT do**: 不修改/v1/profile路由, 不在handler直接操作SQLite
  **Category**: `unspecified-high` | **Skills**: `[]` | **Wave**: 2 | **Blocks**: T9 | **Blocked By**: T1,T2,T4
  **References**: `api/handlers/profile.rs`(handler签名), `api/handlers/memory.rs`(CRUD模式), `api/router.rs`(路由注册)
  **QA**: 14路由注册验证, CRUD完整流程(POST→201→GET→200→PUT→200→DELETE→204→GET→404), Inject端点格式

- [ ] 8. Lifecycle集成 (lifecycle/scheduler.rs)

  **What to do**:
  - 修改 `lifecycle/scheduler.rs`:
    - 新增 `check_dormant_preferences()`: active/reinforce且last_reinforced_at < now-dormant_days → dormant + changelog
    - 新增 `cleanup_deleted_preferences()`: dormant超180天 → 物理删除 + changelog
    - 新增 `cleanup_expired_induction_locks()`: 调用ProfileStore::cleanup_expired_locks()
    - 在 `start()` 注册定时任务, `run_on_start` 清理过期锁
  - 注入 `Arc<ProfileStore>` 到 LifecycleScheduler
  **Must NOT do**: 不引入Weibull/线性confidence衰减, 不修改现有lifecycle任务
  **Category**: `unspecified-high` | **Skills**: `[]` | **Wave**: 2 | **Blocks**: T9 | **Blocked By**: T2
  **References**: `lifecycle/scheduler.rs`(现有定时任务模式), `profile_v2/store.rs`(cleanup方法), 设计文档:450-475,700-720
  **QA**: dormant检查(91天前→dormant), 过期锁清理(ttl=1s→2s后清除), deleted清理(181天dormant→删除)

- [ ] 9. Main.rs + AppState集成

  **What to do**:
  - 修改 `api/server.rs`: AppState新增 `profile_v2_service`, `induction_engine`, `injection_builder`
  - 修改 `main.rs`: category_registry之后初始化 ProfileStore→profile_llm→ProfileV2Service→InductionEngine→InjectionBuilder, 注入AppState + LifecycleScheduler
  - 修改 `api/handlers/mod.rs`: `pub mod profile_v2;`
  **Must NOT do**: 不修改旧profile初始化, 不移除profile_cache字段
  **Category**: `quick` | **Skills**: `[]` | **Wave**: 3 | **Blocks**: T10 | **Blocked By**: T4,T7,T8
  **References**: `main.rs:60-100`(初始化顺序), `api/server.rs:AppState`(字段声明模式)
  **QA**: cargo check通过, AppState三字段正确注册

- [ ] 10. Pipeline集成 (ingest/pipeline.rs)

  **What to do**:
  - 修改 `ingest/pipeline.rs`:
    - Pipeline完成后异步触发: `if let Some(engine) = &self.induction_engine { tokio::spawn(async move { engine.trigger_induction(&tenant_id, "session_end").await; }); }`
    - Pipeline持有 `Option<Arc<InductionEngine>>`
    - 阈值触发: candidate_count >= threshold → trigger_on_threshold
  **Must NOT do**: 不await归纳结果, 不修改现有stage逻辑, 归纳失败不影响pipeline返回
  **Category**: `quick` | **Skills**: `[]` | **Wave**: 3 | **Blocks**: F1-F4 | **Blocked By**: T5,T9
  **References**: `ingest/pipeline.rs`(pipeline完成hook点), `profile_v2/induction.rs`(trigger_induction API)
  **QA**: cargo check通过, mock10s延迟pipeline<10s返回, mock Err pipeline正常返回

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, curl endpoint, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo check` + `cargo clippy` + `cargo test`. Review all changed files for: `unwrap()` in non-test code, empty catches, println in prod, commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction, generic names.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Test cross-task integration (features working together). Test edge cases: empty tenant, concurrent induction, token budget overflow. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git log/diff). Verify 1:1 — everything in spec was built, nothing beyond spec. Check "Must NOT do" compliance. Detect cross-task contamination. Flag unaccounted changes. Verify old profile modules untouched.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `feat(profile_v2): add type definitions, slot system, and SQLite store` - profile_v2/{types,slots,migration,store,service,mod}.rs
- **Wave 2**: `feat(profile_v2): add induction engine, injection protocol, API handlers, and lifecycle` - profile_v2/{induction,injection}.rs, api/handlers/profile_v2.rs, lifecycle changes
- **Wave 3**: `feat(profile_v2): integrate with main.rs and ingest pipeline` - main.rs, api/server.rs, api/router.rs, api/handlers/mod.rs, ingest/pipeline.rs, lifecycle/scheduler.rs
- Pre-commit: `cargo check && cargo test`

---

## Success Criteria

### Verification Commands
```bash
cargo check                              # Expected: no errors
cargo test                               # Expected: 370+ pass, 0 regressions
cargo clippy                             # Expected: no new warnings
curl http://localhost:8080/v2/profile/preferences  # Expected: 200 + JSON array
curl http://localhost:8080/v2/profile/inject       # Expected: 200 + <cerebro-profile> block
curl -X POST http://localhost:8080/v2/profile/induction/trigger  # Expected: 200 + run_id
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All 370 existing tests pass (no regressions)
- [ ] All 14 API endpoints respond correctly
- [ ] Induction engine produces preferences from memories
- [ ] Injection output ≤600 characters
- [ ] Lifecycle dormant check runs periodically
- [ ] Old profile modules completely untouched
