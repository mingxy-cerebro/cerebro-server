# Recall 门控重构 + Compact 后重注入修复

## TL;DR

> **Quick Summary**: 修复两个问题：(1) recall 触发不应依赖死的关键词，应回归服务端 LLM `shouldRecall` 智能判断；(2) compact 后 injectedSessions 未清理导致记忆不再注入。
> 
> **Deliverables**:
> - hooks.ts memoryInjectionHook：移除 `isKeywordTriggered` recall 门控，改为每条消息都调服务端 `shouldRecall`（LLM 判断）
> - keywords.ts：精简为仅 SAVE 关键词，用于 `KEYWORD_NUDGE` 提示
> - hooks.ts compactingHook：清理 injectedSessions 使 compact 后能重注入
> 
> **Estimated Effort**: Medium (30-60 min)
> **Parallel Execution**: YES
> **Critical Path**: Task 1 + Task 2 (并行) → F1 + F2 (并行) → Done

---

## Context

### Original Request
师尊测试记忆注入后发现两个问题：
1. 关键词列表误触发率高（17个高危词），且用 keyword 触发 recall 本身就不合理
2. compact 后记忆不再注入

### 多方评审共识

**探虚(Explore) 调查发现**:
- 服务端 `POST /v1/should-recall`（session_recalls.rs L131-514）一直存在
- 旧版 `autoRecallHook` **没有客户端门控**，每条消息都调服务端 `shouldRecall`
- 新版 `memoryInjectionHook` 加了 `isFirstInjection || isKeywordTriggered` 客户端门控（L650-652），跳过了大部分消息

**玄机(Oracle) 评审结论**（专业建议）:
- **方案A（纯服务端 shouldRecall）最优**
- 服务端已有四重保护：频率限制(60s) + 语义去重(0.7) + LLM判断 + `has_recall_signals`安全阀
- `has_recall_signals`（L269-274）：LLM说no但query有信号时，放宽搜索阈值从0.60→0.50——**服务端已做了keyword兜底**，且比客户端更精准
- 方案C的"keyword兜底"不成立：客户端keyword强制触发会注入低质量记忆（绕过质量门槛）
- 性能可接受：60s内第二条消息 ~1-2ms快速拒绝

**师尊关键反馈**:
> "服务端都有LLM判断是否需要召回了，为什么要用keyword这么复杂，这么死的逻辑呢？？"

**月儿综合判断**: 三方一致同意方案A。客户端门控是过度优化，应回归服务端智能判断。keyword只保留SAVE用途。

### 当前架构（问题所在）

```
用户消息 → keywordDetectionHook → detectKeyword()
                    ↓
          keywordDetectedSessions.add()
                    ↓
memoryInjectionHook L650-652:
  if (!isFirstInjection && !isKeywordTriggered) return;  ← 问题：大部分消息被跳过
                    ↓ (只有首条或关键词才能到这里)
  shouldRecall() → 服务端 LLM 判断  ← 好的判断但来得太晚
```

### 目标架构

```
用户消息 → keywordDetectionHook → detectSaveKeyword()
                    ↓ (只检测 SAVE)
          saveKeywordDetectedSessions.add()
                    ↓
memoryInjectionHook:
  每条消息都调 shouldRecall() → 服务端做智能判断
  → 如果 isSaveKeyword → 额外注入 KEYWORD_NUDGE
  → injectedSessions 只用于去重/日志，不做门控
```

**核心变化**：
- 移除 `isFirstInjection || isKeywordTriggered` 客户端门控
- 每条消息都调 `shouldRecall`（服务端有四重保护，不会炸）
- 关键词检测只用于 SAVE（触发 KEYWORD_NUDGE）
- `injectedSessions` 从"门控标记"变为"去重/日志标记"

---

## Work Objectives

### Core Objective
1. 重构 recall 触发机制：回归服务端 LLM 智能判断，移除死板的客户端关键词门控
2. 修复 compactingHook：清理 injectedSessions
3. 精简 keywords.ts：只保留 SAVE 关键词

### Concrete Deliverables
- `plugins/opencode/src/hooks.ts`: memoryInjectionHook 门控逻辑重构
- `plugins/opencode/src/hooks.ts`: compactingHook 增加 injectedSessions 清理
- `plugins/opencode/src/keywords.ts`: 移除 RECALL_KEYWORDS，只保留 SAVE

### Definition of Done
- [ ] memoryInjectionHook 每条消息都调 shouldRecall（不再有 first/keyword 门控）
- [ ] SAVE 关键词触发 KEYWORD_NUDGE 额外注入
- [ ] compactingHook 清理 injectedSessions
- [ ] keywords.ts 只有 SAVE_KEYWORDS
- [ ] `npx tsc --noEmit` 编译零错误
- [ ] 不改服务端代码

### Must Have
- 服务端 shouldRecall LLM 判断保持不变
- 每条消息都走服务端智能判断
- injectedSessions 用于去重（不重复注入相同记忆内容），不是门控
- SAVE 关键词触发 KEYWORD_NUDGE（提示模型保存）
- compact 后首条消息能触发记忆重注入

### Mandatory Rules (师尊铁律)
- **部署前必须加载 omem-iteration skill**

### Must NOT Have (Guardrails)
- 不改服务端代码（session_recalls.rs、router.rs 等）
- 不改 shouldRecall API 接口
- 不改 autoRecallHook（降级路径保留）
- 不改 index.ts hook 注册逻辑
- 不改 config.ts
- 不改 package.json version
- 不引入新依赖
- 不改 compactingHook 的 ingest/poll/summary 逻辑

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: NO (插件无测试基础设施)
- **Automated tests**: None

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — 2 parallel tasks):
├── Task 1: hooks.ts 门控重构 + keywords.ts 精简 [unspecified-high]
└── Task 2: compactingHook 修复 injectedSessions [quick]

Wave FINAL (2 parallel tasks):
├── Task F1: 编译 + 内容验证 [quick]
└── Task F2: 明镜(Momus) 评审 [oracle]
```

### Dependency Matrix

- **1**: None → F1, F2
- **2**: None → F1, F2

---

## TODOs

- [x] 1. hooks.ts 门控重构 + keywords.ts 精简

  **What to do**:
  
  ### 1A: keywords.ts 精简
  
  - 移除所有 `RECALL_KEYWORDS`，只保留 `SAVE_KEYWORDS`（12个经过生产验证的词）
  - 恢复为原始简洁结构：
    ```typescript
    const SAVE_KEYWORDS: readonly string[] = [
      "remember",
      "save this",
      "don't forget",
      "keep in mind",
      "note that",
      "store this",
      "memorize",
      "记住",
      "记一下",
      "保存",
      "记下来",
      "别忘了",
    ] as const;

    export function detectSaveKeyword(text: string): boolean {
      const lower = text.toLowerCase();
      return SAVE_KEYWORDS.some((kw) => lower.includes(kw));
    }

    export const KEYWORD_NUDGE =
      "The user appears to want you to remember something. " +
      "Consider using the `memory_store` tool to save this information for future reference.";
    ```
  - **导出接口变更**：`detectKeyword` → `detectSaveKeyword`（需同步改 hooks.ts 调用点）
  - `KEYWORD_NUDGE` 回归为仅 SAVE 提示（移除 "or recall" 部分）
  
  ### 1B: hooks.ts memoryInjectionHook 门控重构
  
  **当前代码（L640-652）**:
  ```typescript
  return async (
    input: { sessionID?: string; messageID?: string; model: Model },
    output: { message: UserMessage; parts: Part[] },
  ) => {
    if (!input.sessionID) return;
    const agentId = getAgentName?.() || process.env.OMEM_AGENT_ID || "opencode";
    const policy = resolveAgentPolicy(agentId, config);
    if (policy === "none") return;
    const isFirstInjection = !injectedSessions.has(input.sessionID);
    const isKeywordTriggered = keywordDetectedSessions.has(input.sessionID);
    if (!isFirstInjection && !isKeywordTriggered) return;  ← 移除这行门控
  ```
  
  **改为**:
  ```typescript
  return async (
    input: { sessionID?: string; messageID?: string; model: Model },
    output: { message: UserMessage; parts: Part[] },
  ) => {
    if (!input.sessionID) return;
    const agentId = getAgentName?.() || process.env.OMEM_AGENT_ID || "opencode";
    const policy = resolveAgentPolicy(agentId, config);
    if (policy === "none") return;
    const isSaveKeyword = saveKeywordDetectedSessions.has(input.sessionID);  ← 改名
    // 注意：不再有 isFirstInjection/isKeywordTriggered 门控
    // 每条消息都会调 shouldRecall，服务端有频率限制+语义去重
  ```
  
  **具体改动清单**:
  1. L650-652: 移除 `isFirstInjection`/`isKeywordTriggered` 三行，替换为 `const isSaveKeyword = saveKeywordDetectedSessions.has(input.sessionID);`
  2. L874: `if (isKeywordTriggered) partsToInject.push(KEYWORD_NUDGE);` → `if (isSaveKeyword) partsToInject.push(KEYWORD_NUDGE);`
  3. L899: `keywordDetectedSessions.delete(input.sessionID);` → `saveKeywordDetectedSessions.delete(input.sessionID);`
  
  **关于 `injectedSessions` 的处理**:
  - 保留 `injectedSessions` 但语义从"门控"变为"去重标记"
  - 仍然在注入成功后 `injectedSessions.add(input.sessionID)`
  - 注入前检查：如果 session 已注入过，且 shouldRecall 返回空结果 → 跳过注入
  - 但如果 shouldRecall 返回了新结果（服务端判断需要）→ 仍然注入（即使 session 已在 injectedSessions 中）
  - **简单方案**：移除 injectedSessions 的门控作用，只用于日志记录
  
  ### 1C: hooks.ts keywordDetectionHook 改名
  
  **当前代码（L960-1000）**:
  ```typescript
  export function keywordDetectionHook(...) {
    ...
    if (detectKeyword(textContent)) {
      keywordDetectedSessions.add(input.sessionID);
    }
    ...
  }
  ```
  
  **改为**:
  ```typescript
  export function keywordDetectionHook(...) {
    ...
    if (detectSaveKeyword(textContent)) {
      saveKeywordDetectedSessions.add(input.sessionID);
    }
    ...
  }
  ```
  
  注意：只改内部调用的函数名和 Set 名，函数名 `keywordDetectionHook` 保持不变（不改 index.ts 的注册点）。
  
  **状态变量改名**:
  - `keywordDetectedSessions` → `saveKeywordDetectedSessions`（hooks.ts L170 附近声明处）
  - 所有引用点同步更新

  **Must NOT do**:
  - 不改 keywordDetectionHook 函数签名
  - 不改 keywordDetectionHook 的 sessionMessages 追踪逻辑
  - 不改 memoryInjectionHook 中 shouldRecall 调用方式
  - 不改注入方式（仍是 parts.unshift + synthetic:true）
  - 不改 index.ts
  - 不改 compactingHook（Task 2 单独改）

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 涉及 keywords.ts + hooks.ts 两个文件的协调修改，需要理解门控逻辑
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 需要了解插件构建和验证流程

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 2，但 Task 2 改 hooks.ts 不同位置，不冲突)
  - **Parallel Group**: Wave 1
  - **Blocks**: F1, F2
  - **Blocked By**: None

  **References**:
  
  - `plugins/opencode/src/hooks.ts:640-652` — memoryInjectionHook 门控代码
  - `plugins/opencode/src/hooks.ts:870-899` — partsToInject 构造和 keywordDetectedSessions 清理
  - `plugins/opencode/src/hooks.ts:960-1000` — keywordDetectionHook
  - `plugins/opencode/src/hooks.ts:170` — keywordDetectedSessions 声明
  - `plugins/opencode/src/keywords.ts:1-75` — 当前关键词定义
  - `plugins/opencode/src/client.ts:341` — shouldRecall 方法
  - `omem-server/src/api/handlers/session_recalls.rs:131-514` — 服务端三层门控

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 编译验证
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npx tsc --noEmit
      2. Assert exit code === 0
    Expected Result: 零 TypeScript 错误
    Evidence: .omo/evidence/task-1-build-verify.txt

  Scenario: 门控移除验证
    Tool: Bash (grep)
    Steps:
      1. grep -n "isFirstInjection\|isKeywordTriggered" plugins/opencode/src/hooks.ts — 应不存在
      2. grep -n "isSaveKeyword" plugins/opencode/src/hooks.ts — 应存在
      3. grep -n "saveKeywordDetectedSessions" plugins/opencode/src/hooks.ts — 应存在
    Expected Result: 旧门控已移除，新变量已替换
    Evidence: .omo/evidence/task-1-gate-removed.txt

  Scenario: keywords.ts 只有 SAVE
    Tool: Bash (grep)
    Steps:
      1. grep "RECALL_KEYWORDS" plugins/opencode/src/keywords.ts — 应不存在
      2. grep "detectSaveKeyword" plugins/opencode/src/keywords.ts — 应存在
      3. grep '"remember"' plugins/opencode/src/keywords.ts — 应存在
      4. grep '"记住"' plugins/opencode/src/keywords.ts — 应存在
      5. grep -c '"之前"' plugins/opencode/src/keywords.ts — 应为 0
    Expected Result: 只有 SAVE 关键词
    Evidence: .omo/evidence/task-1-save-only.txt

  Scenario: KEYWORD_NUDGE 回归 SAVE 提示
    Tool: Bash (grep)
    Steps:
      1. grep "KEYWORD_NUDGE" plugins/opencode/src/keywords.ts — 确认不含 "recall" 或 "memory_search"
    Expected Result: KEYWORD_NUDGE 只提示 memory_store
    Evidence: .omo/evidence/task-1-nudge-save-only.txt

  Scenario: compactingHook 不被修改
    Tool: Bash (grep)
    Steps:
      1. grep -n "sessionMessages.delete(input.sessionID)" plugins/opencode/src/hooks.ts — 确认 compactingHook 清理代码不变
    Expected Result: compactingHook 代码未被 Task 1 触碰
    Evidence: .omo/evidence/task-1-compacting-untouched.txt
  ```

  **Commit**: YES
  - Message: `refactor(opencode): remove keyword-gated recall, rely on server-side shouldRecall LLM judgment`
  - Files: `plugins/opencode/src/hooks.ts`, `plugins/opencode/src/keywords.ts`
  - Pre-commit: `npx tsc --noEmit`

- [x] 2. compactingHook 修复 injectedSessions 未清理

  **What to do**:
  - 在 `plugins/opencode/src/hooks.ts` compactingHook 的清理代码块中增加一行
  - 当前清理代码（L1160-1167）：
    ```typescript
    // Cleanup tracked messages regardless of ingest result
    sessionMessages.delete(input.sessionID);
    profileInjectedSessions.delete(input.sessionID);
    firstMessages.delete(input.sessionID);
    if (input.sessionID) {
      const deleted = pendingToolCalls.delete(input.sessionID);
      logDebug("compactingHook cleared session pendingToolCalls", { sessionID: input.sessionID, hadPending: deleted });
    }
    ```
  - **只需增加一行**：`injectedSessions.delete(input.sessionID);`（注意：如果 Task 1 改了变量名，此处用改名后的变量）
  - 放置位置：在 `sessionMessages.delete` 之后
  - 修改后的清理代码：
    ```typescript
    // Cleanup tracked messages regardless of ingest result
    sessionMessages.delete(input.sessionID);
    injectedSessions.delete(input.sessionID);  // ← 新增：允许 compact 后重注入
    profileInjectedSessions.delete(input.sessionID);
    firstMessages.delete(input.sessionID);
    ```

  **⚠️ 与 Task 1 的文件冲突**:
  - Task 1 改 hooks.ts L640-900（门控区域）+ L960-1000（keywordDetectionHook）+ L170（变量声明）
  - Task 2 改 hooks.ts L1160-1163（compactingHook 清理区域）
  - **两个区域不重叠**，可以并行

  **Must NOT do**:
  - 不改 compactingHook 的其他逻辑（ingest、poll、summary）
  - 不改 memoryInjectionHook
  - 不改 keywordDetectionHook
  - 只加一行代码

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 只加一行 delete
  - **Skills**: [`omem-iteration`]

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 1)
  - **Parallel Group**: Wave 1
  - **Blocks**: F1, F2
  - **Blocked By**: None

  **References**:
  - `plugins/opencode/src/hooks.ts:1160-1167` — compactingHook 当前清理代码
  - `plugins/opencode/src/hooks.ts:650` — injectedSessions 使用处（注意 Task 1 可能改此行）

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 编译验证
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npx tsc --noEmit
    Expected Result: 零 TypeScript 错误
    Evidence: .omo/evidence/task-2-build-verify.txt

  Scenario: injectedSessions.delete 已添加
    Tool: Bash (grep)
    Steps:
      1. grep -n "injectedSessions.delete" plugins/opencode/src/hooks.ts — 确认存在
      2. 确认行号在 compactingHook 清理代码块内（约 L1160 附近）
    Expected Result: injectedSessions.delete 在清理代码块中
    Evidence: .omo/evidence/task-2-injected-sessions-delete.txt
  ```

  **Commit**: YES
  - Message: `fix(opencode): clear injectedSessions after compact to allow re-injection`
  - Files: `plugins/opencode/src/hooks.ts`
  - Pre-commit: `npx tsc --noEmit`

---

## Final Verification Wave

- [x] F1. **编译 + 内容验证** — `quick`
  Run `npx tsc --noEmit`. Grep 确认：(1) `isFirstInjection`/`isKeywordTriggered` 不存在，(2) `isSaveKeyword`/`saveKeywordDetectedSessions` 存在，(3) keywords.ts 只有 SAVE，(4) KEYWORD_NUDGE 只提示 memory_store，(5) injectedSessions.delete 在 compactingHook 中，(6) keywordDetectionHook 调用 detectSaveKeyword。
  Output: `Build [PASS/FAIL] | Gate [REMOVED/EXISTS] | Keywords [SAVE_ONLY/HAS_RECALL] | Compact [FIXED/UNFIXED] | VERDICT`

- [x] F2. **明镜(Momus) 评审** — `oracle`
  Read hooks.ts diff + keywords.ts diff。确认：(1) 门控逻辑正确移除（每条消息走 shouldRecall），(2) SAVE 关键词仍触发 KEYWORD_NUDGE，(3) compactingHook 只加了一行 injectedSessions.delete，(4) 没有意外改动。
  Output: `VERDICT: APPROVE/REJECT`

---

## Commit Strategy

- **1**: `refactor(opencode): remove keyword-gated recall, rely on server-side shouldRecall LLM judgment` - hooks.ts, keywords.ts
- **2**: `fix(opencode): clear injectedSessions after compact to allow re-injection` - hooks.ts

---

## Success Criteria

### Verification Commands
```bash
cd plugins/opencode && npx tsc --noEmit  # Expected: exit code 0
grep "isFirstInjection" src/hooks.ts      # Expected: NOT found
grep "isSaveKeyword" src/hooks.ts         # Expected: found
grep "RECALL_KEYWORDS" src/keywords.ts    # Expected: NOT found
grep "injectedSessions.delete" src/hooks.ts  # Expected: found in compactingHook
```

### Final Checklist
- [ ] `isFirstInjection`/`isKeywordTriggered` 门控已移除
- [ ] 每条消息都走服务端 shouldRecall 智能判断
- [ ] keywords.ts 只有 SAVE 关键词
- [ ] KEYWORD_NUDGE 只提示 memory_store
- [ ] compactingHook 清理 injectedSessions
- [ ] `npx tsc --noEmit` 编译零错误
- [ ] detectSaveKeyword 导出签名正确
- [ ] keywordDetectionHook 函数名不变（不改 index.ts）
