# 删除归簇（Clustering）模块

## TL;DR

> **Quick Summary**: 删除 Cerebro 中所有 clustering 相关代码，包括整个 `cluster/` 模块、cluster API 路由、AppState 中的 cluster 字段、session_ingest 中的 cluster assignment、lifecycle scheduler 中的 incremental clustering、config 中的 cluster 配置项。session_ingest 精炼功能已从根源解决记忆噪音，归簇功能不再需要。
> 
> **Deliverables**:
> - 删除 `omem-server/src/cluster/` 整个目录（7个文件）
> - 删除 `omem-server/src/api/handlers/clusters.rs`（整个文件）
> - 删除 `omem-server/src/domain/cluster.rs`（整个文件）
> - 清理 20+ 文件中的 cluster 引用
> - 编译通过 + 部署验证
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 3 waves
> **Critical Path**: Task 1 → Task 4 → Task 7

---

## Context

### Original Request
师尊要求删掉所有和归簇有关的逻辑，因为 session_ingest 已经从根源解决记忆噪音了，归簇没用还浪费钱。

### Interview Summary
**Key Discussions**:
- 归簇功能在 session_ingest 精炼功能上线后已无实际价值
- 精炼功能（refine_service.rs）在每次 ingest 时对同 topic 的 WORK 记忆做 LLM 精炼去重，从源头控制记忆质量
- 归簇每次触发都消耗 LLM token（cluster summary、LLM judge 等），是纯成本
- 31 个 Rust 文件 + 2 个 TypeScript 插件文件引用了 cluster

**Research Findings**:
- `cluster/` 目录：7 个文件（mod, manager, cluster_store, kmeans, assigner, aggregator, background_clustering）
- API 层：clusters.rs handler + router.rs 中 11 条 cluster 路由
- AppState：cluster_llm + cluster_store 两个字段
- session_ingest：line 2269-2340 的 cluster assignment 代码块
- lifecycle scheduler：incremental_clustering + cleanup_orphan_clusters
- config.rs：6 个 cluster 相关配置项（OMEM_CLUSTER_*）
- Memory struct：cluster_id + is_cluster_anchor 两个字段
- LanceStore：42 处 cluster 相关代码（cluster_id 列、update_memory_cluster_id、batch_update_cluster_ids 等）
- plugins/opencode：hooks.ts 和 client.ts 中的 ClusteredRecallResult 相关代码

---

## Work Objectives

### Core Objective
完全移除 clustering 功能的所有代码和配置，不留死代码。

### Concrete Deliverables
- 删除整个 `cluster/` 模块
- 删除 cluster API 端点和 handler
- 从 AppState 移除 cluster_llm 和 cluster_store
- 从 session_ingest 移除 cluster assignment
- 从 lifecycle scheduler 移除 clustering 逻辑
- 从 config 移除所有 OMEM_CLUSTER_* 配置项
- Memory struct 的 cluster_id/is_cluster_anchor 保留（不破坏已有 LanceDB 数据）
- 清理 plugins/opencode 中的 cluster 引用

### Definition of Done
- [ ] `cargo build` 编译通过
- [ ] `cargo test` 测试通过
- [ ] `cargo clippy` 无新 warning
- [ ] 无 cluster 相关的 LLM 调用（不再浪费钱）

### Must Have
- 完全移除 cluster 模块的所有代码
- 所有 cluster API 端点移除（404 而非报错）
- 不破坏已有 LanceDB 数据（Memory struct 的 cluster_id/is_cluster_anchor 字段保留为 Option/bool 默认值）

### Must NOT Have (Guardrails)
- **不修改** Memory struct 的字段定义（cluster_id、is_cluster_anchor 保留，只是不再赋值）
- **不修改** LanceDB schema（cluster_id 列保留，只是不再写入）
- **不删除** LanceStore 中的 cluster_id 相关序列化代码（保持读取兼容）
- **不修改** 精炼（refine）相关的任何代码
- **不破坏** 召回功能（should_recall 端点继续正常工作）

### 召回兼容性分析（IMPORTANT）

**删除 cluster 对召回的影响**：

1. **should_recall 端点**（session_recalls.rs line 521-537）：
   - 现在会在返回前调用 `ClusterAggregator.aggregate()` 生成簇摘要
   - 删除后 `clustered` 字段始终为 `None`
   - 召回结果 `memories` 字段**不受影响**——搜索+排序逻辑完全独立于 cluster

2. **OpenCode Plugin hooks.ts**（line 599-601）：
   - 现在的逻辑：`clustered ? buildClusteredContextBlock(...) : buildContextBlock(...)`
   - 删除后：`clustered` 为 `undefined`，始终走 `buildContextBlock` 路径
   - **召回完全正常**，只是不再按簇聚合展示，改为直接展示每条独立记忆
   - 这是**功能降级**（丢失簇摘要）而非功能中断

3. **需要修改的插件代码**：
   - `client.ts`：删除 `ClusteredRecallResult`、`ClusterSummary` interface
   - `client.ts`：删除 `ShouldRecallResponse.clustered` 字段
   - `hooks.ts`：删除 `buildClusteredContextBlock` 函数
   - `hooks.ts`：简化 `autoRecallHook` 中的 clustered 判断，直接走 `buildContextBlock`
   - 删除后插件**更简洁**，无死代码

4. **omem-web 前端**：
   - 搜索 `*.{ts,tsx,vue,js,jsx}` 文件：**零 cluster 引用**
   - Web 端没有归簇管理页面，无需清理

5. **其他插件**（openclaw, mcp, claude-code）：
   - 搜索确认：仅 opencode 插件有 cluster 引用
   - 其他插件无需修改

6. **Token 预算兼容性**：
   - `buildContextBlock`（fallback 路径）**已有完善的 token 预算分配**：
     - Line 266: `totalScore = results.reduce((sum, r) => sum + r.score, 0)`
     - Line 273-275: 每条记忆按 `score / totalScore * budget` 分配长度上限
     - `budget` 来自 `maxContentChars - profileChars`
   - 与 `buildClusteredContextBlock` 的区别：
     - **cluster 路径**：用簇摘要代替簇内 N 条记忆，节省 token 但丢失细节
     - **无 cluster 路径**：直接展示所有记忆，每条独立，信息更丰富
   - **无 cluster 后不会 token 超限**，因为 `buildContextBlock` 的预算分配逻辑同样严格
   - 唯一差异：不再有"簇摘要"层，但精炼功能已从源头去重，簇本身不再有价值

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES (inline tests)
- **Automated tests**: Tests-after (先删代码，后跑测试验证)
- **Framework**: cargo test

### QA Policy
每个任务完成后执行 cargo build 验证编译通过。

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - 模块级删除，互不依赖):
├── Task 1: 删除 cluster/ 目录 + mod.rs 引用 + lib.rs [quick]
├── Task 2: 删除 clusters.rs handler + mod.rs re-exports [quick]
└── Task 3: 删除 domain/cluster.rs + domain/mod.rs 引用 [quick]

Wave 2 (After Wave 1 - 清理引用，依赖模块删除完成):
├── Task 4: 清理 main.rs + AppState (cluster_llm, cluster_store) + api/mod.rs setup_app [unspecified-high]
├── Task 5: 清理 router.rs 路由 + scheduler.rs (clustering pause/resume) [quick]
├── Task 6: 清理 config.rs (6个 OMEM_CLUSTER_* 配置项) [quick]
├── Task 7: 清理 lifecycle/scheduler.rs (incremental_clustering, cleanup_orphan_clusters) [unspecified-high]
└── Task 8: 清理 llm/mod.rs + llm/openai_compat.rs (create_cluster_llm_service, new_cluster) [quick]

Wave 3 (After Wave 2 - 业务代码清理):
├── Task 9: 清理 memory.rs handler (session_ingest cluster assignment + create_memory) [unspecified-high]
├── Task 10: 清理 plugins/opencode (hooks.ts, client.ts 的 ClusteredRecallResult) [quick]
├── Task 11: 清理其他文件散落引用 (ingest/pipeline.rs, ingest/prompts.rs, api/handlers/sharing.rs, api/handlers/stats.rs, api/handlers/session_recalls.rs, api/handlers/lifecycle.rs, api/scheduler_control.rs) [quick]

Wave FINAL (After ALL tasks — 验证):
├── Task F1: cargo build + cargo test + cargo clippy [quick]
└── Task F2: 部署 + health check [quick]
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| 1 | - | 4, 7, 8, 9 |
| 2 | - | 5 |
| 3 | - | 4 |
| 4 | 1, 3 | 9, F1 |
| 5 | 2 | F1 |
| 6 | - | F1 |
| 7 | 1, 4 | F1 |
| 8 | 1 | F1 |
| 9 | 4 | F1 |
| 10 | - | F1 |
| 11 | 1, 2, 4 | F1 |

### Agent Dispatch Summary

- **Wave 1**: 3 tasks — T1→`quick`, T2→`quick`, T3→`quick`
- **Wave 2**: 5 tasks — T4→`unspecified-high`, T5→`quick`, T6→`quick`, T7→`unspecified-high`, T8→`quick`
- **Wave 3**: 3 tasks — T9→`unspecified-high`（含 ingest/pipeline.rs + memory.rs）, T10→`quick`, T11→`quick`
- **Final**: 2 tasks — F1→`quick`, F2→`quick`

---

## TODOs

- [ ] 1. 删除 cluster/ 目录 + 清理模块声明

  **What to do**:
  - 删除 `omem-server/src/cluster/` 整个目录（7个文件：mod.rs, manager.rs, cluster_store.rs, kmeans.rs, assigner.rs, aggregator.rs, background_clustering.rs）
  - 清理 `omem-server/src/lib.rs` 中的 `pub mod cluster;` 声明

  **Must NOT do**:
  - 不修改 cluster/ 目录外的任何文件（除了 lib.rs）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Tasks 4, 7, 8, 9
  - **Blocked By**: None

  **References**:
  - `omem-server/src/cluster/` — 整个目录删除
  - `omem-server/src/lib.rs` — `pub mod cluster;` 声明需移除

  **QA Scenarios**:
  ```
  Scenario: 编译验证 cluster 模块已移除
    Tool: Bash
    Steps:
      1. cargo build 2>&1 | head -20
    Expected Result: 编译错误仅限于其他文件引用了已删除的 cluster 模块（预期会有引用错误）
    Evidence: .omo/evidence/task-1-cluster-dir-removed.txt
  ```

- [ ] 2. 删除 clusters.rs handler + 清理 mod.rs re-exports

  **What to do**:
  - 删除 `omem-server/src/api/handlers/clusters.rs`（整个文件，约404行）
  - 清理 `omem-server/src/api/handlers/mod.rs` 中的 `pub mod clusters;` 和所有 cluster 相关的 `pub use` 导出（line 2, 59-62）

  **Must NOT do**:
  - 不修改 router.rs（Task 5 处理）
  - 不修改其他 handler 文件

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: Task 5
  - **Blocked By**: None

  **References**:
  - `omem-server/src/api/handlers/clusters.rs` — 整个文件删除
  - `omem-server/src/api/handlers/mod.rs` — line 2 `pub mod clusters`, line 59-62 cluster 相关 pub use

  **QA Scenarios**:
  ```
  Scenario: handler 文件已删除，re-exports 已清理
    Tool: Bash
    Steps:
      1. ls omem-server/src/api/handlers/clusters.rs → 不存在
      2. grep -c "clusters" omem-server/src/api/handlers/mod.rs → 0
    Expected Result: 文件不存在，mod.rs 无 cluster 引用
    Evidence: .omo/evidence/task-2-clusters-handler-removed.txt
  ```

- [ ] 3. 删除 domain/cluster.rs + 清理 domain/mod.rs

  **What to do**:
  - 删除 `omem-server/src/domain/cluster.rs`（整个文件，约130行）
  - 清理 `omem-server/src/domain/mod.rs` 中的 `pub mod cluster;`

  **Must NOT do**:
  - 不修改 Memory struct（cluster_id/is_cluster_anchor 字段保留）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: Task 4
  - **Blocked By**: None

  **References**:
  - `omem-server/src/domain/cluster.rs` — 整个文件删除
  - `omem-server/src/domain/mod.rs` — `pub mod cluster;` 需移除

  **QA Scenarios**:
  ```
  Scenario: domain cluster 已清理
    Tool: Bash
    Steps:
      1. ls omem-server/src/domain/cluster.rs → 不存在
      2. grep "cluster" omem-server/src/domain/mod.rs → 无匹配
    Expected Result: 文件不存在，mod.rs 无 cluster 引用
    Evidence: .omo/evidence/task-3-domain-cluster-removed.txt
  ```

- [ ] 4. 清理 main.rs + AppState + api/mod.rs setup_app

  **What to do**:
  - **main.rs**:
    - 删除 `use omem_server::cluster::cluster_store::ClusterStore;`（line 8）
    - 删除 `use omem_server::llm::{..., create_cluster_llm_service, ...};` 中的 `create_cluster_llm_service`
    - 删除 cluster_llm 创建代码块（line 139-147）
    - 删除 cluster_store 创建代码块（line 151-155）
    - 删除 AppState 构造中的 `cluster_llm,` 和 `cluster_store,`（line 165-166）
    - 删除 lifecycle scheduler 构造中的 `state.cluster_store.clone()`（line 199 附近）
  - **api/server.rs (AppState)**:
    - 删除 `use crate::cluster::cluster_store::ClusterStore;`（line 9）
    - 删除 `pub cluster_llm: Arc<dyn LlmService>,`（line 30）
    - 删除 `pub cluster_store: Arc<ClusterStore>,`（line 31）
  - **api/mod.rs (setup_app)**:
    - 删除测试辅助函数 `setup_app` 中的 cluster_store 创建（line 75-80, 101-102）
    - 删除另一处 setup_app 中的 cluster_store 创建（line 138-143, 164-165）

  **Must NOT do**:
  - 不删除 primary_llm 或 recall_llm
  - 不修改 build_router 调用

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 9, F1
  - **Blocked By**: Tasks 1, 3

  **References**:
  - `omem-server/src/main.rs` — line 8, 10, 139-147, 151-155, 165-166, 199
  - `omem-server/src/api/server.rs` — line 9, 30, 31
  - `omem-server/src/api/mod.rs` — line 75-80, 101-102, 138-143, 164-165

  **QA Scenarios**:
  ```
  Scenario: AppState 无 cluster 字段
    Tool: Bash
    Steps:
      1. grep "cluster" omem-server/src/api/server.rs → 仅剩 cluster_id/is_cluster_anchor（Memory 字段，保留）
      2. grep "cluster" omem-server/src/main.rs → 仅剩注释或无关内容
    Expected Result: AppState 定义中无 cluster_llm/cluster_store 字段
    Evidence: .omo/evidence/task-4-appstate-cleaned.txt
  ```

- [ ] 5. 清理 router.rs 路由 + scheduler.rs handler

  **What to do**:
  - **router.rs**: 删除所有 cluster 相关路由（11条）：
    - line 120-127: `/v1/clusters/*` 路由组（8条）
    - line 142-143: `/v1/scheduler/clustering/*` 路由（2条）
    - line 130: `/v1/clusters/{id}` 路由（1条）
  - **api/handlers/scheduler.rs**: 删除 `pause_clustering` 和 `resume_clustering` 函数
  - **api/handlers/mod.rs**: 删除 scheduler handler 的 `pub use` 中 clustering 相关的导出（line 66）

  **Must NOT do**:
  - 不删除 lifecycle 的 pause/resume（只删 clustering 的）
  - 不修改 get_scheduler_status 中的非 cluster 部分

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 6, 7, 8)
  - **Blocks**: F1
  - **Blocked By**: Task 2

  **References**:
  - `omem-server/src/api/router.rs` — line 120-130, 142-143
  - `omem-server/src/api/handlers/scheduler.rs` — `pause_clustering`, `resume_clustering`
  - `omem-server/src/api/handlers/mod.rs` — line 66

  **QA Scenarios**:
  ```
  Scenario: cluster 路由已全部移除
    Tool: Bash
    Steps:
      1. grep "cluster" omem-server/src/api/router.rs → 0 matches
    Expected Result: router.rs 无任何 cluster 路由
    Evidence: .omo/evidence/task-5-routes-cleaned.txt
  ```

- [ ] 6. 清理 config.rs cluster 配置项

  **What to do**:
  - 删除 OmemConfig struct 中的 6 个 cluster 配置字段（line 23-27, 45-64）：
    - `cluster_llm_provider`, `cluster_llm_api_key`, `cluster_llm_model`, `cluster_llm_base_url`
    - `cluster_similarity_threshold`, `cluster_auto_merge_threshold`
    - `cluster_candidate_count`, `cluster_llm_judge_enabled`
  - 删除 Default impl 中的对应默认值（line 144-147, 154-157）
  - 删除 `from_env()` 中的对应 env var 读取（line 227-230, 251-266）
  - 删除相关注释

  **Must NOT do**:
  - 不删除非 cluster 的配置项
  - 不修改 OMEM_LLM_* 或 OMEM_RECALL_LLM_* 配置

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5, 7, 8)
  - **Blocks**: F1
  - **Blocked By**: None

  **References**:
  - `omem-server/src/config.rs` — line 23-27, 45-64, 144-147, 154-157, 227-230, 251-266

  **QA Scenarios**:
  ```
  Scenario: config 无 cluster 字段
    Tool: Bash
    Steps:
      1. grep -i "cluster" omem-server/src/config.rs → 0 matches
    Expected Result: config.rs 完全无 cluster 引用
    Evidence: .omo/evidence/task-6-config-cleaned.txt
  ```

- [ ] 7. 清理 lifecycle/scheduler.rs clustering 逻辑

  **What to do**:
  - 删除 LifecycleScheduler struct 中的 `cluster_store` 字段
  - 删除 `new()` 中的 `cluster_store` 参数
  - 删除 `cleanup_orphan_clusters()` 整个方法
  - 删除 `run_incremental_clustering()` 整个方法
  - 删除 `run()` 主循环中对这两个方法的调用
  - 删除 cluster 相关的 import（line 11-13）
  - 删除 `SchedulerControl` 中 clustering 相关的 pause 标志（如 scheduler_control.rs 有的话）

  **Must NOT do**:
  - 不删除 lifecycle 的 decay/forgetting/tier 逻辑
  - 不删除 scheduler 的基本循环结构

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5, 6, 8)
  - **Blocks**: F1
  - **Blocked By**: Tasks 1, 4

  **References**:
  - `omem-server/src/lifecycle/scheduler.rs` — line 11-13, 25, 47, 53, 241-244, 253-254, 319-345, 405-446
  - `omem-server/src/api/scheduler_control.rs` — clustering pause 标志

  **QA Scenarios**:
  ```
  Scenario: scheduler 无 clustering 逻辑
    Tool: Bash
    Steps:
      1. grep -i "cluster" omem-server/src/lifecycle/scheduler.rs → 0 matches
    Expected Result: scheduler.rs 完全无 cluster 引用
    Evidence: .omo/evidence/task-7-scheduler-cleaned.txt
  ```

- [ ] 8. 清理 llm/mod.rs + llm/openai_compat.rs

  **What to do**:
  - **llm/mod.rs**: 删除 `create_cluster_llm_service` 函数（约10行）
  - **llm/openai_compat.rs**: 删除 `new_cluster` 方法（约30行）

  **Must NOT do**:
  - 不删除 `create_llm_service`、`create_recall_llm_service`、`create_profile_llm_service`
  - 不修改 `new` 或 `new_recall` 方法

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5, 6, 7)
  - **Blocks**: F1
  - **Blocked By**: Task 1

  **References**:
  - `omem-server/src/llm/mod.rs` — line 43-51 `create_cluster_llm_service`
  - `omem-server/src/llm/openai_compat.rs` — line 141-170 `new_cluster`

  **QA Scenarios**:
  ```
  Scenario: llm 无 cluster 函数
    Tool: Bash
    Steps:
      1. grep -i "cluster" omem-server/src/llm/mod.rs → 0 matches
      2. grep -i "cluster" omem-server/src/llm/openai_compat.rs → 0 matches
    Expected Result: llm 模块完全无 cluster 引用
    Evidence: .omo/evidence/task-8-llm-cleaned.txt
  ```

- [ ] 9. 清理 memory.rs handler + ingest/pipeline.rs 的 cluster 引用

  **What to do**:
  - **memory.rs session_ingest 函数**:
    - 删除 `let cluster_store = state.cluster_store.clone();`（line 1484）
    - 删除 `let llm_for_cluster = Some(state.llm.clone());`（line 1485）
    - 删除 `let cluster_assigner = ...`（line 1498-1499）
    - 删除整个 cluster assignment 代码块（line 2269-2340，约70行）
  - **memory.rs create_memory 函数**:
    - 修改 `IngestPipeline::new()` 调用，删除 `state.cluster_store.clone()` 参数（line 227 附近）
  - **ingest/pipeline.rs（重要！灵犀审查发现遗漏）**:
    - 删除 `ClusterAssigner`, `ClusterStore`, `ClusterManager` 的 import（line 15-17）
    - 从 `IngestPipeline` struct 中删除 `cluster_assigner` 和 `cluster_manager` 字段（line 31-32）
    - 修改 `new()` 签名：删除 `cluster_store` 和相关参数，不再创建 ClusterManager/ClusterAssigner（line 50, 59-60, 70-71）
    - 删除 `process()` 中的 cluster assignment 逻辑（line 144-145, 313-349）
    - 清理测试代码中的 ClusterStore 创建（line 568-582）
  - **split_memory 函数**中的 `cluster_id: original.cluster_id.clone()` 保留（读取兼容），`is_cluster_anchor: false` 保留

  **Must NOT do**:
  - 不删除精炼（refine）相关代码
  - 不删除 split_memory 中的 cluster_id/is_cluster_anchor（保持数据兼容）
  - 不删除 session_ingest 的核心 ingest 逻辑

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 10, 11)
  - **Blocks**: F1
  - **Blocked By**: Task 4

  **References**:
  - `omem-server/src/api/handlers/memory.rs` — line 227, 232, 1484-1485, 1498-1499, 2269-2340
  - `omem-server/src/ingest/pipeline.rs` — line 15-17, 31-32, 50, 59-60, 70-71, 144-145, 313-349, 568-582

  **QA Scenarios**:
  ```
  Scenario: session_ingest + pipeline 无 cluster 调用
    Tool: Bash
    Steps:
      1. grep -c "cluster" omem-server/src/api/handlers/memory.rs → 仅剩 cluster_id/is_cluster_anchor 字段赋值
      2. grep -c "cluster" omem-server/src/ingest/pipeline.rs → 0 matches
    Expected Result: 无 cluster_store/cluster_assigner/cluster_manager/ClusterManager 调用
    Evidence: .omo/evidence/task-9-memory-pipeline-cleaned.txt
  ```

- [ ] 10. 清理 plugins/opencode 的 cluster 引用

  **What to do**:
  - **hooks.ts**: 删除 `buildClusteredContextBlock` 函数中的 cluster 逻辑，简化为直接使用 shouldRecall 的结果
    - `clustered` 判断分支 → 统一走 standalone 路径
    - 删除 `ClusteredRecallResult` 类型引用
  - **client.ts**: 删除 `ClusteredRecallResult` interface 和 `ClusterSummary` interface
    - `ClusteredRecallResult` 包含 `cluster_summaries` 和 `standalone_memories`
    - 删除 `ShouldRecallResponse` 中的 `clustered?: ClusteredRecallResult` 字段

  **Must NOT do**:
  - 不破坏 recall 功能本身
  - 不删除核心的 shouldRecall / autoRecall 逻辑

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 11)
  - **Blocks**: F1
  - **Blocked By**: None

  **References**:
  - `plugins/opencode/src/hooks.ts` — line 294, 302-336, 347, 477, 503-517, 586-619
  - `plugins/opencode/src/client.ts` — line 46, 55, 75

  **QA Scenarios**:
  ```
  Scenario: 插件无 cluster 引用
    Tool: Bash
    Steps:
      1. grep -i "cluster" plugins/opencode/src/hooks.ts → 0 matches
      2. grep -i "cluster" plugins/opencode/src/client.ts → 0 matches
    Expected Result: opencode 插件完全无 cluster 引用
    Evidence: .omo/evidence/task-10-plugin-cleaned.txt
  ```

- [ ] 11. 清理其他散落引用

  **What to do**:
  - **api/handlers/lifecycle.rs（灵犀审查发现遗漏！12处引用）**:
    - 删除 `use crate::cluster::manager::ClusterManager;`（line 9）
    - 删除 `cleanup_orphan_clusters(&state, &removed).await;` 调用（line 56）
    - 删除 `state.cluster_store.optimize().await` 调用（line 65）
    - 删除整个 `cleanup_orphan_clusters` 函数定义（line 117-146，约30行）
  - **api/handlers/session_recalls.rs（12处引用）**:
    - 修改 `ShouldRecallResponse` struct：删除 `clustered` 字段（line 113）
    - 删除所有 `clustered: None` 赋值（line 174, 199, 225, 517）
    - 删除 ClusterAggregator 创建和调用（line 521-530，整个 let clustered = {...} 块）
    - 删除返回值中的 `clustered,`（line 553）
  - **api/handlers/scheduler.rs（9处引用）**:
    - 删除 `get_scheduler_status` 中的 clustering 状态字段（如 `clustering_paused`, `clustering_running`）
    - Task 5 已删除 `pause_clustering`/`resume_clustering` 函数，此处补充 status 查询清理
  - **api/handlers/stats.rs（4处引用）**:
    - 删除测试辅助函数中的 `cluster_llm: NoopLlm` 和 `cluster_store` 创建（line 837-846）
  - **ingest/prompts.rs（4处引用）**:
    - 删除 `CLUSTER_SUMMARY_SYSTEM_PROMPT` 常量（line 550）
    - 删除 `CLUSTER_INITIAL_SUMMARY_SYSTEM_PROMPT` 常量（line 577）
    - 删除 `build_cluster_summary_prompt()` 函数（line 600）
    - 删除 `build_cluster_initial_summary_prompt()` 函数（line 627）
  - **api/handlers/sharing.rs（2处引用）**:
    - `cluster_id: None, is_cluster_anchor: false`（line 253-254）→ **保留**（Memory 字段赋值，数据兼容）
  - **api/scheduler_control.rs（15处引用）**:
    - 删除 `clustering_paused`, `clustering_running`, `clustering_notify` 字段
    - 删除 `is_clustering_paused()`, `pause_clustering()`, `resume_clustering()`, `set_clustering_running()` 方法
  - **store/lancedb.rs（42处引用）**:
    - **删除**纯 cluster 操作方法（只被 cluster 模块调用）：
      - `update_memory_cluster_id()`（line 1806-1825）
      - `batch_update_cluster_ids()`（line 1830-1880）
      - `clear_all_cluster_ids()`（line 1887-1896）
      - `list_by_cluster_id()`（line 1968-1979）
    - **保留** cluster_id 序列化/反序列化代码（数据读写兼容）：
      - btree_cols 中的 "cluster_id"（line 421）
      - Field::new("cluster_id", ...) schema 定义（line 596, 3496）
      - 序列化 cluster_id 值（line 1211-1212, 1795-1796）
      - 反序列化 cluster_id 值（line 1402-1403）
      - is_cluster_anchor 序列化（line 1212, 1796）
  - 注意：lancedb.rs 中的 cluster_id 列定义和序列化代码**保留**（读兼容）

  **Must NOT do**:
  - 不删除 LanceStore 中 Memory 的 cluster_id/is_cluster_anchor 字段序列化（读兼容）
  - 不破坏已有的搜索/召回功能
  - 不删除 sharing.rs 中的 cluster_id: None 赋值

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 10)
  - **Blocks**: F1
  - **Blocked By**: Tasks 1, 2, 4

  **References**:
  - `omem-server/src/api/handlers/lifecycle.rs` — line 9, 56, 65, 117-146
  - `omem-server/src/api/handlers/session_recalls.rs` — line 113, 174, 199, 225, 517, 521-530, 553
  - `omem-server/src/api/handlers/scheduler.rs` — clustering status 字段
  - `omem-server/src/api/handlers/stats.rs` — line 837-846
  - `omem-server/src/ingest/prompts.rs` — line 550, 577, 600, 627
  - `omem-server/src/api/handlers/sharing.rs` — line 253-254（保留）
  - `omem-server/src/api/scheduler_control.rs` — 15处 clustering 字段和方法
  - `omem-server/src/store/lancedb.rs` — line 1806-1896, 1968-1979（删除）；line 421, 596, 1211-1212, 1402-1403, 1795-1796, 3496（保留）

  **QA Scenarios**:
  ```
  Scenario: 散落引用已清理
    Tool: Bash
    Steps:
      1. grep -riP "ClusterStore|ClusterManager|ClusterAssigner|ClusterAggregator|ClusteredResult|BackgroundClusterer|cluster_llm|create_cluster_llm|clustering_paused|clustering_running|cleanup_orphan_clusters|build_cluster_summary|CLUSTER_SUMMARY" omem-server/src/ → 0 matches
    Expected Result: 除了 Memory 字段 cluster_id/is_cluster_anchor 的序列化代码外，无任何 cluster 模块引用
    Evidence: .omo/evidence/task-11-misc-cleaned.txt
  ```

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

- [ ] F1. **编译 + 测试 + Lint**
  执行 `cargo build`、`cargo test`、`cargo clippy`，确认全部通过。检查是否有残留 cluster 引用。
  Output: `Build [PASS/FAIL] | Tests [N pass/N fail] | Clippy [PASS/FAIL] | Cluster refs [CLEAN/N remaining]`

- [ ] F2. **部署验证**
  cargo build --release，SCP 到服务器，kill 旧进程 + 启动新进程，health check。
  确认启动日志中无 cluster 相关初始化。
  Output: `MD5 [hash] | Health [OK/FAIL] | Cluster init [NONE/FOUND]`

---

## Commit Strategy

- **Single commit**: `refactor: remove clustering module — no longer needed after refine feature`
- **Files**: 全部修改文件
- **Pre-commit**: `cargo build && cargo test`

---

## Success Criteria

### Verification Commands
```bash
# 1. 编译通过
cargo build  # Expected: Finished

# 2. 测试通过
cargo test   # Expected: all tests pass

# 3. 无 cluster 模块引用（除 Memory 字段序列化）
grep -ri "cluster_store\|ClusterStore\|ClusterManager\|ClusterAssigner\|BackgroundClusterer\|cluster_llm\|create_cluster_llm" omem-server/src/  # Expected: 0 matches

# 4. cluster/ 目录已删除
ls omem-server/src/cluster/  # Expected: No such file or directory

# 5. 无 cluster API 路由
grep "cluster" omem-server/src/api/router.rs  # Expected: 0 matches
```

### Final Checklist
- [ ] cluster/ 目录已删除
- [ ] clusters.rs handler 已删除
- [ ] domain/cluster.rs 已删除
- [ ] AppState 无 cluster_llm/cluster_store
- [ ] config.rs 无 OMEM_CLUSTER_* 配置
- [ ] session_ingest 无 cluster assignment
- [ ] lifecycle scheduler 无 clustering 逻辑
- [ ] router.rs 无 cluster 路由
- [ ] plugins/opencode 无 cluster 引用
- [ ] Memory struct 的 cluster_id/is_cluster_anchor 保留（数据兼容）
- [ ] cargo build + test + clippy 全部通过
