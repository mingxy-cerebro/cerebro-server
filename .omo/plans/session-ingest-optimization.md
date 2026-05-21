# Session Ingest 质量与效率优化

## TL;DR

> **核心目标**: 优化 session_ingest 管线的去重质量和噪声过滤能力，引入 LLM 智能合并 (replaces tracking) 替代当前的弱去重逻辑 (content-contains + topic name match)，同时增强 prompt 的 VALUE FILTER 指令。
> 
> **产出物**:
> - `SessionTopicSummary` 新增 `replaces` 字段
> - `SESSION_EXTRACT_SYSTEM_PROMPT` 增加 REPLACES TRACKING + VALUE FILTER 指令
> - session_ingest handler 的 merge 逻辑重写为 replaces-driven
> - Batch cluster assignment 替代 N+1 逐条 LLM
> - Session ingest 信号量 + session_locks TTL 清理
> - 完整的 inline tests 覆盖
> 
> **预估工作量**: Medium (2-3天)
> **并行执行**: YES - 3 waves
> **关键路径**: Task 1 → Task 3 → Task 5 → Task 7 → F1-F4

---

## Context

### Original Request
师尊要求基于实际产出样本（bbfe1743, a679c4df）分析 session_ingest 的质量问题并优化。

### Interview Summary
**关键讨论**:
- 两条独立 ingest 路径：session_ingest (memory.rs) vs IngestPipeline (pipeline.rs)，前者完全不使用后者的 NoiseFilter/Reconciler
- 师尊选择方案A (LLM智能合并)，采用 replaces tracking 模式
- 测试策略: TDD（先写测试再实现）
- 不等 Phase 6a，独立优化

**研究发现**:
- session_ingest 的去重是3层弱匹配: content.contains() + topic name ends_with() + session_id match
- SESSION_COMPRESS_SYSTEM_PROMPT 已有 replaces 概念，但 session_ingest 用的是 SESSION_EXTRACT_SYSTEM_PROMPT（不同 prompt）
- 结论：在 extract prompt 中增加 replaces 字段（Option B），不切换到 compress prompt

### Metis Review
**Identified Gaps** (addressed):
- Prompt 不匹配: session_ingest 用 `build_session_extract_prompt_with_memories()` 而非 `build_session_compress_prompt()` → 选择 Option B
- SessionTopicSummary 无 replaces 字段 → 新增
- Lifecycle tags 是新功能 → Phase 1 只加到 prompt，不做差异化降噪
- LLM 调用预算 → 最多2次 LLM 调用

### Oracle P1 Verification
CHECK [5/5] PASS — VERDICT: GO ✅

---

## Work Objectives

### Core Objective
提升 session_ingest 的去重质量和噪声过滤能力，消除重复记忆和噪声存储，同时保持向后兼容。

### Concrete Deliverables
- 修改 `SessionTopicSummary` struct，增加 `replaces` 字段
- 增强 `SESSION_EXTRACT_SYSTEM_PROMPT`，增加 REPLACES TRACKING + VALUE FILTER
- 重写 session_ingest handler 的 EMOTIONAL/WORK/PREFERENCE merge 逻辑
- Batch cluster assignment
- Session ingest 信号量 + session_locks TTL
- 完整 inline tests

### Definition of Done
- [ ] `cargo test -p omem-server` 全部通过
- [ ] `cargo clippy` 无新 warning
- [ ] Session ingest 不再产生重复记忆（相同事件不重复记录）
- [ ] LLM 每次调用 ≤ 2次（extract + cluster batch）
- [ ] 向后兼容：旧记忆无 replaces 字段时，走 CREATE fallback

### Must Have
- replaces tracking: LLM 返回 replaces 数组指示哪些 existing memory 需要更新
- VALUE FILTER: prompt 明确指导什么该记什么该丢
- replaces max length = 5（防止 LLM 合并过多）
- Max 2 LLM calls per session_ingest
- Fallback to current CREATE on merge failure
- Empty replaces = CREATE（不是 SKIP）
- replaces index bounds check

### Must NOT Have (Guardrails)
- 不修改 IngestPipeline (pipeline.rs) — 仅优化 session_ingest 路径
- 不切换到 build_session_compress_prompt() — 保持 extract prompt
- 不实现生命周期标签的差异化降噪 — Phase 1 只加标签
- 不修改 Plugin 端代码
- 不引入新依赖
- 不改变 API 接口（POST /v1/memories/session-ingest 入参/出参不变）

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: YES (TDD)
- **Framework**: cargo test (inline #[cfg(test)] mod tests)
- **TDD**: 每个任务先写测试 → 实现 → 确认通过

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Backend/API**: Use Bash (cargo test) — Run tests, assert pass count
- **Integration**: Use Bash (cargo test specific module) — Verify module-level tests

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - struct + prompt + tests):
├── Task 1: Add replaces field to SessionTopicSummary [quick]
├── Task 2: Enhance SESSION_EXTRACT_SYSTEM_PROMPT [quick]
├── Task 3: Add replaces-driven merge logic [deep]
└── Task 4: Add inline tests for replaces logic [quick]

Wave 2 (After Wave 1 - performance + integration):
├── Task 5: Batch cluster assignment [unspecified-high]
├── Task 6: Session ingest semaphore + session_locks TTL [unspecified-high]
└── Task 7: Integration test for full session_ingest with replaces [deep]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: Task 1 → Task 3 → Task 5 → Task 7 → F1-F4
Parallel Speedup: ~40% faster than sequential
Max Concurrent: 4 (Wave 1)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1 | - | 2, 3, 4 | 1 |
| 2 | - | 3, 4 | 1 |
| 3 | 1, 2 | 5, 7 | 1 |
| 4 | 1, 2 | 7 | 1 |
| 5 | 3 | F1-F4 | 2 |
| 6 | - | F1-F4 | 2 |
| 7 | 3, 4 | F1-F4 | 2 |

### Agent Dispatch Summary

- **Wave 1**: 4 tasks — T1 `quick`, T2 `quick`, T3 `deep`, T4 `quick`
- **Wave 2**: 3 tasks — T5 `unspecified-high`, T6 `unspecified-high`, T7 `deep`
- **FINAL**: 4 tasks — F1 `oracle`, F2 `unspecified-high`, F3 `unspecified-high`, F4 `deep`

---

## TODOs

- [ ] 1. Add `replaces` field to `SessionTopicSummary` struct

  **What to do**:
  - 在 `memory.rs:1311-1326` 的 `SessionTopicSummary` struct 中新增 `replaces: Vec<usize>` 字段
  - 添加 `#[serde(default)]` 确保旧 LLM 输出（无 replaces）仍能正常解析
  - 在 struct 上方添加文档注释说明 replaces 的含义（1-based 索引，引用 Existing Memories 中的位置）
  - 验证：`cargo test -p omem-server` 确认现有测试不被破坏

  **Must NOT do**:
  - 不修改 `SessionIngestBody` 或 API 接口
  - 不改变其他 serde 字段

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单文件单行改动，明确的 struct 字段添加
  - **Skills**: []
  - **Skills Evaluated but Omitted**:
    - `test-driven-development`: 单行改动过于简单，TDD 开销不值

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2)
  - **Blocks**: Tasks 2, 3, 4
  - **Blocked By**: None

  **References**:

  **Pattern References** (existing code to follow):
  - `omem-server/src/api/handlers/memory.rs:1311-1326` — `SessionTopicSummary` struct 定义，注意所有 `#[serde(default)]` 和 `Option` 模式
  - `omem-server/src/ingest/prompts.rs:746-751` — SESSION_COMPRESS_SYSTEM_PROMPT 中的 replaces 语义定义（1-based indices, `replaces: [1]` 表示更新 Previous Summary 1）

  **API/Type References**:
  - `omem-server/src/llm/service.rs` — `complete_json<T>()` 函数，会用 serde 反序列化 LLM 输出为 `SessionTopicSummary`

  **WHY Each Reference Matters**:
  - memory.rs:1311-1326 — 确认字段风格一致（`#[serde(default)]` on all optional fields）
  - prompts.rs:746-751 — 理解 replaces 的语义（1-based, referencing "Previously Stored Summaries" indices）

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: replaces field parses correctly from LLM output
    Tool: Bash (cargo test)
    Preconditions: SessionTopicSummary has replaces: Vec<usize> with #[serde(default)]
    Steps:
      1. Write a test: deserialize JSON `{"topic":"test","summary":"s","replaces":[1,2]}` → assert replaces == vec![1, 2]
      2. Write a test: deserialize JSON without replaces field → assert replaces == vec![]
      3. Run `cargo test -p omem-server session_topic` → all pass
    Expected Result: Both deserialization tests pass, replaces defaults to empty vec when absent
    Failure Indicators: Deserialization panic or wrong default value
    Evidence: .omo/evidence/task-1-replaces-deserialize.txt

  Scenario: Backward compatibility - old LLM output without replaces still works
    Tool: Bash (cargo test)
    Preconditions: Existing test infra unchanged
    Steps:
      1. Run `cargo test -p omem-server` → all existing tests pass
    Expected Result: 0 test failures, 0 compilation errors
    Failure Indicators: Any test failure related to SessionTopicSummary
    Evidence: .omo/evidence/task-1-backward-compat.txt
  ```

  **Commit**: YES (groups with Task 4)
  - Message: `refactor(session-ingest): add replaces tracking to SessionTopicSummary`
  - Files: `omem-server/src/api/handlers/memory.rs`

- [ ] 2. Enhance `SESSION_EXTRACT_SYSTEM_PROMPT` with REPLACES TRACKING + VALUE FILTER

  **What to do**:
  - 在 `prompts.rs` 的 `SESSION_EXTRACT_SYSTEM_PROMPT` 中增加以下段落（在 `## ABSOLUTE RULES` 之前插入）：
  
  **A) REPLACES TRACKING 段落**:
  ```
  ## REPLACES TRACKING (When "## Existing Memories" section exists)
  Each output topic MUST include a `replaces` array listing the 1-based indices of existing memories it updates/replaces:
  - Updating Existing Memory 1 → `"replaces": [1]`
  - Merging Memory 1 and 2 into one → `"replaces": [1, 2]`
  - Brand new topic (no existing equivalent) → `"replaces": []`
  - Existing memories NOT referenced in ANY topic's replaces → PRESERVED unchanged automatically (do NOT create a topic just to copy them)
  - MAX replaces length = 5. If more than 5 existing memories relate, pick the 5 most relevant.
  ```
  
  **B) VALUE FILTER 段落**（增强现有 VALUE FILTER 部分）:
  ```
  ## VALUE FILTER (ENHANCED)
  SKIP (do NOT extract): 
  - Casual small talk, greetings, farewells
  - Debugging status checks ("still failing", "works now")
  - Tool/engine internal outputs (build logs, test output, command results)
  - Meta-discussion about the conversation itself
  - Process details that don't represent decisions (file edits without rationale)
  - Repetitive confirmations of the same fact
  KEEP:
  - Technical decisions with rationale
  - User preferences and constraints
  - Code/architecture changes with impact reasoning
  - File paths, function names, API contracts
  - User emotional states and reactions (anger, satisfaction, frustration)
  - Errors and their root causes (not the debugging process)
  DEDUP RULE: If "## Existing Memories" already captures a fact, DO NOT re-extract it unless you have genuinely new details to add. Use `replaces` to update instead of creating duplicates.
  ```
  
  - 更新 `SESSION_EXTRACT_SYSTEM_PROMPT` 的 OUTPUT FORMAT JSON，增加 `"replaces": number[]` 字段
  - 在 `build_session_extract_prompt_with_memories()` 中，当 `existing_memories_summary` 存在时，将 memories 编号为 `1. {memory1}\n2. {memory2}\n...` 格式（目前是合并字符串），以便 replaces 可以引用编号
  - 添加 inline tests 验证新 prompt 包含 REPLACES TRACKING 和 VALUE FILTER 关键词

  **Must NOT do**:
  - 不替换整个 prompt — 只在现有 prompt 中增加段落
  - 不修改 `SESSION_COMPRESS_SYSTEM_PROMPT`
  - 不修改 `build_session_compress_prompt()`
  - 不修改 `ALLOWED_TAGS_LIST`

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 主要是文本编辑（prompt string），加上小量代码修改
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 1)
  - **Blocks**: Tasks 3, 4
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `omem-server/src/ingest/prompts.rs:734-802` — SESSION_COMPRESS_SYSTEM_PROMPT 中的 replaces tracking 模式，直接复制语义
  - `omem-server/src/ingest/prompts.rs:1053-1088` — `build_session_extract_prompt_with_memories()` 函数，当前 existing_memories_summary 的注入方式
  - `omem-server/src/ingest/prompts.rs:991-1015` — 当前 OUTPUT FORMAT JSON 定义，需要增加 replaces 字段

  **WHY Each Reference Matters**:
  - prompts.rs:734-802 — 这是 replaces 的"源"定义，需要保持语义一致
  - prompts.rs:1053-1088 — 理解 existing memories 如何注入到 prompt 中，需要改为编号格式
  - prompts.rs:991-1015 — 输出格式定义，必须增加 replaces 字段

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Prompt contains REPLACES TRACKING instructions
    Tool: Bash (cargo test)
    Preconditions: SESSION_EXTRACT_SYSTEM_PROMPT modified
    Steps:
      1. Write test: assert SESSION_EXTRACT_SYSTEM_PROMPT.contains("REPLACES TRACKING")
      2. Write test: assert SESSION_EXTRACT_SYSTEM_PROMPT.contains("replaces")
      3. Write test: assert output format JSON contains "replaces"
      4. Run `cargo test -p omem-server session_extract` → all pass
    Expected Result: All assertion tests pass
    Failure Indicators: Missing keywords in prompt
    Evidence: .omo/evidence/task-2-prompt-keywords.txt

  Scenario: build_session_extract_prompt_with_memories numbers existing memories
    Tool: Bash (cargo test)
    Preconditions: Function modified to number memories
    Steps:
      1. Write test: call build_session_extract_prompt_with_memories with "memory1\nmemory2" → assert user prompt contains "1." and "2." numbering
      2. Write test: call without memories → assert no numbering appears
      3. Run `cargo test -p omem-server session_extract` → all pass
    Expected Result: Numbering format correct when memories present, absent when not
    Evidence: .omo/evidence/task-2-numbered-memories.txt
  ```

  **Commit**: YES
  - Message: `feat(session-ingest): enhance extract prompt with replaces + VALUE FILTER`
  - Files: `omem-server/src/ingest/prompts.rs`

- [ ] 3. Replace weak dedup with replaces-driven merge logic in session_ingest handler

  **What to do**:
  这是本次优化的核心任务。重写 `memory.rs` 中 session_ingest handler 的 EMOTIONAL/WORK/PREFERENCE merge 逻辑。

  **当前问题**（需替换的代码路径）:
  - EMOTIONAL path (lines 1625-1709): topic name `ends_with()` + `content.contains()` + session_id match
  - WORK path (lines 1712-1823): section-replace via line-by-line scan + `content.contains()` dedup
  - PREFERENCE path (lines 1827-1938): vector search (0.2 threshold) + tag overlap (≥1 match)

  **新逻辑设计**:

  ```
  // 1. Fetch existing memories (current logic - keep)
  let existing_memories = fetch top 5 EMOTIONAL + top 5 WORK memories
  
  // 2. Build numbered summary for prompt (Task 2 provides numbering)
  // existing_memories_summary = "1. {emotional_summary}\n2. {work_summary}\n..."
  
  // 3. LLM extract → Vec<SessionTopicSummary> (each with replaces: Vec<usize>)
  let topics = complete_json::<Vec<SessionTopicSummary>>(...)
  
  // 4. For each topic, process replaces:
  for topic in topics {
      // Bounds check on replaces indices (1-based → 0-based)
      let valid_replaces: Vec<usize> = topic.replaces.iter()
          .filter(|&&i| i >= 1 && i <= existing_memories.len())
          .map(|&i| i - 1)  // convert to 0-based
          .collect();
      
      if valid_replaces.is_empty() {
          // No replaces → CREATE new memory (current behavior)
          create_new_memory(topic);
      } else if valid_replaces.len() == 1 {
          // Single replace → UPDATE existing memory
          let existing = &existing_memories[valid_replaces[0]];
          let merged = smart_merge(existing, &topic);
          if merged.chars().count() <= 3000 {
              store.update(&merged);
          } else {
              // Overflow: create new with Continues relation
              create_with_continues(topic, existing.id);
          }
      } else {
          // Multi-replace → MERGE multiple memories into one
          // Pick the longest as base, append new sections from others
          let base = pick_longest(&valid_replaces, &existing_memories);
          let merged = merge_multiple(base, &valid_replaces, &existing_memories, &topic);
          if merged.chars().count() <= 3000 {
              store.update(&merged);
              // Archive other replaced memories (mark as superseded)
              for idx in valid_replaces.iter().skip(1) {
                  archive_memory(&existing_memories[*idx]);
              }
          } else {
              create_with_continues(topic, base.id);
          }
      }
  }
  ```

  **smart_merge 函数**:
  - 不增加额外 LLM 调用 — 纯字符串操作
  - EMOTIONAL: `format!("{}\n\n## {} {}\n{}", existing.content, today, topic.topic, topic.summary)`
  - WORK: section-replace（找到匹配的 ## header → 替换内容；找不到 → append）
  - 保留 `apply_append()` 辅助函数用于 l0/l1/l2 更新
  - 合并 tags（union，去重，max 5）

  **Fallback 策略**:
  - 如果 replaces 解析失败 → 走原始 CREATE 路径
  - 如果 merge 后超 3000 chars → 走 CREATE + Continues relation
  - 如果 store.update 失败 → 走 CREATE

  **Must NOT do**:
  - 不增加额外 LLM 调用（merge 是纯代码操作）
  - 不修改 API 接口
  - 不改变 cluster assignment 逻辑（Task 5 单独处理）
  - 不修改 IngestPipeline
  - 不删除 apply_append() 辅助函数（复用）
  - 不改变 3000 char cap

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 核心逻辑重写，涉及多个代码路径的协调和错误处理
  - **Skills**: [`test-driven-development`]
    - `test-driven-development`: 复杂逻辑必须先写测试再实现

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 1 + 2)
  - **Parallel Group**: Wave 1 (sequential after T1, T2)
  - **Blocks**: Tasks 5, 7
  - **Blocked By**: Tasks 1, 2

  **References**:

  **Pattern References**:
  - `omem-server/src/api/handlers/memory.rs:1580-1623` — `apply_append()` 辅助函数，新逻辑应复用此函数
  - `omem-server/src/api/handlers/memory.rs:1625-1938` — 当前 EMOTIONAL/WORK/PREFERENCE 三条路径的完整逻辑（需替换）
  - `omem-server/src/api/handlers/memory.rs:1940-1976` — Continues relation 创建逻辑（保留）
  - `omem-server/src/api/handlers/memory.rs:1979-2023` — `add_continued_by_relation()` 反向链接逻辑（保留）

  **API/Type References**:
  - `omem-server/src/api/handlers/memory.rs:1311-1326` — `SessionTopicSummary` struct（Task 1 新增 replaces 字段后）
  - `omem-server/src/domain/memory.rs` — `Memory` struct 的 content, l0_abstract, l1_overview, l2_content, tags 字段
  - `omem-server/src/store/lancedb.rs` — `LanceStore::update()` 和 `LanceStore::create()` 方法签名

  **Test References**:
  - `omem-server/src/api/mod.rs` — `setup_app()` 工厂函数和 `TestLlm` mock 模式（**所有** session_ingest 集成测试在此文件中）
  - `omem-server/src/api/mod.rs` (tests section) — 现有 35 个集成测试，包含 session_ingest 相关测试

  **WHY Each Reference Matters**:
  - memory.rs:1580-1623 — apply_append 是 l0/l1/l2 更新的核心，必须复用
  - memory.rs:1625-1938 — 这是需要被替换的代码，理解每条路径的边界条件
  - memory.rs:1940-1976 — Continues relation 必须保留用于 overflow 场景
  - memory.rs:1979-2023 — continued_by 反向链接逻辑保留
  - api/mod.rs — integration test 的 mock 模式

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Single replace updates existing memory correctly
    Tool: Bash (cargo test)
    Preconditions: Task 1 + 2 completed, mock LLM returns topic with replaces: [1]
    Steps:
      1. Write test: session_ingest with existing EMOTIONAL memory + new conversation about same topic
      2. Mock LLM returns topic with replaces: [1]
      3. Assert: no new memory created, existing memory updated with appended section
      4. Assert: existing.content contains new section
      5. Run `cargo test -p omem-server` → all pass
    Expected Result: Existing memory updated in-place, content grows with new section
    Failure Indicators: New memory created instead of updating existing
    Evidence: .omo/evidence/task-3-single-replace.txt

  Scenario: Empty replaces creates new memory
    Tool: Bash (cargo test)
    Preconditions: Mock LLM returns topic with replaces: []
    Steps:
      1. Write test: session_ingest with new unrelated topic
      2. Mock LLM returns replaces: []
      3. Assert: new memory created
      4. Run `cargo test -p omem-server` → all pass
    Expected Result: New memory created with correct fields
    Evidence: .omo/evidence/task-3-empty-replaces.txt

  Scenario: Out-of-bounds replaces fallback to CREATE
    Tool: Bash (cargo test)
    Preconditions: Mock LLM returns replaces: [99] (out of bounds)
    Steps:
      1. Write test: session_ingest with out-of-bounds replaces
      2. Assert: fallback to CREATE new memory
      3. Assert: no panic, no crash
      4. Run `cargo test -p omem-server` → all pass
    Expected Result: Graceful fallback, new memory created
    Evidence: .omo/evidence/task-3-oob-replaces.txt

  Scenario: 3000 char overflow triggers Continues relation
    Tool: Bash (cargo test)
    Preconditions: Existing memory near 3000 char cap, new topic with replaces: [1]
    Steps:
      1. Write test: existing memory at 2800 chars, new merge would exceed 3000
      2. Assert: new memory created with Continues relation to original
      3. Assert: original memory NOT updated (would exceed cap)
      4. Run `cargo test -p omem-server` → all pass
    Expected Result: New memory with Continues relation, original preserved
    Evidence: .omo/evidence/task-3-overflow-continues.txt
  ```

  **Commit**: YES
  - Message: `refactor(session-ingest): replace weak dedup with replaces-driven merge`
  - Files: `omem-server/src/api/handlers/memory.rs`

- [ ] 4. Add inline tests for replaces parsing and merge edge cases

  **What to do**:
  在 `memory.rs` 的 `#[cfg(test)] mod tests` 中添加以下单元测试：

  **TDD — 先写这些测试**，然后让 Task 3 的实现使其通过：
  
  - `test_session_topic_summary_with_replaces` — 验证 JSON 反序列化包含 replaces
  - `test_session_topic_summary_without_replaces` — 验证缺少 replaces 时默认空数组
  - `test_replaces_bounds_check` — 验证越界索引被过滤
  - `test_replaces_empty_creates_new` — 验证空 replaces 触发 CREATE
  - `test_smart_merge_emotional_append` — 验证 EMOTIONAL 合并格式
  - `test_smart_merge_work_section_replace` — 验证 WORK section 替换格式
  - `test_smart_merge_overflow_creates_continues` — 验证超 3000 chars 创建 Continues
  - `test_multi_replace_picks_longest_as_base` — 验证多 replace 选最长做 base

  **Must NOT do**:
  - 不添加集成测试（Task 7 负责）
  - 不修改生产代码（只添加 test 函数）

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 纯测试代码，模式明确
  - **Skills**: [`test-driven-development`]
    - `test-driven-development`: 这是 TDD 的 RED 阶段

  **Parallelization**:
  - **Can Run In Parallel**: YES (conceptually parallel with Task 3, but should be done FIRST for TDD)
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)
  - **Blocks**: Task 7
  - **Blocked By**: Tasks 1, 2

  **References**:

  **Pattern References**:
  - `omem-server/src/api/mod.rs` — `setup_app()` 工厂函数 + 35 个集成测试（**所有** session_ingest 测试模式在此文件）
  - `omem-server/src/api/mod.rs` (tests section) — 现有集成测试使用 `TestLlm` mock + `tower::ServiceExt::oneshot`

  **Test References**:
  - `omem-server/src/ingest/prompts.rs:1090-1150` — 现有 prompt 测试模式（assert contains keywords）

  **WHY Each Reference Matters**:
  - memory.rs:2357-2397 — 确认测试 mock 模式和 assertion 风格
  - api/mod.rs — setup_app 是所有集成测试的基础

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: All new test functions compile and run
    Tool: Bash (cargo test)
    Steps:
      1. Run `cargo test -p omem-server session_topic` → all new tests found
      2. Verify no compilation errors
    Expected Result: All 8 new test functions are discovered by test runner
    Failure Indicators: Compilation error or test not found
    Evidence: .omo/evidence/task-4-tests-compile.txt

  Scenario: TDD red-green cycle works
    Tool: Bash (cargo test)
    Steps:
      1. Initially tests fail (RED) — expected behavior before Task 3 implements logic
      2. After Task 3 implementation, tests pass (GREEN)
    Expected Result: RED → GREEN cycle demonstrates TDD discipline
    Evidence: .omo/evidence/task-4-tdd-cycle.txt
  ```

  **Commit**: YES (groups with Task 1)
  - Message: `refactor(session-ingest): add replaces tracking to SessionTopicSummary`
  - Files: `omem-server/src/api/handlers/memory.rs`

- [ ] 5. Batch cluster assignment (replace N+1 LLM calls)

  **What to do**:
  当前 session_ingest 为每条新记忆独立调用 `cluster_assigner.assign()` (memory.rs:2039-2114)，每条可能触发 LLM 裁决。改为 batch 模式：

  - 收集所有 `created_memories` 后，调用 `cluster_assigner.assign_batch()` (或循环但共享 LLM 调用)
  - 如果 `ClusterAssigner` 没有 batch 方法，则在循环中复用 LLM response — 先收集所有 candidates，一次 LLM 调用判断所有 assignment
  - 保留 fallback：batch 失败时回退到逐条 assign
  - 添加 tracing 记录 batch 效率

  **Must NOT do**:
  - 不修改 ClusterAssigner 的核心算法
  - 不修改 cluster 模块的公共 API
  - 不增加新的 LLM 调用（只减少）

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 需要理解 cluster assignment 逻辑并优化，涉及跨模块协调
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 6, 7)
  - **Blocks**: F1-F4
  - **Blocked By**: Task 3

  **References**:

  **Pattern References**:
  - `omem-server/src/api/handlers/memory.rs:2039-2114` — 当前 N+1 cluster assignment 循环
  - `omem-server/src/cluster/assigner.rs` — `ClusterAssigner::assign()` 方法签名和实现
  - `omem-server/src/cluster/manager.rs` — `ClusterManager::assign_to_cluster()` 和 `create_cluster()`

  **WHY Each Reference Matters**:
  - memory.rs:2039-2114 — 需要替换的 N+1 循环
  - assigner.rs — 理解 assign 的三种 action (AutoAssign, CreateNew, LlmJudge)
  - manager.rs — 理解 cluster 操作的实际执行

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Multiple memories trigger fewer cluster LLM calls
    Tool: Bash (cargo test)
    Preconditions: Multiple new memories created in one session_ingest
    Steps:
      1. Write test: session_ingest creates 3 memories, track LLM call count
      2. Assert: cluster-related LLM calls ≤ 1 (batched), not 3
      3. Run `cargo test -p omem-server` → all pass
    Expected Result: LLM calls reduced from N to 1
    Evidence: .omo/evidence/task-5-batch-cluster.txt

  Scenario: Fallback to per-memory assign on batch failure
    Tool: Bash (cargo test)
    Preconditions: Batch assign fails (mock returns error)
    Steps:
      1. Write test: batch assign fails → fallback to per-memory assign
      2. Assert: all memories still get assigned to clusters
      3. Run `cargo test -p omem-server` → all pass
    Expected Result: Graceful degradation, no memory left unassigned
    Evidence: .omo/evidence/task-5-fallback.txt
  ```

  **Commit**: YES
  - Message: `perf(session-ingest): batch cluster assignment`
  - Files: `omem-server/src/api/handlers/memory.rs`

- [ ] 6. Session ingest semaphore + session_locks TTL cleanup

  **What to do**:
  两个独立的性能修复：

  **A) Session ingest 信号量**:
  - 在 `AppState` 中新增 `session_ingest_semaphore: Arc<Semaphore>` (max 5)
  - 在 `session_ingest` 的 `tokio::spawn` 内部 acquire 信号量
  - 防止无限制并发 LLM 调用

  **B) session_locks TTL 清理**:
  - `AppState.session_locks` 类型已是 `DashMap<String, (Arc<Mutex<()>>, Instant)>`（Instant 已在记录）
  - 只需添加后台清理任务：每隔 30 分钟扫描 DashMap，移除 > 1 小时未使用的 lock entry
  - 在 `LifecycleScheduler::run()` 中集成此清理，或单独 spawn 一个 tokio task

  **Must NOT do**:
  - 不改变 API 接口
  - 不引入新依赖（用现有的 tokio::sync::Semaphore）
  - 不修改其他 handler

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 涉及 AppState 修改 + 后台任务，需要理解并发模型
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 7)
  - **Blocks**: F1-F4
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `omem-server/src/api/server.rs` — `AppState` struct 定义（需新增 session_ingest_semaphore 字段）
  - `omem-server/src/api/handlers/memory.rs:1373-1381` — 当前 tokio::spawn + session_locks entry (已有 Instant::now())
  - `omem-server/src/main.rs` — AppState 初始化位置
  - `omem-server/src/api/handlers/imports.rs` — import_semaphore 的使用模式（Arc<Semaphore>(3)）

  **WHY Each Reference Matters**:
  - server.rs — 确认 AppState 字段添加模式
  - memory.rs:1373-1381 — 理解当前 lock 获取和 Instant 存储位置
  - main.rs — 确认 AppState 初始化顺序
  - imports.rs — semaphore 使用模式参照

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Concurrent session_ingest limited by semaphore
    Tool: Bash (cargo test)
    Preconditions: AppState has session_ingest_semaphore(5)
    Steps:
      1. Write test: spawn 10 concurrent session_ingest requests
      2. Assert: at most 5 concurrent LLM calls active at any time
      3. Run `cargo test -p omem-server` → all pass
    Expected Result: Concurrency bounded at 5
    Evidence: .omo/evidence/task-6-semaphore.txt

  Scenario: Stale session_locks cleaned up
    Tool: Bash (cargo test)
    Preconditions: session_locks has TTL cleanup logic
    Steps:
      1. Write test: insert entry with Instant::now() - 2 hours
      2. Run cleanup
      3. Assert: entry removed
      4. Write test: insert entry with Instant::now() - 10 mins
      5. Run cleanup
      6. Assert: entry retained
      7. Run `cargo test -p omem-server` → all pass
    Expected Result: Only entries > 1 hour old are removed
    Evidence: .omo/evidence/task-6-ttl-cleanup.txt
  ```

  **Commit**: YES
  - Message: `fix(session-ingest): add semaphore + session_locks TTL cleanup`
  - Files: `omem-server/src/api/server.rs`, `omem-server/src/api/handlers/memory.rs`, `omem-server/src/main.rs`

- [ ] 7. Integration test for full session_ingest with replaces-driven merge

  **What to do**:
  编写完整的集成测试，覆盖 replaces-driven merge 的端到端流程：

  - **Test 1**: 完整 session_ingest 流程 — 发送消息 → mock LLM 返回带 replaces 的 topics → 验证 merge/update/create 行为
  - **Test 2**: 重复 ingest — 同一 session 两次 ingest → 验证第二次的 replaces 正确引用第一次创建的记忆
  - **Test 3**: 混合场景 — 一个 session 同时有 EMOTIONAL 和 WORK topics → 验证分类和 merge 正确
  - **Test 4**: 向后兼容 — 已有旧格式记忆（无 replaces 相关字段）→ 新 ingest 能正确处理

  使用 `setup_app()` + 自定义 `TestLlm` 返回预定义 JSON。

  **Must NOT do**:
  - 不修改生产代码
  - 不添加外部依赖

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 端到端集成测试，需要 mock 多个组件并验证复杂交互
  - **Skills**: [`test-driven-development`]
    - `test-driven-development`: 集成测试需要精心设计 mock 和 assertion

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 3, 4

  **References**:

  **Pattern References**:
  - `omem-server/src/api/mod.rs:setup_app()` — 集成测试工厂函数
  - `omem-server/src/api/mod.rs` (tests section) — 现有 35 个集成测试的模式
  - `omem-server/src/api/handlers/memory.rs:2357-2397` — 现有 session_ingest inline tests

  **WHY Each Reference Matters**:
  - api/mod.rs — setup_app 是集成测试的基础，提供 mock LLM 和 embedder
  - memory.rs:2357-2397 — 确认现有 session_ingest 测试风格

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: End-to-end session_ingest with replaces merge
    Tool: Bash (cargo test)
    Preconditions: All previous tasks completed
    Steps:
      1. Run `cargo test -p omem-server session_ingest_replaces`
      2. Assert: 4 integration tests pass
      3. Run `cargo test -p omem-server` → all pass (no regression)
    Expected Result: 4 new integration tests pass, 0 regressions
    Failure Indicators: Any test failure or regression
    Evidence: .omo/evidence/task-7-integration-tests.txt

  Scenario: No regression in existing tests
    Tool: Bash (cargo test)
    Steps:
      1. Run `cargo test -p omem-server` → all 373+ tests pass
    Expected Result: 0 test failures
    Evidence: .omo/evidence/task-7-no-regression.txt
  ```

  **Commit**: YES
  - Message: `test(session-ingest): integration test for replaces-driven session_ingest`
  - Files: `omem-server/src/api/handlers/memory.rs`

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns. Check evidence files exist in .omo/evidence/.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy` + `cargo test -p omem-server`. Review all changed files for: `unwrap()` in non-test code, empty catches, unused imports. Check AI slop: excessive comments, over-abstraction.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N pass/N fail] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Test edge cases: empty replaces, out-of-bounds replaces, old memories without replaces.
  Output: `Scenarios [N/N pass] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — everything in spec was built, nothing beyond spec was built. Check "Must NOT do" compliance.
  Output: `Tasks [N/N compliant] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Task 1+4**: `refactor(session-ingest): add replaces tracking to SessionTopicSummary` - memory.rs, prompts.rs
- **Task 2**: `feat(session-ingest): enhance extract prompt with replaces + VALUE FILTER` - prompts.rs
- **Task 3**: `refactor(session-ingest): replace weak dedup with replaces-driven merge` - memory.rs
- **Task 5**: `perf(session-ingest): batch cluster assignment` - memory.rs
- **Task 6**: `fix(session-ingest): add semaphore + session_locks TTL cleanup` - memory.rs, server.rs
- **Task 7**: `test(session-ingest): integration test for replaces-driven session_ingest` - memory.rs

---

## Success Criteria

### Verification Commands
```bash
cargo test -p omem-server                          # All tests pass
cargo clippy -p omem-server                        # No new warnings
cargo test -p omem-server session_ingest           # Session ingest tests pass
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass
- [ ] No new `unwrap()` in non-test code
- [ ] Replaces tracking works with mock LLM
- [ ] Backward compatible with old memories
