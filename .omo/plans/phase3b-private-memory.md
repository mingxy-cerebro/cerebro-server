# Phase 3b — 私密记忆隔离 (Private Memory Isolation)

## TL;DR

> **Quick Summary**: 将 visibility=private 的记忆从主LanceDB table物理隔离到独立"private_memories" table，实现API响应层AES-256-GCM加密，Plugin端可配置召回，GC机制同步建立，现有private记忆迁移。
> 
> **Deliverables**:
> - LanceStore新增"private_memories" table（同Connection，物理隔离）
> - API响应加密模块（AES-256-GCM on content+tags）
> - Per-tenant密钥管理（tenant metadata存储，API分发）
> - 写入路由（visibility=private → private table）
> - 读取路由（include_private参数合并private table结果）
> - /v2/memories/ 新端点（加密响应）+ 密钥API
> - GC机制（after_private_mutation，同步建立防OOM）
> - 迁移工具（主table → private table）
> - Plugin端include_private配置 + 解密逻辑
> 
> **Estimated Effort**: Large
> **Parallel Execution**: YES - 3 waves + Final
> **Critical Path**: T1 → T4/T5 → T6 → T7 → F1-F4

---

## Context

### Original Request
师尊要求将私密记忆（visibility=private）从主LanceDB memory table中物理隔离出来，存储到独立的"private_memories" table。保持ingest格式不变，新增v2 API端点，API响应层加密防抓包。

### Interview Summary
**Key Discussions**:
- 场景：师尊-月儿私密对话 + PII/密码
- 存储方案：方案A（同Connection加新table，共享URI+embedding）
- 加密策略：存储明文 + API响应加密（AES-256-GCM on content+tags）
- Ingest：格式不变，只改存储路由层
- 召回：Plugin端可配置include_private
- GC：⚠️天坑——必须同步建立，复用after_mutation模式
- 迁移：现有private记忆全部迁移到新table
- API：新增/v2端点，v1不返回private记忆
- 测试：Tests-after

**Research Findings**:
- GC: after_mutation()写入计数>=50触发prune+compact+index merge (L2584-2748)
- Collection: LanceDB Connection是目录级数据库，同Connection可加多table
- Visibility: build_visibility_filter()构建OR条件 (L2096-2113)
- 写入路由点: pipeline.rs:293, memory.rs:283, memory.rs:1590
- 读取路由点: retrieve/pipeline.rs:316, memory.rs:2264

### Metis Review
**Identified Gaps** (addressed):
- 加密范围：从"存储层加密"修正为"传输层加密"（师尊决策）
- 迁移原子性：采用"写入路由先行"策略避免双写窗口
- API兼容性：新增v2端点（师尊决策）

---

## Work Objectives

### Core Objective
将visibility=private的记忆物理隔离到独立LanceDB table，API响应加密保护私密内容，Plugin端可配置是否召回私密记忆。

### Concrete Deliverables
- `omem-server/src/store/lancedb.rs` 新增private_memories table + GC
- `omem-server/src/crypto.rs` 新增加密/解密模块
- `omem-server/src/api/handlers/memories_v2.rs` 新增v2端点
- `omem-server/src/api/handlers/encryption_key.rs` 新增密钥API
- `omem-server/src/api/router.rs` 注册/v2路由
- Plugin `src/` 更新include_private配置 + 解密逻辑
- 迁移脚本/工具

### Definition of Done
- [ ] private_memories table创建并可读写
- [ ] API响应中private记忆content+tags已加密
- [ ] Plugin端可配置include_private并正确解密
- [ ] GC机制运行（version不爆炸）
- [ ] 现有private记忆全部迁移完成
- [ ] v1端点不返回private记忆（无breaking change）

### Must Have
- private_memories独立table（同Connection）
- AES-256-GCM加密content+tags字段（API响应层）
- Per-tenant密钥（API自动生成，存tenant metadata）
- after_private_mutation() GC（阈值50，prune+compact+index merge）
- /v2/memories/ 端点（加密响应）
- 密钥分发API（GET /v2/memories/encryption-key）
- 迁移工具（主table → private table）
- Plugin端include_private配置
- 迁移后v1端点过滤掉private记忆

### Must NOT Have (Guardrails)
- ⚠️ 不修改ingest pipeline格式（师尊强调稳定性）
- 不修改普通memory的CRUD逻辑
- 不做存储层加密（改为传输层加密）
- 不实现密钥轮换
- 不修改现有LanceDB主collection的GC机制
- 不创建新的独立LanceDB数据库（用同Connection新table）
- v1端点行为不能breaking（只是不再返回private记忆）

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (cargo test inline tests)
- **Automated tests**: Tests-after
- **Framework**: Rust inline tests (cargo test)

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Rust code**: Use Bash (cargo test, cargo check, cargo clippy)
- **API endpoints**: Use Bash (curl) - Send requests, assert status + response
- **Plugin**: Use Bash (npm test, node script)

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - foundation):
├── Task 1: LanceStore private table + GC [deep]
├── Task 2: Crypto module (AES-256-GCM) [quick]
└── Task 3: Key management + API [quick]

Wave 2 (After Wave 1 - routing layer):
├── Task 4: Write routing (visibility→private table) [deep]
├── Task 5: Read routing (include_private merge) [deep]
└── Task 6: v2 API endpoints + encryption response [unspecified-high]

Wave 3 (After Wave 2 - integration):
├── Task 7: Plugin include_private + decryption [quick]
├── Task 8: Migration tool [deep]
└── Task 9: main.rs/AppState integration + startup GC [quick]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
→ Present results → Get explicit user okay

Critical Path: T1 → T4 → T6 → T7 → F1-F4
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 3 (Waves 1 & 2)
```

### Dependency Matrix

| Task | Blocked By | Blocks |
|------|-----------|--------|
| T1 | None | T4, T5, T9 |
| T2 | None | T6 |
| T3 | None | T6, T7 |
| T4 | T1 | T8 |
| T5 | T1 | T6 |
| T6 | T2, T3, T5 | T7 |
| T7 | T3, T6 | - |
| T8 | T4 | - |
| T9 | T1 | - |
| F1-F4 | T1-T9 | - |

### Agent Dispatch Summary

- **Wave 1**: 3 — T1 → `deep`, T2 → `quick`, T3 → `quick`
- **Wave 2**: 3 — T4 → `deep`, T5 → `deep`, T6 → `unspecified-high`
- **Wave 3**: 3 — T7 → `quick`, T8 → `deep`, T9 → `quick`
- **FINAL**: 4 — F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

- [ ] 1. **LanceStore Private Table + GC机制**

  **What to do**:
  - 在`LanceStore::new()`中新增`private_table: Table`字段，使用`db.open_table("private_memories")` + fallback `create_empty_table`
  - 新增`private_write_count: Arc<AtomicU32>` + `private_rebuilding: Arc<AtomicBool>`
  - 实现`after_private_mutation()`：复用`after_mutation()`逻辑（阈值50, prune 10min + compact + index merge）
  - 在`init_table()`中为private table创建scalar indexes（visibility, owner_agent_id, created_at等）
  - 在`optimize()`中增加private table的compact+prune（参照recall tables处理）
  - 在`rebuild_indices()`中为private table重建索引
  - 实现private table的CRUD方法：`create_private()`, `get_private()`, `update_private()`, `hard_delete_private()`, `search_private()`

  **Must NOT do**:
  - 不修改现有memories table的任何逻辑
  - 不修改after_mutation()或after_recall_mutation()
  - 不创建新的独立LanceDB数据库

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: LanceStore是核心存储层，改动涉及GC机制、schema定义、索引管理，需要深入理解
  - **Skills**: []
  - **Skills Evaluated but Omitted**:
    - `test-driven-development`: Tests-after策略，不需要TDD

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2, T3)
  - **Blocks**: T4, T5, T9
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `omem-server/src/store/lancedb.rs:132-200` — LanceStore::new() 构造函数，table创建模式 **CRITICAL: 照此模式加private table**
  - `omem-server/src/store/lancedb.rs:2584-2748` — after_mutation() GC机制 **CRITICAL: 复制此模式写after_private_mutation()**
  - `omem-server/src/store/lancedb.rs:387-405` — init_table() 索引创建模式
  - `omem-server/src/store/lancedb.rs:2439-2510` — optimize() compact+prune模式
  - `omem-server/src/store/lancedb.rs:2511-2582` — rebuild_indices() 重建模式
  - `omem-server/src/store/lancedb.rs:580-650` — schema() 定义memories table的Arrow schema **CRITICAL: 参考此写private_memories的schema**

  **API/Type References**:
  - `omem-server/src/store/lancedb.rs:2751-2798` — after_recall_mutation() 参考模式（如何在同一Store中管理多个table的GC）

  **WHY Each Reference Matters**:
  - `LanceStore::new()`: 需要在此处新增private table的open/create逻辑
  - `after_mutation()`: private GC直接复制此模式，改table名和计数器即可
  - `schema()`: private_memories table需要相同的schema（含visibility, owner_agent_id等列）
  - `init_table()`: private table需要创建相同的scalar indexes
  - `optimize()`: 定期GC需要覆盖private table

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Private table创建和CRUD
    Tool: Bash (cargo test)
    Preconditions: LanceStore连接到临时目录
    Steps:
      1. cargo test -p omem-server -- private_memories
      2. 验证create_private写入private table
      3. 验证get_private可读取
      4. 验证search_private返回结果
    Expected Result: 所有private CRUD测试通过
    Failure Indicators: 编译失败或测试panic
    Evidence: .omo/evidence/task-1-private-crud.txt

  Scenario: GC after_private_mutation触发
    Tool: Bash (cargo test)
    Preconditions: private_write_count达到50
    Steps:
      1. 循环create_private 50次
      2. 验证private_rebuilding被设置为true
      3. 等待GC完成
      4. 检查version数 <= 10
    Expected Result: GC正确触发，version数被清理
    Failure Indicators: version数持续增长，OOM
    Evidence: .omo/evidence/task-1-gc-test.txt

  Scenario: 写入路径不破坏现有memories table
    Tool: Bash (cargo test)
    Steps:
      1. 运行现有所有LanceStore测试
      2. 验证全部通过
    Expected Result: 零回归
    Evidence: .omo/evidence/task-1-no-regression.txt
  ```

  **Commit**: YES (Wave 1 group)
  - Message: `feat(private-memory): add private table, crypto module, and key management`
  - Files: `omem-server/src/store/lancedb.rs`
  - Pre-commit: `cargo check`

- [ ] 2. **Crypto模块 (AES-256-GCM)**

  **What to do**:
  - 新建`omem-server/src/crypto.rs`
  - 实现`generate_key() -> [u8; 32]`：生成32字节随机密钥
  - 实现`encrypt(plaintext: &str, key: &[u8; 32]) -> Result<EncryptedData>`：AES-256-GCM加密，返回nonce+ciphertext
  - 实现`decrypt(encrypted: &EncryptedData, key: &[u8; 32]) -> Result<String>`：AES-256-GCM解密
  - `EncryptedData`结构体：`{ nonce: String (base64), ciphertext: String (base64) }`
  - 使用`aes-gcm` crate（添加到Cargo.toml依赖）
  - 添加inline tests：加密→解密→原文匹配、错误密钥→解密失败、空字符串处理

  **Must NOT do**:
  - 不自己实现加密算法
  - 不使用不安全的ECB模式

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 标准加密模块，逻辑简单，依赖成熟crate
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T3)
  - **Blocks**: T6
  - **Blocked By**: None

  **References**:

  **External References**:
  - `aes-gcm` crate: https://docs.rs/aes-gcm — AEAD加密标准库

  **Pattern References**:
  - `omem-server/Cargo.toml` — 现有依赖列表，需新增`aes-gcm`

  **WHY Each Reference Matters**:
  - `aes-gcm`: Rust标准AES-GCM实现，无需自己写加密算法

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: 加密解密往返
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server -- crypto
      2. 验证encrypt("hello", key) → decrypt → "hello"
    Expected Result: 往返一致
    Evidence: .omo/evidence/task-2-crypto-roundtrip.txt

  Scenario: 错误密钥解密失败
    Tool: Bash (cargo test)
    Steps:
      1. encrypt("hello", key1)
      2. decrypt(result, key2)
    Expected Result: 返回Err，不panic
    Evidence: .omo/evidence/task-2-wrong-key.txt
  ```

  **Commit**: YES (Wave 1 group)
  - Files: `omem-server/src/crypto.rs`, `omem-server/Cargo.toml`
  - Pre-commit: `cargo test -- crypto`

- [ ] 3. **密钥管理 + API**

  **What to do**:
  - 扩展`TenantStore`（或tenant metadata）：新增`encryption_key`字段存储per-tenant AES密钥
  - 实现`ensure_encryption_key(tenant_id)`：首次访问时自动生成密钥，存入tenant metadata
  - 实现`get_encryption_key(tenant_id) -> [u8; 32]`：读取密钥
  - 新建`omem-server/src/api/handlers/encryption_key.rs`
  - API: `GET /v2/memories/encryption-key` — 返回Base64编码的密钥（需认证）
  - API: `POST /v2/memories/encryption-key/rotate` — 重新生成密钥（暂不实现，返回501）
  - 在`router.rs`中注册路由

  **Must NOT do**:
  - 不实现密钥轮换（POST rotate返回501）
  - 不在日志中打印密钥

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 简单的CRUD API + 密钥生成
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2)
  - **Blocks**: T6, T7
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `omem-server/src/store/tenant.rs` — TenantStore现有结构和API
  - `omem-server/src/api/handlers/memories.rs:2385` — 现有optimize端点作为API注册参考
  - `omem-server/src/api/router.rs` — 路由注册模式

  **API/Type References**:
  - `omem-server/src/crypto.rs:generate_key()` — T2产出的密钥生成函数

  **WHY Each Reference Matters**:
  - `tenant.rs`: 需要扩展此store来存储encryption_key字段
  - `router.rs`: 需要注册新的/v2路由

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: 密钥自动生成
    Tool: Bash (cargo test + curl)
    Steps:
      1. 新tenant首次调用GET /v2/memories/encryption-key
      2. 验证返回32字节Base64编码密钥
      3. 再次调用返回相同密钥
    Expected Result: 首次自动生成，后续返回相同值
    Evidence: .omo/evidence/task-3-key-gen.txt

  Scenario: 未认证请求被拒绝
    Tool: Bash (curl)
    Steps:
      1. curl不带Authorization header请求密钥API
    Expected Result: 401 Unauthorized
    Evidence: .omo/evidence/task-3-auth.txt
  ```

  **Commit**: YES (Wave 1 group)
  - Files: `omem-server/src/store/tenant.rs`, `omem-server/src/api/handlers/encryption_key.rs`, `omem-server/src/api/router.rs`
  - Pre-commit: `cargo check`

- [ ] 4. **写入路由 (visibility=private → private table)**

  **What to do**:
  - 在`ingest/pipeline.rs:293`（reconciler返回Memory后），检查visibility="private" → 调用`store.create_private()`替代`store.create()`
  - 在`api/handlers/memory.rs:283`（create handler），visibility="private" → 写private table
  - 在`api/handlers/memory.rs:1590`（session-ingest），visibility="private" → 写private table
  - 在`api/handlers/memory.rs`（update handler），visibility从非private改为private → 迁移到private table
  - 在`api/handlers/memory.rs`（update handler），visibility从private改为非private → 迁回main table
  - 确保所有写入路径都有after_private_mutation()调用

  **Must NOT do**:
  - 不修改ingest pipeline格式（只改存储路由目标）
  - 不修改reconciler逻辑
  - 不修改ExtractedFact结构

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 涉及3个不同写入路径的路由改造，需要理解ingest pipeline、handler、session-ingest的完整流程
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T5)
  - **Parallel Group**: Wave 2 (with T5, T6)
  - **Blocks**: T8
  - **Blocked By**: T1

  **References**:

  **Pattern References**:
  - `omem-server/src/ingest/pipeline.rs:281-293` — Stage 8 privacy tagging后写入点 **CRITICAL: 在此处插入路由逻辑**
  - `omem-server/src/api/handlers/memory.rs:266-283` — Create handler写入点
  - `omem-server/src/api/handlers/memory.rs:1588-1591` — Session-ingest visibility设置点
  - `omem-server/src/api/handlers/memory.rs:857-909` — Batch update visibility处理

  **API/Type References**:
  - `omem-server/src/store/lancedb.rs:create_private()` — T1产出的private table写入方法

  **WHY Each Reference Matters**:
  - `pipeline.rs:293`: ingest pipeline的主要写入路由点，private记忆需要在这里切换目标table
  - `memory.rs:283`: HTTP API create是另一个写入入口
  - `memory.rs:1590`: session-ingest是第三个写入入口
  - `memory.rs:857`: visibility变更需要双向迁移

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Ingest写入private记忆路由到private table
    Tool: Bash (cargo test + curl)
    Steps:
      1. POST /v1/memories content="我的密码是xxx" visibility=private
      2. 查询private table验证记录存在
      3. 查询main table验证记录不存在
    Expected Result: private记忆只出现在private table
    Evidence: .omo/evidence/task-4-write-route.txt

  Scenario: Visibility变更触发迁移
    Tool: Bash (curl)
    Steps:
      1. 创建visibility=global记忆
      2. PATCH更新为visibility=private
      3. 验证从main table消失，出现在private table
    Expected Result: 双向迁移正确
    Evidence: .omo/evidence/task-4-vis-change.txt

  Scenario: Session-ingest private写入
    Tool: Bash (cargo test)
    Steps:
      1. 模拟session-ingest with scope=private
      2. 验证写入private table
    Expected Result: session private记忆在private table
    Evidence: .omo/evidence/task-4-session.txt
  ```

  **Commit**: YES (Wave 2 group)
  - Files: `omem-server/src/ingest/pipeline.rs`, `omem-server/src/api/handlers/memory.rs`
  - Pre-commit: `cargo check`

- [ ] 5. **读取路由 (include_private合并private table结果)**

  **What to do**:
  - 在`retrieve/pipeline.rs:316`（搜索时），增加include_private参数
  - 当include_private=true时：先查main table（排除private）→ 再查private table → 合并结果
  - 当include_private=false时：只查main table（排除private）— 默认行为
  - 修改`build_visibility_filter()`：在main table查询中排除visibility=private
  - 在`api/handlers/memory.rs:2264`（session emotional fetch），查private table
  - 在profile/service.rs:327的排除逻辑验证（已经排除private，不需要改）

  **Must NOT do**:
  - 不修改build_visibility_filter的OR逻辑结构
  - 不影响cluster/manager.rs的private跳过逻辑（保持不变）
  - 不做向量搜索层面的跨table合并（用两次查询+合并代替）

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 涉及检索管线的核心搜索逻辑，需要理解vector_search和fts_search的filter机制
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T4)
  - **Parallel Group**: Wave 2 (with T4, T6)
  - **Blocks**: T6
  - **Blocked By**: T1

  **References**:

  **Pattern References**:
  - `omem-server/src/retrieve/pipeline.rs:316-320` — visibility_filter构建点 **CRITICAL**
  - `omem-server/src/store/lancedb.rs:2096-2113` — build_visibility_filter() **CRITICAL: 需增加排除private的逻辑**
  - `omem-server/src/store/lancedb.rs:1963-1989` — vector_search filter应用
  - `omem-server/src/store/lancedb.rs:2037-2064` — fts_search filter应用

  **API/Type References**:
  - `omem-server/src/retrieve/pipeline.rs:39-40` — SearchRequest结构体（需新增include_private字段）
  - `omem-server/src/api/handlers/memory.rs:361-362` — handler传入agent_id_filter和accessible_spaces

  **WHY Each Reference Matters**:
  - `pipeline.rs:316`: 检索管线的搜索入口，需要在此处分支查private table
  - `build_visibility_filter()`: 需要修改为main table查询排除private
  - `vector_search/fts_search`: 两个搜索方法都需要理解filter如何传递

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: include_private=false不返回private记忆
    Tool: Bash (curl)
    Steps:
      1. 创建1条global + 1条private记忆
      2. GET /v2/memories?include_private=false
      3. 验证只返回global记忆
    Expected Result: private记忆完全不可见
    Evidence: .omo/evidence/task-5-exclude-private.txt

  Scenario: include_private=true合并两个table结果
    Tool: Bash (curl)
    Steps:
      1. 创建1条global + 1条private记忆
      2. GET /v2/memories?include_private=true
      3. 验证返回2条记忆
    Expected Result: 两个table的结果正确合并
    Evidence: .omo/evidence/task-5-include-private.txt
  ```

  **Commit**: YES (Wave 2 group)
  - Files: `omem-server/src/retrieve/pipeline.rs`, `omem-server/src/store/lancedb.rs`
  - Pre-commit: `cargo check`

- [ ] 6. **v2 API端点 + 加密响应**

  **What to do**:
  - 新建`omem-server/src/api/handlers/memories_v2.rs`
  - `/v2/memories/` GET — 列出记忆（private记忆content+tags加密返回）
  - `/v2/memories/` POST — 创建记忆（自动路由到private table如果visibility=private）
  - `/v2/memories/:id` GET — 单条记忆（private加密）
  - `/v2/memories/:id` PATCH — 更新记忆
  - `/v2/memories/:id` DELETE — 删除记忆
  - `/v2/memories/search` POST — 搜索（支持include_private参数）
  - 加密逻辑：读取private记忆后，用tenant key加密content+tags，返回`EncryptedMemory`结构体
  - `EncryptedMemory`：`{ ...Memory字段, encrypted_content: EncryptedData, encrypted_tags: EncryptedData, is_encrypted: bool }`
  - 在router.rs中注册/v2路由

  **Must NOT do**:
  - v1端点已由T5的build_visibility_filter()排除private记忆，v2无需再改v1
  - 不加密global记忆
  - 不在v2端点中做存储层加密

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 多个API端点 + 加密逻辑 + 路由注册，工作量中等偏高
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (starts after T2+T3+T5)
  - **Blocks**: T7
  - **Blocked By**: T2, T3, T5

  **References**:

  **Pattern References**:
  - `omem-server/src/api/handlers/memory.rs` — 现有v1 handler模式 **CRITICAL: 参考此写v2**
  - `omem-server/src/api/router.rs` — 路由注册

  **API/Type References**:
  - `omem-server/src/crypto.rs:encrypt()` — T2产出的加密函数
  - `omem-server/src/api/handlers/encryption_key.rs` — T3产出的密钥API

  **WHY Each Reference Matters**:
  - `memory.rs`: v2 handler结构参照v1，增加加密逻辑
  - `crypto.rs`: 加密private记忆的content+tags

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: v2端点加密private记忆
    Tool: Bash (curl)
    Steps:
      1. POST /v2/memories content="密码123" visibility=private
      2. GET /v2/memories
      3. 验证返回的encrypted_content是加密的
      4. 用密钥解密验证原文
    Expected Result: private记忆content+tags已加密，global记忆未加密
    Evidence: .omo/evidence/task-6-v2-encrypted.txt

  Scenario: v2搜索支持include_private
    Tool: Bash (curl)
    Steps:
      1. POST /v2/memories/search {"query":"密码", "include_private":true}
      2. 验证返回private记忆（加密状态）
    Expected Result: 搜索结果包含加密的private记忆
    Evidence: .omo/evidence/task-6-v2-search.txt
  ```

  **Commit**: YES (Wave 2 group)
  - Files: `omem-server/src/api/handlers/memories_v2.rs`, `omem-server/src/api/router.rs`
  - Pre-commit: `cargo check`

- [ ] 7. **Plugin端 include_private配置 + 解密逻辑**

  **What to do**:
  - OpenCode plugin: 在config中新增`includePrivateMemories: boolean`配置项（默认false）
  - OpenCode plugin: recall请求中传递`include_private`参数
  - OpenCode plugin: 实现解密函数（Node.js crypto模块，AES-256-GCM解密）
  - OpenCode plugin: 收到加密响应后自动解密content+tags
  - MCP plugin: 同步支持include_private参数
  - 存储密钥到plugin本地config（首次获取后缓存）

  **Must NOT do**:
  - 不在日志中打印解密后的私密内容
  - 不将密钥硬编码

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Plugin端改动较简单，主要是配置项+解密函数
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T8, T9)
  - **Parallel Group**: Wave 3 (with T8, T9)
  - **Blocks**: None
  - **Blocked By**: T3, T6

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/config.ts` — 现有配置结构
  - `plugins/opencode/src/client.ts:265-266` — getProfile()参考API调用模式
  - `plugins/opencode/src/hooks.ts:378-418` — 现有profile注入逻辑

  **API/Type References**:
  - `omem-server/src/crypto.rs:EncryptedData` — 加密数据结构

  **WHY Each Reference Matters**:
  - `config.ts`: 新增includePrivateMemories配置项
  - `client.ts`: API调用模式参考
  - `hooks.ts`: 需要在recall流程中传递include_private参数

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Plugin解密private记忆
    Tool: Bash (npm test)
    Steps:
      1. npm test in plugins/opencode
      2. 验证解密函数：加密数据 + 密钥 → 原文
    Expected Result: 解密正确
    Evidence: .omo/evidence/task-7-plugin-decrypt.txt

  Scenario: include_private配置传递
    Tool: Bash (npm test)
    Steps:
      1. 验证config中includePrivateMemories=true时请求包含参数
      2. 验证=false时不包含参数
    Expected Result: 配置正确传递
    Evidence: .omo/evidence/task-7-plugin-config.txt
  ```

  **Commit**: YES (Wave 3 group)
  - Files: `plugins/opencode/src/config.ts`, `plugins/opencode/src/client.ts`, `plugins/opencode/src/hooks.ts`
  - Pre-commit: `npm run build`

- [ ] 8. **迁移工具 (主table → private table)**

  **What to do**:
  - 新建迁移命令/函数：`migrate_private_memories(store: &LanceStore)`
  - 步骤：
    1. 扫描main table中所有visibility="private"的记忆
    2. 批量读取完整Memory对象
    3. 写入private_memories table（create_private batch）
    4. 验证写入数量一致
    5. 从main table hard_delete已迁移的记录
    6. 触发main table的after_mutation()清理version
    7. 触发private table的after_private_mutation()清理version
  - 提供`POST /v2/memories/migrate` API端点触发迁移
  - 迁移日志：记录迁移数量、耗时、错误
  - 容错：单条失败不影响其他记录，记录失败ID

  **Must NOT do**:
  - 不在迁移过程中锁定整个table
  - 不删除失败记录的原始数据
  - 不做downtime迁移（在线迁移）

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 数据迁移涉及批量操作、一致性验证、错误处理
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7, T9)
  - **Parallel Group**: Wave 3 (with T7, T9)
  - **Blocks**: None
  - **Blocked By**: T4

  **References**:

  **Pattern References**:
  - `omem-server/src/store/lancedb.rs:hard_delete_by_ids()` — 批量删除模式
  - `omem-server/src/api/handlers/imports.rs` — 现有导入/批量操作参考

  **API/Type References**:
  - `omem-server/src/store/lancedb.rs:create_private()` — T1产出
  - `omem-server/src/store/lancedb.rs:after_mutation()` — GC触发

  **WHY Each Reference Matters**:
  - `hard_delete_by_ids()`: 迁移完成后从main table删除
  - `imports.rs`: 批量操作的进度报告和错误处理模式

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: 完整迁移流程
    Tool: Bash (curl + cargo test)
    Steps:
      1. 预先在main table插入5条private记忆 + 10条global记忆
      2. POST /v2/memories/migrate
      3. 验证main table: 0条private, 10条global
      4. 验证private table: 5条private
      5. 验证每条记忆内容完整（对比原数据）
    Expected Result: 迁移数量一致，内容无损
    Evidence: .omo/evidence/task-8-migration.txt

  Scenario: 迁移中途失败恢复
    Tool: Bash (cargo test)
    Steps:
      1. 模拟第3条记录写入private table失败
      2. 验证前2条已迁移，后3条仍在main table
      3. 重新运行迁移
      4. 验证所有5条最终都在private table
    Expected Result: 幂等迁移，可重复执行
    Evidence: .omo/evidence/task-8-idempotent.txt
  ```

  **Commit**: YES (Wave 3 group)
  - Files: 新建迁移模块 + handler
  - Pre-commit: `cargo check`

- [ ] 9. **main.rs/AppState集成 + 启动GC**

  **What to do**:
  - 在`AppState`中新增相关字段（如需）
  - 在`main.rs`启动流程中，确保`optimize_all_on_disk()`覆盖private table
  - 验证`LifecycleScheduler`的定期GC覆盖private table
  - 验证启动时`init_table()`创建private table indexes
  - 端到端验证：启动 → 创建private记忆 → 搜索 → 加密响应 → GC运行

  **Must NOT do**:
  - 不修改AppState的其他字段
  - 不修改启动顺序的其他步骤

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 主要是集成验证，改动小
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7, T8)
  - **Parallel Group**: Wave 3 (with T7, T8)
  - **Blocks**: None
  - **Blocked By**: T1

  **References**:

  **Pattern References**:
  - `omem-server/src/main.rs:164-168` — 启动时GC调用
  - `omem-server/src/api/server.rs` — AppState定义
  - `omem-server/src/lifecycle/scheduler.rs:197` — LifecycleScheduler定期任务

  **WHY Each Reference Matters**:
  - `main.rs:164`: 需要验证启动GC覆盖private table
  - `AppState`: 可能需要新增字段
  - `scheduler.rs:197`: 验证定期GC覆盖

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: 启动后private table可用
    Tool: Bash (cargo test)
    Steps:
      1. 启动server
      2. 创建private记忆
      3. 查询private table
      4. 验证GC在写入50次后触发
    Expected Result: 完整流程通过
    Evidence: .omo/evidence/task-9-integration.txt

  Scenario: 编译通过无回归
    Tool: Bash (cargo check && cargo test)
    Steps:
      1. cargo check
      2. cargo test
    Expected Result: 编译0 error，所有测试通过
    Evidence: .omo/evidence/task-9-build.txt
  ```

  **Commit**: YES (Wave 3 group)
  - Files: `omem-server/src/main.rs`, `omem-server/src/api/server.rs`
  - Pre-commit: `cargo test`

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search for forbidden patterns. Check evidence files.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo check` + `cargo clippy` + `cargo test`. Review all changed files for: `unwrap()` in non-test, empty catches, unused imports, AI slop.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N pass/N fail] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start server. Test: create private memory → verify in private table → v2 API returns encrypted → decrypt with key → match original. Test migration. Test GC.
  Output: `Scenarios [N/N pass] | Integration [N/N] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: verify everything in spec was built, nothing beyond spec was built. Check "Must NOT do" compliance. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `feat(private-memory): add private table, crypto module, and key management` - store/lancedb.rs, crypto.rs, tenant.rs, handlers/encryption_key.rs
- **Wave 2**: `feat(private-memory): add write/read routing and v2 API endpoints` - pipeline.rs, memory.rs, retrieve/pipeline.rs, handlers/memories_v2.rs, router.rs
- **Wave 3**: `feat(private-memory): add plugin integration, migration tool, and startup GC` - plugins/, migration, main.rs

---

## Success Criteria

### Verification Commands
```bash
cargo check                                 # Expected: no errors
cargo test                                  # Expected: all tests pass (including new ones)
cargo clippy                                # Expected: no new warnings
curl http://localhost:8080/v2/memories/      # Expected: 200 + encrypted private content
curl http://localhost:8080/v2/memories/encryption-key  # Expected: 200 + key JSON
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All existing tests pass (no regressions)
- [ ] private_memories table created and functional
- [ ] API response encryption works (content+tags encrypted)
- [ ] Key management API works (generate + retrieve)
- [ ] GC runs on private table (no version explosion)
- [ ] Migration tool completes successfully
- [ ] Plugin can decrypt and display private memories
