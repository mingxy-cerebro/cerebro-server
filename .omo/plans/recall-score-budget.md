# 召回记忆去截断 + Score 权重预算分配

## TL;DR

> **Quick Summary**: 服务端 balanced 策略去掉 medium 截断，Plugin 端预算从均匀分配改为按 score 权重分配，清理 refine_medium_chars 死代码。
> 
> **Deliverables**:
> - 服务端：pipeline.rs medium 分支不再截断 content，仅标记 refine_relevance
> - 服务端：清理 refine_medium_chars 相关配置项和字段
> - Plugin 端：预算计算改为线性比例权重法（高分给更多 chars）
> - Plugin 端：buildContextBlock/buildClusteredContextBlock 支持逐 item 独立 maxLength
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 2 waves + Final
> **Critical Path**: T1 → T3 → T4 → F1-F4

---

## Context

### Original Request
师尊决定放弃"精炼结果+memory_get回查"方案（从未好使过），改为服务端完整返回 + Plugin 端按 score 权重分配预算。

### Interview Summary
**Key Discussions**:
- 截断发生在服务端 `pipeline.rs:1109-1119`，balanced 策略对 medium 记忆截断到 200 chars
- Plugin 端已有 budget 机制：maxContentChars=30000, maxContentLength=3000（师尊实际配置）
- 师尊选择不改为 loose 策略（保留 LLM refine 过滤 irrelevant 的能力）
- 师尊选择 clustered 模式也做权重分配（ClusterSummary.relevance_score）
- 师尊选择 refine_medium_chars 死代码一并清理
- 测试策略：TDD

**Research Findings**:
- should_recall 默认返回 5 条记忆
- Plugin 端 truncate 函数智能（句子边界 + UTF-16 保护）
- buildContextBlock 接收统一 maxLength，需改造为逐 item 独立
- clustered 模式：ClusterSummary 有 relevance_score，standalone_memories 无 score 用均匀分配

### Metis Review
**Identified Gaps** (addressed):
- maxContentLength=500 硬天花板问题 → 师尊实际配置为 3000，不存在此问题
- buildContextBlock 需改造函数签名 → 纳入计划 T4
- clustered 模式 score 来源 → ClusterSummary.relevance_score + standalone_memories 均匀分配
- refine_medium_chars 死代码 → 师尊选择一并清理，纳入 T2
- l2_content 双重序列化 → Scope OUT，后续优化

### Oracle Phase 1 Verification
**VERDICT: NO-GO → GO** (after resolving 3 blocking issues)
- 测试策略已决定：TDD
- clustered 模式已明确：relevance_score 权重 + standalone 均匀
- refine_medium_chars 已决定：一并清理

---

## Work Objectives

### Core Objective
服务端 balanced 策略去掉 medium 截断（medium 记忆完整返回 content），Plugin 端预算从均匀分配改为按 score 线性比例权重分配，清理 refine_medium_chars 死代码。

### Concrete Deliverables
- 修改: `omem-server/src/retrieve/pipeline.rs` — medium 分支不截断 content + 删除 medium_chars 参数
- 修改: `omem-server/src/config.rs` — 删除 recall_refine_medium_chars 配置项
- 修改: `omem-server/src/api/handlers/session_recalls.rs` — 删除 refine_medium_chars API 字段
- 修改: `plugins/opencode/src/hooks.ts` — 预算权重分配 + buildContextBlock 改造
- 删除: hooks.ts 中 refineMediumChars 相关代码

### Definition of Done
- [ ] `cargo test -p omem-server` 所有测试通过
- [ ] `cd plugins/opencode && npm run build` 无错误
- [ ] balanced 策略下 medium 记忆 content 完整返回（未被截断或替换）
- [ ] Plugin 端按 score 权重分配 chars（高分记忆 maxLength > 低分记忆）
- [ ] refine_medium_chars 相关代码和配置已清除

### Must Have
- balanced 策略下 medium 记忆 content 不截断、不替换
- Plugin 端线性比例权重法：weight = score / sum(scores), maxLength = clamp(weight × budget, MIN, maxContentLength)
- buildContextBlock/buildClusteredContextBlock 支持逐 item 独立 maxLength
- clustered 模式：ClusterSummary 用 relevance_score 权重，key_memories 共享簇 maxLength，standalone_memories 均匀分配
- refine_medium_chars 配置项、API 字段、pipeline 参数一并清理
- TDD：先写测试再写实现

### Must NOT Have (Guardrails)
- ❌ 不改 loose/strict 策略的行为
- ❌ 不新增配置项或环境变量
- ❌ 不改 Memory 结构体的序列化行为（l2_content 双重序列化留后续）
- ❌ 不改 openclaw/mcp/claude-code 插件
- ❌ 不碰 profile_v2/ 目录
- ❌ 不在非测试代码中使用 unwrap()/expect()
- ❌ ESM import 必须带 .js 扩展名（Plugin TypeScript）

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test 373 tests) / Plugin 端无测试框架
- **Automated tests**: TDD — 先写测试/改测试，再写实现
- **Framework**: cargo test (Rust) + tsc --noEmit (TypeScript 编译验证) + agent QA

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Rust module**: Use Bash (cargo test) - Run inline tests, check compilation
- **TypeScript Plugin**: Use Bash (tsc --noEmit) - Verify compilation
- **Integration**: Use Bash (cargo test + tsc --noEmit) - Verify no regressions

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - Rust service-side changes):
├── Task 1: pipeline.rs 去掉 medium 截断 + 删除 medium_chars 参数 [unspecified-high]
├── Task 2: config.rs + session_recalls.rs 清理 refine_medium_chars [quick]
└── Task 3: pipeline.rs TDD 补充测试 [deep]

Wave 2 (After Wave 1 - Plugin budget changes):
├── Task 4: hooks.ts 预算权重分配 + buildContextBlock 改造 [deep]
├── Task 5: hooks.ts clustered 模式权重分配 [quick]
└── Task 6: hooks.ts TDD 补充测试 [deep]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: T1 → T3 → T4 → T6 → F1-F4
Parallel Speedup: ~50% faster than sequential
Max Concurrent: 3 (Wave 1)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| T1   | - | T3 | 1 |
| T2   | - | - | 1 |
| T3   | T1 | T4 | 1 |
| T4   | T3 | T5, T6 | 2 |
| T5   | T4 | T6 | 2 |
| T6   | T4, T5 | F1-F4 | 2 |
| F1-F4 | ALL | - | FINAL |

### Agent Dispatch Summary

- **Wave 1**: 3 tasks — T1→`unspecified-high`, T2→`quick`, T3→`deep`
- **Wave 2**: 3 tasks — T4→`deep`, T5→`quick`, T6→`deep`
- **FINAL**: 4 tasks — F1→`oracle`, F2→`unspecified-high`, F3→`unspecified-high`, F4→`deep`

---

## TODOs

- [ ] 1. pipeline.rs 去掉 medium 截断 + 删除 medium_chars 参数

  **What to do**:
  - 修改 `omem-server/src/retrieve/pipeline.rs`:
    - `stage_llm_refine` 函数签名：删除 `medium_chars: usize` 参数（L955）
    - `stage_llm_refine` 函数体：删除 medium 分支的截断逻辑（L1104-1121），只保留 `refine_relevance` 标记
    - 具体改动 L1104-1121：
      ```rust
      // 改前：
      if relevance == "medium" {
          if refine_strategy == "strict" {
              r.memory.content.clear();
          } else {
              r.memory.content = if !r.memory.l1_overview.is_empty() {
                  std::mem::take(&mut r.memory.l1_overview)
              } else {
                  // truncate to medium_chars
              };
          }
      }
      // 改后：只标记 relevance，不截断 content
      // medium 分支变为空（或直接移除 if 块，因为上面已统一设置 refine_relevance）
      ```
    - 调用 `stage_llm_refine` 的地方：删除传入的 `medium_chars` 参数（L292 附近）
    - `SearchOverrides` struct：删除 `refine_medium_chars` 字段（L25）
    - 删除 `let refine_medium_chars = ...` 变量声明（L205）
  - **TDD**：先写测试验证 medium 记忆的 content 不被截断

  **Must NOT do**:
  - 不改 `loose`/`strict` 策略的分支逻辑
  - 不改 `irrelevant` 过滤行为
  - 不删 `refine_strategy` 参数本身（仍然需要区分 balanced/strict/loose）

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: pipeline.rs 是检索核心（1833行），stage_llm_refine 逻辑复杂需要精准修改
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2)
  - **Blocks**: T3
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `omem-server/src/retrieve/pipeline.rs:1097-1123` — medium 截断精确位置（核心改动点）
  - `omem-server/src/retrieve/pipeline.rs:948-955` — stage_llm_refine 函数签名（删除 medium_chars 参数）
  - `omem-server/src/retrieve/pipeline.rs:24-25` — SearchOverrides struct（删除 refine_medium_chars 字段）
  - `omem-server/src/retrieve/pipeline.rs:205` — medium_chars 变量声明（删除）
  - `omem-server/src/retrieve/pipeline.rs:292` — stage_llm_refine 调用点（删除 medium_chars 传参）

  **WHY Each Reference Matters**:
  - L1097-1123: medium 截断的完整逻辑，是本次核心改动
  - L948-955: 函数签名变更影响所有调用方
  - L24-25: SearchOverrides 是跨层参数传递，删除字段需同步清理

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: balanced 策略下 medium 记忆 content 不截断
    Tool: Bash (cargo test)
    Preconditions: pipeline.rs 已修改，测试已编写
    Steps:
      1. cargo test -p omem-server -- stage_llm_refine
      2. 验证测试中 medium 记忆的 content 保持原始长度
    Expected Result: 测试通过，medium content 未被截断或替换
    Failure Indicators: content 长度被截断到 200 或被 l1_overview 替换
    Evidence: .omo/evidence/task-1-medium-no-truncate.txt

  Scenario: strict 策略行为不变
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server
    Expected Result: 所有现有测试通过，strict 模式仍丢弃 medium
    Evidence: .omo/evidence/task-1-strict-unchanged.txt

  Scenario: loose 策略行为不变
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server
    Expected Result: loose 模式跳过 LLM refine，不截断
    Evidence: .omo/evidence/task-1-loose-unchanged.txt

  Scenario: 编译通过
    Tool: Bash (cargo check)
    Steps:
      1. cargo check -p omem-server
    Expected Result: "Finished" 无 error
    Evidence: .omo/evidence/task-1-compile.txt
  ```

  **Commit**: YES (groups with T2)
  - Message: `refactor(recall): remove medium truncation and clean up refine_medium_chars`
  - Files: `omem-server/src/retrieve/pipeline.rs`
  - Pre-commit: `cargo test -p omem-server`

- [ ] 2. config.rs + session_recalls.rs 清理 refine_medium_chars

  **What to do**:
  - 修改 `omem-server/src/config.rs`:
    - 删除 `pub recall_refine_medium_chars: usize` 字段（L107）
    - 删除默认值 `recall_refine_medium_chars: 200`（L186）
    - 删除环境变量读取 `OMEM_RECALL_REFINE_MEDIUM_CHARS`（L370-375）
  - 修改 `omem-server/src/api/handlers/session_recalls.rs`:
    - 删除 `ShouldRecallRequest` 中的 `pub refine_medium_chars: Option<usize>` 字段（L73）
    - 删除 overrides 构建中的 `refine_medium_chars` 赋值（L340 附近）
  - **TDD**：先更新受影响的测试

  **Must NOT do**:
  - 不删 `recall_refine_strategy` 配置项（仍需要 balanced/strict/loose 控制）
  - 不改其他 refine 相关配置（refine_timeout_secs、llm_max_eval 等）

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 纯配置字段删除，无业务逻辑变更
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1)
  - **Blocks**: None
  - **Blocked By**: None

  **References**:
  - `omem-server/src/config.rs:107` — `recall_refine_medium_chars` 字段定义
  - `omem-server/src/config.rs:186` — 默认值 200
  - `omem-server/src/config.rs:370-375` — 环境变量读取
  - `omem-server/src/api/handlers/session_recalls.rs:73` — API 请求字段
  - `omem-server/src/api/handlers/session_recalls.rs:340` — overrides 构建

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: refine_medium_chars 完全清除
    Tool: Bash (grep)
    Steps:
      1. grep -r "refine_medium_chars\|recall_refine_medium_chars\|REFINE_MEDIUM_CHARS" omem-server/src/ --include="*.rs"
    Expected Result: 0 匹配
    Evidence: .omo/evidence/task-2-no-medium-chars.txt

  Scenario: 编译通过
    Tool: Bash (cargo check)
    Steps:
      1. cargo check -p omem-server
    Expected Result: "Finished" 无 error
    Evidence: .omo/evidence/task-2-compile.txt

  Scenario: 所有测试通过
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server
    Expected Result: 0 failure
    Evidence: .omo/evidence/task-2-tests.txt
  ```

  **Commit**: YES (groups with T1)
  - Message: `refactor(recall): remove medium truncation and clean up refine_medium_chars`
  - Files: config.rs, session_recalls.rs
  - Pre-commit: `cargo test -p omem-server`

- [ ] 3. pipeline.rs TDD 补充测试

  **What to do**:
  - 在 `omem-server/src/retrieve/pipeline.rs` 的 `#[cfg(test)]` 块中新增测试：
    - 测试 balanced 策略下 medium 记忆 content 保持原样（不被截断、不被 l1_overview 替换）
    - 测试 strict 策略下 medium 记忆仍被丢弃（行为不变）
    - 测试 loose 策略跳过 LLM refine（行为不变）
    - 测试所有 score 相同时权重分配退化为均匀分配
  - 测试应使用 mock LLM 返回 medium/irrelevant/high 标签
  - 参考 pipeline.rs 中已有测试的 mock 模式

  **Must NOT do**:
  - 不改生产代码（本任务只写测试）

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: pipeline.rs 测试需要理解 mock 架构和 stage_llm_refine 的完整流程
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 1 (sequential after T1)
  - **Blocks**: T4
  - **Blocked By**: T1

  **References**:
  - `omem-server/src/retrieve/pipeline.rs` 末尾 — 现有 `#[cfg(test)] mod tests` 块
  - `omem-server/src/retrieve/pipeline.rs:948-955` — stage_llm_refine 函数签名（改后无 medium_chars）

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: balanced medium 不截断测试存在
    Tool: Bash (grep)
    Steps:
      1. grep "medium.*content\|content.*medium" omem-server/src/retrieve/pipeline.rs
    Expected Result: 测试函数中包含 medium content 验证
    Evidence: .omo/evidence/task-3-medium-test.txt

  Scenario: 所有测试通过
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p omem-server
    Expected Result: 0 failure
    Evidence: .omo/evidence/task-3-tests.txt
  ```

  **Commit**: YES (groups with T1, T2)
  - Message: `refactor(recall): remove medium truncation and clean up refine_medium_chars`
  - Files: pipeline.rs (test additions)
  - Pre-commit: `cargo test -p omem-server`

- [ ] 4. hooks.ts 预算权重分配 + buildContextBlock 改造

  **What to do**:
  - 修改 `plugins/opencode/src/hooks.ts`:

    **核心算法改动**（autoRecallHook 中 L503-520 区域）：
    ```typescript
    // 改前：均匀分配
    const dynamicMaxContentLength = Math.min(maxContentLength, Math.max(MIN_ITEM_CONTENT_CHARS, Math.floor(budgetRemaining / itemCount)));

    // 改后：线性比例权重法
    const totalScore = results.reduce((sum, r) => sum + r.score, 0);
    // 每条记忆的 maxLength 在 buildContextBlock 中逐条计算
    ```

    **buildContextBlock 函数改造**（L265-283）：
    - 改函数签名，接收 `SearchResult[]` 和预算参数而非统一 maxLength
    - 逐 item 计算：`weight = score / totalScore`，`maxLength = clamp(weight * budgetRemaining, MIN, maxContentLength)`
    - 保留 truncate 函数不变

    **边界情况处理**：
    - `totalScore === 0`：退化为均匀分配（和现在一样）
    - 单条记忆：直接给 `min(maxContentLength, budgetRemaining)`
    - 所有 score 相同：退化为均匀分配

  - **TDD**：先在 hooks.ts 底部 `#[cfg(test)]` 或单独测试文件中写测试
    - 由于 Plugin 端无测试框架，本任务的 TDD 通过 **tsc 编译 + agent QA 场景** 验证
    - 需验证：高分记忆的 maxLength > 低分记忆的 maxLength
    - 需验证：score 全相同时退化为均匀分配

  **Must NOT do**:
  - 不改 truncate 函数
  - 不改 MIN_ITEM_CONTENT_CHARS / MIN_CONTENT_CHARS 常量
  - 不改 maxContentLength / maxContentChars 默认值
  - 不改 clustered 模式的逻辑（T5 处理）

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: hooks.ts 1200+ 行，预算计算是核心注入逻辑，改造 buildContextBlock 函数签名影响面大
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (sequential, first)
  - **Blocks**: T5, T6
  - **Blocked By**: T3

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/hooks.ts:265-283` — buildContextBlock 函数（需改造签名）
  - `plugins/opencode/src/hooks.ts:503-520` — 预算计算逻辑（核心改动点）
  - `plugins/opencode/src/hooks.ts:208-227` — truncate 函数（保持不变）
  - `plugins/opencode/src/hooks.ts:8-11` — 常量定义（保持不变）

  **API/Type References**:
  - `plugins/opencode/src/client.ts:67` — ShouldRecallResponse 中 memories 有 score 字段
  - `plugins/opencode/src/client.ts:30` — SearchResult 有 score: number

  **WHY Each Reference Matters**:
  - L265-283: buildContextBlock 是消费预算的函数，改造签名才能支持逐 item 独立 maxLength
  - L503-520: 预算计算是权重分配的核心，所有下游依赖这个值
  - client.ts: 需确认 SearchResult 的 score 字段确实存在且可用于权重计算

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: TypeScript 编译通过
    Tool: Bash (tsc)
    Steps:
      1. cd plugins/opencode && npx tsc --noEmit
    Expected Result: 零 error
    Evidence: .omo/evidence/task-4-tsc.txt

  Scenario: 权重分配逻辑存在
    Tool: Bash (grep)
    Steps:
      1. grep "totalScore\|weight.*score" plugins/opencode/src/hooks.ts
    Expected Result: 权重计算代码存在
    Evidence: .omo/evidence/task-4-weight-logic.txt

  Scenario: buildContextBlock 支持逐 item maxLength
    Tool: Bash (grep)
    Steps:
      1. 确认 buildContextBlock 不再接收统一的 maxContentLength 参数
      2. 或确认函数内部逐 item 计算独立 maxLength
    Expected Result: 函数签名或内部逻辑已改造
    Evidence: .omo/evidence/task-4-per-item.txt

  Scenario: score=0 边界保护
    Tool: Bash (grep)
    Steps:
      1. grep "totalScore.*0\|score.*===.*0" plugins/opencode/src/hooks.ts
    Expected Result: 有 totalScore===0 的 fallback 处理
    Evidence: .omo/evidence/task-4-zero-score.txt
  ```

  **Commit**: YES (groups with T5)
  - Message: `feat(plugin): score-weighted budget allocation for memory injection`
  - Files: `plugins/opencode/src/hooks.ts`
  - Pre-commit: `cd plugins/opencode && npx tsc --noEmit`

- [ ] 5. hooks.ts clustered 模式权重分配

  **What to do**:
  - 修改 `plugins/opencode/src/hooks.ts` 中 `buildClusteredContextBlock` 函数（L275-317）：
    - 改函数签名：接收预算参数而非统一 maxLength
    - `cluster_summaries`：用 `relevance_score` 做权重分配
      - `totalClusterScore = sum(cs.relevance_score for cs in cluster_summaries)`
      - 每个 cluster 的 `clusterMaxLen = clamp(cs.relevance_score / totalClusterScore * budget, MIN, maxContentLength)`
      - 簇内 `key_memories` 共享该簇的 `clusterMaxLen`
    - `standalone_memories`：均匀分配或固定 minLength（因为没有 score）
    - 边界：无 cluster_summaries 时 standalone_memories 用全部预算

  **Must NOT do**:
  - 不改 ClusterSummary 接口定义（client.ts）
  - 不改 standalone_memories 的注入格式

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 改造逻辑模式与 T4 相同，只是数据源从 score 改为 relevance_score
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (sequential after T4)
  - **Blocks**: T6
  - **Blocked By**: T4

  **References**:
  - `plugins/opencode/src/hooks.ts:275-317` — buildClusteredContextBlock 完整实现
  - `plugins/opencode/src/client.ts:45-52` — ClusterSummary 接口（有 relevance_score）

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: TypeScript 编译通过
    Tool: Bash (tsc)
    Steps:
      1. cd plugins/opencode && npx tsc --noEmit
    Expected Result: 零 error
    Evidence: .omo/evidence/task-5-tsc.txt

  Scenario: clustered 权重逻辑存在
    Tool: Bash (grep)
    Steps:
      1. grep "relevance_score" plugins/opencode/src/hooks.ts
    Expected Result: buildClusteredContextBlock 中使用 relevance_score 做权重
    Evidence: .omo/evidence/task-5-cluster-weight.txt
  ```

  **Commit**: YES (groups with T4)
  - Message: `feat(plugin): score-weighted budget allocation for memory injection`
  - Files: `plugins/opencode/src/hooks.ts`
  - Pre-commit: `cd plugins/opencode && npx tsc --noEmit`

- [ ] 6. hooks.ts TDD 补充测试（Plugin 端编译验证 + QA）

  **What to do**:
  - 由于 Plugin 端无测试框架，TDD 通过以下方式验证：
    - 确认所有改动后 `npx tsc --noEmit` 编译通过
    - 确认 hooks.ts 中 refineMediumChars 变量已清除（不再是死代码引用）
    - 验证 autoRecallHook 中旧的均匀分配逻辑已完全替换
    - 验证 buildContextBlock 和 buildClusteredContextBlock 的调用处传参正确
  - 在 hooks.ts 中检查：
    - `refineMediumChars` 变量（L329）是否还存在于 autoRecallHook 中（服务端已删除此参数，Plugin 端如果还传就是死代码）
    - 如果 Plugin 端有 `refineMediumChars` 相关代码也一并清理

  **Must NOT do**:
  - 不引入测试框架（如 vitest），本次只用 tsc 编译验证

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要全面审查 hooks.ts 中所有预算相关代码路径的改造完整性
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (sequential after T4, T5)
  - **Blocks**: F1-F4
  - **Blocked By**: T4, T5

  **References**:
  - `plugins/opencode/src/hooks.ts:329` — refineMediumChars 变量（可能需清理）
  - `plugins/opencode/src/hooks.ts:503-520` — 改后的预算计算逻辑（审查完整性）

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: TypeScript 编译通过
    Tool: Bash (tsc)
    Steps:
      1. cd plugins/opencode && npx tsc --noEmit
    Expected Result: 零 error
    Evidence: .omo/evidence/task-6-tsc.txt

  Scenario: refineMediumChars 死代码清理
    Tool: Bash (grep)
    Steps:
      1. grep "refineMediumChars" plugins/opencode/src/hooks.ts
    Expected Result: 0 匹配（或仅注释）
    Evidence: .omo/evidence/task-6-no-refine-medium.txt

  Scenario: npm run build 通过
    Tool: Bash (npm)
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: 零 error
    Evidence: .omo/evidence/task-6-build.txt
  ```

  **Commit**: YES
  - Message: `feat(plugin): score-weighted budget allocation for memory injection`
  - Files: `plugins/opencode/src/hooks.ts`
  - Pre-commit: `cd plugins/opencode && npm run build`

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .omo/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo check` + `cargo clippy` + `cargo test` + `cd plugins/opencode && npm run build`. Review all changed files for: `unwrap()` in non-test code, empty catches, println in prod, commented-out code, unused imports. Check AI slop.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Test edge cases: all scores same, score=0, single memory, many memories. Save to `.omo/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git log/diff). Verify 1:1 — everything in spec was built, nothing beyond spec. Check "Must NOT do" compliance. Detect cross-task contamination. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `refactor(recall): remove medium truncation and clean up refine_medium_chars` - pipeline.rs, config.rs, session_recalls.rs
- **Wave 2**: `feat(plugin): score-weighted budget allocation for memory injection` - hooks.ts
- Pre-commit: `cargo test -p omem-server && cd plugins/opencode && npm run build`

---

## Success Criteria

### Verification Commands
```bash
cargo test -p omem-server                    # Expected: all pass, 0 regressions
cd plugins/opencode && npm run build          # Expected: no errors
grep "medium_chars" omem-server/src/retrieve/pipeline.rs  # Expected: 0 (dead code removed)
grep "refine_medium_chars" omem-server/src/config.rs      # Expected: 0 (config cleaned)
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All existing tests pass (no regressions)
- [ ] Plugin compiles with zero TypeScript errors
- [ ] balanced strategy: medium memories have full content
- [ ] Plugin budget: high-score memories get more chars than low-score
