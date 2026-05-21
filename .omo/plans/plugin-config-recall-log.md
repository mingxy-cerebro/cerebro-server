# Plugin Config重构 + 召回日志 + Reconciler交并差

## TL;DR

> **Quick Summary**: Plugin端config按功能对象化 + 子agent记忆权限路由表 + 召回日志补全 + omem→cerebro命名统一 + 服务端Reconciler交并差改进
> 
> **Deliverables**:
> - config.ts OmemPluginConfig嵌套分组（connection/content/ingest/recall/logging/ui）
> - agentMemoryPolicy路由表（readonly vs readwrite）
> - autoRecallHook + keywordDetectionHook 结构化日志
> - omem→cerebro统一命名（类名/变量名/XML标签/错误前缀/文件名）
> - 废弃parent_session_id
> - RECONCILE_SYSTEM_PROMPT交并差指令 + 记忆格式加粗视觉优化
> - events/cases开放MERGE权限
> - fast_session_merge差集合并
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 3 waves
> **Critical Path**: Task 1 (config) → Task 2 (logger) → Task 3 (hooks) → Task 5 (reconciler) → Task 6 (prompt) → FINAL

---

## Context

### Original Request
师尊要求优化Plugin端config配置按功能对象化，加记忆召回日志，结合之前讨论的子agent记忆权限路由和Reconciler交并差改进。

### Interview Summary
**Key Discussions**:
- Config扁平15字段→按功能6组嵌套: (b33)
- 子agent记忆权限: 方案C按agent类型分级路由（readonly/readwrite）: (b31)
- 废弃parent_session_id: 师尊明确决定废弃: (b31)
- Reconciler交并差: prompt太弱+events禁止MERGE导致重复: (b33)
- 记忆召回日志缺失: autoRecallHook零日志, keywordDetectionHook零日志: (b33)

**Research Findings**:
- config.ts 15个扁平字段, loadPluginConfig() 4层覆盖: (b33)
- hooks.ts 4个hook, autoRecallHook L232-375 零日志, keywordDetectionHook L377-412 零日志: (b33)
- reconciler.rs MERGE处理 L155-194, fast_session_merge L254-300: (b33)
- RECONCILE_SYSTEM_PROMPT L143-181, events禁止MERGE L158: (b33)

### Metis Review
**Identified Gaps** (addressed):
- 向后兼容性: 加flat-to-nested迁移逻辑 → MINOR auto-resolved
- defaultPolicy: 默认readonly → AMBIGUOUS, 安全优先
- unspecified-*通配: 精确匹配 → MINOR auto-resolved
- getCfg()重构: 改签名所有调用点 → MINOR auto-resolved
- env vars覆盖: 保留扁平映射 → MINOR auto-resolved

---

## Work Objectives

### Core Objective
Plugin端config按功能对象化重组 + 子agent记忆权限控制 + 召回日志补全 + Reconciler交并差改进

### Concrete Deliverables
- `plugins/opencode/src/config.ts` — 嵌套OmemPluginConfig + agentMemoryPolicy + flat-to-nested迁移
- `plugins/opencode/src/logger.ts` — 适配新config结构
- `plugins/opencode/src/hooks.ts` — 召回日志 + agentMemoryPolicy检查 + 废弃parentSessionId
- `plugins/opencode/src/client.ts` — getCfg()新签名适配
- `plugins/opencode/omem.example.jsonc` — 新格式示例
- `omem-server/src/ingest/prompts.rs` — RECONCILE_SYSTEM_PROMPT交并差指令
- `omem-server/src/ingest/reconciler.rs` — events开放MERGE + fast_session_merge差集合并

### Definition of Done
- [ ] `cd plugins/opencode && npx tsc --noEmit` → 0 errors
- [ ] `cargo check -p omem-server` → PASS
- [ ] 所有QA场景证据文件存在
- [ ] 玄机(oracle)评审通过

### Must Have
- config嵌套后旧config.json不崩溃(graceful fallback)
- agentMemoryPolicy精确匹配agent名字，defaultPolicy兜底
- autoRecallHook入口/召回/注入/记录全链路日志
- RECONCILE_SYSTEM_PROMPT明确交并差指令
- events/cases类别开放MERGE

### Must NOT Have (Guardrails)
- 服务端不因parentSessionId废弃而改动（Plugin端删除参数传递即可）
- agentMemoryPolicy不用正则匹配（精确匹配）
- config重组不删字段（只重组）
- 不改client.ts的API方法签名（只改内部getCfg调用）
- 不引入新的外部依赖
- **Tag前缀 `omem_user_` / `omem_project_` 不改** — 改了会导致已有记忆召回失败
- **每个task改完代码后必须找玄机(oracle)+明镜(momus)双重评审** — 无例外！

### omem→cerebro 命名统一清单
> 以下内容在Task 1(config.ts)和Task 5(hooks.ts+client.ts)中一并完成

| 改什么 | 当前 | 改为 | 文件 | 处数 |
|--------|------|------|------|------|
| 文件名 | `omem.example.jsonc` | `cerebro.example.jsonc` | 根目录 | 1 |
| 类名 | `OmemClient` | `CerebroClient` | client.ts + hooks.ts + index.ts + tools.ts | 10 |
| 变量名 | `omemClient` | `cerebroClient` | index.ts + hooks.ts | 4 |
| XML标签 | `<omem-context>` / `<omem-profile>` | `<cerebro-context>` / `<cerebro-profile>` | hooks.ts | 6 |
| 错误前缀 | `[omem]` | `[cerebro]` | client.ts + hooks.ts + index.ts | 6 |
| **Tag前缀** | `omem_user_` / `omem_project_` | **不改** | tags.ts | 0 |
| **npm包名引用** | `@mingxy/omem` / `@ourmem/opencode` | `@mingxy/cerebro` | index.ts L77 | 1 |

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (tsc for TS, cargo test for Rust)
- **Automated tests**: Tests-after（改完后跑现有测试确认不break）
- **Framework**: tsc --noEmit + cargo test

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **TypeScript**: `npx tsc --noEmit` + `npm run build` in plugins/opencode
- **Rust**: `cargo check` + `cargo test` in omem-server
- **Integration**: 重启opencode验证config加载 + 日志输出

---

## Execution Strategy

### ⚠️ UNATTENDED EXECUTION — 无人值守模式

> **师尊启动 `/start-work` 后会直接离开，不会在线回复任何问题。执行器必须完全自主完成所有任务。**

**执行铁律**:
1. **零等待**: 遇到问题自行决策，不暂停等用户回复。用合理默认值继续。
2. **零提问**: 不要输出"请确认"、"需要用户决定"等阻断性语句。
3. **自动评审**: 每个Task完成后自动触发玄机(oracle)+明镜(momus)评审，根据反馈自行修复。
4. **自动继续**: Wave内并行任务全部完成后，自动推进到下一Wave，不等待指令。
5. **自动封印**: 全部Task + Final Verification完成后，自动执行 `git add -A && git commit && git push`，不需要等师尊回来。
6. **报错自愈**: 编译失败→自动修复→重试；评审不通过→自动改→重新提交评审。最多重试3次。
   - **评审兜底**: 如果3次重试后评审仍不通过，自动标记为"条件通过(有保留意见)"并继续推进。汇总所有未通过项到Final Verification阶段集中处理。**绝不能因为评审死循环而阻塞整个执行流程。**
7. **部署自执行**: Task 8包含build+publish+部署，自动SSH到服务器执行。
8. **记录备查**: 所有关键决策、评审结果、修复记录写入 `.sisyphus/evidence/`，师尊醒来可审查。

**唯一例外**: Final Verification F1-F4全部完成后，呈现结果摘要等师尊回来确认（不自动标记complete）。

### Parallel Execution Waves

```
Wave 1 (Start Immediately - config + foundation):
├── Task 1: config.ts嵌套重构 + 迁移逻辑 [unspecified-high]
├── Task 2: logger.ts适配新config [quick]
└── Task 3: omem.example.jsonc更新 [quick]

Wave 2 (After Wave 1 - core logic, MAX PARALLEL):
├── Task 4: client.ts getCfg()适配 [quick]
├── Task 5: hooks.ts召回日志 + agentMemoryPolicy + 废弃parentSessionId [unspecified-high]
├── Task 6: RECONCILE_SYSTEM_PROMPT交并差指令 [deep]
└── Task 7: reconciler.rs events开放MERGE + fast_session_merge差集 [deep]

Wave 3 (After Wave 2 - build + publish):
└── Task 8: build验证 + npm publish [quick]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> 自动封印(git commit+push) -> 等师尊醒来审查Final结果

Critical Path: Task 1 → Task 4/5 → Task 8 → FINAL
Parallel Speedup: ~50% faster than sequential
Max Concurrent: 4 (Wave 2)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1 | - | 2, 3, 4, 5 | 1 |
| 2 | 1 | 8 | 1 |
| 3 | 1 | 8 | 1 |
| 4 | 1 | 5, 8 | 2 |
| 5 | 1, 4 | 8 | 2 |
| 6 | - | 7, 8 | 2 |
| 7 | 6 | 8 | 2 |
| 8 | 2, 3, 4, 5, 7 | FINAL | 3 |

### Agent Dispatch Summary

- **Wave 1**: 3 tasks — T1 `unspecified-high`, T2 `quick`, T3 `quick`
- **Wave 2**: 4 tasks — T4 `quick`, T5 `unspecified-high`, T6 `deep`, T7 `deep`
- **Wave 3**: 1 task — T8 `quick`
- **FINAL**: 4 tasks — F1 `oracle`, F2 `unspecified-high`, F3 `unspecified-high`, F4 `deep`

### Mandatory Skills (每个执行Agent必须加载)

> **所有Task执行时必须加载 `omem-iteration` skill！**
> 这个skill包含了月儿的迭代管理铁律：证据驱动、先封印再飞升、审查必严。
> 
> 加载方式: `skill(name="omem-iteration")`
> 
> **核心要求**（从omem-iteration skill提取）：
> 1. 🔍 **证据驱动** — 没编译通过就别说"完成了"
> 2. 🪞 **审查必严** — 每个Task改完代码后必须找**玄机(oracle)+明镜(momus)** 双重评审
> 3. 🔒 **先封印再飞升** — git commit + push 必须在部署之前
> 4. ✅ **完美交付** — 彻底解决问题，不留尾巴

---

## TODOs

- [x] 1. config.ts 嵌套重构 + flat-to-nested迁移逻辑

  **What to do**:
  - 将 `OmemPluginConfig` 从15个扁平字段改为6个嵌套分组:
    ```typescript
    interface OmemPluginConfig {
      connection: { apiUrl: string; apiKey: string; requestTimeoutMs: number };
      content: { maxQueryLength: number; maxContentChars: number; maxContentLength: number };
      ingest: { autoCaptureThreshold: number; ingestMode: "smart" | "raw" };
      recall: { similarityThreshold: number; maxRecallResults: number };
      logging: { logEnabled: boolean; logLevel: "DEBUG"|"INFO"|"WARN"|"ERROR"; logDir: string };
      ui: { toastDelayMs: number };
      agentMemoryPolicy?: Record<string, "none" | "readonly" | "readwrite">;
      defaultPolicy?: "none" | "readonly" | "readwrite";  // 默认"readonly"（安全优先，向后兼容）
    }
    ```
    > **agentMemoryPolicy匹配规则**: 精确匹配agent名字（不支持前缀/正则）。不在列表中的agent → fallback到defaultPolicy → 最终fallback到"readonly"。
    > **"none"级别**: 既不召回也不存储，完全隔离。用于不需要记忆的临时agent。
    > **向后兼容**: defaultPolicy默认"readonly"（非"none"），确保未配置时现有agent行为不受影响。
  - DEFAULTS改为嵌套结构
  - `loadPluginConfig()` 加 flat-to-nested 迁移: 检测config.json是旧格式(扁平)时自动转为新格式(嵌套)
  - 环境变量映射保留扁平: OMEM_LOG_LEVEL → logging.logLevel 等
  - 加 `resolveAgentPolicy(agentName: string): "readonly" | "readwrite"` 工具函数
    - 精确匹配 agentMemoryPolicy[agentName]，无匹配则 fallback defaultPolicy（默认"readonly"）
  - config.json路径保持 `~/.config/cerebro/config.json`

  **Must NOT do**:
  - 不删除任何现有字段
  - 不引入新依赖
  - 不改config.json路径

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 多类型重构，需要理解TypeScript类型系统+向后兼容逻辑
  - **Skills**: `["omem-iteration"]`
    - `omem-iteration`: 迭代管理铁律（证据驱动、审查必严、先封印再飞升）

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Tasks 2, 3, 4, 5
  - **Blocked By**: None (can start immediately)

  **References**:
  - `plugins/opencode/src/config.ts` — 当前104行, OmemPluginConfig接口L1-17, DEFAULTS L19-37, loadPluginConfig() L39-87
    - 提取: 当前扁平结构、4层覆盖逻辑、config.json路径
  - `plugins/opencode/omem.example.jsonc` — 当前24行，旧格式示例
    - 提取: 当前字段列表和注释格式

  **Acceptance Criteria**:
  - [ ] OmemPluginConfig改为嵌套6组 + agentMemoryPolicy + defaultPolicy
  - [ ] loadPluginConfig()检测旧格式自动迁移
  - [ ] resolveAgentPolicy()工具函数导出
  - [ ] 环境变量映射保留扁平兼容
  - [ ] `npx tsc --noEmit` → 0 errors

  **QA Scenarios**:

  ```
  Scenario: 新格式config.json正确加载
    Tool: Bash
    Preconditions: config.json使用新嵌套格式
    Steps:
      1. 创建临时test文件，import { loadPluginConfig } from './config'
      2. 调用loadPluginConfig()，断言返回值结构为嵌套
      3. 检查connection.apiUrl, logging.logLevel等字段可访问
    Expected Result: 嵌套结构正确加载，所有字段有值
    Evidence: .sisyphus/evidence/task-1-nested-config.txt

  Scenario: 旧格式config.json自动迁移
    Tool: Bash
    Preconditions: config.json使用旧扁平格式(apiUrl, logLevel等直接在顶层)
    Steps:
      1. 用旧格式config.json调用loadPluginConfig()
      2. 检查返回值是否被自动转为嵌套结构
      3. 验证所有字段值正确迁移
    Expected Result: 扁平→嵌套自动转换，无字段丢失
    Evidence: .sisyphus/evidence/task-1-migration.txt

  Scenario: resolveAgentPolicy正确路由 + defaultPolicy兜底 (P0 smoke test)
    Tool: Bash
    Steps:
      1. node -e "const {resolveAgentPolicy} = require('./dist/config.js'); console.log(resolveAgentPolicy('explore'))" → 预期readonly
      2. node -e "const {resolveAgentPolicy} = require('./dist/config.js'); console.log(resolveAgentPolicy('deep'))" → 预期readwrite
      3. node -e "const {resolveAgentPolicy} = require('./dist/config.js'); console.log(resolveAgentPolicy('unknown_agent_xyz'))" → 预期readonly (defaultPolicy兜底)
      4. node -e "const {resolveAgentPolicy} = require('./dist/config.js'); console.log(resolveAgentPolicy('Oracle'))" → 预期readonly (大小写不敏感匹配)
    Expected Result: 精确匹配+defaultPolicy兜底+大小写兼容均正确
    Evidence: .sisyphus/evidence/task-1-agent-policy.txt
  ```

  **Commit**: YES (group with Tasks 2, 3)
  - Message: `refactor(plugin): restructure config with nested groups`
  - Files: `plugins/opencode/src/config.ts`
  - Pre-commit: `cd plugins/opencode && npx tsc --noEmit`

- [x] 2. logger.ts 适配新config结构

  **What to do**:
  - 修改logger.ts从新嵌套config读取配置
  - `loadPluginConfig()` 返回嵌套结构后，logger读取 `cfg.logging.logEnabled`、`cfg.logging.logLevel`、`cfg.logging.logDir`
  - 其余逻辑不变（LEVEL_MAP, writeLog, 4个导出函数）

  **Must NOT do**:
  - 不改日志格式
  - 不改日志文件名(plugin.log)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 小改动，3行读取路径适配
  - **Skills**: `["omem-iteration"]`
    - `omem-iteration`: 迭代管理铁律（证据驱动、审查必严、先封印再飞升）

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3) — 但依赖Task 1的config结构
  - **Blocks**: Task 8
  - **Blocked By**: Task 1 (需要新的config接口定义)

  **References**:
  - `plugins/opencode/src/logger.ts` — 当前63行
    - L5-10: LEVEL_MAP, L12: MIN_LEVEL从loadPluginConfig读取logLevel, L14: LOG_DIR
    - 提取: 3个需要改的读取点（logEnabled, logLevel, logDir）

  **Acceptance Criteria**:
  - [ ] logger.ts从嵌套config读取logging组
  - [ ] `npx tsc --noEmit` → 0 errors

  **QA Scenarios**:

  ```
  Scenario: logger从嵌套config读取配置
    Tool: Bash
    Steps:
      1. 设置config.json logging组: { logEnabled: true, logLevel: "DEBUG", logDir: "/tmp/test-cerebro" }
      2. import { logDebug, logInfo } from './logger'
      3. 调用logDebug("test")，检查日志文件是否出现在/tmp/test-cerebro/plugin.log
      4. 设置logLevel: "INFO"，调用logDebug，确认不输出
    Expected Result: logLevel过滤正确，logDir可配置
    Evidence: .sisyphus/evidence/task-2-logger-config.txt

  Scenario: logger嵌套config smoke test (P0)
    Tool: Bash
    Steps:
      1. node -e "const cfg = require('./dist/config.js').loadPluginConfig(); console.log(typeof cfg.logging, cfg.logging.logLevel)" → 预期 object INFO
      2. node -e "const cfg = require('./dist/config.js').loadPluginConfig(); console.log(cfg.logging.logEnabled)" → 预期 true
    Expected Result: 嵌套config读取正确，logging字段存在
    Evidence: .sisyphus/evidence/task-2-logger-smoke.txt
  ```

  **Commit**: YES (group with Tasks 1, 3)
  - Message: `refactor(plugin): restructure config with nested groups`
  - Files: `plugins/opencode/src/logger.ts`

- [x] 3. cerebro.example.jsonc 更新为新格式

  **What to do**:
  - 文件名从 `omem.example.jsonc` 改为 `cerebro.example.jsonc`
  - 按新的嵌套结构重写内容
  - 包含所有6个分组 + agentMemoryPolicy + defaultPolicy
  - 每个字段加注释说明用途和默认值

  **Must NOT do**:
  - 不删字段
  - 不加不在OmemPluginConfig中的字段

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 纯文档更新，改格式
  - **Skills**: `["omem-iteration"]`
    - `omem-iteration`: 迭代管理铁律（证据驱动、审查必严、先封印再飞升）

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2) — 但依赖Task 1的config结构
  - **Blocks**: Task 8
  - **Blocked By**: Task 1 (需要新的config接口定义)

  **References**:
  - `plugins/opencode/omem.example.jsonc` — 当前24行旧格式
  - `plugins/opencode/src/config.ts` — Task 1完成后的新接口定义
    - 提取: 完整字段列表和分组

  **Acceptance Criteria**:
  - [ ] omem.example.jsonc使用嵌套格式
  - [ ] 包含agentMemoryPolicy路由表示例
  - [ ] 包含defaultPolicy示例

  **QA Scenarios**:

  ```
  Scenario: example jsonc格式正确
    Tool: Bash
    Steps:
      1. 读取omem.example.jsonc
      2. 去掉注释后JSON.parse验证格式合法
      3. 检查包含6个分组 + agentMemoryPolicy + defaultPolicy
    Expected Result: JSON合法，所有分组存在
    Evidence: .sisyphus/evidence/task-3-example-jsonc.txt
  ```

  **Commit**: YES (group with Tasks 1, 2)
  - Message: `refactor(plugin): restructure config with nested groups`
  - Files: `plugins/opencode/omem.example.jsonc`

- [x] 4. client.ts getCfg()适配嵌套config

  **What to do**:
  - 修改client.ts的 `getCfg<K>(key, fallback)` → `getCfg<K>(section, key, fallback)`
  - 所有调用点从 `getCfg("similarityThreshold", 0.4)` 改为 `getCfg("recall", "similarityThreshold", 0.4)`
  - OmemClient构造函数的config类型改为新的嵌套OmemPluginConfig
  - 不改任何公共API方法签名（sessionIngest, searchMemories等参数不变）

  **Must NOT do**:
  - 不改API方法签名
  - 不改HTTP请求逻辑

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 机械替换getCfg调用点
  - **Skills**: `["omem-iteration"]`
    - `omem-iteration`: 迭代管理铁律（证据驱动、审查必严、先封印再飞升）

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 7)
  - **Blocks**: Task 5, Task 8
  - **Blocked By**: Task 1 (需要新的config接口)

  **References**:
  - `plugins/opencode/src/client.ts` — ~500行, 18个API方法
    - getCfg<K>(key, fallback) 私有方法, 从this.config读扁平key
    - 提取: 所有getCfg调用点和当前key名称
  - `plugins/opencode/src/config.ts` — Task 1完成后的新嵌套结构
    - 提取: section名称 → 原扁平key的映射关系

  **Acceptance Criteria**:
  - [ ] getCfg改为3参数签名(section, key, fallback)
  - [ ] 所有调用点更新到新section
  - [ ] `npx tsc --noEmit` → 0 errors

  **QA Scenarios**:

  ```
  Scenario: client正确读取嵌套config值
    Tool: Bash
    Steps:
      1. 创建config with recall.similarityThreshold=0.6
      2. 创建OmemClient with 该config
      3. 调用shouldRecall，检查是否使用0.6阈值
    Expected Result: 配置值正确从嵌套结构读取
    Evidence: .sisyphus/evidence/task-4-client-nested.txt

  Scenario: getCfg 3参数签名 smoke test (P0)
    Tool: Bash
    Steps:
      1. grep -n "getCfg(" plugins/opencode/src/client.ts | head -5 → 预期所有调用都是3参数(section, key, fallback)
      2. grep -c "getCfg(\"" plugins/opencode/src/client.ts → 预期0（旧2参数格式应全删除）
    Expected Result: 所有getCfg调用已改为3参数
    Evidence: .sisyphus/evidence/task-4-getcfg-smoke.txt
  ```

  **Commit**: YES (group with Task 5)
  - Message: `feat(plugin): add agent memory policy + recall logging + drop parentSessionId`
  - Files: `plugins/opencode/src/client.ts`
  - Pre-commit: `cd plugins/opencode && npx tsc --noEmit`

- [x] 5. hooks.ts 召回日志 + agentMemoryPolicy + 废弃parentSessionId + omem→cerebro改名

  **What to do**:

  **5a. agentMemoryPolicy检查**:
  - 在autoRecallHook入口，用resolveAgentPolicy(agentId)检查权限
  - readonly → 正常召回但不ingest
  - readwrite → 召回 + ingest时传主session_id
  - 子agent识别: 通过OpenCode上下文判断是否为子agent（参考hooks.ts现有isSubAgent逻辑）
  - 子agent传主session_id: 通过getMainSessionId()获取主窗口session

  **5b. 召回日志补全**:
  - autoRecallHook (L232-375): 加入口日志、shouldRecall前后、getProfile前后、记忆注入后、recordSessionRecall后
  - keywordDetectionHook (L377-412): 加关键词检测命中日志
  - 日志格式与现有log格式一致（使用logDebug/logInfo）

  **5c. 废弃parentSessionId**:
  - compactingHook L451: 删除parentSessionId参数传递
  - sessionIdleHook L572: 已无parentSessionId（之前也是bug漏了），确认无需改动
  - client.ts sessionIngest: 从6参数改为5参数（删除parentSessionId参数）
  - index.ts: 如有引用parentSessionId的地方一并清理

  **5d. omem→cerebro命名统一**:
  - `OmemClient` → `CerebroClient`（client.ts L94, hooks.ts 5处, index.ts 1处, tools.ts 1处）
  - `omemClient` 变量 → `cerebroClient`（index.ts 3处, hooks.ts 1处）
  - `<omem-context>` → `<cerebro-context>`（hooks.ts L185,190,223,228）
  - `<omem-profile>` → `<cerebro-profile>`（hooks.ts L264,266）
  - `[omem]` 错误前缀 → `[cerebro]`（client.ts L133,142, hooks.ts L358,360, index.ts L93,94, tools.ts L98）
  - `@mingxy/omem` / `@ourmem/opencode` → `@mingxy/cerebro`（index.ts L77 plugin_config键名）
  - **Tag前缀 `omem_user_` / `omem_project_` 不改**（会导致已有记忆召回失败）

  **Must NOT do**:
  - 不改服务端（memory.rs等）
  - 不改autoRecallHook的召回逻辑，只加日志
  - 不引入新依赖
  - 不改Tag前缀（omem_user_ / omem_project_）

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 涉及多个hook改动 + 新权限逻辑 + 日志补全
  - **Skills**: `["omem-iteration"]`
    - `omem-iteration`: 迭代管理铁律（证据驱动、审查必严、先封印再飞升）

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 6, 7)
  - **Blocks**: Task 8
  - **Blocked By**: Task 1 (resolveAgentPolicy), Task 4 (getCfg新签名)

  **References**:
  - `plugins/opencode/src/hooks.ts` — 591行
    - autoRecallHook L232-375: 零日志，需加5+个log点
    - keywordDetectionHook L377-412: 零日志，需加1-2个log点
    - compactingHook L414-478: L451 parentSessionId需删除
    - sessionIdleHook L483-591: L572 确认无需改
    - isSubAgent逻辑: 查看现有判断方式
    - getMainSessionId(): 获取主窗口session的方法
  - `plugins/opencode/src/client.ts` — sessionIngest签名
    - L375: `sessionIngest(messages, sessionId, agentId, sessionTitle, projectName, parentSessionId?)`
    - 需改为5参数（删除parentSessionId）
  - `plugins/opencode/src/config.ts` — resolveAgentPolicy() (Task 1产出)
    - 提取: 精确匹配agentMemoryPolicy，defaultPolicy兜底

  **Acceptance Criteria**:
  - [ ] autoRecallHook有5+个结构化日志点
  - [ ] keywordDetectionHook有1+个日志点
  - [ ] resolveAgentPolicy在autoRecallHook入口检查
  - [ ] compactingHook删除parentSessionId
  - [ ] client.ts sessionIngest改为5参数
  - [ ] `npx tsc --noEmit` → 0 errors

  **QA Scenarios**:

  ```
  Scenario: autoRecallHook全链路日志输出
    Tool: Bash
    Preconditions: config.json logLevel=DEBUG, logEnabled=true
    Steps:
      1. 触发autoRecallHook（模拟用户消息）
      2. 读取plugin.log
      3. 检查包含: "autoRecallHook entry", "shouldRecall result", "profile loaded", "memories injected", "sessionRecall recorded"
    Expected Result: 全链路日志完整，格式统一
    Evidence: .sisyphus/evidence/task-5-recall-log.txt

  Scenario: readonly agent不触发ingest
    Tool: Bash
    Preconditions: config.json agentMemoryPolicy.explore=readonly
    Steps:
      1. 模拟explore agent触发sessionIdleHook
      2. 检查plugin.log，确认无sessionIngest调用
      3. 确认recall正常工作
    Expected Result: readonly agent只召回不存储
    Evidence: .sisyphus/evidence/task-5-readonly-agent.txt

  Scenario: parentSessionId完全移除
    Tool: Bash
    Steps:
      1. grep -r "parentSessionId" plugins/opencode/src/ --include="*.ts"
      2. 确认0匹配（完全移除）
    Expected Result: 0 results
    Evidence: .sisyphus/evidence/task-5-no-parent-session.txt

  Scenario: omem→cerebro改名完整 smoke test (P0)
    Tool: Bash
    Steps:
      1. grep -r "OmemClient" plugins/opencode/src/ --include="*.ts" → 预期0（应全改为CerebroClient）
      2. grep -r "omemClient" plugins/opencode/src/ --include="*.ts" → 预期0（应全改为cerebroClient）
      3. grep -r "omem_client" plugins/opencode/src/ --include="*.ts" → 预期0
    Expected Result: 所有omem命名已改为cerebro
    Evidence: .sisyphus/evidence/task-5-rename-smoke.txt
  ```

  **Commit**: YES (group with Task 4)
  - Message: `feat(plugin): add agent memory policy + recall logging + drop parentSessionId`
  - Files: `plugins/opencode/src/hooks.ts`, `plugins/opencode/src/client.ts`
  - Pre-commit: `cd plugins/opencode && npx tsc --noEmit`

- [x] 6. prompts.rs RECONCILE交并差指令 + 记忆格式视觉优化

  **What to do**:

  **6a. RECONCILE_SYSTEM_PROMPT交并差指令**:
  - 修改 `omem-server/src/ingest/prompts.rs` 中的 RECONCILE_SYSTEM_PROMPT (L143-181)
  - **MERGE指令增强**: 从 "combining both old and new info" 改为明确的交并差指令:
    ```
    For MERGE: You must produce merged_content by performing set operations on the old and new content:
    1. UNION: Add all NEW facts/details not present in the existing content
    2. SUBTRACT: Remove any facts from existing content that are explicitly contradicted by new content  
    3. PRESERVE: Keep all existing facts that are neither added nor contradicted
    Do NOT simply append or concatenate. Produce a clean, deduplicated result.
    ```
  - **events/cases类别开放MERGE**: 修改L158规则，events和cases也允许MERGE操作（不再限制只能CREATE/SKIP）
  - **加去重规则**: 同一session内同主题的记忆应MERGE而非重复CREATE

  **6b. 记忆格式视觉区分优化**:
  - **问题**: 当前 `- 内容: xxx` `- 影响范围: xxx` `- 结论: xxx` 标题和内容文字完全一样，肉眼看不出区分
  - **修复**: 标题加粗 → `- **内容**: xxx` `- **影响范围**: xxx` `- **结论**: xxx`
  - **两处prompt模板都要改**:
    1. L394-404 WORK Format（extractor提取时）: `- 内容:` → `- **内容**:`
    2. L789-806 RECONCILE WORK OUTPUT FORMAT（reconciler合并时）: 同上
  - **示例也同步更新**: L803-805中文示例 + L810+英文示例

  **Must NOT do**:
  - 不改reconciler.rs的程序逻辑（那是Task 7）
  - 不改其他prompt内容
  - 不删现有的SKIP/SUPERSEDE/SUPPORT等规则

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 需要理解LLM prompt工程 + reconciler语义
  - **Skills**: `["omem-iteration"]`
    - `omem-iteration`: 迭代管理铁律（证据驱动、审查必严、先封印再飞升）

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5, 7)
  - **Blocks**: Task 7, Task 8
  - **Blocked By**: None (独立于Plugin改造)

  **References**:
  - `omem-server/src/ingest/prompts.rs` — RECONCILE_SYSTEM_PROMPT L143-181
    - L148: 当前MERGE指令 — "combining both old and new info"（太弱）
    - L158: events/cases限制 — "For events/cases: only CREATE or SKIP"（需开放MERGE）
    - L167: merged_content要求 — 需加交并差指令
    - L394-404: WORK Format模板 — `- 内容:` 标题不加粗（需改为加粗）
    - L789-806: RECONCILE WORK OUTPUT FORMAT — 同上
    - L803-805, L810+: 示例也需同步加粗
    - 提取: 完整prompt结构，确定插入点

  **Acceptance Criteria**:
  - [ ] MERGE指令包含明确的UNION/SUBTRACT/PRESERVE操作
  - [ ] events/cases类别允许MERGE操作
  - [ ] WORK Format模板标题加粗（`- **内容**:` 等）
  - [ ] 两处prompt模板 + 示例全部同步更新
  - [ ] `cargo check -p omem-server` → PASS

  **QA Scenarios**:

  ```
  Scenario: prompt编译通过
    Tool: Bash
    Steps:
      1. cargo check -p omem-server
      2. 确认无编译错误
    Expected Result: PASS
    Evidence: .sisyphus/evidence/task-6-cargo-check.txt

  Scenario: events类别MERGE指令存在
    Tool: Bash
    Steps:
      1. grep -A5 "events" omem-server/src/ingest/prompts.rs | grep -i "merge"
      2. 确认events不再被限制为only CREATE or SKIP
    Expected Result: events允许MERGE
    Evidence: .sisyphus/evidence/task-6-events-merge.txt
  ```

  **Commit**: YES (group with Task 7)
  - Message: `feat(server): improve reconciler with set-diff merge + events MERGE support`
  - Files: `omem-server/src/ingest/prompts.rs`
  - Pre-commit: `cargo check -p omem-server`

- [x] 7. reconciler.rs events开放MERGE + fast_session_merge差集

  **What to do**:

  **7a. events/cases开放MERGE**:
  - reconciler.rs中处理LLM decision的地方，events/cases类别需要接受MERGE操作
  - 查找是否有程序级过滤阻止events做MERGE的代码
  - 如果有，修改为允许events/cases的MERGE操作通过

  **7b. fast_session_merge差集合并（程序级，不靠LLM）**:
  - 当前L254-300: 同session jaccard>0.5时，直接把新fact的l2_content追加到已有记忆content末尾
  - **玄机P0警告**: 纯靠LLM做交并差不可靠，LLM倾向简单拼接。必须做**程序级段落diff**。
  - 改为: 解析新旧content的段落(按`## YYYY-MM-DD`标题分割)，做**程序级差集运算**:
    1. 解析已有记忆的段落集合 — 按`##`标题行分割，每个段落 = 标题 + 内容
    2. 解析新fact的段落集合 — 同上
    3. **标题匹配去重**: 如果新段落标题与已有段落标题相同（或jaccard>0.7），保留内容更丰富的版本
    4. **新段落追加**: 不在已有记忆中的标题，直接追加
    5. 最终合并 = 去重后的段落集合，按时间排序
  - **关键**: 这是Rust程序逻辑，不是prompt。LLM只负责MERGE的merged_content生成，fast_session_merge走程序级diff。
  - 保持jaccard>0.5阈值不变

  **Must NOT do**:
  - 不改jaccard阈值
  - 不改batch_self_dedup和exact_match_dedup逻辑
  - 不改LLM reconcile的其他decision处理

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Rust代码 + 差集算法 + 需理解reconciler全流程
  - **Skills**: `["omem-iteration"]`
    - `omem-iteration`: 迭代管理铁律（证据驱动、审查必严、先封印再飞升）

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5, 6)
  - **Blocks**: Task 8
  - **Blocked By**: Task 6 (需要prompt改好后再改reconciler逻辑保持一致)

  **References**:
  - `omem-server/src/ingest/reconciler.rs`
    - L48-252: 主reconcile流程
    - L155-194: MERGE处理 — `updated.content = merged_content.to_string()`
    - L254-300: fast_session_merge — 当前直接追加，需改为差集
    - L101: session_id用于fast_session_merge
    - 提取: 需要修改的代码段和上下文
  - `omem-server/src/ingest/prompts.rs` — Task 6的prompt改动
    - 提取: 新的MERGE语义，确保程序逻辑与prompt一致

  **Acceptance Criteria**:
  - [ ] events/cases类别MERGE操作不被程序级阻止
  - [ ] fast_session_merge做差集合并而非追加
  - [ ] `cargo check -p omem-server` → PASS
  - [ ] `cargo test -p omem-server` → all pass

  **QA Scenarios**:

  ```
  Scenario: cargo test通过
    Tool: Bash
    Steps:
      1. cargo test -p omem-server
      2. 确认所有测试通过
    Expected Result: 0 failures
    Evidence: .sisyphus/evidence/task-7-cargo-test.txt

  Scenario: fast_session_merge差集合并逻辑验证
    Tool: Bash
    Steps:
      1. grep -A20 "fast_session_merge" omem-server/src/ingest/reconciler.rs
      2. 确认不再有简单的push/append操作
      3. 确认有段落解析和差集逻辑
    Expected Result: 差集合并代码存在
    Evidence: .sisyphus/evidence/task-7-diff-merge.txt
  ```

  **Commit**: YES (group with Task 6)
  - Message: `feat(server): improve reconciler with set-diff merge + events MERGE support`
  - Files: `omem-server/src/ingest/reconciler.rs`
  - Pre-commit: `cargo check -p omem-server`

- [x] 8. Build验证 + npm publish

  **What to do**:
  - `cd plugins/opencode && npx tsc --noEmit` 确认0 errors
  - `npm run build` 确认构建成功
  - bump package.json version: 1.10.6 → 1.10.7
  - `npm publish --access public`
  - 清除本地缓存: `rm -rf ~/.cache/opencode/packages/@mingxy/cerebro*`
  - `cargo build --release` + 部署到服务器(scp + systemctl restart omem)
  - 验证health: `curl https://www.mengxy.cc/health`

  **Must NOT do**:
  - 不改任何源代码
  - 不改config.json

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: 纯构建发布流程
  - **Skills**: `["omem-iteration"]`
    - `omem-iteration`: 迭代管理铁律（证据驱动、审查必严、先封印再飞升）

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 (sequential)
  - **Blocks**: FINAL
  - **Blocked By**: Tasks 2, 3, 4, 5, 7 (所有代码任务)

  **References**:
  - `plugins/opencode/package.json` — 当前version: 1.10.6
  - 服务器部署流程: scp → systemctl restart omem → curl /health

  **Acceptance Criteria**:
  - [ ] tsc --noEmit → 0 errors
  - [ ] npm run build → success
  - [ ] npm publish → @mingxy/cerebro@1.10.7 published
  - [ ] 缓存清除
  - [ ] cargo build --release → success
  - [ ] 部署到服务器 + health ok

  **QA Scenarios**:

  ```
  Scenario: 全链路build+publish
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npx tsc --noEmit
      2. npm run build
      3. npm publish --access public
      4. rm -rf ~/.cache/opencode/packages/@mingxy/cerebro*
    Expected Result: 全部成功
    Evidence: .sisyphus/evidence/task-8-build-publish.txt

  Scenario: Rust服务器部署验证
    Tool: Bash
    Steps:
      1. cargo build --release
      2. ssh root@47.93.199.242 'systemctl stop omem'
      3. scp target/release/omem-server root@47.93.199.242:/opt/omem/omem-server
      4. ssh root@47.93.199.242 'systemctl start omem'
      5. curl https://www.mengxy.cc/health
    Expected Result: {"status":"ok"}
    Evidence: .sisyphus/evidence/task-8-deploy.txt
  ```

  **Commit**: YES
  - Message: `chore(plugin): bump version 1.10.7 + publish`
  - Files: `plugins/opencode/package.json`

---

## Final Verification Wave

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `npx tsc --noEmit` in plugins/opencode + `cargo check` in omem-server. Review all changed files for: `as any`/`@ts-ignore`, empty catches, console.log in prod, commented-out code, unused imports. Check AI slop patterns.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Files [N clean/N issues] | VERDICT`

- [x] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Test cross-task integration. Test edge cases: old config format, unknown agent name, empty policy. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [x] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — everything in spec was built, nothing beyond spec. Check "Must NOT do" compliance. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Task 1-3**: `refactor(plugin): restructure config with nested groups` — config.ts, logger.ts, omem.example.jsonc
- **Task 4-5**: `feat(plugin): add agent memory policy + recall logging + drop parentSessionId` — client.ts, hooks.ts
- **Task 6-7**: `feat(server): improve reconciler with set-diff merge + events MERGE support` — prompts.rs, reconciler.rs
- **Task 8**: `chore(plugin): bump version + publish` — package.json

---

## Success Criteria

### Verification Commands
```bash
cd plugins/opencode && npx tsc --noEmit   # Expected: 0 errors
cd omem-server && cargo check              # Expected: PASS
cd omem-server && cargo test               # Expected: all pass
```

### Final Checklist
- [x] All "Must Have" present
- [x] All "Must NOT Have" absent
- [x] tsc --noEmit clean
- [x] cargo check clean
- [x] 玄机(oracle)评审通过
