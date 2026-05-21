# OMEM 记忆召回 Context 预算优化

## TL;DR

> **Quick Summary**: 优化 OpenCode plugin 的记忆召回注入机制，引入 token 预算系统防止 context 窗口溢出。核心问题：maxContentLength 500→3000 后，10 条记忆 × 3000 字符 = 30K 字符注入，导致 MAX CONTEXT LIMIT 警告。
>
> **设计原则**：站在 AI 使用者（被注入方）角度设计 — 召回的内容必须**对题、有用、完整**。少量精准 > 大量模糊。
> 
> **Deliverables**:
> - hooks.ts: 预算感知的动态内容截断 + 句子边界感知截断
> - config.ts: 激活已存在的 maxContentChars 字段，统一 similarityThreshold 默认值
> - client.ts: 无 API 变更（仅类型对齐）
> 
> **Estimated Effort**: Short（4-6 tasks，2-3 波次）
> **Parallel Execution**: YES - 2 waves
> **Critical Path**: Task 1 (config) → Task 2 (truncate) → Task 3 (budget engine) → Task 4 (hooks integration)

---

## Context

### Original Request
师尊要求优化 omem 记忆召回系统，maxContentLength 从 500 调整到 3000 后触发 MAX CONTEXT LIMIT 警告。要求月儿自己做决策，不问问题。

### Interview Summary
**Key Discussions**:
- 确认 plugin-side only（不动 Rust 服务端）
- 确认 char-based budget 近似（4 char ≈ 1 token）
- 确认不做 semantic dedup，用 tag/category overlap 替代

**Research Findings**:
- `maxContentChars: 30000` 已存在于 config.ts 但从未使用 — 恰好是预算基础
- `similarityThreshold` 双重默认值 bug：config.ts=0.4 vs hooks.ts=0.6
- Profile JSON 使用 2 空格缩进浪费 ~30% 字符
- `truncate()` 做简单 slice 可能在句子/代码/CJK 中间截断
- `MemoryDto.l2_content` 字段可能造成双重注入
- `buildClusteredContextBlock` 和 `buildContextBlock` 两条路径都需优化

### Metis Review
**Identified Gaps** (addressed):
- `maxContentChars` 死字段 → 激活为预算基础
- 双重默认值 → 统一为 config.ts 的 0.4
- Profile 空格浪费 → 改为紧凑 JSON
- 截断质量问题 → 句子边界感知
- l2_content 双重注入 → 需确认并处理
- 两条路径都需优化 → 统一预算引擎

---

## Work Objectives

### Core Objective
引入 token 预算系统，确保记忆召回注入的总字符数不超过 `maxContentChars`（默认 30000），同时提升单条记忆的截断质量。

### Concrete Deliverables
- `plugins/opencode/src/config.ts`: 激活 maxContentChars，统一 similarityThreshold 默认值
- `plugins/opencode/src/hooks.ts`: 预算感知的动态内容分配 + 句子边界截断
- 零服务端变更，零新依赖，零 breaking change

### Definition of Done
- [ ] 任何 recall 场景下，profile + context wrapper 总字符数 ≤ maxContentChars (30000)
- [ ] similarityThreshold 只有一个默认值来源
- [ ] 截断不切断句子中间（英文句号/中文句号/换行边界）
- [ ] 现有用户无感升级（默认值向后兼容）

### Must Have
- 动态 maxContentLength：`min(maxContentLength, floor(budgetRemaining / N))`
- 句子边界感知截断（sentence-boundary aware truncation）
- Profile JSON 紧凑序列化
- **similarityThreshold 从 config.json 统一读取**（师尊配了 0.3，代码不能有硬编码 fallback）
- 预算使用日志（debug 级别）
- l2_content 字段处理
- ~~Profile web 可见性~~（师尊决定：拆分为独立计划，不纳入本次 scope）

### Must NOT Have (Guardrails)
- ❌ 不改 Rust 服务端代码（recall 相关）
- ❌ 不引入新依赖（纯 TypeScript）
- ❌ 不改 API 契约（client.ts 接口不变）
- ❌ 不做 semantic dedup（太复杂）
- ❌ 不做 LLM 摘要（只做截断优化）
- ❌ 不改 toast 逻辑（toast 已正常工作）
- ❌ 不改 agent policy 逻辑
- ❌ 不改 tools.ts 的 memory_store
- ❌ 不引入 tiktoken（char-based budget 即可）
- ❌ 不添加新 API 端点（recall 相关）

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: NO（plugin 无独立测试框架）
- **Automated tests**: NO（plugin 侧无测试基础设施）
- **Framework**: none
- **Verification**: Agent-executed QA scenarios only

### QA Policy
Every task includes agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Plugin TypeScript**: Use Bash (npx tsc --noEmit) — type check
- **Logic verification**: Use Bash (node -e) — unit-style inline tests
- **Integration**: Use Bash (npm run build) — build verification

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - foundation):
├── Task 1: Config 统一 + 激活 maxContentChars [quick]
├── Task 2: 句子边界感知截断函数 [quick]
└── Task 3: l2_content 字段调查 + 处理策略 [quick]

Wave 2 (After Wave 1 - core integration):
├── Task 4: 预算感知的 hooks.ts 集成 (depends: 1, 2, 3) [deep]
├── Task 5: Profile JSON 压缩 + 预算日志 (depends: 1, 4) [quick]
└── Task 6: Clustered 路径预算适配 (depends: 4) [quick]

Wave FINAL (After ALL tasks):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Build + type-check verification (unspecified-high)
└── Task F4: Scope fidelity check (deep)
→ Present results → Get user okay

Critical Path: Task 1 → Task 4 → Task 5 → FINAL
Parallel Speedup: ~40% faster than sequential
Max Concurrent: 3 (Wave 1)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1 | - | 4, 5 | 1 |
| 2 | - | 4 | 1 |
| 3 | - | 4 | 1 |
| 4 | 1, 2, 3 | 5, 6 | 2 |
| 5 | 1, 4 | FINAL | 2 |
| 6 | 4 | FINAL | 2 |

### Agent Dispatch Summary

- **Wave 1**: **3** — T1 → `quick`, T2 → `quick`, T3 → `quick`
- **Wave 2**: **3** — T4 → `deep`, T5 → `quick`, T6 → `quick`
- **FINAL**: **4** — F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

- [ ] 1. Config 统一 + 激活 maxContentChars

  **What to do**:
  - 在 `config.ts` 中确认 `maxContentChars: 30000` 已存在（它是死字段，从未被 hooks.ts 使用）
  - **similarityThreshold 统一**：师尊反馈 config.json 中配置了 `"recall": { "similarityThreshold": 0.3, "maxRecallResults": 1 }`
    - 确认 plugin 加载 config.json 后是否正确传递 similarityThreshold 到 hooks.ts
    - hooks.ts L233 的 fallback `0.6` 必须删除或改为从 config 读取的值
    - **原则：不论哪个代码文件，统一使用 config.json 中的配置值，不允许硬编码 fallback**
  - 确保 `maxContentChars` 作为新字段被导出到 config 接口，供 hooks.ts 使用
  - 在 config.ts 的 `content` 块中添加注释说明 `maxContentChars` 的用途（总注入字符预算）

  **Must NOT do**:
  - 不硬编码 maxContentLength — 从 config.json 读取（师尊配了 `"content": { "maxContentLength": 3000 }`）
  - 不硬编码 maxRecallResults — 从 config.json 读取（师尊配了 `"recall": { "maxRecallResults": 1 }`）
  - 不改 toastDelayMs
  - 不改 agent policy 相关 config

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单文件 config 修改，范围明确
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Tasks 4, 5
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/config.ts` — 整个文件，重点看 `content` 配置块，找到 `maxContentChars: 30000` 死字段
  - `plugins/opencode/src/hooks.ts:233` — `similarityThreshold` 的硬编码 fallback `0.6`，需删除，统一从 config 读取

  **Why References Matter**:
  - config.ts: 需要理解完整的 config 结构，确保 maxContentChars 能被正确导出
  - hooks.ts:233: 这是双重默认值 bug 的位置，需要确认改的是正确的 fallback

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Config 字段统一验证
    Tool: Bash (grep)
    Preconditions: config.ts 已修改
    Steps:
      1. grep -n "maxContentChars" plugins/opencode/src/config.ts
      2. grep -n "similarityThreshold" plugins/opencode/src/config.ts
      3. grep -n "0\\.6" plugins/opencode/src/hooks.ts | head -5
    Expected Result: maxContentChars 存在且有注释；hooks.ts 中 similarityThreshold fallback 不再是 0.6
    Evidence: .sisyphus/evidence/task-1-config-unified.txt

  Scenario: 构建不破坏
    Tool: Bash
    Preconditions: config.ts 修改完成
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: 零错误构建成功
    Evidence: .sisyphus/evidence/task-1-build.txt
  ```

  **Commit**: YES (groups with 2, 3)
  - Message: `fix(plugin): unify config defaults and activate maxContentChars`
  - Files: `plugins/opencode/src/config.ts`, `plugins/opencode/src/hooks.ts`

- [ ] 2. 句子边界感知截断函数

  **What to do**:
  - 在 `hooks.ts` 中替换现有的 `truncate(text, max)` 函数（当前是简单 `text.slice(0, max)`）
  - 新函数 `truncateAtBoundary(text: string, maxLength: number): string`:
    1. 如果 text.length ≤ maxLength，直接返回
    2. 在 `maxLength` 位置向前查找最近的句子边界：
       - 英文句号 `.`、中文句号 `。`、感叹号 `!`、问号 `?`
       - 换行符 `\n`
       - 分号 `;`、中文分号 `；`
    3. 在找到的边界处截断（包含边界字符）
    4. 如果 100 字符内找不到边界，退回到 maxLength 处截断
    5. 截断后追加 `...` 省略号
  - 保持函数签名兼容（都是 string → string）

  **Must NOT do**:
  - 不引入外部库做 NLP 句子分割
  - 不做 LLM 摘要
  - 不改变函数的调用方式

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单函数替换，逻辑清晰
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: Task 4
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/hooks.ts:truncate()` — 找到现有 truncate 函数的位置和实现

  **Why References Matter**:
  - 需要知道 truncate 函数的确切位置和当前实现，以便替换

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 英文句子边界截断
    Tool: Bash (node -e)
    Steps:
      1. 测试 truncateAtBoundary("Hello world. This is a test. More text here.", 20)
      2. 断言结果在第一个句号后截断
    Expected Result: "Hello world." + "..." (13 chars)
    Evidence: .sisyphus/evidence/task-2-en-boundary.txt

  Scenario: 中文句子边界截断
    Tool: Bash (node -e)
    Steps:
      1. 测试 truncateAtBoundary("这是第一句话。这是第二句话。这是第三句话。", 15)
    Expected Result: 在第一个中文句号后截断
    Evidence: .sisyphus/evidence/task-2-cn-boundary.txt

  Scenario: 无边界时回退
    Tool: Bash (node -e)
    Steps:
      1. 测试 truncateAtBoundary("abcdefghijklmnopqrstuvwxyz", 10)
    Expected Result: "abcdefghij..." (在 maxLength 处截断，无边界可找)
    Evidence: .sisyphus/evidence/task-2-no-boundary.txt

  Scenario: 短文本不截断
    Tool: Bash (node -e)
    Steps:
      1. 测试 truncateAtBoundary("Short text", 100)
    Expected Result: "Short text"（原样返回，无省略号）
    Evidence: .sisyphus/evidence/task-2-no-truncate.txt
  ```

  **Commit**: YES (groups with 1, 3)
  - Message: `fix(plugin): unify config defaults and activate maxContentChars`
  - Files: `plugins/opencode/src/hooks.ts`

- [ ] 3. l2_content 字段调查 + 处理策略

  **What to do**:
  - 在 `plugins/opencode/src/client.ts` 中查找 `MemoryDto` 类型定义
  - 确认 `l2_content` 字段是否存在、是否可选
  - 在 `hooks.ts` 的 `buildContextBlock` 和 `buildClusteredContextBlock` 中查找 content 的使用
  - 如果 `memory.content` 包含了 l2_content（服务端已合并），则无需额外处理
  - 如果 `memory.l2_content` 是独立字段且也被注入 context，则需要在预算计算中考虑
  - 在 buildContextBlock 的 truncation 点，确认只截断 `memory.content` 而非双重注入

  **Must NOT do**:
  - 不改服务端返回格式
  - 不添加新字段到 MemoryDto
  - 如果 l2_content 未被使用，不做任何代码变更（仅记录发现）

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 主要是调查确认，可能零代码变更
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: Task 4
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/client.ts` — MemoryDto 类型定义，查找 l2_content 字段
  - `plugins/opencode/src/hooks.ts:buildContextBlock` (L168-192) — 看 content 如何被使用
  - `plugins/opencode/src/hooks.ts:buildClusteredContextBlock` (L194-230) — 同上

  **Why References Matter**:
  - client.ts: 确认 MemoryDto 的完整类型定义
  - hooks.ts 两个 builder: 确认 content 注入路径是否有 l2_content 参与

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: l2_content 字段调查结果
    Tool: Bash (grep)
    Steps:
      1. grep -n "l2_content" plugins/opencode/src/client.ts
      2. grep -n "l2_content" plugins/opencode/src/hooks.ts
      3. grep -rn "l2_content" plugins/opencode/src/
    Expected Result: 明确 l2_content 是否存在于类型定义中，是否在 hooks.ts 中被使用
    Evidence: .sisyphus/evidence/task-3-l2-content-survey.txt

  Scenario: 如果 l2_content 被使用，确认不影响预算
    Tool: Bash (grep)
    Steps:
      1. 在 hooks.ts 中确认 content 注入只使用 memory.content（不含 l2_content）
    Expected Result: 无双重注入，或已标记为需要预算内处理
    Evidence: .sisyphus/evidence/task-3-no-double-inject.txt
  ```

  **Commit**: YES (groups with 1, 2) or NO (if no code changes needed)
  - Message: `docs(plugin): document l2_content field status` (if no changes)
  - Files: `plugins/opencode/src/hooks.ts` (if changes needed)

- [ ] 4. 预算感知的 hooks.ts 集成（核心任务）

  **What to do**:
  - 在 `hooks.ts` 的 `autoRecallHook` 函数中实现预算引擎：
    1. 从 config 读取 `maxContentChars`（默认 30000）
    2. 在注入前计算总预算：
       - `totalBudget = maxContentChars`
       - `profileChars = profile ? JSON.stringify(profile).length : 0`（紧凑序列化）
       - `wrapperOverhead = "<cerebro-context>...</cerebro-context>".length + category labels ≈ 500 chars`
       - `contentBudget = totalBudget - profileChars - wrapperOverhead`
    3. 对召回的 memories 应用动态 maxContentLength：
       - `N = memories.length`
       - `dynamicMax = Math.min(maxContentLength, Math.floor(contentBudget / N))`
       - 每条 memory 使用 `truncateAtBoundary(memory.content, dynamicMax)` 截断
    4. 用 `truncateAtBoundary` 替换所有 `truncate` 调用
    5. 在注入后记录预算使用日志：`logger.debug("Recall budget", { total: totalBudget, used: actualUsed, profileChars, N, dynamicMax })`
  - 修改 `buildContextBlock` (L168-192)：
    - 接受 `contentBudget` 参数
    - 对每个 category 内的 memory 使用动态 maxContentLength
    - 用 `truncateAtBoundary` 替换 `truncate`
  - 修改 `buildClusteredContextBlock` (L194-230)：
    - 同样接受 `contentBudget` 参数
    - cluster summaries + key memories 都纳入预算
    - 用 `truncateAtBoundary` 替换 `truncate`
  - 在 `autoRecallHook` 中调用 `shouldRecall` 后、构建 context block 前插入预算计算逻辑

  **Must NOT do**:
  - 不改 `shouldRecall` API 调用
  - 不改 `resolveAgentPolicy()` 逻辑
  - 不改 dedup 逻辑（`injectedMemoryIds`）
  - 不改 session recall 记录逻辑
  - 不添加新 API 端点
  - 不做 semantic dedup

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 核心逻辑变更，涉及多个函数的参数变更和预算计算
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (sequential after Wave 1)
  - **Blocks**: Tasks 5, 6
  - **Blocked By**: Tasks 1, 2, 3

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/hooks.ts:232-384` — autoRecallHook 完整流程，理解预算插入点
  - `plugins/opencode/src/hooks.ts:168-192` — buildContextBlock 当前实现
  - `plugins/opencode/src/hooks.ts:194-230` — buildClusteredContextBlock 当前实现
  - `plugins/opencode/src/hooks.ts:truncate()` — 需要替换为 truncateAtBoundary

  **API/Type References**:
  - `plugins/opencode/src/config.ts` — maxContentChars 字段（Task 1 激活后）

  **Why References Matter**:
  - autoRecallHook: 预算逻辑需要在 shouldRecall 之后、context block 构建之前插入
  - 两个 builder: 需要添加 contentBudget 参数并替换 truncate
  - config.ts: 读取 maxContentChars 的来源

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 预算限制正常工作
    Tool: Bash (node -e)
    Preconditions: Task 1-3 已完成
    Steps:
      1. 在 hooks.ts 中找到预算计算代码
      2. 模拟 10 条 memories，每条 3000 chars
      3. 验证 dynamicMax = min(3000, floor(28000/10)) = min(3000, 2800) = 2800
      4. 验证总注入 ≤ 30000 chars
    Expected Result: dynamicMax < maxContentLength，总量受控
    Evidence: .sisyphus/evidence/task-4-budget-limit.txt

  Scenario: 少量 memories 时使用完整 maxContentLength
    Tool: Bash (node -e)
    Steps:
      1. 模拟 3 条 memories
      2. 验证 dynamicMax = min(3000, floor(28000/3)) = min(3000, 9333) = 3000
    Expected Result: 少量 memories 不被过度截断
    Evidence: .sisyphus/evidence/task-4-few-memories.txt

  Scenario: 构建成功
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: 零错误
    Evidence: .sisyphus/evidence/task-4-build.txt
  ```

  **Commit**: YES (groups with 5, 6)
  - Message: `feat(plugin): add token budget system for recall injection`
  - Files: `plugins/opencode/src/hooks.ts`
  - Pre-commit: `cd plugins/opencode && npm run build`

- [ ] 5. Profile JSON 压缩 + 预算日志

  **What to do**:
  - 在 `hooks.ts` 中找到 Profile 注入代码（L266-283 附近）
  - 将 `JSON.stringify(profile, null, 2)` 改为 `JSON.stringify(profile)`（去掉 pretty-print）
  - 确认 Profile 注入只发生在 first message（已有逻辑，不需改）
  - 在预算计算中，Profile 的字符数应从 contentBudget 中扣除
  - 在 autoRecallHook 的末尾添加预算使用日志：
    ```typescript
    logger.debug("Recall budget report", {
      totalBudget: maxContentChars,
      profileChars,
      wrapperOverhead,
      contentBudget,
      memoriesCount: N,
      dynamicMaxPerMemory,
      actualInjected: output.system.map(s => s.length).reduce((a,b) => a+b, 0)
    })
    ```

  **Must NOT do**:
  - 不改 Profile 的数据结构
  - 不改 Profile 的注入时机（first message only）
  - 不删除 Profile 功能

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 小改动，JSON.stringify 参数变更 + 日志添加
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 4)
  - **Parallel Group**: Wave 2 (after Task 4)
  - **Blocks**: FINAL
  - **Blocked By**: Tasks 1, 4

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/hooks.ts:266-283` — Profile 注入代码，找到 JSON.stringify 调用
  - `plugins/opencode/src/logger.ts` — 了解 logger 使用方式

  **Why References Matter**:
  - hooks.ts:266-283: Profile 注入点，需改为紧凑序列化
  - logger.ts: 日志 API 使用方式

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Profile JSON 紧凑化
    Tool: Bash (grep)
    Steps:
      1. grep -n "JSON.stringify(profile" plugins/opencode/src/hooks.ts
    Expected Result: 不包含 "null, 2" 参数
    Evidence: .sisyphus/evidence/task-5-compact-profile.txt

  Scenario: 预算日志存在
    Tool: Bash (grep)
    Steps:
      1. grep -n "Recall budget report" plugins/opencode/src/hooks.ts
    Expected Result: 找到 debug 日志代码
    Evidence: .sisyphus/evidence/task-5-budget-log.txt

  Scenario: 构建成功
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: 零错误
    Evidence: .sisyphus/evidence/task-5-build.txt
  ```

  **Commit**: YES (groups with 4, 6)
  - Message: `feat(plugin): add token budget system for recall injection`
  - Files: `plugins/opencode/src/hooks.ts`

- [ ] 6. Clustered 路径预算适配

  **What to do**:
  - 在 `buildClusteredContextBlock` (L194-230) 中确保预算逻辑正确：
    - Cluster summaries 占用预算的一部分
    - Key memories 从剩余预算中分配
    - Standalone memories 使用相同的动态分配
  - 预算分配策略（建议）：
    - `clusterSummaryBudget = contentBudget * 0.3`（30% 给 cluster summaries）
    - `memoryBudget = contentBudget * 0.7`（70% 给 key memories + standalone）
    - 每个 cluster summary 使用 `truncateAtBoundary(summary, Math.floor(clusterSummaryBudget / clusterCount))`
    - 每个 key memory / standalone memory 使用动态 maxContentLength
  - 确保与 `buildContextBlock` 的预算逻辑一致

  **Must NOT do**:
  - 不改 cluster 数据结构
  - 不改服务端 clustered 返回格式
  - 不做 LLM 摘要

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 在 Task 4 基础上的适配，逻辑类似
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 4)
  - **Parallel Group**: Wave 2 (after Task 4, can run with Task 5)
  - **Blocks**: FINAL
  - **Blocked By**: Task 4

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/hooks.ts:194-230` — buildClusteredContextBlock 完整实现
  - Task 4 的预算引擎代码（同一文件）

  **Why References Matter**:
  - buildClusteredContextBlock: 需要理解 cluster summaries + key memories + standalone 的结构
  - Task 4 的预算代码: 保持一致的预算计算方式

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Clustered 路径预算验证
    Tool: Bash (node -e)
    Steps:
      1. 模拟 3 个 clusters，每个有 2-3 个 key memories + 5 个 standalone
      2. 验证总注入 ≤ maxContentChars
      3. 验证 cluster summaries 和 memories 都被正确截断
    Expected Result: 所有内容都在预算内
    Evidence: .sisyphus/evidence/task-6-clustered-budget.txt

  Scenario: 构建成功
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: 零错误
    Evidence: .sisyphus/evidence/task-6-build.txt
  ```

  **Commit**: YES (groups with 4, 5)
  - Message: `feat(plugin): add token budget system for recall injection`
  - Files: `plugins/opencode/src/hooks.ts`

---

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, check code). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Review all changed files for: `as any`/`@ts-ignore`, empty catches, console.log in prod, unused imports, excessive comments, over-abstraction. Check AI slop patterns.
  Output: `Type-check [PASS/FAIL] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Build + Type-Check Verification** — `unspecified-high`
  Run `cd plugins/opencode && npm run build`. Verify zero errors. Run `npx tsc --noEmit` if available. Verify all exports are intact.
  Output: `Build [PASS/FAIL] | Type-check [PASS/FAIL] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git log/diff). Verify 1:1 — everything in spec was built (no missing), nothing beyond spec was built (no creep). Check "Must NOT do" compliance.
  Output: `Tasks [N/N compliant] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Task 1-3**: `fix(plugin): unify config defaults and add sentence-aware truncation` - config.ts, hooks.ts (truncate function)
- **Task 4-6**: `feat(plugin): add token budget system for recall injection` - hooks.ts, config.ts
- Pre-commit: `cd plugins/opencode && npm run build`

---

## Success Criteria

### Verification Commands
```bash
cd plugins/opencode && npm run build           # Expected: successful build, zero errors
grep -n "maxContentChars" src/hooks.ts         # Expected: found (was unused, now used)
grep -n "similarityThreshold" src/hooks.ts     # Expected: single default source
grep -n "JSON.stringify(profile" src/hooks.ts  # Expected: no "null, 2" (compact JSON)
```

### Final Checklist
- [ ] Total injection ≤ maxContentChars (30000) in all scenarios
- [ ] similarityThreshold unified to config.json single source (no hardcoded fallbacks)
- [ ] Sentence-boundary aware truncation implemented
- [ ] Profile JSON uses compact serialization
- [ ] l2_content handled (skipped or budgeted)
- [ ] Budget logging at debug level
- [ ] Both flat and clustered context paths optimized
- [ ] Zero breaking changes
- [ ] Zero new dependencies
- [ ] Zero server-side changes
