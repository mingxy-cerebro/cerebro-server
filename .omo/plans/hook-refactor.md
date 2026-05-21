# Cerebro 插件 Hook 注入策略重构

## TL;DR

> **Quick Summary**: 把记忆注入从 `system.transform` 迁移到 `chat.message` + `parts.unshift` + `synthetic:true`，灵魂低语保留在 `system.transform`，同时升级 compactingHook 移植 supermemory 三阶段策略。
> 
> **Deliverables**:
> - 新 `memoryInjectionHook` 替代 `autoRecallHook`（chat.message hook）
> - 精简 `system.transform` 仅保留灵魂低语
> - 升级 `compactingHook`：6段式 prompt + 摘要保存
> - 降级策略：配置开关切换 parts/system 注入模式
> 
> **Estimated Effort**: Medium (1-2天)
> **Parallel Execution**: YES - 3 waves
> **Critical Path**: POC验证 → memoryInjectionHook → compactingHook升级 → 集成测试

---

## Context

### Original Request
v1.14.1~v1.14.6 反复打补丁解决智谱 GLM API 只处理 `system[0]` 的问题，但治标不治本。需要从根本上改变注入策略。

### Interview Summary
**Key Discussions**:
- 三方评审：玄机（GO with conditions）、灵犀（方案C+推荐）、supermemory 权威参考
- 技术验证：灵犀+玄机一致确认 Part.synthetic 官方支持，GLM 兼容 parts 注入
- compactingHook 升级：移植 supermemory 三阶段策略，增加 6段式 prompt + 摘要保存

**Research Findings**:
- supermemory 和 opencode-mem 都用 `parts.unshift` + `synthetic:true`（生产验证）
- OpenCode SDK `TextPart` 有 `synthetic?: boolean` 官方字段
- `chat.message` 先于 `system.transform` 执行，不冲突
- GLM API 对 user message 的 parts 完整处理，不受 system[0] 截断影响

### Metis Review（灵犀 gap analysis）
**Identified Gaps**（全部已解决）:
- Part 类型验证 → 灵犀确认 SDK 官方支持
- GLM 兼容性 → parts 作为 user message 发送，安全
- 降级策略 → 配置开关 `injectionStrategy: "parts" | "system"`
- 多轮 compacting 去重 → 摘要 hash + 30秒冷却期

---

## Work Objectives

### Core Objective
把记忆注入从 system.transform 改到 chat.message（parts.unshift + synthetic:true），彻底解决 GLM API 只处理 system[0] 的问题，同时升级 compactingHook。

### Concrete Deliverables
- `plugins/opencode/src/hooks.ts` — 新增 memoryInjectionHook，精简 system.transform hook
- `plugins/opencode/src/config.ts` — 新增 injectionStrategy 配置项
- `plugins/opencode/src/index.ts` — hook 注册变更
- 版本升级到 v1.15.0

### Definition of Done
- [ ] GLM 模型端到端测试：模型能正确"看到"并引用注入的记忆内容
- [ ] 非GLM模型（Claude/GPT）不受影响
- [ ] 灵魂低语仍正确注入到 system[0]
- [ ] Compacting 后记忆正确重新注入
- [ ] Keyword 触发时记忆正确补充注入
- [ ] Dedup 正确：同一条记忆不被重复注入
- [ ] npm run build 通过，插件正常加载

### Must Have
- 记忆注入通过 chat.message + parts.unshift + synthetic:true
- 灵魂低语保留在 system.transform + system[0] +=
- injectOn: first + keyword 触发
- dedup: injectedMemoryIds
- compactingHook: 6段式 prompt + 摘要保存
- 降级策略：injectionStrategy 配置开关

### Must NOT Have (Guardrails)
- ❌ 不改后端 shouldRecall API
- ❌ 不改 openclaw/mcp 插件
- ❌ 不做语义触发（只保持当前 keyword 列表）
- ❌ 不新增 REST API 端点
- ❌ 不重构 compactingHook 的 poll 机制（保留现有 poll，只增强回调后逻辑）
- ❌ 不大规模改 config.ts schema（只加 injectionStrategy 字段）
- ❌ 不做测试基础设施搭建（插件无测试框架）
- ❌ AI slop：不要过度注释、不要抽象出不必要的工具函数

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: NO（插件无测试框架）
- **Automated tests**: None（不做测试基础设施搭建）
- **Agent-Executed QA**: ALWAYS — 每个任务都包含 QA 场景

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

---

## 开发规范（MANDATORY）

> 以下规范适用于本计划的所有执行者（月儿及所有弟子），不可违反。

### 1. CodeGraph 搜索优先（铁律）
- **搜索代码必须使用 codegraph 工具**（`codegraph_search`、`codegraph_explore`、`codegraph_node`、`codegraph_callers`、`codegraph_callees`、`codegraph_impact`）
- 禁止用 grep/read 做代码结构搜索（grep 只用于文本搜索、日志搜索等非结构化场景）
- **每完成一轮代码修改后，必须执行 `codegraph sync`** 更新索引，确保后续搜索结果准确

### 2. 弟子委派方式（铁律）
- **所有弟子委派必须使用 `run_in_background=true`**
- 师尊需要在 OpenCode UI 里点击查看弟子进度
- 委派 prompt 必须包含上述 CodeGraph 搜索规范

### 3. 明镜评审循环（铁律）
- **每轮代码改动完成后，必须委派明镜（Momus）评审**
- 评审不通过 → 修复问题 → 再次评审 → 循环直至通过
- 评审通过才能标记任务完成
- 明镜评审使用 `task(subagent_type="momus", run_in_background=false, ...)`

### 4. OMEM 迭代 Skills（铁律）
- **编码和部署过程中必须加载 `omem-iteration` skill**
- 所有弟子委派的 `load_skills` 中必须包含 `"omem-iteration"`
- 按标准流程进行开发：需求 → 方案 → 编码 → 自测 → 评审 → 终审

- **插件构建**: `npm run build` — 编译通过
- **插件加载**: OpenCode 启动无报错
- **端到端**: 实际对话验证注入效果
- **降级测试**: 切换 injectionStrategy 验证 fallback

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (POC 验证 — 2个并行任务，验证生死线):
├── Task 1: Part 类型 + synthetic 字段验证 [quick]
└── Task 2: Hook 执行顺序 + GLM API 兼容验证 [quick]

Wave 2 (核心重构 — POC通过后，3个并行任务):
├── Task 3: 新建 memoryInjectionHook (替代 autoRecallHook) [deep]
├── Task 4: 精简 system.transform hook (仅灵魂低语) [quick]
└── Task 5: config.ts 新增 injectionStrategy [quick]

Wave 3 (集成 + 升级 — 依赖 Wave 2):
├── Task 6: index.ts hook 注册重构 (整合 Wave 2 所有变更) [unspecified-high]
├── Task 7: compactingHook 升级 (6段式 prompt + 摘要保存) [deep]
└── Task 8: 降级策略 + injectionStrategy 切换逻辑 [quick]

Wave FINAL (验证 — 4个并行):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real QA — 端到端测试 (unspecified-high)
└── Task F4: Scope fidelity check (deep)

Critical Path: Task 1/2 → Task 3 → Task 6 → F1-F4
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 3 (Wave 2)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1 | - | 3, 4, 5 | 1 |
| 2 | - | 3, 4, 5 | 1 |
| 3 | 1, 2 | 6 | 2 |
| 4 | 1, 2 | 6 | 2 |
| 5 | 1, 2 | 6, 8 | 2 |
| 6 | 3, 4, 5 | F1-F4 | 3 |
| 7 | - | F1-F4 | 3 |
| 8 | 5 | 6 | 2 |

### Agent Dispatch Summary

- **Wave 1**: 2 tasks — T1 → `quick`, T2 → `quick`
- **Wave 2**: 3 tasks — T3 → `deep`, T4 → `quick`, T5 → `quick`
- **Wave 3**: 3 tasks — T6 → `unspecified-high`, T7 → `deep`, T8 → `quick`
- **FINAL**: 4 tasks — F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

- [x] 1. Part 类型 + synthetic 字段验证

  **What to do**:
  - 读 OpenCode SDK `types.gen.d.ts`（在 opencode-mem 或 supermemory 的 node_modules 里），确认 `TextPart` 有 `synthetic?: boolean` 字段
  - 读 supermemory `src/index.ts:159-168` 和 opencode-mem `src/index.ts:216-224`，确认它们怎么构造 Part
  - 在一个最小 POC 插件里 `output.parts.unshift({ type:"text", text:"test", synthetic:true, ... })` 验证字段被接受
  - 打印 `output.parts[0]` 的所有 keys，确认 synthetic 存在

  **Must NOT do**:
  - 不修改任何现有代码
  - 不需要端到端测试，只验证类型定义

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 2)
  - **Parallel Group**: Wave 1
  - **Blocks**: Tasks 3, 4, 5
  - **Blocked By**: None

  **References**:
  - `/mnt/d/dev/github/project/opencode-mem/src/index.ts:216-224` — opencode-mem 的 Part 构造方式
  - `/mnt/d/dev/github/project/supermemory/src/index.ts:159-168` — supermemory 的 Part 构造方式
  - `plugins/opencode/src/hooks.ts` — 当前 keywordDetectionHook 如何使用 output.parts（只读不写）

  **Acceptance Criteria**:
  - [ ] TextPart 类型定义中确认有 `synthetic?: boolean` 字段
  - [ ] POC 插件 `parts.unshift({ type:"text", text:"...", synthetic:true })` 不报类型错误
  - Evidence: `.omo/evidence/task-1-part-type-verification.txt`

- [x] 2. Hook 执行顺序 + GLM API 兼容验证

  **What to do**:
  - 在 `chat.message` hook 和 `system.transform` hook 里分别加 `console.log(Date.now())`
  - 发送一条消息，观察日志确认 chat.message 先于 system.transform 执行
  - 验证 synthetic part 在 GLM 模型下是否可见（如果无法直接测 GLM，则基于 OpenCode 引擎源码推断）
  - 确认 OpenCode 引擎在组装 API 请求时，synthetic parts 被合并到 user message（不是 system message）

  **Must NOT do**:
  - 不修改任何现有代码（POC 可以临时改，但 revert）
  - 不需要实际调用 GLM API（基于源码推断即可）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 1)
  - **Parallel Group**: Wave 1
  - **Blocks**: Tasks 3, 4, 5
  - **Blocked By**: None

  **References**:
  - `/mnt/d/dev/github/project/opencode-mem/src/index.ts` — opencode-mem 的 hook 注册方式
  - `/mnt/d/dev/github/project/supermemory/src/index.ts` — supermemory 的 hook 注册方式
  - `plugins/opencode/src/index.ts` — 当前 hook 注册和执行流程

  **Acceptance Criteria**:
  - [ ] 确认 chat.message hook 先于 system.transform 执行
  - [ ] 确认 synthetic parts 作为 user message 发送（不受 system[0] 截断影响）
  - [ ] POC 临时变更已 revert
  - Evidence: `.omo/evidence/task-2-hook-order-glm-verification.txt`

- [x] 3. 新建 memoryInjectionHook（替代 autoRecallHook）

  **What to do**:
  - 在 `hooks.ts` 中新增 `memoryInjectionHook` 函数
  - 复用现有 `autoRecallHook` 的检索逻辑（shouldRecall、buildContextBlock、buildClusteredContextBlock 等）
  - 核心变更：把 `appendToSystem(output.system, block)` 改为 `output.parts.unshift(contextPart)` + `synthetic: true`
  - Part 构造：
    ```typescript
    const contextPart = {
      id: `prt_cerebro-context-${Date.now()}`,
      sessionID: input.sessionID,
      messageID: output.message.id,
      type: "text",
      text: injectText,
      synthetic: true,
    };
    output.parts.unshift(contextPart);
    ```
  - injectOn 逻辑：`isFirstInjection || isKeywordTriggered` 时注入
  - dedup：保留 `injectedMemoryIds` Map，注入前过滤已注入的记忆ID
  - Profile 注入：首条注入 + 30min TTL（复用现有 `profileInjectedSessions`）
  - 错误处理：与现有 autoRecallHook 一致（catch → showToast）
  - **不删除 autoRecallHook**，只注释掉，保留 fallback 能力

  **Must NOT do**:
  - 不改 autoRecallHook 本身（只注释不删）
  - 不改后端 API 调用逻辑
  - 不改 keywordDetectionHook
  - 不做降级策略（Task 8 单独做）

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 核心重构，需要理解现有 autoRecallHook 逻辑并正确迁移
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4, 5)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 6
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `plugins/opencode/src/hooks.ts:autoRecallHook`（约 L320-600）— 当前注入逻辑，需要迁移的核心代码
  - `plugins/opencode/src/hooks.ts:appendToSystem()` — 当前注入方式，将被 parts.unshift 替代
  - `plugins/opencode/src/hooks.ts:injectedMemoryIds` — dedup Map，保留
  - `plugins/opencode/src/hooks.ts:profileInjectedSessions` — Profile TTL Map，保留
  - `plugins/opencode/src/hooks.ts:buildContextBlock()` — 记忆块构建函数，复用
  - `plugins/opencode/src/hooks.ts:buildClusteredContextBlock()` — 聚合记忆块构建函数，复用
  - `/mnt/d/dev/github/project/opencode-mem/src/index.ts:216-224` — opencode-mem 的 Part 构造参考
  - `/mnt/d/dev/github/project/supermemory/src/index.ts:159-168` — supermemory 的 Part 构造参考

  **QA Scenarios**:
  ```
  Scenario: 首条消息注入成功
    Tool: Bash
    Steps:
      1. npm run build — 编译通过
      2. 检查 hooks.ts 中存在 memoryInjectionHook 函数
      3. 检查函数内有 output.parts.unshift 调用
      4. 检查 Part 构造包含 synthetic: true
    Expected: 编译成功，函数存在，parts.unshift + synthetic:true 确认
    Evidence: .omo/evidence/task-3-memory-injection-hook.txt

  Scenario: injectOn first 逻辑正确
    Tool: Bash
    Steps:
      1. 检查代码中有 isFirstInjection 判断
      2. 检查注入后 injectedSessions.add(sessionID)
      3. 检查非首次且非 keyword 触发时 return
    Expected: first-only + keyword 触发逻辑完整
    Evidence: .omo/evidence/task-3-inject-on-logic.txt
  ```

  **Commit**: YES
  - Message: `refactor(opencode): add memoryInjectionHook with parts.unshift injection`
  - Files: `plugins/opencode/src/hooks.ts`

- [x] 4. 精简 system.transform hook（仅灵魂低语）

  **What to do**:
  - 在 `index.ts` 中精简 `experimental.chat.system.transform` hook
  - 移除记忆注入逻辑（autoRecallHook 调用）
  - 仅保留灵魂低语（soulWhisper）注入：`output.system[0] += whisperText`
  - 保留 mainSessionId 锁定逻辑
  - 灵魂低语文案用 `<system-reminder>` 标签包裹（参考 DCP nudgeForce 机制）
  - 如果 output.system 为空，push 新元素而非崩溃

  **Must NOT do**:
  - 不删除灵魂低语功能
  - 不改 soulWhisperToolTracker
  - 不改 tool.execute.before hook

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 只是移除代码，逻辑简单
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 3, 5)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 6
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `plugins/opencode/src/index.ts` — 当前 wrappedRecallHook 注册（需要精简）
  - `plugins/opencode/src/hooks.ts:autoRecallHook` — 需要移除的调用
  - `plugins/opencode/src/hooks.ts:soulWhisperToolTracker` — 保留不动
  - `.omo/drafts/hook-refactor.md` — DCP nudgeForce 机制参考（灵魂低语文案方向）

  **QA Scenarios**:
  ```
  Scenario: system.transform 仅含灵魂低语
    Tool: Bash
    Steps:
      1. npm run build — 编译通过
      2. 检查 index.ts 的 system.transform hook 中无 autoRecallHook 调用
      3. 检查仍有 soulWhisper 或 output.system[0] += 逻辑
      4. 检查 mainSessionId 锁定仍在
    Expected: 编译成功，无 autoRecall 调用，灵魂低语保留
    Evidence: .omo/evidence/task-4-simplified-system-transform.txt
  ```

  **Commit**: NO (groups with Task 6)

- [x] 5. config.ts 新增 injectionStrategy 配置

  **What to do**:
  - 在 `config.ts` 的 OmemPluginConfig 接口中新增顶层可选字段 `injectionStrategy?: "parts" | "system"`
  - 放置位置：与 `soulWhisper?`、`agentMemoryPolicy?` 同级（OmemPluginConfig 顶层）
  - 默认值 `"parts"`
  - 在 DEFAULTS 中添加 `injectionStrategy: "parts" as const`
  - 在 deepMerge 中添加 `result.injectionStrategy = overrides.injectionStrategy ?? base.injectionStrategy;`
  - 不改其他 config 结构

  **Must NOT do**:
  - 不大规模重构 config.ts schema
  - 不改其他配置项的默认值或类型

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 单字段添加，5分钟搞定
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 3, 4)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 6, 8
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `plugins/opencode/src/config.ts` — 当前配置类型定义和解析逻辑

  **QA Scenarios**:
  ```
  Scenario: injectionStrategy 配置存在
    Tool: Bash
    Steps:
      1. npm run build — 编译通过
      2. grep "injectionStrategy" config.ts — 确认字段存在
      3. 确认默认值为 "parts"
      4. 确认类型为 "parts" | "system"
    Expected: 字段存在，默认 "parts"，类型联合正确
    Evidence: .omo/evidence/task-5-config-injection-strategy.txt
  ```

  **Commit**: NO (groups with Task 6)

- [x] 6. index.ts hook 注册重构（整合 Wave 2 变更）

  **What to do**:
  - 修改 `index.ts` 的 return 对象：
    - `chat.message`: 合并 keywordDetectionHook + memoryInjectionHook（顺序执行）
    - `experimental.chat.system.transform`: 使用精简后的灵魂低语 hook（Task 4）
    - 其余 hook 不变
  - 注入策略判断：读取 config.injectionStrategy，决定用 memoryInjectionHook 还是旧 autoRecallHook
  - 清理注释掉的旧代码
  - 确保所有 hook 共享状态（injectedMemoryIds, sessionMessages 等）在模块级正确定义

  **Must NOT do**:
  - 不改 `session.compacting` hook（Task 7 单独改）
  - 不改 `tool.execute.before` hook
  - 不改 `tool` 导出

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 需要理解多个 hook 的交互和状态共享
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential (depends on Tasks 3, 4, 5)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 3, 4, 5, 8

  **References**:
  - `plugins/opencode/src/index.ts` — 当前 hook 注册和导出结构
  - Task 3 的 memoryInjectionHook — 需要注册到 chat.message
  - Task 4 的精简 system.transform — 需要替换 wrappedRecallHook
  - Task 5 的 injectionStrategy — 需要在注册时读取
  - `/mnt/d/dev/github/project/opencode-mem/src/index.ts` — opencode-mem 的 hook 注册参考

  **QA Scenarios**:
  ```
  Scenario: Hook 注册正确
    Tool: Bash
    Steps:
      1. npm run build — 编译通过
      2. 检查 chat.message hook 中有 keywordDetection + memoryInjection 调用
      3. 检查 system.transform hook 中无 autoRecallHook 调用
      4. 检查有 injectionStrategy 判断逻辑
      5. 检查 session.compacting hook 未被修改
    Expected: 编译成功，hook 注册正确，降级逻辑存在
    Evidence: .omo/evidence/task-6-hook-registration.txt

  Scenario: 降级模式切换
    Tool: Bash
    Steps:
      1. 检查 injectionStrategy === "system" 时使用旧 autoRecallHook
      2. 检查 injectionStrategy === "parts" 时使用新 memoryInjectionHook
    Expected: 两种模式都有对应路径
    Evidence: .omo/evidence/task-6-fallback-logic.txt
  ```

  **Commit**: YES
  - Message: `refactor(opencode): integrate memoryInjectionHook + simplify system.transform`
  - Files: `plugins/opencode/src/hooks.ts`, `plugins/opencode/src/index.ts`, `plugins/opencode/src/config.ts`

- [x] 7. compactingHook 升级（6段式 prompt + 增强摘要保存）

  > ⚠️ **注意**：compactingHook（hooks.ts:869-898）已有完整的摘要保存逻辑（`client.ingestMessages` + `compact-summary` tag + toast 提示）。
  > 本任务是**增强/升级**现有逻辑，不是从零新增。

  **What to do**:
  - 在 `hooks.ts` 中新增 `createCerebroCompactionPrompt()` 函数
  - 6段式 prompt 模板（中文输出，保留用户原始语言）：
    ```
    1. 用户原始请求（逐字保留）
    2. 最终目标
    3. 已完成工作（文件路径、技术决策）
    4. 未完成任务
    5. 禁止事项（关键约束）
    6. 已有项目知识（从 Cerebro 实时拉取）
    ```
  - 修改 compactingHook：在注入 `output.context` 时使用新 prompt 模板（替代现有的 `buildContextBlock(results)` 直接注入）
  - **增强**现有摘要保存逻辑：将已有的 `compact-summary` tag 摘要升级为 `[Session Summary]` 前缀格式，增加多轮 compacting 去重（摘要内容 hash + sessionID，30秒冷却期）
  - injectedMemoryIds 在 compacting 后保留（不 clear），避免重复注入

  **Must NOT do**:
  - 不重构 poll 机制（保留现有 poll）
  - 不新增 API 端点
  - 不移植 supermemory 的 embedding 聚合/聚类功能

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: compacting 逻辑复杂，需要理解 poll 机制和现有 compactingHook 结构
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 6, 8，但建议在 Task 6 之后做)
  - **Parallel Group**: Wave 3
  - **Blocks**: F1-F4
  - **Blocked By**: None（可独立于其他 Wave 2 任务）

  **References**:
  - `plugins/opencode/src/hooks.ts:compactingHook`（约 L650-905）— 当前 compacting 逻辑
  - `/mnt/d/dev/github/project/supermemory/src/services/compaction.ts` — supermemory 三阶段参考（540行）
  - `.omo/drafts/hook-refactor.md` — CompactingHook 升级计划章节

  **QA Scenarios**:
  ```
  Scenario: 6段式 prompt 生成正确
    Tool: Bash
    Steps:
      1. 检查 createCerebroCompactionPrompt 函数存在
      2. 检查 prompt 包含所有6个段落标题
      3. 检查 prompt 要求保留用户原始语言
      4. 检查 projectMemories 被注入到第6段
    Expected: 6段式模板完整，语言保留指令存在
    Evidence: .omo/evidence/task-7-compaction-prompt.txt

  Scenario: 摘要保存逻辑
    Tool: Bash
    Steps:
      1. 检查 compacting 完成后有 ingest 调用
      2. 检查 ingest 内容带 [Session Summary] 前缀
      3. 检查有去重逻辑（hash 或 sessionID）
    Expected: 摘要保存 + 去重逻辑完整
    Evidence: .omo/evidence/task-7-summary-save.txt
  ```

  **Commit**: YES
  - Message: `feat(opencode): upgrade compactingHook with 6-section prompt + summary save`
  - Files: `plugins/opencode/src/hooks.ts`

- [x] 8. 降级策略 + injectionStrategy 切换逻辑

  > ⚠️ **重要**：降级在 **index.ts 注册层面**实现（非 hook 内部分支）。`chat.message` hook 的 output 是 `{ message, parts }`，没有 `system` 字段。所以不能在 chat.message hook 内做 `appendToSystem(output.system, ...)` 分支。
  > 正确做法：在 index.ts 中根据 `config.injectionStrategy` 决定注册哪个 hook。

  **What to do**:
  - 在 **index.ts**（非 hooks.ts）中实现降级逻辑：
    ```typescript
    // index.ts — 注册层面做选择
    if (config.injectionStrategy === "system") {
      // 降级：system.transform 注册旧 autoRecallHook（system[0] +=）
      "experimental.chat.system.transform": wrappedRecallHook, // 旧逻辑
      "chat.message": keywordDetectionHook(...),                // 只做 keyword
    } else {
      // 默认(parts)：system.transform 只做灵魂低语，chat.message 做记忆注入
      "experimental.chat.system.transform": soulWhisperOnlyHook, // 精简版
      "chat.message": mergedChatMessageHook(...),                // keyword + memoryInjection
    }
    ```
  - 确保 `injectionStrategy === "system"` 时，走旧路径（autoRecallHook + appendToSystem）
  - 清理注释掉的旧 autoRecallHook（确认降级路径不需要它时）

  **Must NOT do**:
  - 不在 chat.message hook 内部做 `output.system` 分支（output 没有 system 字段）
  - 不删除旧的 appendToSystem 函数（降级需要）
  - 不改默认值为 "system"（默认是 "parts"）

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 只是加 if/else 判断
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 6, 7)
  - **Parallel Group**: Wave 2-3
  - **Blocks**: Task 6
  - **Blocked By**: Task 5

  **References**:
  - `plugins/opencode/src/hooks.ts:appendToSystem()` — 降级时使用的旧注入函数
  - `plugins/opencode/src/config.ts` — injectionStrategy 配置（Task 5 添加）

  **QA Scenarios**:
  ```
  Scenario: parts 模式（默认）
    Tool: Bash
    Steps:
      1. 确认 injectionStrategy 默认为 "parts"
      2. 确认 "parts" 模式使用 parts.unshift
    Expected: 默认路径正确

  Scenario: system 降级模式
    Tool: Bash
    Steps:
      1. 确认 injectionStrategy === "system" 时使用 appendToSystem
      2. 确认 appendToSystem 函数仍存在且可用
    Expected: 降级路径正确
    Evidence: .omo/evidence/task-8-fallback-strategy.txt
  ```

  **Commit**: NO (groups with Task 6)

- [x] 9. 版本升级到 v1.15.0

  **What to do**:
  - 更新 `plugins/opencode/package.json` 的 version 为 `1.15.0`
  - 更新 CHANGELOG（如有）
  - `npm run build` 确认编译通过

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`omem-iteration`]
    - `omem-iteration`: 编码和部署必须加载 OMEM 迭代管理技能，按标准流程开发

  **Parallelization**:
  - **Can Run In Parallel**: YES (with F1-F4)
  - **Blocked By**: Tasks 3-8

  **Commit**: YES
  - Message: `chore(opencode): bump version to v1.15.0`
  - Files: `plugins/opencode/package.json`

---

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE.

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns. Check evidence files.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `npm run build` + review all changed files for: unused imports, console.log in prod, commented-out code, `as any` that can be replaced with proper types.
  Output: `Build [PASS/FAIL] | Files [N clean/N issues] | VERDICT`

- [x] F3. **Real QA** — `unspecified-high`
  Build plugin, load in OpenCode, verify: (1) memory injection works via parts, (2) soul whisper still in system[0], (3) keyword trigger works, (4) compacting recovery works, (5) GLM model sees injected content.
  Output: `Scenarios [N/N pass] | VERDICT`

- [x] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — no scope creep, no missing items.
  Output: `Tasks [N/N compliant] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `refactor(opencode): verify POC for parts.unshift injection strategy` — POC 验证文件
- **Wave 2**: `refactor(opencode): add memoryInjectionHook + simplify system.transform` — hooks.ts, index.ts, config.ts
- **Wave 3**: `feat(opencode): upgrade compactingHook with 6-section prompt + summary save` — hooks.ts
- **Final**: `chore(opencode): bump version to v1.15.0` — package.json

---

## Success Criteria

### Verification Commands
```bash
cd plugins/opencode && npm run build     # Expected: 编译成功，无错误
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] Plugin builds and loads without errors
- [ ] GLM model correctly sees injected memory content
- [ ] Soul whisper still injected to system[0]
- [ ] Compacting recovery works correctly
- [ ] Dedup prevents duplicate memory injection
- [ ] Fallback strategy (injectionStrategy config) works
