# Phase 3: 记忆隔离 (project_path)

## TL;DR

> **核心目标**: Memory模型新增 `project_path: Option<String>` 字段，召回时按项目路径隔离。全局类别（profile/preferences/identity/private/emotional）绕过过滤，项目类别（scope=project）按project_path精确匹配。旧数据需做数据治理和迁移（见Task 7补充）。
>
> **交付物**:
> - Memory.project_path 字段 + LanceDB schema migration
> - IngestRequest.project_path 字段 + 写入路径串接
> - SearchRequest.project_path_filter + WHERE 子句硬过滤
> - should_recall 两阶段搜索适配
> - sanitize_project_path() 安全函数
> - 旧数据迁移策略（数据治理）
>
> **预估工作量**: Medium（~600行，8-10文件）
> **并行执行**: YES - 3 waves
> **关键路径**: Task 1(schema) → Task 2(ingest写入) → Task 3(recall过滤) → Tests

---

## Context

### 原始需求
Memory表新增project_path字段，召回时按项目路径隔离，实现"工作记忆按项目隔离、全局记忆共享"的分层策略。

### 访谈总结
**关键讨论**:
- Phase 3 等Phase 2完成后再做（依赖CategoryRegistry scope字典）
- Phase 3 = 纯project_path隔离，不含private memory加密
- SessionRecall补字段 → 已完成
- TDD策略

**研究结论**:
- `reconciler.rs create_fact_memory()` L669 是唯一Memory创建入口（chokepoint）
- `Memory.scope` 有5个值（public/private/global/team/org），是访问控制维度，不能复用做项目隔离
- `tags_filter` 仅影响RRF排名，不影响WHERE过滤 — 必须在WHERE子句做硬过滤
- `scope_filter` 在should_recall中始终为None，从未被激活
- LanceDB schema evolution 通过 `AllNulls` + `fix_null_columns` 自动兼容新字段

### 灵犀(Metis)审查
**已识别并解决的差距**:
- scope字段语义：确认project_path与scope正交，不复用scope
- tags机制：确认tags仅影响排名，不能依赖做硬隔离
- SQL注入：project_path含特殊字符需sanitize_project_path()
- make_shared_copy：共享时project_path处理记录为Tech Debt
- clustering跨project混合：P0风险但Phase 3不改，记录Tech Debt

---

## Work Objectives

### 核心目标
为Memory添加project_path维度，实现项目级别的记忆隔离，同时保持全局类别记忆的跨项目共享。

### 具体交付物
- `domain/memory.rs`: Memory struct新增 `project_path: Option<String>`
- `store/lancedb.rs`: LanceDB schema新增project_path列 + memory_to_batch/batch_to_memories适配
- `ingest/types.rs`: IngestRequest新增 `project_path: Option<String>`
- `ingest/pipeline.rs`: 传递project_path到reconciler
- `ingest/reconciler.rs`: create_fact_memory()写入project_path
- `retrieve/pipeline.rs`: SearchRequest新增 `project_path_filter: Option<String>`
- `store/lancedb.rs`: vector_search/fts_search WHERE子句添加project_path过滤
- `api/handlers/session_recalls.rs`: should_recall两阶段搜索传递project_path
- `api/handlers/memory.rs`: session_ingest传递project_path，search传递project_path_filter
- 安全函数: sanitize_project_path() + escape_sql()防护

### 完成定义
- [ ] `cargo test -p omem-server` 全部通过（含新增project_path相关测试）
- [ ] `cargo check` 无错误
- [ ] 所有Memory创建路径都正确写入project_path
- [ ] vector_search/fts_search按project_path正确过滤
- [ ] should_recall两阶段搜索正确处理project_path

### Must Have
- Memory.project_path 字段 + LanceDB schema migration
- IngestRequest.project_path 字段
- reconciler create_fact_memory() 唯一chokepoint写入project_path
- SearchRequest.project_path_filter + WHERE子句硬过滤
- should_recall两阶段搜索适配project_path
- sanitize_project_path() 安全函数
- TDD测试覆盖（使用专用测试API Key隔离测试数据）
- 向后兼容：不传project_path的请求行为不变
- 旧数据迁移：project_path=NULL的数据需治理（至少记录统计，可选填充）
- 测试使用新建专用API Key，确保测试精准不污染生产数据

### Must NOT Have (Guardrails)
- 不修改 Memory.scope 的现有值和语义（scope是访问控制，project_path是独立维度）
- 不依赖 tags_filter 做项目隔离（tags仅影响排名）
- 不修改 clustering/lifecycle/profile/sharing 逻辑
- 不修改插件代码（plugins/*）
- 不修改 API 路由结构或认证逻辑
- 不添加路径规范化/aliasing（存原始路径）
- 不修改 scope_filter 现有行为

---

## Verification Strategy

> **零人工干预** — 所有验证agent执行。无例外。

### 测试决策
- **已有基础设施**: YES（373个inline测试，49个文件）
- **自动化测试**: TDD（先写测试再写实现）
- **框架**: cargo test（inline tests）
- **TDD**: 每个task遵循 RED(失败测试) → GREEN(最小实现) → REFACTOR

### QA策略
每个task必须包含agent执行的QA场景。
证据保存到 `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`。

- **Rust内部**: 使用 Bash (cargo test) — 运行特定测试模块
- **LanceDB操作**: 使用 Bash (cargo test) — inline测试使用内存SQLite/LanceDB

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — 基础设施):
├── Task 1: Memory model + LanceDB schema + migration [quick]
└── Task 2: IngestRequest + pipeline传递 + reconcile写入 [quick]

Wave 2 (After Wave 1 — 核心过滤):
├── Task 3: SearchRequest + vector_search/fts_search WHERE过滤 [deep]
├── Task 4: should_recall两阶段搜索适配 [deep]
└── Task 5: API handlers传递project_path [quick]

Wave 3 (After Wave 2 — 安全+测试):
├── Task 6: sanitize_project_path()安全函数 [quick]
└── Task 7: 集成测试 + 旧数据兼容验证 [deep]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
→ Present results → Get explicit user okay

Critical Path: Task 1 → Task 2 → Task 3 → Task 4 → Task 7 → F1-F4 → user okay
Parallel Speedup: ~40% faster than sequential
Max Concurrent: 3 (Wave 2)
```

### Dependency Matrix

| Task | Blocked By | Blocks |
|------|-----------|--------|
| 1    | -         | 2, 3   |
| 2    | 1         | 4, 5   |
| 3    | 1         | 4, 7   |
| 4    | 2, 3      | 7      |
| 5    | 2         | 7      |
| 6    | -         | 7      |
| 7    | 3, 4, 5, 6 | F1-F4 |

### Agent Dispatch Summary

- **Wave 1**: 2 tasks — T1 `quick`, T2 `quick`
- **Wave 2**: 3 tasks — T3 `deep`, T4 `deep`, T5 `quick`
- **Wave 3**: 2 tasks — T6 `quick`, T7 `deep`
- **FINAL**: 4 tasks — F1 `oracle`, F2 `unspecified-high`, F3 `unspecified-high`, F4 `deep`

---

## TODOs

> 每个task: 实现 + 测试 = 一个task，不分离。

- [x] 1. Memory Model + LanceDB Schema + Migration

  **What to do**:
  - 在 `domain/memory.rs` Memory struct 新增 `pub project_path: Option<String>` 字段
  - 更新 `Memory::new()` 构造函数，默认 `project_path: None`
  - 在 `store/lancedb.rs` 的 LanceDB schema（L551-596）新增 `Field::new("project_path", DataType::Utf8, true)` 列
  - 更新 `memory_to_batch()` (L1136) 添加 project_path 列
  - 更新 `batch_to_memories()` 解析 project_path 列
  - 添加 TDD 测试：验证 Memory::new() 默认 project_path=None
  - 添加 TDD 测试：验证 project_path 序列化/反序列化roundtrip
  - 添加 TDD 测试：验证 LanceDB schema evolution（新列自动补NULL）

  **Must NOT do**:
  - 不修改 Memory.scope 字段
  - 不修改其他 Memory 字段

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO（基础，后续task都依赖）
  - **Parallel Group**: Wave 1
  - **Blocks**: Tasks 2, 3
  - **Blocked By**: None

  **References**:
  - `omem-server/src/domain/memory.rs:30-70` — Memory struct，30字段，需加 project_path
  - `omem-server/src/domain/memory.rs:73-114` — Memory::new() 构造函数，默认 scope="global", space_id=""
  - `omem-server/src/store/lancedb.rs:551-596` — LanceDB schema定义，36列
  - `omem-server/src/store/lancedb.rs:241-267` — init_table() AllNulls + fix_null_columns 自动migration模式
  - `omem-server/src/store/lancedb.rs:1136` — memory_to_batch() 序列化Memory→RecordBatch
  - `omem-server/src/store/lancedb.rs:batch_to_memories` — 反序列化RecordBatch→Memory

  **Acceptance Criteria**:
  - [ ] Memory struct 有 `project_path: Option<String>` 字段
  - [ ] Memory::new() 默认 `project_path: None`
  - [ ] LanceDB schema 有 project_path 列（nullable Utf8）
  - [ ] memory_to_batch() 包含 project_path 列
  - [ ] batch_to_memories() 正确解析 project_path
  - [ ] TDD: 3个测试通过（默认值、serde roundtrip、schema evolution）

  **QA Scenarios**:
  ```
  Scenario: Memory默认project_path为None
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server test_memory_default_project_path_none
    Expected Result: 测试通过，Memory::new().project_path == None
    Evidence: .sisyphus/evidence/task-1-default-none.txt

  Scenario: project_path serde roundtrip
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server test_project_path_serde_roundtrip
    Expected Result: 序列化+反序列化后project_path保持一致
    Evidence: .sisyphus/evidence/task-1-serde-roundtrip.txt

  Scenario: LanceDB schema evolution兼容旧数据
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server test_schema_evolution_project_path
    Expected Result: 旧数据无project_path列时，读取返回None
    Evidence: .sisyphus/evidence/task-1-schema-evolution.txt
  ```

  **Commit**: YES (C1)
  - Message: `feat(domain+store): add project_path field to Memory model and LanceDB schema`
  - Files: domain/memory.rs, store/lancedb.rs

- [x] 2. Ingest写入路径：IngestRequest + Pipeline + Reconciler

  **What to do**:
  - 在 `ingest/types.rs` IngestRequest struct 新增 `pub project_path: Option<String>` 字段
  - 在 `ingest/pipeline.rs` ~L283 传递 project_path 给 reconciler.reconcile()
  - 修改 `ingest/reconciler.rs` Reconciler::reconcile() 签名接收 project_path
  - 修改 `create_fact_memory()` (L669) 将 project_path 写入 Memory
  - 添加 TDD 测试：验证 reconciler 传递 project_path 到 Memory
  - 添加 TDD 测试：验证 IngestRequest 解析 project_path
  - 添加 TDD 测试：验证空字符串 normalize 为 None

  **Must NOT do**:
  - 不修改 Reconciler 的 merge/SKIP 等决策逻辑
  - 不修改 files.rs 上传路径（上传产生的记忆 project_path = None = global）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO（依赖Task 1）
  - **Parallel Group**: Wave 1 (after Task 1)
  - **Blocks**: Tasks 4, 5
  - **Blocked By**: Task 1

  **References**:
  - `omem-server/src/ingest/types.rs:66-79` — IngestRequest struct，有 project_name 无 project_path
  - `omem-server/src/ingest/pipeline.rs:283` — pipeline 传参给 reconciler.reconcile() 的位置
  - `omem-server/src/ingest/reconciler.rs:669-703` — create_fact_memory() 唯一Memory创建入口
  - `omem-server/src/ingest/reconciler.rs:Reconciler::reconcile` — reconcile 签名需加 project_path

  **Acceptance Criteria**:
  - [ ] IngestRequest 有 `project_path: Option<String>` 字段
  - [ ] Pipeline 将 project_path 传递给 reconciler
  - [ ] create_fact_memory() 将 project_path 写入 Memory
  - [ ] session_ingest 从请求体传递 project_path
  - [ ] TDD: reconciler传递测试通过
  - [ ] TDD: 空字符串 "" normalize 为 None

  **QA Scenarios**:
  ```
  Scenario: IngestRequest解析project_path
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server test_ingest_request_project_path
    Expected Result: JSON含project_path字段时正确解析为Some("/path/to/project")
    Evidence: .sisyphus/evidence/task-2-ingest-parse.txt

  Scenario: Reconciler写入project_path到Memory
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server test_reconciler_project_path_write
    Expected Result: create_fact_memory()生成的Memory含正确project_path
    Evidence: .sisyphus/evidence/task-2-reconciler-write.txt

  Scenario: 空字符串normalize为None
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server test_project_path_empty_normalize
    Expected Result: project_path="" 被normalize为None
    Evidence: .sisyphus/evidence/task-2-empty-normalize.txt
  ```

  **Commit**: YES
  - Message: `feat(ingest): thread project_path through ingest pipeline`
  - Files: ingest/types.rs, ingest/pipeline.rs, ingest/reconciler.rs

- [x] 3. Search过滤：SearchRequest + vector_search/fts_search WHERE

  **What to do**:
  - 在 `retrieve/pipeline.rs` SearchRequest struct 新增 `pub project_path_filter: Option<String>` 字段
  - 在 `store/lancedb.rs` `vector_search()` 的 WHERE 子句添加 project_path 过滤逻辑：
    ```
    当 project_path_filter = Some("/foo") 时:
      WHERE ... AND (project_path IS NULL OR project_path = '/foo')
    当 project_path_filter = None 时:
      不过滤（当前行为不变）
    ```
  - 在 `store/lancedb.rs` `fts_search()` 同样添加 project_path 过滤逻辑
  - 添加 TDD 测试：验证 project_path 过滤正确性
  - 添加 TDD 测试：验证 NULL project_path 记忆在过滤时被包含（视为global）
  - 添加 TDD 测试：验证 project_path_filter=None 时不过滤

  **Must NOT do**:
  - 不修改 scope_filter 逻辑
  - 不修改 tags_filter 逻辑
  - 不修改 build_visibility_filter()

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES（与Task 2无依赖）
  - **Parallel Group**: Wave 2 (after Task 1)
  - **Blocks**: Tasks 4, 7
  - **Blocked By**: Task 1

  **References**:
  - `omem-server/src/retrieve/pipeline.rs:29-42` — SearchRequest struct
  - `omem-server/src/store/lancedb.rs:vector_search` — 向量搜索，WHERE用 `.only_if()` 拼接
  - `omem-server/src/store/lancedb.rs:fts_search` — 全文搜索，WHERE同上
  - `omem-server/src/store/lancedb.rs:1978-2004` — build_visibility_filter() OR clause模式
  - `omem-server/src/store/lancedb.rs:escape_sql` — SQL转义（仅转义 `'` → `''`）

  **Acceptance Criteria**:
  - [ ] SearchRequest 有 `project_path_filter: Option<String>` 字段
  - [ ] vector_search WHERE 包含 project_path 过滤（Some时）
  - [ ] fts_search WHERE 包含 project_path 过滤（Some时）
  - [ ] NULL project_path 记录在过滤时被包含（IS NULL OR = value）
  - [ ] project_path_filter=None 时不过滤
  - [ ] TDD: 3个搜索过滤测试通过

  **QA Scenarios**:
  ```
  Scenario: project_path过滤只返回匹配或NULL记录
    Tool: Bash (cargo test)
    Steps:
      1. 创建2条记忆：project_path=Some("/A"), project_path=Some("/B"), project_path=None
      2. 以 project_path_filter=Some("/A") 搜索
      3. 验证只返回 /A 和 None 的记忆
    Expected Result: 不返回 /B 的记忆
    Evidence: .sisyphus/evidence/task-3-filter-match.txt

  Scenario: project_path_filter=None不过滤
    Tool: Bash (cargo test)
    Steps:
      1. 创建3条记忆（不同project_path）
      2. 以 project_path_filter=None 搜索
      3. 验证返回所有记忆
    Expected Result: 无project_path过滤
    Evidence: .sisyphus/evidence/task-3-filter-none.txt

  Scenario: SQL注入防护
    Tool: Bash (cargo test)
    Steps:
      1. 以 project_path_filter=Some("'; DROP TABLE --") 搜索
      2. 验证不触发SQL错误，无数据丢失
    Expected Result: 安全处理特殊字符
    Evidence: .sisyphus/evidence/task-3-sql-injection.txt
  ```

  **Commit**: YES
  - Message: `feat(retrieve): add project_path filtering to search`
  - Files: retrieve/pipeline.rs, store/lancedb.rs

- [x] 4. should_recall两阶段搜索适配

  **What to do**:
  - 在 `api/handlers/session_recalls.rs` ShouldRecallRequest 新增 `pub project_path: Option<String>` 字段
  - Phase 1 (project_tags搜索) 添加 project_path_filter 到 SearchRequest
  - Phase 2 (global fallback) 添加 project_path_filter 到 global SearchRequest
  - 全局类别绕过逻辑：当有 project_path 时，Phase 2 的全局回退应包含 project_path IS NULL 的全局记忆
  - 添加 TDD 测试：验证 should_recall 正确传递 project_path
  - 添加 TDD 测试：验证两阶段搜索的 project_path 隔离

  **Must NOT do**:
  - 不修改 should_recall 的 LLM 判断逻辑
  - 不修改质量门槛逻辑
  - 不修改 rate limiting

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES（与Task 5平行）
  - **Parallel Group**: Wave 2 (after Tasks 1, 2, 3)
  - **Blocks**: Task 7
  - **Blocked By**: Tasks 2, 3

  **References**:
  - `omem-server/src/api/handlers/session_recalls.rs:33-59` — ShouldRecallRequest struct
  - `omem-server/src/api/handlers/session_recalls.rs:309-409` — 两阶段搜索实现
  - `omem-server/src/api/handlers/session_recalls.rs:319-332` — Phase 1 SearchRequest构建
  - `omem-server/src/api/handlers/session_recalls.rs:364-377` — Phase 2 global SearchRequest构建

  **Acceptance Criteria**:
  - [ ] ShouldRecallRequest 有 `project_path: Option<String>` 字段
  - [ ] Phase 1 传递 project_path_filter 到 SearchRequest
  - [ ] Phase 2 传递 project_path_filter 到 global SearchRequest
  - [ ] TDD: should_recall project_path传递测试通过

  **QA Scenarios**:
  ```
  Scenario: should_recall传递project_path到搜索
    Tool: Bash (cargo test)
    Steps:
      1. 创建记忆（project_path="/A"）
      2. 调用should_recall(project_path=Some("/A"))
      3. 验证搜索结果包含/A的记忆
    Expected Result: project_path正确传递到搜索
    Evidence: .sisyphus/evidence/task-4-recall-path.txt

  Scenario: should_recall全局回退不过滤其他project
    Tool: Bash (cargo test)
    Steps:
      1. 创建记忆（/A, /B, None）
      2. 调用should_recall(project_path=Some("/A"))
      3. 验证全局回退不返回/B的记忆
    Expected Result: 全局回退只返回NULL或/A的记忆
    Evidence: .sisyphus/evidence/task-4-recall-global.txt
  ```

  **Commit**: YES
  - Message: `feat(api): wire project_path to session_recall two-phase search`
  - Files: api/handlers/session_recalls.rs

- [x] 5. API handlers传递project_path（所有handler wiring）

  **What to do**:
  - 在 `api/handlers/memory.rs` search_memories handler 传递 project_path_filter 到 SearchRequest
  - 在 `api/handlers/memory.rs` session_ingest handler (L1569) 传递 project_path 到 IngestRequest
  - 在 `api/handlers/memory.rs` create_memory (direct POST, L240) 传递 project_path
  - 在 SearchQuery DTO 新增 `pub project_path: Option<String>` 字段
  - 在 SessionIngestBody DTO 新增 `pub project_path: Option<String>` 字段（如无则复用IngestRequest字段）
  - 添加 TDD 测试：验证 search endpoint 传递 project_path_filter
  - 添加 TDD 测试：验证 session_ingest endpoint 传递 project_path

  **Must NOT do**:
  - 不修改 search 的 scope/tags 逻辑
  - 不修改 create_memory 的其他字段

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES（与Task 4平行）
  - **Parallel Group**: Wave 2 (after Task 2)
  - **Blocks**: Task 7
  - **Blocked By**: Task 2

  **References**:
  - `omem-server/src/api/handlers/memory.rs:SearchQuery` — search endpoint DTO
  - `omem-server/src/api/handlers/memory.rs:session_ingest` — session ingest handler (L1569)
  - `omem-server/src/api/handlers/memory.rs:create_memory` — direct POST handler (L240)

  **Acceptance Criteria**:
  - [ ] SearchQuery 有 `project_path: Option<String>` 字段
  - [ ] search_memories 传递 project_path_filter 到 SearchRequest
  - [ ] session_ingest 传递 project_path 到 IngestRequest
  - [ ] create_memory (direct POST) 传递 project_path
  - [ ] TDD: 2个API传递测试通过

  **QA Scenarios**:
  ```
  Scenario: search endpoint传递project_path_filter
    Tool: Bash (cargo test)
    Steps:
      1. GET /v1/memories/search?q=test&project_path=/foo
      2. 验证SearchRequest包含project_path_filter=Some("/foo")
    Expected Result: project_path正确传递到搜索
    Evidence: .sisyphus/evidence/task-5-search-filter.txt

  Scenario: session_ingest传递project_path
    Tool: Bash (cargo test)
    Steps:
      1. POST /v1/memories/session-ingest { project_path: "/bar", messages: [...] }
      2. 验证IngestRequest包含project_path=Some("/bar")
    Expected Result: project_path正确传递到ingest
    Evidence: .sisyphus/evidence/task-5-session-ingest.txt
  ```

  **Commit**: YES
  - Message: `feat(api): wire project_path to all handlers`
  - Files: api/handlers/memory.rs

- [x] 6. sanitize_project_path() 安全函数

  **What to do**:
  - 在 `domain/memory.rs` 或新建 `api/utils.rs` 添加 `sanitize_project_path(path: &str) -> Result<String, OmemError>` 函数
  - 验证路径格式：不含 `../`、不含SQL特殊字符、长度上限（512字符）
  - 在所有接收 project_path 的入口（IngestRequest、SearchQuery、ShouldRecallRequest）调用 sanitize
  - 确保 `escape_sql()` 额外处理 project_path 中的特殊字符
  - 添加 TDD 测试：验证各种恶意路径被拒绝
  - 添加 TDD 测试：验证合法路径通过

  **Must NOT do**:
  - 不做路径规范化（不resolve symlinks、不normalize大小写）
  - 不修改现有 escape_sql() 函数本身

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES（独立于其他task）
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 7
  - **Blocked By**: None

  **References**:
  - `omem-server/src/store/lancedb.rs:escape_sql` — 当前只转义 `'` → `''`
  - `omem-server/src/domain/error.rs` — OmemError::Validation 用于非法路径

  **Acceptance Criteria**:
  - [ ] sanitize_project_path() 函数存在并验证路径格式
  - [ ] 拒绝含 `../` 的路径
  - [ ] 拒绝超长路径（>512字符）
  - [ ] 拒绝含SQL注入字符的路径
  - [ ] 合法路径通过
  - [ ] TDD: 4个安全测试通过

  **QA Scenarios**:
  ```
  Scenario: 合法路径通过sanitize
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server test_sanitize_valid_path
    Expected Result: "/mnt/d/dev/project" → Ok
    Evidence: .sisyphus/evidence/task-6-valid-path.txt

  Scenario: 恶意路径被拒绝
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server test_sanitize_reject_malicious
    Expected Result: "../etc/passwd" → Err, "'; DROP --" → Err, 超512字符 → Err
    Evidence: .sisyphus/evidence/task-6-malicious-reject.txt
  ```

  **Commit**: YES
  - Message: `feat(security): add sanitize_project_path() for SQL injection protection`
  - Files: domain/memory.rs (or new file)

- [x] 7. 集成测试 + 旧数据迁移 + 测试API Key

  **What to do**:
  - 端到端测试：ingest → search → recall 全链路验证
  - **新建专用测试API Key**，确保测试数据与生产数据隔离
  - 旧数据迁移策略：
    - 添加统计API/CLI：统计 project_path=NULL 的记忆数量（按tenant）
    - 可选：对 project_path=NULL 的记忆标记为 `scope=global`（不自动填充project_path）
    - 记录迁移策略为配置项（后续可手动触发迁移）
  - 全局类别绕过测试：profile/preferences 记忆不受 project_path 过滤影响
  - 验证不传 project_path 的请求行为完全不变（向后兼容）
  - 验证 files.rs 上传产生的记忆 project_path = None
  - 验证 intelligence task 产生的记忆 project_path = None

  **Must NOT do**:
  - 不自动迁移旧数据的project_path（只统计，不修改）
  - 不修改任何生产代码，仅添加测试和统计

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO（需要所有前置task完成）
  - **Parallel Group**: Wave 3 (after Tasks 3, 4, 5, 6)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 3, 4, 5, 6

  **References**:
  - `omem-server/src/api/mod.rs` — setup_app() 测试工厂 + tower oneshot 模式
  - `omem-server/src/ingest/pipeline.rs` — ingest 全链路
  - `omem-server/src/retrieve/pipeline.rs` — search 全链路
  - `omem-server/src/api/handlers/session_recalls.rs` — should_recall 全链路
  - `omem-server/src/api/handlers/tenant.rs` — create_tenant (创建测试API Key)

  **Acceptance Criteria**:
  - [ ] 端到端测试通过：ingest(project_path) → search(project_path_filter) → 隔离正确
  - [ ] 使用专用测试API Key进行所有测试
  - [ ] 旧数据统计功能：能统计 project_path=NULL 的记忆数量
  - [ ] 向后兼容：不传 project_path 时行为不变
  - [ ] 全局类别绕过：profile/preferences 不受 project_path 过滤

  **QA Scenarios**:
  ```
  Scenario: 端到端ingest-search隔离
    Tool: Bash (cargo test)
    Steps:
      1. 创建专用测试tenant
      2. Ingest记忆到project /A
      3. Ingest记忆到project /B
      4. Search with project_path_filter=Some("/A")
      5. 验证只返回 /A 和 global 记忆
    Expected Result: 不返回 /B 记忆
    Evidence: .sisyphus/evidence/task-7-e2e-isolation.txt

  Scenario: 旧数据统计
    Tool: Bash (cargo test)
    Steps:
      1. 创建测试tenant
      2. 创建记忆（project_path=None）
      3. 调用统计API
      4. 验证返回正确的NULL计数
    Expected Result: 统计准确
    Evidence: .sisyphus/evidence/task-7-null-stats.txt

  Scenario: 不传project_path时行为不变
    Tool: Bash (cargo test)
    Steps:
      1. 使用测试API Key
      2. 创建多条记忆（不同project_path）
      3. Search with project_path_filter=None
      4. 验证返回所有记忆（和旧版本行为一致）
    Expected Result: 向后兼容
    Evidence: .sisyphus/evidence/task-7-no-filter-compat.txt
  ```

  **Commit**: YES
  - Message: `test: add integration tests for project_path isolation + data migration stats`
  - Files: various test files

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE.

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy` + `cargo test`. Review all changed files for: `as any`/`unwrap()` in production, empty catches, console.log in prod, commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [x] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task. Test cross-task integration. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | VERDICT`

- [x] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — everything in spec was built, nothing beyond spec was built. Check "Must NOT do" compliance. Detect cross-task contamination.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | VERDICT`

---

## Commit Strategy

- **C1**: `feat(domain+store): add project_path field to Memory model and LanceDB schema` — domain/memory.rs, store/lancedb.rs
- **C2**: `feat(ingest): thread project_path through ingest pipeline` — ingest/types.rs, ingest/pipeline.rs, ingest/reconciler.rs
- **C3**: `feat(retrieve): add project_path filtering to search` — retrieve/pipeline.rs, store/lancedb.rs
- **C4**: `feat(api): wire project_path to session_recall two-phase search` — api/handlers/session_recalls.rs
- **C5**: `feat(api): wire project_path to all handlers` — api/handlers/memory.rs
- **C6**: `feat(security): add sanitize_project_path() for SQL injection protection` — domain/memory.rs (or new file)
- **C7**: `test: add integration tests for project_path isolation` — various test files

---

## Success Criteria

### Verification Commands
```bash
cargo check                                # Expected: no errors
cargo test -p omem-server                  # Expected: all pass
cargo test -p omem-server project_path     # Expected: all project_path tests pass
cargo clippy                               # Expected: no warnings
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass (existing + new)
- [ ] Old data (project_path=NULL) works as global (no filter)
- [ ] No scope/tags semantics changed
- [ ] No clustering/lifecycle/profile/sharing touched
