# FETCH_POLICY + 灵魂低语 Prompt 内容升级

## TL;DR

> **Quick Summary**: 升级 FETCH_POLICY 和灵魂低语的 prompt 语气为指令性（nudgeForce = strong），不改任何注入逻辑或 XML 结构。
> 
> **Deliverables**:
> - FETCH_POLICY prompt 文本升级
> - 灵魂低语 buildWhisperText prompt 微调
> 
> **Estimated Effort**: Quick (<30 min)
> **Parallel Execution**: NO - 两个文本修改可以合并为一个任务
> **Critical Path**: Task 1 → Task 2 (review) → Done

---

## Context

### Original Request
师尊要求 FETCH_POLICY 和灵魂低语的 prompt 按 nudgeForce = strong 的逻辑设计——注入位置 + 语气双 strong。

### Interview Summary
**Key Discussions**:
- Oracle（玄机）分析后认为 FETCH_POLICY 不应迁移到 system.transform（语义依赖"上面有摘要"）
- 师尊采纳玄机建议：FETCH_POLICY 继续跟摘要走 parts.unshift
- 月儿发现上一版（v1.15.0）已用 `<cerebro-context>` XML 标签，结构无需改动
- 最终确认：**只需要改两个 prompt 的文本内容**

**Research Findings**:
- DCP nudgeForce strong: 注入到 user 消息（强注意力位置）
- DCP system.ts 语气参考: `"The ONLY tool..."`, `"Do NOT output them."`, `"It is of your responsibility to..."`
- 上一版代码已正确使用 `<cerebro-context>` + `<cerebro-fetch-policy>` + `<cerebro-profile>` 分层 XML 标签

### Metis Review
**Identified Gaps** (addressed):
- 空摘要 edge case: FETCH_POLICY 已有 `if (block)` 条件保护，无摘要时不注入 ✅
- profileBlock/KEYWORD_NUDGE 不合并: 上一版已独立注入 ✅
- 多引用点同步: FETCH_POLICY 是 const 常量，改一处自动同步 ✅

---

## Work Objectives

### Core Objective
升级 FETCH_POLICY 和灵魂低语的 prompt 文本内容，从建议性语气改为指令性语气，不改任何代码逻辑。

### Concrete Deliverables
- `plugins/opencode/src/hooks.ts`: FETCH_POLICY 常量文本替换
- `plugins/opencode/src/hooks.ts`: buildWhisperText 函数文本替换

### Definition of Done
- [ ] `npm run build` 编译零错误
- [ ] FETCH_POLICY 使用 IMPORTANT/MUST 指令性语气
- [ ] 灵魂低语 prompt 稍加强化但保持建议性

### Must Have
- FETCH_POLICY 使用强制性指令语气（参考 DCP system.ts 风格）
- 灵魂低语保持建议性但比当前版本更具体、更实用
- 编译零错误

### Mandatory Rules (师尊铁律)
- **搜索代码必须使用 codegraph** — 禁止用 grep/find 搜索代码，必须用 codegraph_search / codegraph_node / codegraph_explore
- **部署前必须加载 omem-iteration skill** — 所有弟子委派必须带上 `omem-iteration` skill

### Must NOT Have (Guardrails)
- 不改任何注入逻辑（partsToInject 构造、output.parts.unshift）
- 不改 XML 标签结构（`<cerebro-context>`, `<cerebro-fetch-policy>`, `<cerebro-profile>`）
- 不改注入路径（parts vs system.transform）
- 不改触发条件（pendingToolCalls、isFirstInjection 等）
- 不改 autoRecallHook/compactingHook 的流程逻辑
- 不改 index.ts、config.ts、package.json
- 不改后端代码
- 不引入新依赖

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: NO (插件无测试基础设施)
- **Automated tests**: None
- **Framework**: N/A

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

- **编译验证**: Use Bash — `npm run build`, assert exit code 0
- **内容验证**: Use Bash — grep 确认新文本已替换

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately):
└── Task 1: 升级 FETCH_POLICY + 灵魂低语 prompt 文本 [quick]

Wave FINAL:
├── Task F1: 编译验证 + 内容验证 [quick]
└── Task F2: 明镜(Momus) 评审 [oracle]
```

### Dependency Matrix

- **1**: None → F1, F2

### Agent Dispatch Summary

- **Wave 1**: 1 task — T1 → `quick`
- **FINAL**: 2 tasks — F1 → `quick`, F2 → `oracle`

---

## TODOs

- [x] 1. 升级 FETCH_POLICY + 灵魂低语 Prompt 文本

  **What to do**:
  - 修改 `plugins/opencode/src/hooks.ts` L249-253 的 `FETCH_POLICY` 常量
    - 从当前建议性语气改为指令性语气
    - 参考 DCP system.ts 风格: `"The ONLY tool..."`, `"Do NOT output them."`, `"It is of your responsibility to..."`
    - 保持 `<cerebro-fetch-policy>` XML 标签包裹
    - 核心信息不变：memory_get("id") 可以获取完整内容
  - 修改 `plugins/opencode/src/hooks.ts` L1479-1487 的 `buildWhisperText` 函数中的 prompt
    - 稍加强化但保持建议性
    - 保持 `<cerebro-memory-activation>` XML 标签包裹
    - 保持两种分支（toolNames <= maxToolNames 和 > maxToolNames）
    - 核心信息不变：提醒先搜记忆再行动

  **FETCH_POLICY 当前文本** (hooks.ts L249-253):
  ```
  <cerebro-fetch-policy>
  Each memory above is a condensed summary with a retrievable ID. memory_get("id") unlocks the full content — your knowledge depth control. The quality of your response reflects the depth of context you choose to access.
  </cerebro-fetch-policy>
  ```

  **FETCH_POLICY 目标风格** (指令性，DCP strong 参考):
  ```
  <cerebro-fetch-policy>
  IMPORTANT: Each memory above is a condensed summary. The full version contains critical details that may change your response quality.
  You MUST use memory_get("id") to retrieve the complete content before making decisions based on any summary.
  Do NOT rely on condensed summaries alone — depth of recall determines quality of response.
  </cerebro-fetch-policy>
  ```

  **灵魂低语当前文本** (hooks.ts L1479-1487):
  - 短版(<=maxToolNames): `"Your {toolNames} usage gets sharper with context. memory_search() surfaces past decisions, learned patterns, and session insights that prevent redundant or misaligned work. A moment of recall elevates every action."`
  - 长版(>maxToolNames): `"Before you act — your memory holds cross-session knowledge: past decisions, user preferences, hard-won insights. memory_search() activates this advantage. The strongest responses are built on remembered context."`

  **灵魂低语目标风格** (建议性但更具体):
  - 短版: `"Before using {toolNames}, memory_search() may surface relevant past decisions or patterns. Brief recall → better outcomes."`
  - 长版: `"Before you act — memory_search() surfaces cross-session context: past decisions, user preferences, hard-won insights. The strongest responses are built on remembered context."`

  **Must NOT do**:
  - 不改 `<cerebro-fetch-policy>` 和 `<cerebro-memory-activation>` 标签名
  - 不改 buildWhisperText 的函数签名或返回值类型
  - 不改 KEYWORD_NUDGE（keywords.ts）的内容
  - 不改 profileBlock 的格式
  - 不改 partsToInject 的构造逻辑
  - 不改 autoRecallHook、compactingHook 的流程

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 只改两个 const 字符串的文本内容
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 需要了解插件构建和验证流程

  **Parallelization**:
  - **Can Run In Parallel**: NO (single task)
  - **Parallel Group**: Sequential
  - **Blocks**: F1, F2
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `plugins/opencode/src/hooks.ts:249-253` — FETCH_POLICY 常量当前定义
  - `plugins/opencode/src/hooks.ts:1476-1490` — buildWhisperText 函数当前定义

  **API/Type References**:
  - `plugins/opencode/src/hooks.ts:871` — FETCH_POLICY 在 memoryInjectionHook 中的使用（`if (block) partsToInject.push(FETCH_POLICY)`）
  - `plugins/opencode/src/hooks.ts:535` — FETCH_POLICY 在 autoRecallHook 中的使用
  - `plugins/opencode/src/hooks.ts:1071-1073` — FETCH_POLICY 在 compactingHook 中的使用

  **External References**:
  - DCP system.ts 风格参考: `"The ONLY tool you have..."`, `"Do NOT output them."`, `"It is of your responsibility to..."`

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 编译验证
    Tool: Bash
    Preconditions: 代码已修改
    Steps:
      1. cd /mnt/d/dev/github/project/omem-server-source/plugins/opencode && npm run build
      2. Assert exit code === 0
      3. Assert no TypeScript compilation errors
    Expected Result: Build succeeds with zero errors
    Failure Indicators: Exit code !== 0, or any TypeScript error output
    Evidence: .omo/evidence/task-1-build-verify.txt

  Scenario: FETCH_POLICY 内容验证
    Tool: Bash (grep)
    Preconditions: 代码已修改并编译通过
    Steps:
      1. grep -n "IMPORTANT" plugins/opencode/src/hooks.ts — 确认新文本存在
      2. grep -n "MUST use memory_get" plugins/opencode/src/hooks.ts — 确认指令性语气
      3. grep -n "<cerebro-fetch-policy>" plugins/opencode/src/hooks.ts — 确认XML标签保持
      4. grep -c "Each memory above is a condensed summary with a retrievable ID" — 确认旧文本已删除
    Expected Result: 新文本存在，旧文本已删除，XML标签保持
    Evidence: .omo/evidence/task-1-fetch-policy-content.txt

  Scenario: 灵魂低语内容验证
    Tool: Bash (grep)
    Preconditions: 代码已修改并编译通过
    Steps:
      1. grep -n "Brief recall" plugins/opencode/src/hooks.ts — 确认新短版文本
      2. grep -n "Before you act" plugins/opencode/src/hooks.ts — 确认新长版文本
      3. grep -n "<cerebro-memory-activation>" plugins/opencode/src/hooks.ts — 确认XML标签保持
    Expected Result: 新文本存在，XML标签保持
    Evidence: .omo/evidence/task-1-whisper-content.txt

  Scenario: 不回归验证 — 确认注入逻辑未变
    Tool: Bash (grep)
    Preconditions: 代码已修改
    Steps:
      1. grep -n "partsToInject.push(FETCH_POLICY)" plugins/opencode/src/hooks.ts — 确认只有L871
      2. grep -n "partsToInject.push(block)" plugins/opencode/src/hooks.ts — 确认注入逻辑不变
      3. grep -n "output.parts.unshift" plugins/opencode/src/hooks.ts — 确认注入方式不变
    Expected Result: 注入逻辑代码行号和内容与修改前一致
    Evidence: .omo/evidence/task-1-no-regression.txt
  ```

  **Commit**: YES
  - Message: `feat(opencode): upgrade FETCH_POLICY and soul whisper prompt to strong directive tone`
  - Files: `plugins/opencode/src/hooks.ts`
  - Pre-commit: `npm run build`

---

## Final Verification Wave

- [x] F1. **编译 + 内容验证** — `quick`
  Run `npm run build`. Grep FETCH_POLICY 确认新文本（IMPORTANT/MUST）。Grep buildWhisperText 确认新文本。Grep 确认注入逻辑未变（partsToInject 构造、output.parts.unshift）。Grep 确认旧文本已删除。
  Output: `Build [PASS/FAIL] | FETCH_POLICY [NEW/OLD] | Whisper [NEW/OLD] | Injection Logic [UNCHANGED/CHANGED] | VERDICT`

- [x] F2. **明镜(Momus) 评审** — `oracle`
  Read hooks.ts diff。确认只改了两个 prompt 文本，没有意外改动。确认 XML 标签保持。确认语气升级合理。
  Output: `VERDICT: APPROVE/REJECT`

---

## Commit Strategy

- **1**: `feat(opencode): upgrade FETCH_POLICY and soul whisper prompt to strong directive tone` - plugins/opencode/src/hooks.ts

---

## Success Criteria

### Verification Commands
```bash
cd plugins/opencode && npm run build  # Expected: exit code 0, no errors
grep "IMPORTANT" src/hooks.ts         # Expected: FETCH_POLICY new text found
grep "Brief recall" src/hooks.ts      # Expected: whisper new text found
```

### Final Checklist
- [ ] FETCH_POLICY 使用 IMPORTANT/MUST 指令性语气
- [ ] 灵魂低语保持建议性但更具体实用
- [ ] `<cerebro-fetch-policy>` XML 标签保持
- [ ] `<cerebro-memory-activation>` XML 标签保持
- [ ] 注入逻辑零改动
- [ ] `npm run build` 编译零错误
