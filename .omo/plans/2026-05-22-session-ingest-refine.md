# Session Ingest 精炼去重 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** session_ingest时，对WORK类记忆的关联链进行LLM精炼去重，保留有用段落删除多余内容，l0/l1/l2同步更新，格式保持section结构不变

**Architecture:** 在session_ingest handler层替换简陋的section追加为智能精炼。per-topic独立判断：embedding cosine前置检查 → 相似度高走LLM精炼 → 相似度低直接创建。精炼失败回退原有追加

**Tech Stack:** Rust (axum, tokio), LanceDB, LLM Service trait, Embedding Service

---

## Context

### Original Request
session_ingest路径不走Reconciler，只有handler层简陋的section追加逻辑（搜索 `if memory_type == "WORK"` 定位），导致记忆越积越冗长、重复。需要在源头精炼，保证记忆始终精简。

### Interview Summary
**Key Discussions**:
- 不动Reconciler：Reconciler只走import路径，工作正常
- 精炼范围：关联链级（Continues/ContinuedBy relation链）
- 私密记忆不精炼：scope=private/EMOTIONAL保持原有追加
- 偏好已独立：preference_slots.rs单独处理，不涉及精炼
- 召回精炼保留：plugin端加开关默认false
- 归簇后面单独砍

**Research Findings**:
- session_ingest入口：搜索 `pub async fn session_ingest`
- WORK追加逻辑：搜索 `if memory_type == "WORK"`
- EMOTIONAL追加逻辑：搜索 `if memory_type == "EMOTIONAL"`
- apply_append闭包：搜索 `let apply_append = |`（注意是局部闭包，不是公开函数）
  - Memory struct已存在（物理删除旧记忆，不用superseded_by）

### Metis Review（灵犀审查，师尊已确认采纳）
1. ~~物理删除改标记覆盖~~ → 师尊改为**直接物理删除**（store.batch_hard_delete_by_ids()），不用superseded_by
2. 阈值0.7→0.65 + session_id双条件
3. session锁拆分：匹配阶段持锁，精炼阶段释放锁
4. 继承旧记忆tier/importance/tags：取链上max
5. BFS加环路检测+深度限制：visited set + max_depth=5
6. 字数硬截断：代码层验证
7. 精炼入口双重guard：memory_type != EMOTIONAL AND scope != private

---

## Work Objectives

### Core Objective
改造session_ingest handler层的WORK记忆追加逻辑，替换为per-topic embedding前置检查 + LLM精炼去重

### Concrete Deliverables
- 新文件：`ingest/refine_prompt.rs`（精炼prompt）
- 新文件：`ingest/refine_service.rs`（精炼服务：收集链、调LLM、存结果）
- 修改：`ingest/mod.rs`（添加mod声明）
- 修改：`ingest/prompts.rs`（SESSION_EXTRACT的l1格式 + 字数限制更新）
- 修改：`api/handlers/memory.rs`（WORK追加逻辑替换为精炼）

### Definition of Done
- [ ] WORK记忆精炼：同session同主题的记忆链被精炼去重
- [ ] EMOTIONAL记忆不受影响：保持原有追加
- [ ] l0/l1/l2同步更新：精炼后4个字段一致
- [ ] 3000字split：精炼后超限继续split
- [ ] 精炼失败回退：不丢数据
- [ ] 所有现有测试通过：cargo test -p omem-server

### Must Have
- 语义相似度前置检查（embedding cosine，阈值0.72，先用l0_abstract）
- Per-topic独立判断
- LLM精炼输出4字段（content + l0 + l1 + l2）
- l1固定箭头脉络格式
- 字数精简（content WORK≤500, l2≤300）
- 物理删除旧记忆（store.batch_hard_delete_by_ids()）— LanceDB的after_mutation()已有GC机制（prune old versions + compact fragments），每次delete自动计write_count，达GC_WRITE_THRESHOLD后自动清理，不会OOM
- session_id双条件匹配
- BFS环路检测+max_depth=5
- 字数硬截断（按句子边界）
- 精炼入口双重guard
- 继承旧记忆tier/importance/tags（tier按优先级，tags取并集）
- 精炼失败回退原有追加

### Must NOT Have（Guardrails）
- 不动Reconciler（reconciler.rs）
- 不改EMOTIONAL记忆逻辑
- 不改import路径
- 不删召回精炼代码

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES（cargo test -p omem-server, 373 inline tests）
- **Automated tests**: Tests-after（先实现后补测试）
- **Framework**: cargo test（inline #[cfg(test)]）

### QA Policy
每个task都包含agent-executed QA场景。
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation - 新文件，零依赖):
├── Task 1: 精炼prompt模块 (refine_prompt.rs) [quick]
├── Task 2: 精炼服务骨架 (refine_service.rs) [deep]
└── Task 3: SESSION_EXTRACT prompt l1格式+字数更新 [quick]

Wave 2 (Integration - 依赖Wave 1):
├── Task 4: handler层WORK追加→精炼替换 (depends: 1, 2) [deep]
├── Task 5: 物理删除旧记忆逻辑 (depends: 2) [quick]
└── Task 6: handler层EMOTIONAL guard + 双重检查 (depends: 4) [quick]

Wave 3 (Hardening):
├── Task 7: 字数硬截断 + OOM保护 (depends: 2, 4) [quick]
├── Task 8: 精炼失败回退 + 日志完善 (depends: 4) [quick]
└── Task 9: 集成测试 (depends: all) [unspecified-high]

Wave FINAL (Verification):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real QA scenarios (unspecified-high)
└── Task F4: Scope fidelity check (deep)

Critical Path: Task 1 → Task 4 → Task 9 → F1-F4
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 3 (Wave 1)
```

### Dependency Matrix
- **1**: - → 4
- **2**: - → 4, 5, 7
- **3**: - → (independent, Wave 1)
- **4**: 1, 2 → 6, 7, 8
- **5**: 2 → (standalone)
- **6**: 4 → (standalone)
- **7**: 2, 4 → 9
- **8**: 4 → 9
- **9**: 4, 5, 6, 7, 8 → F1-F4

### Agent Dispatch Summary
- **Wave 1**: 3 — T1 `quick`, T2 `deep`, T3 `quick`
- **Wave 2**: 3 — T4 `deep`, T5 `quick`, T6 `quick`
- **Wave 3**: 3 — T7 `quick`, T8 `quick`, T9 `unspecified-high`
- **FINAL**: 4 — F1 `oracle`, F2 `unspecified-high`, F3 `unspecified-high`, F4 `deep`

---

## TODOs

- [ ] 1. **精炼prompt模块** — `ingest/refine_prompt.rs`

  **What to do**:
  - 创建新文件 `omem-server/src/ingest/refine_prompt.rs`
  - 定义 `RefineInput` 结构体（existing_contents: Vec<String>, new_fact: String, topic: String）
  - 定义 `RefineOutput` 结构体（refined_content, l0_abstract, l1_overview, l2_content: 四个String）
  - 实现 `build_refine_prompt(input: &RefineInput) -> (String, String)` 函数
  - **完整prompt文本如下**（必须原样使用）：

  **SYSTEM PROMPT**（常量 `REFINE_SYSTEM_PROMPT`）：
  ```
  You are a memory refinement engine. Your task is to read one or more existing memory entries about the same topic, plus a new fact, then produce a SINGLE refined, deduplicated memory.

  ## ABSOLUTE RULES

  ### Rule 1: Language Preservation (MANDATORY)
  - YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT. NEVER translate. NEVER mix languages.
  - Tags are ALWAYS in English. Exception: "私密" is system-reserved.

  ### Rule 2: Deduplication (CORE TASK)
  - Remove duplicate/redundant information across all sections.
  - If multiple sections describe the same event/decision, MERGE into one section using the LATEST timestamp.
  - Keep ONLY: final conclusions, key decisions, important outcomes, critical data points.
  - Remove: intermediate steps, verbose process details, outdated information, repetitive descriptions.

  ### Rule 3: Format Preservation
  - Maintain `## YYYY-MM-DD HH:MM Topic` section structure for distinct events.
  - Each section covers ONE distinct event/decision/milestone.
  - Chronological order (oldest first).

  ### Rule 4: Precision Over Recall
  - It is BETTER to lose minor details than to keep redundant content.
  - The refined content MUST be shorter than the sum of all input contents.
  - Target: compress to 30-60% of original total length.

  ## OUTPUT FORMAT
  Return ONLY valid JSON:
  {
    "refined_content": "Deduplicated content in section format",
    "l0_abstract": "Topic label covering the full scope (≤100 chars)",
    "l1_overview": "Timeline in arrow format: A→B→C→result (≤150 chars)",
    "l2_content": "Key facts: decisions, conclusions, data (≤300 chars)"
  }

  ## l1_overview FORMAT (MANDATORY)
  Must use arrow notation: `verb phrase→verb phrase→result`
  Examples:
  - "diagnosed bug→traced to handler→fixed with lookup table→verified→deployed v1.16.10"
  - "requirement analysis→design review→implemented→tested→released"
  - "identified perf issue→benchmarked 3 solutions→chose option B→deployed→latency reduced 70%"
  Each node = verb phrase (what happened), arrows = temporal/causal progression.

  ## l2_content FORMAT
  Compress to structured key facts only:
  - Root cause: X
  - Fix: Y
  - Verification: Z
  - Key metric: N
  Remove all narrative/process description.
  ```

  **USER PROMPT BUILDER**（函数 `build_refine_prompt`）：
  ```
  fn build_refine_prompt(input: &RefineInput) -> (String, String) {
      let system = REFINE_SYSTEM_PROMPT.to_string();
      
      let mut user = format!("## Topic: {}\n\n", input.topic);
      
      for (i, content) in input.existing_contents.iter().enumerate() {
          user.push_str(&format!("### Existing Memory #{}\n{}\n\n", i + 1, content));
      }
      
      if !input.new_fact.is_empty() {
          user.push_str(&format!("### New Information\n{}\n\n", input.new_fact));
      }
      
      user.push_str("Produce the refined memory. Return ONLY valid JSON.");
      
      (system, user)
  }
  ```

  **Must NOT do**:
  - 不修改现有prompt（SESSION_EXTRACT等）
  - 不引入新依赖

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Task 4
  - **Blocked By**: None

  **References**:
- `omem-server/src/ingest/prompts.rs` — SESSION_COMPRESS_SYSTEM_PROMPT（搜索 `SESSION_COMPRESS_SYSTEM_PROMPT` 定位，参考set operations设计）
- `omem-server/src/ingest/prompts.rs` — SESSION_EXTRACT_SYSTEM_PROMPT（搜索 `SESSION_EXTRACT_SYSTEM_PROMPT` 定位，参考WORK format规则）
  - `omem-server/src/ingest/mod.rs` — 现有mod声明列表（添加 `pub mod refine_prompt;`）

  **Acceptance Criteria**:
  - [ ] 文件 `ingest/refine_prompt.rs` 存在
  - [ ] `cargo build -p omem-server` 通过
  - [ ] RefineOutput包含4个字段：refined_content, l0_abstract, l1_overview, l2_content

  **QA Scenarios**:
  ```
  Scenario: 编译验证
    Tool: Bash
    Steps:
      1. cargo build -p omem-server 2>&1 | tail -5
    Expected Result: 编译成功，无error
    Evidence: .omo/evidence/task-1-build.txt

  Scenario: 模块声明验证
    Tool: Bash
    Steps:
      1. grep "refine_prompt" omem-server/src/ingest/mod.rs
    Expected Result: 输出包含 "pub mod refine_prompt"
    Evidence: .omo/evidence/task-1-mod-decl.txt
  ```

  **Commit**: YES (groups with T2, T3)
  - Message: `feat(ingest): add refine prompt and service modules`
  - Files: `omem-server/src/ingest/refine_prompt.rs, omem-server/src/ingest/mod.rs`

- [ ] 2. **精炼服务骨架** — `ingest/refine_service.rs`

  **What to do**:
  - 创建新文件 `omem-server/src/ingest/refine_service.rs`
  - 实现3个核心函数：

  **函数1: `collect_chain_memories(store, root_memory) -> Result<Vec<Memory>>`**
  - BFS遍历Continues/ContinuedBy relation链
  - visited set防环路
  - max_depth=5限制
  - 输入：LanceStore + 根Memory
  - 输出：链上所有Memory实体（含root）

  **函数2: `find_similar_work_memory(store, embed, topic_l0, session_id, tenant_id) -> Result<Option<Memory>>`**
  - 用topic.l0_abstract做embedding
  - 搜索同session_id的WORK记忆
  - cosine > 0.72且session_id匹配 → 返回最相似的
  - 实现方式：先调 `find_memories_by_session_id(session_id, limit)` 取同session所有记忆，再在内存中按 `memory_type == "WORK"` 过滤，然后对每个WORK记忆调 `get_vector_by_id(id)` 获取vector（Memory struct不含vector字段），最后在内存中用query vector与已有vector算cosine similarity

  **函数3: `refine_and_replace(store, llm, embed, root_memory, chain_memories, new_fact, topic) -> Result<Memory>`**
  - 调build_refine_prompt构建prompt
  - 调LLM `complete_json::<RefineOutput>(&*llm, &system_prompt, &user_prompt).await` 获取精炼结果（free function，非trait method；自动修复JSON+重试）
  - 字数硬截断（content≤500, l1≤150, l2≤300，按句子边界截断）
  - 继承旧记忆的tier/importance/tags（tier按优先级l3>l2>l1>l0，tags取并集，importance取max）
  - 先存精炼结果，再物理删除链上所有旧记忆（`store.batch_hard_delete_by_ids()`，先存后删保证原子性）
  - 超过3000字 → split（按section分割 + Continues/ContinuedBy relation）

  **Must NOT do**:
  - 不手动解析JSON（必须用complete_json）
  - 不处理EMOTIONAL记忆
  - 不引入新依赖

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: Tasks 4, 5, 7
  - **Blocked By**: None

  **References**:
  - `omem-server/src/api/handlers/memory.rs` — `apply_append` 局部闭包（搜索 `let apply_append = |` 定位，在 session_ingest 函数内，用于 l0/l1/l2 覆盖更新。注意：是闭包非公开函数，refine_service.rs 不可直接调用）
  - `omem-server/src/api/handlers/memory.rs` — auto-split + Continues/ContinuedBy relation 建立逻辑（搜索 `Continues` 和 `section_split` 定位）
  - `omem-server/src/domain/memory.rs` — Memory struct定义（搜索 `pub struct Memory` 定位）
  - `omem-server/src/domain/relation.rs` — RelationType枚举（Continues, ContinuedBy，搜索 `pub enum RelationType` 定位）
  - `omem-server/src/store/lancedb.rs:1596-1619` — store.create() 模式
  - `omem-server/src/store/lancedb.rs:1644` — store.get_vector_by_id() 获取单条记忆的vector（Memory struct不含vector字段，需单独获取）
  - `omem-server/src/store/lancedb.rs:1922` — store.hard_delete() 和 batch_hard_delete_by_ids()
  - `omem-server/src/llm/service.rs` — LlmService trait + `complete_json::<T>(&dyn LlmService, &str, &str)` 签名（free function，非trait method，自动修复JSON+重试）
  - `omem-server/src/embed/service.rs` — EmbedService trait + embed签名
  - `omem-server/src/store/lancedb.rs` — find_memories_by_session_id()（注意：不过滤memory_type，需在内存中过滤）
  - **注意**：`apply_append` 是 memory.rs handler 内的局部闭包（不是公开函数），refine_service.rs 不可直接调用。精炼回退时由 handler 层走原有闭包路径。
  - `omem-server/src/ingest/refine_prompt.rs`（Task 1产出，RefineInput/RefineOutput类型）

  **Acceptance Criteria**:
  - [ ] 文件 `ingest/refine_service.rs` 存在
  - [ ] 3个公开函数：collect_chain_memories, find_similar_work_memory, refine_and_replace
  - [ ] BFS有visited set + max_depth=5
  - [ ] 物理删除用 store.batch_hard_delete_by_ids()（先存后删）
  - [ ] 用 complete_json::<RefineOutput>() 而非手动解析
  - [ ] 字数硬截断按句子边界
  - [ ] tier/importance/tags继承
  - [ ] `cargo build -p omem-server` 通过

  **QA Scenarios**:
  ```
  Scenario: 编译验证
    Tool: Bash
    Steps:
      1. cargo build -p omem-server 2>&1 | tail -5
    Expected Result: 编译成功
    Evidence: .omo/evidence/task-2-build.txt

  Scenario: BFS安全检查
    Tool: Bash
    Steps:
      1. grep "max_depth" omem-server/src/ingest/refine_service.rs
      2. grep "visited" omem-server/src/ingest/refine_service.rs
    Expected Result: 两项都找到
    Evidence: .omo/evidence/task-2-bfs-safety.txt

  Scenario: 物理删除检查
    Tool: Bash
    Steps:
      1. grep "batch_hard_delete_by_ids\|hard_delete" omem-server/src/ingest/refine_service.rs
    Expected Result: 找到hard_delete调用（物理删除旧记忆）
    Evidence: .omo/evidence/task-2-delete.txt
  ```

  **Commit**: YES (groups with T1, T3)
  - Message: `feat(ingest): add refine prompt and service modules`
  - Files: `omem-server/src/ingest/refine_service.rs, omem-server/src/ingest/mod.rs`

- [ ] 3. **SESSION_EXTRACT prompt l1格式+字数更新** — `ingest/prompts.rs`

  **What to do**:
  - 修改 `SESSION_EXTRACT_SYSTEM_PROMPT`（搜索 `SESSION_EXTRACT_SYSTEM_PROMPT` 定位）
  - 更新l1_overview描述：添加箭头脉络格式说明
    - 示例：`"l1_overview": "发现问题→定位根因→修复→验证→发布"`
    - 每个节点是动词短语，箭头表示时间/因果递进
  - 更新字数限制：
    - summary(content): WORK从≤800字改为≤500字
    - detail(l2_content): 从≤500字改为≤300字
  - 在WORK OUTPUT FORMAT部分的l1_overview说明中添加箭头格式要求

  **Must NOT do**:
  - 不改EMOTIONAL相关的prompt内容
  - 不改Reconciler prompt
  - 不改输出JSON结构

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: None（独立改动）
  - **Blocked By**: None

  **References**:
  - `omem-server/src/ingest/prompts.rs` — SESSION_EXTRACT_SYSTEM_PROMPT（搜索 `SESSION_EXTRACT_SYSTEM_PROMPT` 定位）
  - `omem-server/src/ingest/prompts.rs` — OUTPUT FORMAT中的l1_overview和detail字数说明（搜索 `l1_overview` 和字数限制关键词定位）

  **Acceptance Criteria**:
  - [ ] SESSION_EXTRACT_SYSTEM_PROMPT包含l1箭头脉络格式说明
  - [ ] WORK summary字数限制改为≤500
  - [ ] detail字数限制改为≤300
  - [ ] `cargo build -p omem-server` 通过
  - [ ] `cargo test -p omem-server` 通过

  **QA Scenarios**:
  ```
  Scenario: l1箭头格式验证
    Tool: Bash
    Steps:
      1. grep -n "箭头\|arrow\|节点.*→" omem-server/src/ingest/prompts.rs
    Expected Result: 找到l1箭头脉络格式说明
    Evidence: .omo/evidence/task-3-l1-format.txt

  Scenario: 字数限制验证
    Tool: Bash
    Steps:
      1. grep -n "500" omem-server/src/ingest/prompts.rs | grep -i "summary\|content"
      2. grep -n "300" omem-server/src/ingest/prompts.rs | grep -i "detail"
    Expected Result: WORK summary≤500, detail≤300
    Evidence: .omo/evidence/task-3-word-limits.txt

  Scenario: 测试通过
    Tool: Bash
    Steps:
      1. cargo test -p omem-server ingest::prompts 2>&1 | tail -10
    Expected Result: 所有prompt相关测试通过
    Evidence: .omo/evidence/task-3-tests.txt
  ```

  **Commit**: YES (groups with T1, T2)
  - Message: `feat(ingest): add refine prompt and service modules`
  - Files: `omem-server/src/ingest/prompts.rs`

- [ ] 4. **handler层WORK追加→精炼替换** — `api/handlers/memory.rs`

  **What to do**:
  - 修改 memory.rs 中 `if memory_type == "WORK"` 区域的WORK记忆追加逻辑
  - 替换为per-topic精炼判断：

  ```
  for each WORK topic:
    ① 双重guard: memory_type == "WORK" AND scope != "private"
    ② 调 find_similar_work_memory(store, embed, topic.l0_abstract, session_id, tenant_id)
    ③ 有匹配（cosine > 0.72）:
       a. collect_chain_memories(store, matched_memory)
       b. refine_and_replace(store, llm, embed, matched, chain, topic.summary, topic.topic)
       c. 更新 existing_work_memory = Some(refined_mem)
    ④ 无匹配: 直接创建新记忆（保持原有逻辑）
  ```

  - EMOTIONAL追加逻辑（搜索 `if memory_type == "EMOTIONAL"` 定位）保持不动
  - 精炼失败时回退到原有追加逻辑

  **Must NOT do**:
  - 不改EMOTIONAL追加逻辑
  - 不改3000字split逻辑（精炼后仍需检查）
  - 不在精炼期间释放session锁（全程持锁，玄机建议）
  - 不删原有追加代码（作为fallback保留）

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 6, 7, 8
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `omem-server/src/api/handlers/memory.rs` — session_ingest入口（搜索 `pub async fn session_ingest` 定位，fire-and-forget spawn + per-session Mutex）
  - `omem-server/src/api/handlers/memory.rs` — WORK追加逻辑（搜索 `if memory_type == "WORK"` 定位，section替换逻辑）
  - `omem-server/src/api/handlers/memory.rs` — 记忆创建逻辑（搜索 `Memory::new` 定位）
  - `omem-server/src/ingest/refine_service.rs` — Task 2产出的3个函数
  - `omem-server/src/ingest/refine_prompt.rs` — Task 1产出的prompt构建

  **Acceptance Criteria**:
  - [ ] WORK记忆有旧记忆时走精炼路径
  - [ ] 无旧记忆时直接创建（不调LLM）
  - [ ] EMOTIONAL记忆不受影响
  - [ ] 精炼失败回退到原有追加
  - [ ] `cargo build -p omem-server` 通过
  - [ ] `cargo test -p omem-server` 通过

  **QA Scenarios**:
  ```
  Scenario: 编译+测试
    Tool: Bash
    Steps:
      1. cargo build -p omem-server 2>&1 | tail -5
      2. cargo test -p omem-server 2>&1 | tail -10
    Expected Result: 编译成功，所有测试通过
    Evidence: .omo/evidence/task-4-build-test.txt

  Scenario: EMOTIONAL不受影响
    Tool: Bash
    Steps:
      1. grep -n "EMOTIONAL" omem-server/src/api/handlers/memory.rs | head -20
    Expected Result: EMOTIONAL追加逻辑未被修改
    Evidence: .omo/evidence/task-4-emotional-unchanged.txt
  ```

  **Commit**: YES (groups with T5, T6)
  - Message: `feat(ingest): replace WORK append with refine logic`
  - Files: `omem-server/src/api/handlers/memory.rs`
  - Pre-commit: `cargo test -p omem-server`

- [ ] 5. **物理删除旧记忆逻辑** — `ingest/refine_service.rs`

  **What to do**:
  - 在refine_service.rs的refine_and_replace函数中实现物理删除逻辑
  - 精炼成功后：
    1. 收集链上所有旧记忆的ID列表
    2. 调 `store.batch_hard_delete_by_ids(&old_ids)` 批量物理删除
    3. 日志记录被删除的记忆ID列表
  - 注意：先存精炼后的新记忆，再删旧记忆（保证不丢数据）
  - 如果新记忆存入失败 → 不删旧记忆（安全优先）
  - 删除后不做全局relation清理（其他记忆指向已删ID是可接受的soft state，不影响功能，后续可按需清理）

  **Must NOT do**:
  - 不用superseded_by逻辑删除
  - 新记忆存入失败时不删旧记忆

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 6)
  - **Blocks**: Task 9
  - **Blocked By**: Task 2

  **References**:
  - `omem-server/src/store/lancedb.rs` — LanceStore.hard_delete() 和 batch_hard_delete_by_ids() 方法签名
  - `omem-server/src/ingest/refine_service.rs` — refine_and_replace函数（在Task 2中创建）

  **Acceptance Criteria**:
  - [ ] 精炼成功后旧记忆被物理删除
  - [ ] 新记忆存入失败时不删旧记忆
  - [ ] `cargo build -p omem-server` 通过

  **QA Scenarios**:
  ```
  Scenario: 物理删除逻辑检查
    Tool: Bash
    Steps:
      1. grep "batch_hard_delete_by_ids\|hard_delete" omem-server/src/ingest/refine_service.rs
    Expected Result: 找到hard_delete调用
    Evidence: .omo/evidence/task-5-delete.txt

  Scenario: 安全优先检查
    Tool: Bash
    Steps:
      1. grep -B5 "delete" omem-server/src/ingest/refine_service.rs | head -20
    Expected Result: delete在create成功之后执行
    Evidence: .omo/evidence/task-5-safety.txt
  ```

  **Commit**: YES (groups with T4, T6)
  - Message: `feat(ingest): replace WORK append with refine logic`
  - Files: `omem-server/src/ingest/refine_service.rs`

- [ ] 6. **handler层EMOTIONAL guard + 双重检查** — `api/handlers/memory.rs`

  **What to do**:
  - 在精炼入口添加双重guard：
    ```rust
    if memory_type == "WORK" && topic.scope.as_deref() != Some("private") {
        // 走精炼逻辑
    } else {
        // 保持原有追加逻辑（EMOTIONAL + private scope）
    }
    ```
  - 确认EMOTIONAL和private scope的记忆完全绕过精炼路径
  - 在精炼入口添加日志：info级别记录是否走精炼、跳过原因

  **Must NOT do**:
  - 不改EMOTIONAL的追加行为
  - 不改private scope的处理

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5)
  - **Blocks**: Task 9
  - **Blocked By**: Task 4

  **References**:
  - `omem-server/src/api/handlers/memory.rs` — WORK判断区域（搜索 `if memory_type == "WORK"` 定位）
  - `omem-server/src/api/handlers/memory.rs` — memory_type和scope赋值逻辑（搜索 `memory_type` 和 `scope` 变量赋值定位）

  **Acceptance Criteria**:
  - [ ] 精炼只在 memory_type == "WORK" && scope != "private" 时触发
  - [ ] EMOTIONAL和private scope走原有追加
  - [ ] 有清晰的日志区分
  - [ ] `cargo build -p omem-server` 通过

  **QA Scenarios**:
  ```
  Scenario: 双重guard验证
    Tool: Bash
    Steps:
      1. grep -n "WORK.*private\|private.*WORK\|memory_type.*scope" omem-server/src/api/handlers/memory.rs
    Expected Result: 找到双重guard条件
    Evidence: .omo/evidence/task-6-dual-guard.txt
  ```

  **Commit**: YES (groups with T4, T5)
  - Message: `feat(ingest): replace WORK append with refine logic`
  - Files: `omem-server/src/api/handlers/memory.rs`

- [ ] 7. **字数硬截断 + OOM保护** — `ingest/refine_service.rs`

  **What to do**:
  - 在refine_service.rs中添加保护逻辑：

  **字数硬截断**（不依赖LLM遵守）：
- l1_overview: 按字符数截断，优先在句子边界（。！？\n）处截断，超限则强制 chars().take(150)
- l2_content: 按字符数截断，优先在句子边界（。！？\n）处截断，超限则强制 chars().take(300)
  - refined_content: 不硬截断（不超过3000字split即可）
  - 截断后加 "..." 后缀

  **OOM保护**（精炼输入）：
  - 传入LLM的旧记忆总长度限制：MAX_INPUT_CHARS = 8000
  - 链太长时只取最新3条记忆
  - 单条记忆超过3000字截断到3000字
  - 如果截断后仍超限，只保留最新1条

  **Must NOT do**:
  - 不在截断中丢失关键信息（截断content，不截断l0/l1/l2摘要）
  - 不修改LLM prompt的字数说明（prompt仍然要求LLM遵守，代码层是兜底）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 8, 9)
  - **Blocks**: Task 9
  - **Blocked By**: Tasks 2, 4

  **References**:
  - `omem-server/src/ingest/refine_service.rs` — refine_and_replace函数（在Task 2中创建）

  **Acceptance Criteria**:
  - [ ] refined_content不硬截断（3000字split兜底）
  - [ ] l1硬截断≤150字
  - [ ] l2硬截断≤300字
  - [ ] 输入总长度≤8000字
  - [ ] 最多取3条旧记忆
  - [ ] `cargo build -p omem-server` 通过

  **QA Scenarios**:
  ```
  Scenario: 硬截断常量检查
    Tool: Bash
    Steps:
      1. grep -n "take(150)\|take(300)\|MAX_INPUT" omem-server/src/ingest/refine_service.rs
    Expected Result: 找到l1/l2截断常量
    Evidence: .omo/evidence/task-7-truncation.txt
  ```

  **Commit**: YES (groups with T8)
  - Message: `fix(ingest): add hardening guards and fallback`
  - Files: `omem-server/src/ingest/refine_service.rs`

- [ ] 8. **精炼失败回退 + 日志完善** — `api/handlers/memory.rs`, `ingest/refine_service.rs`

  **What to do**:
  - handler层：精炼调用用 `tokio::time::timeout(Duration::from_secs(30), refine_and_replace(...))` 包裹（防LLM hang住），Err/Timeout时回退到原有追加逻辑
    ```rust
    match tokio::time::timeout(
        Duration::from_secs(30),
        refine_and_replace(...)
    ).await {
        Ok(Ok(refined)) => { existing_work_memory = Some(refined); }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "session_ingest: refine failed, falling back to append");
            // 执行原有section追加逻辑
        }
        Err(_) => {
            tracing::warn!("session_ingest: refine timed out (30s), falling back to append");
            // 执行原有section追加逻辑
        }
    }
    ```
  - 添加关键日志：
    - 精炼入口：info级别，记录topic、旧记忆数
    - 精炼成功：info级别，记录新content字数、是否split
    - 精炼失败：warn级别，记录错误原因、回退到追加
    - 物理删除：info级别，记录被删除的旧记忆ID列表

  **Must NOT do**:
  - 不在精炼失败时丢数据（必须回退到追加）
  - 不添加debug级别以下的日志

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 7, 9)
  - **Blocks**: Task 9
  - **Blocked By**: Task 4

  **References**:
  - `omem-server/src/api/handlers/memory.rs` — 原有WORK追加逻辑（搜索 `if memory_type == "WORK"` 定位，作为fallback保留）

  **Acceptance Criteria**:
  - [ ] 精炼失败时执行原有追加
  - [ ] 有warn级别日志记录失败原因
  - [ ] `cargo build -p omem-server` 通过

  **QA Scenarios**:
  ```
  Scenario: fallback逻辑检查
    Tool: Bash
    Steps:
      1. grep -n "refine failed\|falling back" omem-server/src/api/handlers/memory.rs
    Expected Result: 找到fallback日志
    Evidence: .omo/evidence/task-8-fallback.txt
  ```

  **Commit**: YES (groups with T7)
  - Message: `fix(ingest): add hardening guards and fallback`
  - Files: `omem-server/src/api/handlers/memory.rs, omem-server/src/ingest/refine_service.rs`

- [ ] 9. **集成测试** — `api/mod.rs`

  **What to do**:
  - 在 `api/mod.rs` 的 tests 模块中添加session_ingest精炼测试
  - 使用现有 `setup_app()` helper
  - 测试场景：
    1. **首次ingest无旧记忆**：直接创建，不调精炼LLM
    2. **第二次ingest同主题**：触发精炼，验证content去重、l0/l1/l2更新
    3. **第三次ingest新主题**：不匹配，创建新记忆
    4. **EMOTIONAL不精炼**：EMOTIONAL记忆走原有追加
    5. **超3000字split**：精炼后内容超长，验证split + relation
  - Mock LLM：返回预设的精炼JSON
  - 验证：新记忆content长度 < 旧记忆content长度（去重生效）

  **Must NOT do**:
  - 不引入新测试框架
  - 不破坏现有测试

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 (sequential, depends on all)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 4, 5, 6, 7, 8

  **References**:
  - `omem-server/src/api/mod.rs` — 现有35个集成测试
  - `omem-server/src/api/mod.rs:setup_app()` — 测试helper
  - 测试模式参考：现有session_ingest测试（如果有的话）

  **Acceptance Criteria**:
  - [ ] 至少5个测试场景覆盖
  - [ ] `cargo test -p omem-server api::tests` 全部通过
  - [ ] EMOTIONAL不精炼有专门测试

  **QA Scenarios**:
  ```
  Scenario: 测试通过
    Tool: Bash
    Steps:
      1. cargo test -p omem-server api::tests 2>&1 | tail -20
    Expected Result: 所有测试通过（含新增精炼测试）
    Evidence: .omo/evidence/task-9-tests.txt

  Scenario: 新增测试数量
    Tool: Bash
    Steps:
      1. grep -c "async fn test_refine\|async fn test_session_ingest_refine" omem-server/src/api/mod.rs
    Expected Result: ≥5个新增测试
    Evidence: .omo/evidence/task-9-test-count.txt
  ```

  **Commit**: YES
  - Message: `test(ingest): add refine integration tests`
  - Files: `omem-server/src/api/mod.rs`
  - Pre-commit: `cargo test -p omem-server`

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, grep pattern). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .omo/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy` + `cargo test -p omem-server`. Review all changed files for: unwrap() in prod code, empty catches, unused imports, over-abstraction. Check AI slop: excessive comments, generic names.
  Output: `Clippy [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real QA Scenarios** — `unspecified-high`
  Start server, execute QA scenarios from every task — follow exact steps, capture evidence. Test cross-task integration. Test edge cases: empty content, very long content, concurrent ingests. Save to `.omo/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git diff). Verify 1:1 — everything in spec was built, nothing beyond spec was built. Check "Must NOT do" compliance. Detect cross-task contamination. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **T1-T3**: `feat(ingest): add refine prompt and service modules` — ingest/refine_prompt.rs, ingest/refine_service.rs, ingest/prompts.rs
- **T4-T6**: `feat(ingest): replace WORK append with refine logic` — api/handlers/memory.rs, ingest/refine_service.rs
- **T7-T8**: `fix(ingest): add hardening guards and fallback` — ingest/refine_service.rs, api/handlers/memory.rs
- **T9**: `test(ingest): add refine integration tests` — api/mod.rs

---

## Success Criteria

### Verification Commands
```bash
cargo build -p omem-server          # Expected: success, zero new warnings
cargo test -p omem-server           # Expected: all tests pass
cargo clippy -p omem-server         # Expected: no new warnings
```

### Final Checklist
- [ ] WORK记忆精炼正常工作（同主题去重）
- [ ] EMOTIONAL记忆不受影响
- [ ] l0/l1/l2同步更新（箭头格式l1）
- [ ] 3000字split正常
- [ ] 旧记忆物理删除（store.batch_hard_delete_by_ids()）
- [ ] 精炼失败回退到原有追加
- [ ] 所有现有测试通过
