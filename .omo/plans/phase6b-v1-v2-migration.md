# Phase 6b — V1→V2 Profile 全量迁移 + Plugin 适配

## TL;DR

> **Quick Summary**: 砍掉 session_ingest 的 PREFERENCE 路径，废弃 V1 profile 模块（保留 /v1/profile 路由做兼容代理），Plugin 端完成 V2 切换。
> 
> **Deliverables**:
> - Rust 端：prompts.rs 去掉 PREFERENCE + 废弃 profile/preference_slots 模块 + 清理 AppState + /v1/profile 路由代理到 V2
> - Plugin 端（opencode）：tools.ts + client.ts 切 V2（hooks.ts/index.ts 已完成）
> 
> **Estimated Effort**: Medium-Large
> **Parallel Execution**: YES - 2 waves + Wave 3(数据迁移) + Final
> **Critical Path**: T1 → T2 → T3 → T6 → T7(迁移) → F1-F4

---

## Context

### Original Request
Phase 6b V1→V2 Profile 全量迁移 + Plugin 端 V2 Profile 适配。

### Interview Summary
**Key Discussions**:
- 砍掉 session_ingest 的 PREFERENCE 路径（师尊关键决策）：LLM prompt 只输出 WORK/EMOTIONAL
- Plugin 端直接用 V2 inject 返回的 content，不二次格式化
- /v1/profile 路由保留做兼容代理（师尊决策：openclaw/mcp 仍依赖此端点）
- 仅改 opencode plugin，其他 3 个后续按需

**Research Findings**:
- V2 归纳引擎代码已存在（profile_v2/induction.rs），pipeline.rs:354-364 已集成
- V2 inject API 已实现：GET /v2/profile/inject?project_path=xxx
- V2 inject 输出格式 `· {slot} — {value}`，与 V1 `· {l2_content}` 不同（师尊选择直接用 V2 格式）
- hooks.ts 已完成 V2 切换（getProfile/buildProfileBlock 已移除，getInjection 已在用）
- index.ts 只有 141 行，无 profile 预热逻辑
- openclaw（context-engine.ts:82）和 mcp（tools.ts:265,699）仍调用 /v1/profile → 需保留路由

### Metis Review
**Identified Gaps** (addressed):
- H2 格式不一致 → 师尊决策直接用 V2 content
- memory.rs PREFERENCE dedup ~100 行需清理 → 纳入计划
- reconciler.rs preference_slots import → 纳入计划
- config.rs 共享配置项 → 保留给 V2，不重命名
- tools.ts memory_profile 工具 → 用 /v2/profile（列表），注入用 /v2/profile/inject
- AppState.profile_cache 删除 → 需同步更新 35 个测试

### Oracle Phase 1 Verification
**VERDICT: GO** — All 5 checks passed. 2 minor edge-case decisions deferred to plan-time (tools.ts endpoint, project_path undefined).

---

## Work Objectives

### Core Objective
砍掉 session_ingest 的 PREFERENCE 提取路径，废弃 V1 profile 模块（服务端），/v1/profile 路由代理到 V2，opencode Plugin 端完成 V2 切换。

### Concrete Deliverables
- 修改: `omem-server/src/ingest/prompts.rs` — 去掉 PREFERENCE 输出格式
- 修改: `omem-server/src/ingest/reconciler.rs` — 去掉 preference_slots 引用 + PREFERENCE 分支 + preference_slot_guard 方法
- 修改: `omem-server/src/api/handlers/memory.rs` — 移除 PREFERENCE dedup 逻辑 + memory_type fallback
- 删除: `omem-server/src/ingest/preference_slots.rs` (171行)
- 删除: `omem-server/src/profile/` 整个目录 (service.rs 783行)
- 删除: `omem-server/src/domain/profile.rs` (161行)
- 删除: `omem-server/src/api/handlers/profile.rs` (53行)
- 修改: `omem-server/src/api/router.rs` — /v1/profile 路由代理到 V2
- 修改: `omem-server/src/api/server.rs` — 删除 AppState.profile_cache
- 修改: `omem-server/src/api/mod.rs` — 更新 setup_app()（1 处定义）
- 修改: `omem-server/src/api/mod.rs` — 更新 35 个测试的 setup_app()
- 修改: `omem-server/src/main.rs` — 删除 V1 profile 初始化
- 修改: `omem-server/src/lib.rs` — 删除 `pub mod profile;`
- 修改: `omem-server/src/ingest/preference_slots.rs` → 删除
- 修改: `omem-server/src/domain/mod.rs` — 删除 profile mod
- 修改: `omem-server/src/api/handlers/mod.rs` — 删除 profile mod
- 修改: `omem-server/src/profile/mod.rs` → 删除整个 profile/ 目录
- 修改: `plugins/opencode/src/client.ts` — getProfile() 改路径（已有 getInjection）
- 修改: `plugins/opencode/src/tools.ts` — memory_profile 切 V2

### Definition of Done
- [ ] `cargo build -p omem-server` 无错误
- [ ] `cargo test -p omem-server` 所有测试通过（含更新后的 setup_app 测试）
- [ ] `cargo clippy` 无新 warning
- [ ] `cd plugins/opencode && npm run build` 无错误
- [ ] prompts.rs 中不包含 "PREFERENCE" 输出格式（除注释）
- [ ] Plugin 端 tools.ts 不包含 /v1/profile 调用
- [ ] /v1/profile 路由保留但代理到 V2（openclaw/mcp 兼容）
- [ ] memory.rs 不接受 LLM 输出的 PREFERENCE memory_type（fallback 到 WORK/EMOTIONAL）
- [ ] V2 profile 包含从 PREFERENCE 存量归纳的偏好（slot+value 对）
- [ ] V2 inject 输出包含归纳偏好的 content

### Must Have
- prompts.rs LLM 输出只保留 WORK/EMOTIONAL 两种 memory_type
- V1 profile 模块完全删除（profile/, domain/profile.rs, preference_slots.rs）+ lib.rs mod 声明
- AppState.profile_cache 字段删除
- /v1/profile 路由保留，代理到 V2（openclaw/mcp 兼容）
- handlers/profile.rs 重写为 V2 代理（调用 profile_v2 inject 并转格式）
- reconciler.rs 完整清理 preference_slots import + preference_slot_guard 方法 + PREFERENCE 分支
- memory.rs:1591 移除 PREFERENCE memory_type 接受（LLM 输出的非法 PREFERENCE fallback 到 WORK/EMOTIONAL）
- opencode tools.ts memory_profile 切到 /v2/profile
- opencode client.ts getProfile() 改路径
- PREFERENCE 存量数据通过归纳引擎迁移到 V2 Profile SQLite slots
### Must NOT Have (Guardrails)
- ❌ 不碰 profile_v2/ 目录任何文件
- ❌ 不碰 openclaw/mcp/claude-code 三个插件（保留 /v1/profile 路由兼容）
- ❌ 不重构 hooks.ts 函数结构/命名/代码组织
- ❌ 不处理 PREFERENCE 存量数据迁移（LanceDB 中已有的 PREFERENCE Memory 保留不动）
- ❌ 不删除 /v1/profile 路由（改为代理到 V2）
- ❌ 不在非测试代码中使用 unwrap()/expect()
- ❌ ESM import 必须带 .js 扩展名（Plugin TypeScript）
- ❌ 不创建需要"用户手动测试"的验收标准

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test 373 tests + npm run build)
- **Automated tests**: TDD — 先改测试/写测试，再改代码
- **Framework**: cargo test (Rust) + tsc (TypeScript)

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Rust module**: Use Bash (cargo test) - Run inline tests, check compilation
- **TypeScript Plugin**: Use Bash (npm run build / tsc --noEmit) - Verify compilation
- **Integration**: Use Bash (cargo check + cargo test) - Verify no regressions

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Rust V1 cleanup — T1→T2→T3 串行 + T4 并行):
├── Task 1: prompts.rs 砍 PREFERENCE + reconciler 完整清理 [unspecified-high]
├── Task 2: 删除 V1 profile 模块 (profile/ + domain/profile.rs + preference_slots.rs + lib.rs) [quick]
│   (depends: T1 — reconciler 先清 preference_slots import)
├── Task 3: AppState 清理 + /v1/profile 代理 + memory.rs PREFERENCE fallback [deep]
│   (depends: T2 — 先删文件再清编译错误)
└── Task 4: config.rs V1 配置项清理 [quick] ← 可与 T1 并行

Wave 2 (After Wave 1 — Plugin V2 adaptation, 仅 opencode):
├── Task 5: client.ts getProfile() 改路径 [quick]
└── Task 6: tools.ts memory_profile 切 V2 [quick]

Wave 3 (After deployment — PREFERENCE 数据迁移):
└── Task 7: 触发归纳引擎迁移 PREFERENCE 存量到 V2 Profile SQLite slots [quick]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: T1 → T2 → T3 → T5 → T7(部署后) → F1-F4
Parallel Speedup: T4 可与 T1 并行；Wave 2 两个任务可并行
Max Concurrent: 2 (T1 + T4)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| T1   | - | T2, T5 | 1 |
| T2   | T1 | T3 | 1 |
| T3   | T2 | T5, T6 | 1 |
| T4   | - | - | 1 |
| T5   | T3 | T6 | 2 |
| T6   | T5 | - | 2 |
| T7   | T1-T6 deployed | F1-F4 | 3 |
| F1-F4 | ALL | - | FINAL |

### Agent Dispatch Summary

- **Wave 1**: 4 tasks — T1→`unspecified-high`, T2→`quick`(depends T1), T3→`deep`(depends T2), T4→`quick`(parallel with T1)
- **Wave 2**: 2 tasks — T5→`quick`, T6→`quick`（可并行）
- **Wave 3**: 1 task — T7→`quick`（部署后数据迁移）
- **FINAL**: 4 tasks — F1→`oracle`, F2→`unspecified-high`, F3→`unspecified-high`, F4→`deep`

---

## TODOs

- [ ] 1. prompts.rs 砍 PREFERENCE + reconciler 清理

  **What to do**:
  - 修改 `omem-server/src/ingest/prompts.rs`:
    - 在 `BASE_SYSTEM_PROMPT` 中移除所有 PREFERENCE 相关的提取指令和示例
    - 在 JSON schema 的 `memory_type` 枚举中只保留 `"EMOTIONAL"|"WORK"`
    - 移除 PREFERENCE 类型的字段长度说明（`PREFERENCE ≤500 chars`）
    - 移除所有 PREFERENCE 的分类处理规则（如 Rule 4 中的 PREFERENCE 分支）
    - 移除 PREFERENCE 输出示例（中文/英文各一个）
    - 确保 ALLOWED_TAGS 和其他 WORK/EMOTIONAL 相关规则不受影响
  - 修改 `omem-server/src/ingest/reconciler.rs`:
    - 移除 `use crate::ingest::preference_slots;` (L13)
    - 移除 L127 `self.preference_slot_guard(fact, ...)` 调用
    - 移除 L330-358 `preference_slot_guard()` 方法定义（29 行）
    - 移除 L500 `"preferences" | "project"` 中的 `"preferences"` 分支（保留 `"project"`）
    - 注意：reconciler 测试中的 `"preferences"` category 引用可保留（category 与 memory_type 是不同维度）
  - 先写测试验证：
    - 验证 prompts.rs 的 `build_system_prompt()` 输出不包含 "PREFERENCE" 字符串
    - 验证 reconciler 编译通过且不影响 WORK/EMOTIONAL 的 reconcile 逻辑

  **Must NOT do**:
  - 不修改 ALLOWED_TAGS 列表
  - 不修改 WORK/EMOTIONAL 的提取/对账逻辑
  - 不碰 noise.rs、admission.rs 等其他 ingest 文件

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: prompts.rs 是 LLM prompt 模板（800+ 行），需要精准移除 PREFERENCE 相关内容不破坏其他逻辑；reconciler.rs 有复杂的对账引擎
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2, T3, T4)
  - **Blocks**: T5, T6
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `omem-server/src/ingest/prompts.rs:930-1019` — PREFERENCE 提取指令、JSON schema、PREFERENCE_EXTRACT_SYSTEM_PROMPT 常量
  - `omem-server/src/ingest/prompts.rs:968-990` — Rule 4 PREFERENCE 分支处理规则
  - `omem-server/src/ingest/reconciler.rs:13` — `use crate::ingest::preference_slots;` import
  - `omem-server/src/ingest/reconciler.rs:127` — `preference_slot_guard()` 调用点
  - `omem-server/src/ingest/reconciler.rs:330-358` — `preference_slot_guard()` 方法定义（29 行）
  - `omem-server/src/ingest/reconciler.rs:500` — `"preferences" | "project"` 分支
  - `omem-server/src/ingest/preference_slots.rs` — 整个文件 171 行（被 T2 删除）

  **API/Type References**:
  - `omem-server/src/ingest/types.rs:ExtractedFact` — memory_type 字段定义
  - `omem-server/src/ingest/reconciler.rs` — ReconcileDecision 处理分支

  **WHY Each Reference Matters**:
  - `prompts.rs:949-1008`: PREFERENCE 的输出格式定义和示例都在这里，是砍掉 PREFERENCE 的主要目标
  - `reconciler.rs:13`: preference_slots 的唯一消费者，移除后 preference_slots.rs 可安全删除
  - `types.rs`: 需确认 ExtractedFact 的 memory_type 是字符串还是枚举（字符串，无编译约束）

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: prompts.rs 不包含 PREFERENCE 输出格式
    Tool: Bash (grep)
    Preconditions: prompts.rs 已修改
    Steps:
      1. grep -c "PREFERENCE" omem-server/src/ingest/prompts.rs
      2. 确认结果为 0 或仅注释中出现
    Expected Result: grep 返回 0 或只有注释行
    Failure Indicators: 非 PREFERENCE prompt 代码中出现 PREFERENCE
    Evidence: .omo/evidence/task-1-no-preference.txt

  Scenario: memory_type JSON schema 只包含 WORK/EMOTIONAL
    Tool: Bash (grep)
    Preconditions: prompts.rs 已修改
    Steps:
      1. grep "memory_type" omem-server/src/ingest/prompts.rs
      2. 确认只有 "EMOTIONAL"|"WORK"
    Expected Result: 不包含 "PREFERENCE" 在 memory_type 枚举中
    Evidence: .omo/evidence/task-1-schema-check.txt

  Scenario: reconciler 编译通过
    Tool: Bash (cargo check)
    Preconditions: reconciler.rs 已修改，移除 preference_slots import
    Steps:
      1. cargo check -p omem-server
    Expected Result: "Finished" 无 error
    Failure Indicators: "error[E....]" 编译错误
    Evidence: .omo/evidence/task-1-reconciler-check.txt

  Scenario: 现有测试通过
    Tool: Bash (cargo test)
    Preconditions: prompts.rs + reconciler.rs 已修改
    Steps:
      1. cargo test -p omem-server
    Expected Result: 所有测试通过，0 failure
    Evidence: .omo/evidence/task-1-tests.txt
  ```

  **Commit**: YES (groups with T2, T3, T4)
  - Message: `refactor(profile): remove V1 profile system and PREFERENCE ingestion path`
  - Files: `omem-server/src/ingest/prompts.rs`, `omem-server/src/ingest/reconciler.rs`
  - Pre-commit: `cargo test -p omem-server`

- [ ] 2. 删除 V1 profile 模块 + lib.rs 清理

  **What to do**:
  - 删除 `omem-server/src/ingest/preference_slots.rs` (171行)
  - 删除 `omem-server/src/profile/service.rs` (783行)
  - 删除 `omem-server/src/profile/mod.rs`
  - 删除 `omem-server/src/domain/profile.rs` (161行)
  - 修改 `omem-server/src/ingest/mod.rs`: 移除 `pub mod preference_slots;`
  - 修改 `omem-server/src/domain/mod.rs`: 移除 `pub mod profile;`（注意不要误删 profile_v2）
  - 修改 `omem-server/src/lib.rs:11`: 移除 `pub mod profile;`
  - **注意**：`api/handlers/profile.rs` 不删除，改为 V2 代理（T3 处理）

  **Must NOT do**:
  - 不碰 profile_v2/ 目录
  - 不修改其他 handler 文件中对 profile 类型的间接引用（由 T3 处理）
  - 不删除 config.rs 中的配置项（由 T4 处理）

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 纯文件删除 + mod.rs 清理，无业务逻辑
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T3, T4)
  - **Blocks**: T3 (需要先删文件，T3 才能清理编译错误)
  - **Blocked By**: T1 (reconciler.rs 要先移除 preference_slots import)

  **References**:

  **Pattern References**:
  - `omem-server/src/ingest/preference_slots.rs` — 整个文件，被 reconciler.rs 引用（T1 已移除引用）
  - `omem-server/src/profile/service.rs` — ProfileService 主实现
  - `omem-server/src/profile/mod.rs` — profile 模块入口
  - `omem-server/src/domain/profile.rs` — UserProfile 类型定义
  - `omem-server/src/lib.rs:11` — `pub mod profile;`（⚠️ 计划修订前遗漏）

  **WHY Each Reference Matters**:
  - 需要确认每个文件确实可以安全删除（无其他消费者）
  - preference_slots.rs: T1 已从 reconciler.rs 移除引用
  - profile/: 只有 handlers/profile.rs 和 AppState 使用，T3 会清理

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 文件已删除
    Tool: Bash (ls)
    Preconditions: 删除操作完成
    Steps:
      1. ls omem-server/src/ingest/preference_slots.rs → 不存在
      2. ls omem-server/src/profile/ → 目录不存在
      3. ls omem-server/src/domain/profile.rs → 不存在
    Expected Result: 所有文件/目录不存在
    Evidence: .omo/evidence/task-2-files-deleted.txt

  Scenario: mod.rs + lib.rs 清理
    Tool: Bash (grep)
    Preconditions: mod.rs + lib.rs 已修改
    Steps:
      1. grep "preference_slots" omem-server/src/ingest/mod.rs → 无结果
      2. grep "pub mod profile;" omem-server/src/domain/mod.rs → 无结果（注意不要误删 profile_v2）
      3. grep "pub mod profile;" omem-server/src/lib.rs → 无结果（⚠️ 区分 profile 和 profile_v2）
    Expected Result: 所有 V1 profile 模块引用已移除
    Evidence: .omo/evidence/task-2-mod-cleanup.txt
  ```

  **Commit**: YES (groups with T1, T3, T4)
  - Message: `refactor(profile): remove V1 profile system and PREFERENCE ingestion path`
  - Files: 删除的文件列表 + mod.rs 修改
  - Pre-commit: `cargo check -p omem-server` (此时可能编译失败，T3 修复)

- [ ] 3. AppState 清理 + /v1/profile 代理 + memory.rs PREFERENCE fallback

  **What to do**:
  - 修改 `omem-server/src/api/server.rs`:
    - 移除 `pub profile_cache: Arc<DashMap<String, CachedProfile>>` 字段
    - 移除 `use crate::profile::service::CachedProfile;` 等 import
    - 如果有 `use dashmap::DashMap;` 只被 profile_cache 使用，也移除
  - 修改 `omem-server/src/main.rs`:
    - 移除 V1 ProfileService 初始化代码
    - 移除 profile_cache 创建代码（L175 附近）
    - 移除 `use omem_server::profile::service::ProfileService;` import
  - 修改 `omem-server/src/api/handlers/stats.rs`:
    - 移除 stats handler 中的 profile_cache 引用（L856 附近）
  - 修改 `omem-server/src/api/mod.rs`:
    - 移除 `setup_app()` 定义中的 `.profile_cache: Arc::new(DashMap::new())` 字段赋值（**只有 1 处**，所有测试共享此函数）
  - 修改 `omem-server/src/api/router.rs`:
    - **保留** `/v1/profile` 路由注册（openclaw/mcp 兼容）
    - handler 改为调用 V2 inject 并转格式
  - 修改 `omem-server/src/api/handlers/profile.rs`:
    - 重写为 V2 代理：调用 profile_v2 inject，将 V2 格式转为 V1 兼容格式
    - 保持 V1 API 契约（openclaw/mcp 期望的 JSON 结构）
    - 这样 openclaw/mcp 不需要修改
  - 修改 `omem-server/src/api/handlers/memory.rs`:
    - 移除 PREFERENCE dedup 逻辑（L1909-2022 约 110 行）
    - L1591 移除 `"PREFERENCE"` 分支：`"EMOTIONAL" | "WORK" | "PREFERENCE" =>` → `"EMOTIONAL" | "WORK" =>`
    - 让 LLM 输出的非法 PREFERENCE fallback 到 L1594 的 scope-based 赋值（WORK/EMOTIONAL）
  - 用 `lsp_find_references` 确认 `CachedProfile`、`ProfileService`、`profile_cache` 的所有引用点

  **Must NOT do**:
  - 不修改 V2 profile 相关的 AppState 字段（profile_v2_service 等）
  - 不修改 LanceStore 或 StoreManager 的任何代码
  - 不碰 profile_v2/ 目录

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: AppState 核心状态 + /v1/profile 代理重写 + memory.rs PREFERENCE fallback + PREFERENCE dedup 移除，涉及 7 个文件
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2, T4)
  - **Blocks**: T5, T6, T7, T8
  - **Blocked By**: T2 (需要先删除文件才能清理编译错误)

  **References**:

  **Pattern References**:
  - `omem-server/src/api/server.rs:AppState` — 15 字段结构体，profile_cache 是其中之一
  - `omem-server/src/main.rs:175` — profile_cache 构造
  - `omem-server/src/api/handlers/stats.rs:856` — stats handler 中 profile_cache 构造
  - `omem-server/src/api/mod.rs:111` — setup_app() 定义（**只有 1 处**，非 35 处）
  - `omem-server/src/api/handlers/memory.rs:1591` — PREFERENCE memory_type 接受分支
  - `omem-server/src/api/handlers/memory.rs:1909-2022` — PREFERENCE dedup 逻辑（110 行）
  - `omem-server/src/api/handlers/profile.rs` — 需重写为 V2 代理（保留文件）

  **API/Type References**:
  - `omem-server/src/profile/service.rs:CachedProfile` — profile_cache 的值类型（随 profile/ 一起删除）

  **WHY Each Reference Matters**:
  - `server.rs:AppState`: 删除 profile_cache 字段后所有引用 AppState 的代码都需要适配
  - `mod.rs:111`: 只有 1 处 setup_app() 定义需修改（所有测试共享此函数）
  - `memory.rs:1591`: 移除 PREFERENCE 让 LLM 输出的非法值 fallback 到 WORK/EMOTIONAL
  - `memory.rs:1909-2022`: PREFERENCE dedup 是砍 PREFERENCE 的完整闭环
  - `profile.rs`: 重写为 V2 代理保持 openclaw/mcp 兼容

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: AppState 编译通过
    Tool: Bash (cargo check)
    Preconditions: server.rs + mod.rs + main.rs 已修改
    Steps:
      1. cargo check -p omem-server
    Expected Result: "Finished" 无 error
    Failure Indicators: "error[E....]" 缺少字段或类型不匹配
    Evidence: .omo/evidence/task-3-compile-check.txt

  Scenario: 所有测试通过
    Tool: Bash (cargo test)
    Preconditions: 所有 mod.rs 测试已更新
    Steps:
      1. cargo test -p omem-server
    Expected Result: 所有测试通过，0 failure
    Failure Indicators: 任何测试 failure
    Evidence: .omo/evidence/task-3-tests.txt

  Scenario: profile_cache 引用完全清除
    Tool: Bash (grep)
    Preconditions: 所有文件已修改
    Steps:
      1. grep -r "profile_cache" omem-server/src/ --include="*.rs"
    Expected Result: 无结果（除可能的注释）
    Evidence: .omo/evidence/task-3-no-profile-cache.txt

  Scenario: /v1/profile 路由保留但代理到 V2
    Tool: Bash (grep)
    Preconditions: router.rs + profile handler 已修改
    Steps:
      1. grep "/v1/profile" omem-server/src/api/router.rs → 确认路由存在
      2. grep "profile_v2\|inject" omem-server/src/api/handlers/profile.rs → 确认代理到 V2
    Expected Result: 路由保留，handler 代理到 V2
    Evidence: .omo/evidence/task-3-v1-proxy.txt

  Scenario: memory.rs 不再接受 PREFERENCE memory_type
    Tool: Bash (grep)
    Preconditions: memory.rs 已修改
    Steps:
      1. grep '"PREFERENCE"' omem-server/src/api/handlers/memory.rs
    Expected Result: 无结果（PREFERENCE fallback 到 WORK/EMOTIONAL）
    Evidence: .omo/evidence/task-3-no-preference-type.txt

  Scenario: PREFERENCE dedup 逻辑已移除
    Tool: Bash (grep)
    Preconditions: memory.rs 已修改
    Steps:
      1. grep -c "preference" omem-server/src/api/handlers/memory.rs → 确认 dedup 块已移除
    Expected Result: PREFERENCE dedup 相关代码不再存在
    Evidence: .omo/evidence/task-3-no-dedup.txt
  ```

  **Commit**: YES (groups with T1, T2, T4)
  - Message: `refactor(profile): remove V1 profile system and PREFERENCE ingestion path`
  - Files: server.rs, router.rs, main.rs, mod.rs, stats.rs, memory.rs, profile.rs(handlers)
  - Pre-commit: `cargo test -p omem-server`

- [ ] 4. config.rs V1 配置项清理

  **What to do**:
  - 修改 `omem-server/src/config.rs`:
    - 检查是否有 V1 profile 专用的配置项（如 profile_cache_ttl_secs 仅 V1 使用）
    - 保留 V1/V2 共享的配置项（profile_cache_ttl_secs 被 V2 InjectionBuilder 也使用）
    - 移除只有 V1 ProfileService 使用的配置项（如果有）
    - **关键**：不能删 V2 也用的配置项，`profile_cache_ttl_secs` 必须保留
  - 验证编译通过

  **Must NOT do**:
  - 不删除 V2 profile 也在使用的配置项
  - 不修改 OMEM_PROFILE_* 前缀的 V2 配置项

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 配置项检查，工作量小
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2, T3)
  - **Blocks**: None
  - **Blocked By**: None

  **References**:
  - `omem-server/src/config.rs` — OmemConfig struct，查看所有 profile 相关字段
  - `omem-server/src/profile_v2/service.rs:32` — V2 也使用 `profile_cache_ttl_secs`

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 配置项编译通过
    Tool: Bash (cargo check)
    Preconditions: config.rs 已修改
    Steps:
      1. cargo check -p omem-server
    Expected Result: "Finished" 无 error
    Evidence: .omo/evidence/task-4-compile-check.txt

  Scenario: V2 配置项保留
    Tool: Bash (grep)
    Preconditions: config.rs 已修改
    Steps:
      1. grep "profile_cache_ttl" omem-server/src/config.rs → 确认保留
    Expected Result: V2 使用的配置项仍然存在
    Evidence: .omo/evidence/task-4-v2-config.txt
  ```

  **Commit**: YES (groups with T1, T2, T3)
  - Message: `refactor(profile): remove V1 profile system and PREFERENCE ingestion path`
  - Files: config.rs
  - Pre-commit: `cargo check -p omem-server`

- [ ] 5. client.ts getProfile() 改路径

  **What to do**:
  - 修改 `plugins/opencode/src/client.ts`:
    - `getProfile()` 方法（L271）改为调用 `/v2/profile`（返回偏好列表）而非 `/v1/profile`
    - `getInjection()` 方法已存在（L282 附近），无需新增
    - 确保 ESM import 带 `.js` 扩展名

  **Must NOT do**:
  - 不修改其他 plugin（openclaw/mcp/claude-code）
  - 不修改超时配置

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 改一个 API 路径字符串
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T6)
  - **Blocks**: T6
  - **Blocked By**: T3

  **References**:
  - `plugins/opencode/src/client.ts:271` — 现有 `getProfile()` 方法
  - `plugins/opencode/src/client.ts:149-155` — get() 方法和超时处理

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: TypeScript 编译通过
    Tool: Bash (tsc)
    Preconditions: client.ts 已修改
    Steps:
      1. cd plugins/opencode && npx tsc --noEmit
    Expected Result: 零 error
    Evidence: .omo/evidence/task-5-tsc.txt

  Scenario: getInjection 方法存在
    Tool: Bash (grep)
    Preconditions: client.ts 已修改
    Steps:
      1. grep "getInjection" plugins/opencode/src/client.ts
    Expected Result: getInjection 方法定义存在
    Evidence: .omo/evidence/task-5-method.txt
  ```

  **Commit**: YES (groups with T6, T7, T8)
  - Message: `feat(plugin): switch opencode plugin to V2 profile inject API`
  - Files: `plugins/opencode/src/client.ts`
  - Pre-commit: `cd plugins/opencode && npx tsc --noEmit`

- [ ] 6. tools.ts memory_profile 切 V2

  **What to do**:
  - 修改 `plugins/opencode/src/tools.ts`:
    - 将 `memory_profile` 工具中的 `client.getProfile()` 改为调用 `/v2/profile`（返回偏好列表）
    - **玄机建议**：`memory_profile` 工具应该展示偏好列表（用 /v2/profile），而非注入格式（/v2/profile/inject）
    - 更新返回值格式适配

  **Must NOT do**:
  - 不修改工具的触发条件或名称
  - 不添加新的 memory 工具

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单工具函数修改
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T5)
  - **Blocks**: None
  - **Blocked By**: T5

  **References**:
  - `plugins/opencode/src/tools.ts:166-175` — memory_profile 工具实现

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: TypeScript 编译通过
    Tool: Bash (tsc)
    Steps:
      1. cd plugins/opencode && npx tsc --noEmit
    Expected Result: 零 error
    Evidence: .omo/evidence/task-6-tsc.txt

  Scenario: tools.ts 不包含 /v1/profile
    Tool: Bash (grep)
    Steps:
      1. grep "/v1/profile" plugins/opencode/src/tools.ts
    Expected Result: 无结果
    Evidence: .omo/evidence/task-6-no-v1.txt
  ```

  **Commit**: YES (groups with T5)
  - Message: `feat(plugin): switch opencode plugin to V2 profile API`
  - Files: `plugins/opencode/src/tools.ts`
  - Pre-commit: `cd plugins/opencode && npx tsc --noEmit`

---

## Wave 3: PREFERENCE 存量数据迁移 + 部署

### Phase 6a 模型配置参考（部署时使用）

Phase 6a 独立画像系统使用独立的 Profile LLM，配置如下：

| 环境变量 | 默认值 | 说明 |
|---------|--------|------|
| `OMEM_PROFILE_LLM_PROVIDER` | `openai` | 归纳引擎 LLM provider |
| `OMEM_PROFILE_LLM_API_KEY` | (empty) | 归纳引擎 LLM API Key |
| `OMEM_PROFILE_LLM_MODEL` | `deepseek-v4-flash` | 归纳引擎模型 |
| `OMEM_PROFILE_LLM_BASE_URL` | `https://opencode.ai/zen/v1` | 归纳引擎 API Base URL |

> **注意**：如果 `OMEM_PROFILE_LLM_API_KEY` 为空，归纳引擎返回 noop（不触发归纳）。部署时必须配置此值。

其他 Phase 6a 相关配置项（已由 6a 实现自动包含）：
- `OMEM_PROFILE_ENABLED` (default: `true`)
- `OMEM_PROFILE_CACHE_TTL_SECS` (default: `1800`) — V2 inject 缓存 TTL
- `OMEM_PROFILE_INDUCTION_COOLDOWN_SECS` (default: `3600`) — 归纳冷却期
- `OMEM_PROFILE_DORMANT_DAYS` (default: `90`) — 偏好休眠天数

- [ ] 7. PREFERENCE 存量数据迁移到 V2 Profile

  **What to do**:
  - **前置条件**：Wave 1 + Wave 2 全部完成并部署到生产服务器
  - **步骤 1**：查询 LanceDB 中所有 `memory_type = "PREFERENCE"` 的记忆，收集 content
    ```bash
    curl -s http://localhost:8080/v1/memories?limit=500 \
      -H "X-API-Key: $API_KEY" | \
      jq '[.memories[] | select(.memory_type == "PREFERENCE") | .content]'
    ```
  - **步骤 2**：调用 V2 归纳引擎 API 触发归纳
    ```bash
    curl -s -X POST http://localhost:8080/v2/profile/induction/trigger \
      -H "X-API-Key: $API_KEY" \
      -H "Content-Type: application/json" \
      -d '{"candidate_texts": [...PREFERENCE content 数组...]}'
    ```
  - **步骤 3**：验证归纳结果
    ```bash
    curl -s http://localhost:8080/v2/profile \
      -H "X-API-Key: $API_KEY" | jq '.preferences | length'
    # 预期：有新归纳的偏好（slot+value 对）
    ```
  - **步骤 4**：验证 V2 inject 输出
    ```bash
    curl -s "http://localhost:8080/v2/profile/inject?project_path=xxx" \
      -H "X-API-Key: $API_KEY"
    # 预期：返回包含归纳偏好的 content
    ```
  - **步骤 5**（可选）：迁移完成后，将 PREFERENCE 存量记忆的 memory_type 批量改为 WORK 或 EMOTIONAL
    - private scope → EMOTIONAL
    - 其他 → WORK
    - 或直接删除（如果归纳引擎已成功提取所有偏好）

  **Must NOT do**:
  - 不在代码编译通过前执行迁移
  - 不删除 PREFERENCE 存量记忆直到确认归纳引擎成功
  - 不修改归纳引擎的 LLM prompt 或 cooldown 配置

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 主要是 curl API 调用 + 验证，不涉及代码修改
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 (sequential, depends on deployment)
  - **Blocks**: F1-F4
  - **Blocked By**: ALL (需要先部署 Wave 1+2 到生产)

  **References**:
  - `omem-server/src/profile_v2/induction.rs:49` — `trigger_induction(tenant_id, reason, candidate_texts)`
  - `omem-server/src/profile_v2/store.rs:114` — `upsert_preference()` SQLite 写入
  - `omem-server/src/profile_v2/slots.rs:9-24` — 14 个内置 slot 定义
  - `omem-server/src/profile_v2/types.rs:41` — Preference 结构体（slot, value, confidence, scope）
  - `.omo/plans/phase6a-independent-profile.md:426-429` — Profile LLM 环境变量配置

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 归纳引擎成功触发
    Tool: Bash (curl)
    Preconditions: Wave 1+2 已部署，OMEM_PROFILE_LLM_API_KEY 已配置
    Steps:
      1. curl -X POST /v2/profile/induction/trigger -d '{"candidate_texts": [...]}'
    Expected Result: 返回 { "run_id": "...", "extracted_count": N }，N > 0
    Failure Indicators: extracted_count = 0 或 error
    Evidence: .omo/evidence/task-7-induction-trigger.txt

  Scenario: V2 profile 包含归纳偏好
    Tool: Bash (curl)
    Preconditions: 归纳引擎已成功运行
    Steps:
      1. curl /v2/profile → 检查 preferences 数组
    Expected Result: 至少有 3 条以上偏好（slot+value 对）
    Evidence: .omo/evidence/task-7-profile-check.txt

  Scenario: V2 inject 输出包含偏好内容
    Tool: Bash (curl)
    Preconditions: 偏好已归纳到 SQLite
    Steps:
      1. curl /v2/profile/inject?project_path=xxx
    Expected Result: content 非空字符串，包含 `· slot — value` 格式
    Evidence: .omo/evidence/task-7-inject-check.txt
  ```

  **Commit**: YES
  - Message: `chore: migrate PREFERENCE memories to V2 profile via induction engine`
  - Files: 无代码修改（纯数据迁移，可选更新 PREFERENCE 存量 memory_type）
  - Pre-commit: 无

---

## Deployment Checklist

> 部署到生产服务器前确认以下配置：

### 必须配置的环境变量

```bash
# Profile LLM（归纳引擎，Phase 6a 引入）
OMEM_PROFILE_LLM_API_KEY=<your-api-key>    # 必填，否则归纳引擎 noop
OMEM_PROFILE_LLM_MODEL=deepseek-v4-flash   # 默认值
OMEM_PROFILE_LLM_BASE_URL=https://opencode.ai/zen/v1  # 默认值

# 已有配置（确认值正确）
OMEM_LLM_API_KEY=<primary-llm-key>
OMEM_LLM_MODEL=gpt-4o-mini
OMEM_EMBED_API_KEY=<embed-key>
OMEM_EMBED_MODEL=text-embedding-3-small
```

### 部署后验证

1. `curl http://localhost:8080/health` → 200 OK
2. `curl -H "X-API-Key: $KEY" http://localhost:8080/v2/profile` → 返回偏好列表（可能为空）
3. `curl -H "X-API-Key: $KEY" "http://localhost:8080/v2/profile/inject"` → 返回 inject content
4. `curl -H "X-API-Key: $KEY" http://localhost:8080/v1/profile` → 返回 V1 兼容格式（代理到 V2）

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .omo/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo check` + `cargo clippy` + `cargo test` + `cd plugins/opencode && npm run build`. Review all changed files for: `unwrap()` in non-test code, empty catches, println in prod, commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction, generic names.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Test cross-task integration (features working together). Test edge cases: empty profile, TTL expiry, concurrent requests. Save to `.omo/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git log/diff). Verify 1:1 — everything in spec was built, nothing beyond spec. Check "Must NOT do" compliance. Detect cross-task contamination: Task N touching Task M's files. Verify profile_v2/ untouched. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `refactor(profile): remove V1 profile system and PREFERENCE ingestion path` - prompts.rs, reconciler.rs, profile/, domain/profile.rs, preference_slots.rs, server.rs, router.rs, main.rs, lib.rs, domain/mod.rs, api/mod.rs, stats.rs, memory.rs, handlers/profile.rs(proxy)
- **Wave 2**: `feat(plugin): switch opencode plugin to V2 profile API` - client.ts, tools.ts
- Pre-commit: `cargo test -p omem-server && cd plugins/opencode && npm run build`

---

## Success Criteria

### Verification Commands
```bash
cargo build -p omem-server                    # Expected: no errors
cargo test -p omem-server                     # Expected: 373+ pass, 0 regressions
cargo clippy                                  # Expected: no new warnings
cd plugins/opencode && npm run build          # Expected: no errors
grep -c "PREFERENCE" omem-server/src/ingest/prompts.rs  # Expected: 0 (or comments only)
grep "/v1/profile" omem-server/src/api/router.rs         # Expected: present (proxy)
grep "profile_v2\|inject" omem-server/src/api/handlers/profile.rs  # Expected: present (V2 proxy)
grep "/v1/profile" plugins/opencode/src/tools.ts         # Expected: 0
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All 373+ existing tests pass (no regressions)
- [ ] Plugin compiles with zero TypeScript errors
- [ ] profile_v2/ directory completely untouched
- [ ] /v1/profile route preserved as V2 proxy (openclaw/mcp compatible)
- [ ] memory.rs no longer accepts PREFERENCE memory_type
- [ ] lib.rs `pub mod profile;` removed
