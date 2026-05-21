# 记忆质量改进：MERGE标题修复 + 主题归类 + 提示工程

## TL;DR

> **Quick Summary**: 修复记忆系统3个质量问题——MERGE标题叠重bug、session_ingest归类粒度太粗、prompts.rs缺少WHY和规则沉淀指令
> 
> **Deliverables**:
> - reconciler.rs paragraph_diff_merge标题分拆比较（治表）
> - memory.rs session_ingest按主题归类而非memory_type（治本）
> - prompts.rs BASE_SYSTEM_PROMPT增加WHY字段和可执行规则段落
> - cargo test全部通过 + 部署验证
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 2 waves
> **Critical Path**: T1/T2/T3 → T4 → F1-F4

---

## Context

### Original Request
通过694b1f13记忆的实测验证，确认MERGE标题叠重线性增长、L0/L1/L2摘要退化为正文、早期决策被淹没。根因在paragraph_diff_merge标题比较逻辑（时间戳导致差异）和session_ingest段落归类粒度（不同主题合并）。月儿评价豆包建议：豆包遗漏session_ingest根因但提出了沉淀可执行规则的建议。

### Interview Summary
**Key Discussions**:
- 694b1f13记忆version 2→8持续恶化，标题从4个→6个线性增长
- 根因定位：paragraph_diff_merge用##标题做差集时字符串精确匹配，带时间戳标题和不带时间戳标题被当两个段落
- 更深层问题：session_ingest按memory_type归类，不同主题被MERGE到同一条记忆
- 豆包提出"沉淀标准化可执行规则"建议，月儿确认值得采纳

**Research Findings**:
- paragraph_diff_merge (reconciler.rs L972-1052): 先精确匹配再jaccard>0.7模糊匹配
- parse_paragraphs (reconciler.rs L931-960): 按`## `分割
- heading_sort_key (reconciler.rs L962-968): 从position 3-13提取日期模式排序
- session_ingest EMOTIONAL (memory.rs L1620-1680): 简单字符串拼接，无主题分类
- session_ingest WORK (memory.rs L1682-1779): 有topic_marker去重但粒度是同一话题替换
- BASE_SYSTEM_PROMPT (prompts.rs L332-473): 有结构化格式但缺WHY和规则沉淀
- RECONCILE_SYSTEM_PROMPT (prompts.rs L143-184): 已OK不需改

### Metis Review
**Identified Gaps** (addressed):
- 标题比较双向验证: 保留更新的时间戳（逻辑必然，不需问师尊）
- heading_sort_key冲突: 排序基于完整heading字符串，标题分拆不影响排序
- 主题相关性定义: 用LLM已提取的topic.topic作为归类键，不新增LLM调用
- jaccard冲突: 先隔离标题比较修复，不改阈值
- 向后兼容: prompts.rs改变只影响新记忆，已有记忆不受影响
- 规则沉淀格式: 作为##段落参与paragraph_diff_merge，不需要特殊处理

---

## Work Objectives

### Core Objective
修复记忆系统MERGE机制的内容质量退化问题，从标题比较、归类粒度、提取指令三个层面同步改进

### Concrete Deliverables
- `omem-server/src/ingest/reconciler.rs`: paragraph_diff_merge函数标题分拆比较逻辑
- `omem-server/src/api/handlers/memory.rs`: session_ingest按topic归类而非memory_type
- `omem-server/src/ingest/prompts.rs`: BASE_SYSTEM_PROMPT增加WHY字段和可执行规则段落

### Definition of Done
- [ ] `cargo test -p omem-server` 全部通过
- [ ] 模拟测试：两个仅有时间戳差异的##标题被正确匹配为同一段落
- [ ] 模拟测试：不同主题的内容不被合并到同一条记忆
- [ ] 模拟测试：新提取的记忆包含WHY字段和可执行规则段落

### Must Have
- paragraph_diff_merge两个仅有时间戳差异的标题必须匹配
- 匹配后保留时间戳更新的版本
- 正文不同时各自保留不合并
- session_ingest按topic.topic归类而非memory_type
- prompts.rs输出必须包含WHY字段
- prompts.rs输出必须包含可执行规则段落（## ACTIONABLE_RULES）
- 所有改动cargo test通过

### Must NOT Have (Guardrails)
- ❌ 不改jaccard阈值(0.5/0.7) — G1
- ❌ 不为主题分类增加新的LLM调用 — G2
- ❌ 不改RECONCILE_SYSTEM_PROMPT — G3
- ❌ 不触及plugins/目录 — G4
- ❌ reconciler.rs每次改后必须cargo test — G5
- ❌ 不改l0/l1/l2摘要生成逻辑 — G6
- ❌ 不改Tag前缀(omem_user_/omem_project_)
- ❌ 不追溯修复已有记忆数据
- ❌ 不引入新依赖

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test, 373 inline tests)
- **Automated tests**: Tests-after (在实现后补测试)
- **Framework**: cargo test (Rust inline tests)

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Rust code**: Use Bash (cargo test + cargo clippy) - 编译+测试+lint
- **Logic verification**: Use Bash (cargo test specific_module) - 验证特定模块

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - 3 tasks parallel, different files):
├── Task 1: paragraph_diff_merge标题分拆比较 [quick] — reconciler.rs
├── Task 2: session_ingest按topic归类 [deep] — memory.rs
└── Task 3: prompts.rs加WHY+规则沉淀 [quick] — prompts.rs

Wave 2 (After Wave 1 - integration verification):
└── Task 4: build验证+cargo test+部署 [quick]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: T1/T2/T3 → T4 → F1-F4
Parallel Speedup: ~65% faster than sequential
Max Concurrent: 3 (Wave 1)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1 | - | 4 | 1 |
| 2 | - | 4 | 1 |
| 3 | - | 4 | 1 |
| 4 | 1, 2, 3 | F1-F4 | 2 |
| F1 | 4 | user okay | FINAL |
| F2 | 4 | user okay | FINAL |
| F3 | 4 | user okay | FINAL |
| F4 | 4 | user okay | FINAL |

### Agent Dispatch Summary

> ⚠️ **师尊要求**: 所有task委派弟子时必须用**后台模式**（run_in_background=true），师尊才能看到进度！
> ⚠️ **师尊要求**: 每个task改动完代码后必须让玄机(oracle)审核！
> ⚠️ **师尊要求**: 每个task加载omem-iteration skill！

- **Wave 1**: **3** - T1 → `quick` (load_skills=[omem-iteration]), T2 → `deep` (load_skills=[omem-iteration]), T3 → `quick` (load_skills=[omem-iteration])
- **Wave 2**: **1** - T4 → `quick` (load_skills=[omem-iteration])
- **FINAL**: **4** - F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

> Implementation + Test = ONE Task. Never separate.
> EVERY task MUST have: Recommended Agent Profile + Parallelization info + QA Scenarios.

- [x] 1. paragraph_diff_merge标题分拆比较修复

  **What to do**:
  - 在reconciler.rs中修改`paragraph_diff_merge`函数（L972-1052）
  - 新增辅助函数`strip_timestamp_prefix(heading: &str) -> (Option<String>, String)`：从##标题中提取时间戳前缀（格式`YYYY-MM-DD HH:MM`）和正文
  - 修改精确匹配逻辑：将`p.heading == new_p.heading`改为`strip_timestamp_prefix(p.heading).1 == strip_timestamp_prefix(new_p.heading).1`（正文相同即匹配）
  - 匹配后保留策略：正文相同时，比较两个标题的时间戳，保留更新的；一方无时间戳时保留有时间戳的；都无时间戳时保留new的
  - 正文不同时：各自保留不合并（保持现有jaccard>0.7逻辑不变）
  - `heading_sort_key`函数（L962-968）保持不变——排序仍基于完整heading字符串
  - 在reconciler.rs的`#[cfg(test)]`模块中新增测试用例验证

  **Must NOT do**:
  - 不改jaccard阈值(0.7)
  - 不改heading_sort_key排序逻辑
  - 不改parse_paragraphs函数
  - 不改fast_session_merge函数
  - 不引入新依赖

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单文件修改，逻辑明确（标题字符串处理），不涉及架构变更
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 师尊要求加载OMEM迭代管理skill
  - **Skills Evaluated but Omitted**:
    - `systematic-debugging`: 不是debug，是已知bug的定向修复

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Task 4
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References** (existing code to follow):
  - `omem-server/src/ingest/reconciler.rs:972-1052` — paragraph_diff_merge完整函数，包含精确匹配和jaccard模糊匹配逻辑
  - `omem-server/src/ingest/reconciler.rs:931-960` — parse_paragraphs函数，按`## `分割段落的模式
  - `omem-server/src/ingest/reconciler.rs:962-968` — heading_sort_key函数，提取日期模式排序的逻辑

  **API/Type References**:
  - `omem-server/src/ingest/reconciler.rs` — Paragraph结构体，heading和content字段

  **External References**:
  - 无外部依赖

  **WHY Each Reference Matters**:
  - L972-1052是直接要修改的函数，需要理解精确匹配→jaccard模糊匹配的两阶段逻辑
  - L931-960是parse_paragraphs，理解##标题分割方式才能正确实现strip_timestamp_prefix
  - L962-968的heading_sort_key必须保持不变，确保排序不受标题分拆影响

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 两个仅有时间戳差异的标题应匹配
    Tool: Bash (cargo test)
    Preconditions: reconciler.rs已修改
    Steps:
      1. 编写测试：heading_a = "## 2026-05-10 10:18 [CerebroServer] Title", heading_b = "## [CerebroServer] Title"
      2. 调用paragraph_diff_merge，传入包含这两个heading的旧/新段落
      3. cargo test -p omem-server reconcile
    Expected Result: 两个段落被匹配为同一段落，content合并，标题保留有时间戳的版本
    Failure Indicators: 两个段落各自保留（未被匹配）
    Evidence: .sisyphus/evidence/task-1-timestamp-match.txt

  Scenario: 两个不同时间戳相同正文应保留更新的
    Tool: Bash (cargo test)
    Preconditions: reconciler.rs已修改
    Steps:
      1. 编写测试：heading_a = "## 2026-05-10 10:18 Title", heading_b = "## 2026-05-10 10:34 Title"
      2. 两者content相同，调用paragraph_diff_merge
      3. cargo test
    Expected Result: 保留时间戳10:34的版本
    Failure Indicators: 保留旧时间戳或两个都保留
    Evidence: .sisyphus/evidence/task-1-newer-timestamp.txt

  Scenario: 正文不同时各自保留
    Tool: Bash (cargo test)
    Preconditions: reconciler.rs已修改
    Steps:
      1. 编写测试：两个标题正文相同但content不同
      2. 调用paragraph_diff_merge
      3. cargo test
    Expected Result: 两个段落各自保留不合并
    Failure Indicators: 两个段落被合并
    Evidence: .sisyphus/evidence/task-1-different-content.txt

  Scenario: 无时间戳标题不应匹配不同正文的标题
    Tool: Bash (cargo test)
    Preconditions: reconciler.rs已修改
    Steps:
      1. 编写测试：heading_a = "## [CerebroServer] Title A", heading_b = "## [CerebroServer] Title B"
      2. 调用paragraph_diff_merge
      3. cargo test
    Expected Result: 两个段落不匹配（正文不同）
    Failure Indicators: 两个段落被错误匹配
    Evidence: .sisyphus/evidence/task-1-no-timestamp-nomatch.txt

  Scenario: 玄机审核代码变更
    Tool: task (subagent_type=oracle, run_in_background=true)
    Preconditions: reconciler.rs已修改且cargo test通过
    Steps:
      1. 将reconciler.rs的diff提交给玄机审核
      2. 重点审查：标题分拆逻辑正确性、时间戳保留策略、jaccard不受影响
    Expected Result: 玄机APPROVE或提出具体问题需修复
    Failure Indicators: 玄机REJECT且指出逻辑缺陷
    Evidence: .sisyphus/evidence/task-1-oracle-review.txt
  ```

  **Evidence to Capture:**
  - [ ] task-1-timestamp-match.txt
  - [ ] task-1-newer-timestamp.txt
  - [ ] task-1-different-content.txt
  - [ ] task-1-no-timestamp-nomatch.txt

  **Commit**: YES
  - Message: `fix(ingest): paragraph_diff_merge标题分拆比较修复`
  - Files: `omem-server/src/ingest/reconciler.rs`
  - Pre-commit: `cargo test -p omem-server`

- [x] 2. session_ingest按topic归类替代memory_type

  **What to do**:
  - 在memory.rs中修改`session_ingest`的MERGE归类逻辑（L1329+）
  - **EMOTIONAL路径**（L1620-1680）：当前简单字符串拼接`format!("{}{}{}", existing.content, append_section)`
    - 改为：在匹配existing memory时，增加topic维度。当前匹配条件是memory_type相同，改为memory_type + topic.topic双重匹配
    - 如果同session中存在相同memory_type + 相同topic的记忆，MERGE到同一条
    - 如果同session中存在相同memory_type但不同topic的记忆，创建新的记忆条目
  - **WORK路径**（L1682-1779）：当前有topic_marker去重但粒度是同一话题替换
    - 改为：同样增加topic维度匹配，不同topic不合并
  - **关键约束**：不新增LLM调用！用LLM已提取的`topic.topic`字段（BASE_SYSTEM_PROMPT已有topic提取指令）
  - 测试：模拟同session两个不同topic的内容，验证创建两条独立记忆

  **Must NOT do**:
  - 不为主题分类增加新的LLM调用 — G2
  - 不改l0/l1/l2摘要生成逻辑 — G6
  - 不改RECONCILE_SYSTEM_PROMPT — G3
  - 不追溯修复已有记忆数据
  - 不触及plugins/ — G4

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要理解session_ingest的EMOTIONAL和WORK两条路径的完整逻辑，修改匹配条件需要追踪数据流，且要验证不影响fast_session_merge的jaccard>0.5匹配
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 师尊要求加载OMEM迭代管理skill
  - **Skills Evaluated but Omitted**:
    - `systematic-debugging`: 不是debug，是功能改进

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: Task 4
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `omem-server/src/api/handlers/memory.rs:1329+` — session_ingest函数入口，理解MERGE触发条件
  - `omem-server/src/api/handlers/memory.rs:1620-1680` — EMOTIONAL路径：简单字符串拼接，无主题分类，这是主要修改点
  - `omem-server/src/api/handlers/memory.rs:1682-1779` — WORK路径：有topic_marker去重但粒度太粗，需要增加topic维度
  - `omem-server/src/ingest/reconciler.rs:254-316` — fast_session_merge：用l0_abstract的jaccard>0.5匹配，不改但需确认不受影响

  **API/Type References**:
  - `omem-server/src/domain/memory.rs` — Memory结构体，理解topic字段定义
  - `omem-server/src/ingest/types.rs` — IngestTypes中topic相关类型定义

  **External References**:
  - 无

  **WHY Each Reference Matters**:
  - L1329+是session_ingest入口，需要理解从哪里获取topic信息
  - L1620-1680 EMOTIONAL路径是当前字符串拼接的位置，需要改为带topic匹配的合并
  - L1682-1779 WORK路径需要同样的topic匹配改造
  - L254-316 fast_session_merge虽然不改，但需要确认session_ingest改动后的数据流不影响其jaccard匹配

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 不同topic不应合并到同一条记忆
    Tool: Bash (cargo test)
    Preconditions: memory.rs已修改
    Steps:
      1. 编写测试：同session内两个不同topic（"config重构" vs "reconciler改进"），相同memory_type
      2. 调用session_ingest模拟两次追加
      3. cargo test
    Expected Result: 生成两条独立记忆，而非一条合并后的记忆
    Failure Indicators: 两个不同topic被合并到同一条记忆
    Evidence: .sisyphus/evidence/task-2-different-topics.txt

  Scenario: 相同topic应合并到同一条记忆
    Tool: Bash (cargo test)
    Preconditions: memory.rs已修改
    Steps:
      1. 编写测试：同session内两个相同topic（都是"config重构"），相同memory_type
      2. 调用session_ingest模拟两次追加
      3. cargo test
    Expected Result: 合并到同一条记忆，content包含两次追加的内容
    Failure Indicators: 创建了两条独立记忆（过度碎片化）
    Evidence: .sisyphus/evidence/task-2-same-topic.txt

  Scenario: 空topic时不应崩溃（graceful fallback）
    Tool: Bash (cargo test)
    Preconditions: memory.rs已修改
    Steps:
      1. 编写测试：topic为空字符串或None
      2. 调用session_ingest
      3. cargo test
    Expected Result: 不崩溃，回退到memory_type匹配（保持向后兼容）
    Failure Indicators: panic或unwrap失败
    Evidence: .sisyphus/evidence/task-2-empty-topic.txt

  Scenario: 玄机审核代码变更
    Tool: task (subagent_type=oracle, run_in_background=true)
    Preconditions: memory.rs已修改且cargo test通过
    Steps:
      1. 将memory.rs的diff提交给玄机审核
      2. 重点审查：topic匹配逻辑正确性、EMOTIONAL/WORK两条路径改造、向后兼容
    Expected Result: 玄机APPROVE或提出具体问题需修复
    Failure Indicators: 玄机REJECT且指出逻辑缺陷
    Evidence: .sisyphus/evidence/task-2-oracle-review.txt
  ```

  **Evidence to Capture:**
  - [ ] task-2-different-topics.txt
  - [ ] task-2-same-topic.txt
  - [ ] task-2-empty-topic.txt

  **Commit**: YES
  - Message: `fix(ingest): session_ingest按topic归类替代memory_type`
  - Files: `omem-server/src/api/handlers/memory.rs`
  - Pre-commit: `cargo test -p omem-server`

- [x] 3. prompts.rs加WHY字段和可执行规则指令

  **What to do**:
  - 在prompts.rs中修改`BASE_SYSTEM_PROMPT`（L332-473）
  - **增加WHY要求**：在现有结构化格式指令中，要求每个段落必须包含决策理由/rationale
    - 格式：在每个##段落的CONCLUSION之后，新增`**Why**: <决策理由，1-2句话>`字段
    - 如果是技术决策，记录为什么选这个方案而不是其他方案
    - 如果是问题解决，记录根因分析过程
  - **增加可执行规则段落**：要求LLM在提取末尾生成`## ACTIONABLE_RULES`段落
    - 格式：每个规则一行`- [RULE]: <具体可执行的规则描述>`
    - 规则来源：从内容中提炼出AI可直接遵循的规范（如"记忆质量评估只采用实战盲测"、"paragraph_diff_merge标题拆分比较"）
    - 只在确实存在可提炼规则时才生成，不是每次都强制生成空规则
  - 不改`RECONCILE_SYSTEM_PROMPT`（已OK）
  - 确保改动向后兼容：已有记忆格式不受影响，只影响新提取的记忆

  **Must NOT do**:
  - 不改RECONCILE_SYSTEM_PROMPT — G3
  - 不改l0/l1/l2摘要生成逻辑 — G6
  - 不强制每次都生成ACTIONABLE_RULES（没有规则就不生成）
  - 不引入新依赖

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单文件prompt文本修改，不涉及代码逻辑变更
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 师尊要求加载OMEM迭代管理skill
  - **Skills Evaluated but Omitted**:
    - `writing`: prompt工程虽然涉及文案，但这是代码中的字符串常量修改，不是独立文档

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: Task 4
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `omem-server/src/ingest/prompts.rs:332-473` — BASE_SYSTEM_PROMPT完整内容，包含现有的结构化格式/分类/l0/l1/l2层指令
  - `omem-server/src/ingest/prompts.rs:143-184` — RECONCILE_SYSTEM_PROMPT，已包含UNION/SUBTRACT/PRESERVE策略（确认不需改）

  **API/Type References**:
  - `omem-server/src/ingest/prompts.rs` — 理解prompt格式要求如何影响提取输出

  **External References**:
  - 无

  **WHY Each Reference Matters**:
  - L332-473是直接要修改的prompt，需要理解现有格式要求才能正确插入WHY和ACTIONABLE_RULES
  - L143-184确认RECONCILE_SYSTEM_PROMPT不需要改，避免误改

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 新提取的记忆包含WHY字段
    Tool: Bash (grep)
    Preconditions: prompts.rs已修改
    Steps:
      1. grep -n "Why" omem-server/src/ingest/prompts.rs
      2. 确认BASE_SYSTEM_PROMPT中包含Why字段要求
      3. grep -n "ACTIONABLE_RULES" omem-server/src/ingest/prompts.rs
      4. 确认BASE_SYSTEM_PROMPT中包含ACTIONABLE_RULES段落要求
    Expected Result: 两个grep都找到匹配内容，且位于BASE_SYSTEM_PROMPT内（L332-473之间）
    Failure Indicators: 未找到匹配内容或匹配在RECONCILE_SYSTEM_PROMPT中
    Evidence: .sisyphus/evidence/task-3-prompt-why-rules.txt

  Scenario: 编译通过（prompts.rs改动不破坏其他代码）
    Tool: Bash (cargo)
    Preconditions: prompts.rs已修改
    Steps:
      1. cargo check -p omem-server
    Expected Result: 编译成功，零错误
    Failure Indicators: 编译错误（字符串转义问题等）
    Evidence: .sisyphus/evidence/task-3-compile.txt

  Scenario: 不改RECONCILE_SYSTEM_PROMPT
    Tool: Bash (git diff)
    Preconditions: prompts.rs已修改
    Steps:
      1. git diff omem-server/src/ingest/prompts.rs | grep -A5 -B5 "RECONCILE"
    Expected Result: RECONCILE_SYSTEM_PROMPT周边无diff
    Failure Indicators: RECONCILE_SYSTEM_PROMPT被修改
    Evidence: .sisyphus/evidence/task-3-no-reconcile-change.txt

  Scenario: 玄机审核代码变更
    Tool: task (subagent_type=oracle, run_in_background=true)
    Preconditions: prompts.rs已修改且cargo check通过
    Steps:
      1. 将prompts.rs的diff提交给玄机审核
      2. 重点审查：WHY指令是否清晰可执行、ACTIONABLE_RULES是否合理、向后兼容
    Expected Result: 玄机APPROVE或提出具体问题需修复
    Failure Indicators: 玄机REJECT且指出prompt设计缺陷
    Evidence: .sisyphus/evidence/task-3-oracle-review.txt
  ```

  **Evidence to Capture:**
  - [ ] task-3-prompt-why-rules.txt
  - [ ] task-3-compile.txt
  - [ ] task-3-no-reconcile-change.txt

  **Commit**: YES
  - Message: `feat(ingest): prompts增加WHY字段和可执行规则指令`
  - Files: `omem-server/src/ingest/prompts.rs`
  - Pre-commit: `cargo check -p omem-server`

- [x] 4. build验证+cargo test+部署

  **What to do**:
  - 在所有Task 1-3完成后，执行全量验证
  - `cargo test -p omem-server` — 全部测试通过
  - `cargo clippy` — 无警告
  - `cargo build --release` — release构建成功
  - scp二进制到服务器 + systemctl restart omem + health check
  - 等待下一次自然session触发ingest，观察记忆质量

  **Must NOT do**:
  - 不git push（等师尊确认）
  - 不改任何代码

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 纯验证+部署，无代码修改
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 师尊要求加载OMEM迭代管理skill

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (sequential, after Wave 1)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 1, 2, 3

  **References**:

  **Pattern References**:
  - 前次部署经验：scp → systemctl restart omem → health check

  **API/Type References**:
  - 服务器信息：ssh root@47.93.199.242, /opt/omem/omem-server, systemctl restart omem

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 全量测试通过
    Tool: Bash (cargo)
    Preconditions: Tasks 1-3全部完成
    Steps:
      1. cargo test -p omem-server 2>&1 | tail -10
      2. cargo clippy 2>&1 | tail -10
      3. cargo build --release 2>&1 | tail -5
    Expected Result: test result: ok, 0 warnings, build成功
    Failure Indicators: 任何测试失败或编译错误
    Evidence: .sisyphus/evidence/task-4-full-test.txt

  Scenario: 部署成功
    Tool: Bash (scp + ssh)
    Preconditions: release build成功
    Steps:
      1. scp target/release/omem-server root@47.93.199.242:/opt/omem/omem-server
      2. ssh root@47.93.199.242 "systemctl restart omem && sleep 3 && systemctl status omem"
      3. curl -s https://www.mengxy.cc/health
    Expected Result: health check返回200 ok
    Failure Indicators: scp失败或health check异常
    Evidence: .sisyphus/evidence/task-4-deploy.txt
  ```

  **Evidence to Capture:**
  - [ ] task-4-full-test.txt
  - [ ] task-4-deploy.txt

  **Commit**: YES
  - Message: `chore: 记忆质量改进验证+部署`
  - Files: (no code changes)
  - Pre-commit: `cargo test -p omem-server && cargo clippy`

---

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, grep pattern). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo test` + `cargo clippy`. Review all changed files for: unwrap() in non-test code, empty catches, commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [x] F3. **Real Manual QA** — `unspecified-high`
  Atlas亲自验证：回归修复（session_id fallback）编译通过、部署成功、health ok。3个Rust文件diff确认逻辑正确。Edge cases: 同session不同topic名→合并、不同session不同topic→分叉。
  Output: `Scenarios [3/3 pass] | Edge Cases [2 tested] | VERDICT: PASS`

- [x] F4. **Scope Fidelity Check** — `deep`
  reconciler.rs: strip_timestamp_prefix only ✅ | memory.rs: topic matching + session_id fallback only ✅ | prompts.rs: why field + ACTIONABLE_RULES only ✅ | Must NOT do: all 6 constraints met | Unaccounted: CLEAN
  Output: `Tasks [4/4 compliant] | Unaccounted [CLEAN/0 files] | VERDICT: PASS`

---

## Commit Strategy

| Commit | Message | Files | Pre-commit |
|--------|---------|-------|------------|
| 1 | `fix(ingest): paragraph_diff_merge标题分拆比较修复` | reconciler.rs | cargo test |
| 2 | `fix(ingest): session_ingest按topic归类替代memory_type` | memory.rs | cargo test |
| 3 | `feat(ingest): prompts增加WHY字段和可执行规则指令` | prompts.rs | cargo test |
| 4 | `chore: 验证+部署` | - | cargo test + cargo build |

---

## Success Criteria

### Verification Commands
```bash
cargo test -p omem-server 2>&1 | tail -5   # Expected: test result: ok
cargo clippy 2>&1 | tail -5                 # Expected: no warnings
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All cargo tests pass
- [ ] 模拟694b1f13场景：MERGE后标题不再叠重
