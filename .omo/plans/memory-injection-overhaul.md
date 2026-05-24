# 记忆召回注入彻底改造

## TL;DR

> **Quick Summary**: 扒 Supermemory 设计，将 Cerebro Plugin 的记忆召回从 `system.transform`(system末尾追加) 迁移到 `chat.message`(parts.unshift 用户消息前注入)，首轮一次注入 Profile+项目最新记忆+搜索结果，后续靠 tool。同时修复 web server 不停止 + compact 后记忆不保存两个 bug。
> 
> **Deliverables**:
> - Plugin 端 hooks.ts 重构（新注入逻辑 + 旧代码清理）
> - Plugin 端 index.ts hook 注册迁移
> - Plugin 端 keywords.ts 关键字优化
> - 服务端 profile_v2/injection.rs 格式改为 markdown
> - Bug 修复：web server shutdown + compact 后 processedMessageIds
> 
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 3 waves
> **Critical Path**: Task 1(POC) → Task 2-3(bug fix) → Task 4-9(改造) → Task 10(服务端) → F1-F4

---

## Context

### Original Request
师尊要求彻底改造记忆召回注入机制，参考 Supermemory 的 `chat.message` + `parts.unshift` 方案，替换当前 `system.transform` + `output.system[last] +=` 方案。

### Interview Summary
**Key Discussions**:
- 注入 Hook 从 `system.transform` 改为 `chat.message`：system末尾注意力不够，parts.unshift在用户消息前面更可见
- 砍掉 shouldRecall LLM Gate：省掉3s+复杂度，首轮直接搜
- 砍掉 ID/分类标题/FETCH_POLICY：精简注入内容
- Profile和Context合并为单一 `[CEREBRO-MEMORY]` 块
- 加入项目最新5条记忆（project_path 过滤，按 updated_at desc 排序）
- 服务端 v2-profile inject 返回格式从 `<cerebro-profile>` XML 改为 markdown

**Research Findings**:
- Supermemory 验证了 `parts.unshift(synthetic:true)` 在用户消息前注入的有效性
- 服务端 `GET /v1/memories` 已支持 `project_path` + `sort=updated_at` + `order=desc` ✅
- `ListQuery` struct (`memory.rs:83-101`) 已有完整过滤参数
- Plugin 端 `client.ts:301` 已有 `listRecent()` 方法，需扩展支持 project_path
- `<cerebro-profile>` 生成在 `profile_v2/injection.rs:123`，另在 `memory.rs:2351` XML清理列表中
- opencode-mem shutdown 方案：显式 `process.exit(0)` + `process.on('disconnect')`

### Metis Review (1st round)
**Identified Gaps (addressed)**:
- 首轮 query 为空时 injectedSessions 已 set → 改为先检查 query 再 add
- synthetic:true 未验证 → 新增 POC 任务
- 搜索+项目记忆重叠无去重 → 新增 seenIds 去重
- Token 无 budget 控制 → 新增 maxChars=4000 budget
- API 超时无保护 → Promise.race 超时 (3s/2s/3s)

### Momus Review (1st round — REJECT, 3 compilation errors)
- FETCH_POLICY 删但 compactingHook 仍引用 → 保留，仅从新注入路径删
- profileInjectedSessions/lastUserMsgCount 删但 compactingHook+sessionIdleHook 仍引用 → 保留
- autoRecallHook 删但 index.ts import 未更新 → 更新 import

---

## Work Objectives

### Core Objective
将记忆召回注入从 system 末尾迁移到用户消息前面，让 LLM 真正看到并利用记忆。

### Concrete Deliverables
- `plugins/opencode/src/hooks.ts` — 新增 `buildMemoryInjection()` + `chatMessageRecallHook()`，删除 `autoRecallHook()`
- `plugins/opencode/src/index.ts` — hook 注册迁移，删除 `system.transform`
- `plugins/opencode/src/client.ts` — 扩展 `listRecent()` 支持 project_path
- `plugins/opencode/src/keywords.ts` — 优化关键字列表 + nudge 文案
- `omem-server/src/profile_v2/injection.rs` — `<cerebro-profile>` 改为 markdown
- `omem-server/src/api/handlers/memory.rs` — XML 清理列表添加 `[CEREBRO-MEMORY]` + `[CEREBRO-PROFILE]`

### Definition of Done
- [ ] `cd plugins/opencode && npm run build` → 零错误
- [ ] `cd omem-server && cargo build` → 零错误
- [ ] 新 session 首条消息 parts[0] 含 `[CEREBRO-MEMORY]`
- [ ] 同 session 第二条消息无 `[CEREBRO-MEMORY]`
- [ ] 注入内容含 `## User Profile`（markdown 格式，无 XML 标签）
- [ ] 注入内容含 `## Recent Project Activity`（5条项目记忆）
- [ ] OpenCode 窗口关闭后 localhost:5212 不再监听
- [ ] compact 后继续对话，session.idle 能保存新消息
- [ ] **[B2]** API 全超时时：不标记 injectedSessions + 不注入空壳 + 显示 warning toast
- [ ] **[B3]** 新用户无记忆时：不注入空壳 `[CEREBRO-MEMORY]\n\n[/CEREBRO-MEMORY]`
- [ ] **[B1]** memory.rs bracket_patterns 能正确清理 `[CEREBRO-MEMORY]...[/CEREBRO-MEMORY]` 块
- [ ] **[H3]** 服务端先部署，Plugin 后部署（部署顺序约束）

### Must Have
- `parts.unshift(synthetic:true)` 注入生效（POC 验证后）
- 首轮一次注入，后续靠 tool
- Profile 在上，项目记忆在中，搜索结果在下
- 项目最新5条按 `updated_at` desc（session_ingest 合并只更新 updated_at）
- seenIds 去重（项目记忆 vs 搜索结果重叠）
- maxChars=4000 token budget
- API 超时保护（profile 3s, list 2s, search 3s）

### Must NOT Have (Guardrails)
- 不改 session_ingest 路径
- 不改 tools.ts 的 16 个 tool 定义
- 不改 compactingHook 的 ingest 逻辑
- 不改 autocontinueHook 逻辑
- 不删 FETCH_POLICY（compactingHook 仍引用）
- 不删 profileInjectedSessions/lastUserMsgCount（compactingHook+sessionIdleHook 仍引用）
- 不引入新外部依赖

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: NO (Plugin 端无测试)
- **Automated tests**: None（Plugin 无 test 框架）
- **Framework**: N/A

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.omo/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Plugin**: Use `interactive_bash` (tmux) — Build, check output, verify no errors
- **Server**: Use `Bash` — `cargo build`, `cargo test` related tests
- **Integration**: Use `Bash` (curl) — Hit API endpoints, verify response format

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (POC + Bug Fixes — start immediately):
├── Task 1: POC 验证 parts.unshift(synthetic:true) [quick]
├── Task 2: 修复 web server 不停止 bug [quick]
└── Task 3: 修复 compact 后记忆不保存 bug [quick]

Wave 2 (Plugin 端改造 — after Wave 1 POC passes):
├── Task 4: client.ts 扩展 listRecent() [quick]
├── Task 5: hooks.ts 新增 buildMemoryInjection() [unspecified-high]
├── Task 6: hooks.ts 新增 chatMessageRecallHook() [unspecified-high]
├── Task 7: keywords.ts 优化关键字列表 [quick]
└── Task 8: index.ts hook 注册迁移 + 旧代码清理 [unspecified-high]

Wave 3 (服务端 — must deploy BEFORE Wave 2 merges to main):
└── Task 9: 服务端 profile 格式改 markdown + 清理逻辑修复 [quick]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit [oracle]
├── Task F2: Code quality review [unspecified-high]
├── Task F3: Real manual QA [unspecified-high]
└── Task F4: Scope fidelity check [deep]
→ Present results → Get explicit user okay

Critical Path: Task 1 → Task 5 → Task 6 → Task 8 → F1-F4
Max Concurrent: 3 (Wave 1), 5 (Wave 2+3)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1 | - | 5,6,8 | 1 |
| 2 | - | - | 1 |
| 3 | - | - | 1 |
| 4 | 1(POC pass) | 5 | 2 |
| 5 | 4 | 6 | 2 |
| 6 | 5 | 8 | 2 |
| 7 | - | 8 | 2 |
| 8 | 6,7 | F1-F4 | 2 |
| 9 | - | F1-F4 | 3 |

> **[H3 FIX] 部署约束**: Task 9（服务端）必须先于 Task 8（Plugin 集成）合入主分支并部署。
> 理由：服务端改 markdown 格式后，旧 Plugin 不直接消费 `/v2/profile/inject`（零影响）；
> 但新 Plugin 上线后需要服务端已返回 markdown 格式。
| F1-F4 | 8,9 | user okay | FINAL |

### Agent Dispatch Summary

- **Wave 1**: 3 tasks — T1 `quick`, T2 `quick`, T3 `quick`
- **Wave 2**: 5 tasks — T4 `quick`, T5 `unspecified-high`, T6 `unspecified-high`, T7 `quick`, T8 `unspecified-high`
- **Wave 3**: 1 task — T9 `quick`
- **FINAL**: 4 tasks — F1 `oracle`, F2 `unspecified-high`, F3 `unspecified-high`, F4 `deep`

---

## TODOs

- [ ] 1. POC: 验证 parts.unshift(synthetic:true) 是否被 LLM 看到

  **What to do**:
  - 在 `plugins/opencode/src/hooks.ts` 末尾添加一个临时 POC hook 函数
  - 在 `plugins/opencode/src/index.ts` 的 `chat.message` handler 中添加 POC 测试代码
  - 逻辑：当 `!input.sessionID` 或已注入时跳过，否则 `output.parts.unshift({ type: "text", text: "[CEREBRO-POC] Test injection - if you see this, respond with 'POC received'.", synthetic: true })`
  - 编译后实际启动 OpenCode，新开 session 发消息，观察 LLM 是否响应 "POC received"
  - **如果 POC 失败**（LLM 看不到 synthetic parts），则整个改造方案需重新评估

  **Must NOT do**:
  - 不要修改任何现有 hook 逻辑
  - 不要删除任何现有代码
  - POC 代码是临时的，验证后删除

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Tasks 5, 6, 8 (POC 必须通过才继续)
  - **Blocked By**: None

  **References**:
  - `plugins/opencode/src/hooks.ts:581-621` — keywordDetectionHook 示例（chat.message hook 的 input/output 签名）
  - `plugins/opencode/src/index.ts:153` — 当前 chat.message 注册位置
  - `plugins/opencode/src/client.ts:64-75` — Part/UserMessage 类型导入

  **Acceptance Criteria**:
  - [ ] `cd plugins/opencode && npm run build` → PASS
  - [ ] OpenCode 新 session 发消息后，LLM 回复包含 "POC received"
  - [ ] 截图/日志保存到 `.omo/evidence/task-1-poc-result.txt`

  **QA Scenarios**:
  ```
  Scenario: POC injection visible to LLM
    Tool: Bash (npm run build) + interactive_bash (tmux)
    Preconditions: Plugin compiled successfully
    Steps:
      1. cd plugins/opencode && npm run build
      2. Start OpenCode with plugin loaded
      3. Send "hello" in new session
      4. Check LLM response for "POC received"
    Expected Result: LLM response contains "POC received"
    Failure Indicators: LLM responds normally without acknowledging POC message
    Evidence: .omo/evidence/task-1-poc-result.txt

  Scenario: POC does not break existing functionality
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: Build succeeds with zero errors
    Evidence: .omo/evidence/task-1-build.txt
  ```

  **Commit**: YES
  - Message: `test(plugin): add POC for parts.unshift synthetic injection`
  - Files: `plugins/opencode/src/hooks.ts`, `plugins/opencode/src/index.ts`

- [ ] 2. 修复 web server 不停止 bug

  **What to do**:
  - 修改 `plugins/opencode/src/index.ts` 行127-134 的 shutdown handler
  - 参照 opencode-mem (`src/index.ts:128-142`) 的方案：
    1. 在 shutdown handler 末尾添加 `process.exit(0)`
    2. 新增 `process.on('disconnect', shutdownHandler)` 处理 IPC 断开场景
    3. 添加 `process.on('exit', () => { webServer?.close() })` 作为最后防线

  **具体改动** (`plugins/opencode/src/index.ts`):
  ```typescript
  // 替换行127-134
  const shutdown = async () => {
    try {
      if (webServer) {
        await stopWebServer(webServer);
        webServer = null;
      }
    } catch {}
    process.exit(0);  // 强制退出，确保 HTTP server 停止
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);
  process.on("disconnect", shutdown);  // OpenCode 窗口关闭时触发
  ```

  **Must NOT do**:
  - 不要改 `web-server.ts` 的 stopWebServer 逻辑（已有 closeAllConnections + 3s 超时，够用）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: None
  - **Blocked By**: None

  **References**:
  - `plugins/opencode/src/index.ts:127-134` — 当前 shutdown handler（需替换）
  - `plugins/opencode/src/web-server.ts:172-182` — stopWebServer 函数（不改）
  - `/mnt/d/dev/github/project/opencode-mem/src/index.ts:128-142` — 参考实现

  **Acceptance Criteria**:
  - [ ] `cd plugins/opencode && npm run build` → PASS
  - [ ] shutdown handler 包含 `process.exit(0)` + `process.on('disconnect')`

  **QA Scenarios**:
  ```
  Scenario: Web server stops on shutdown signal
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
      2. Verify shutdown handler in compiled output contains process.exit(0)
    Expected Result: Build succeeds, shutdown handler has process.exit(0)
    Evidence: .omo/evidence/task-2-build.txt
  ```

  **Commit**: YES
  - Message: `fix(plugin): web server shutdown on window close`
  - Files: `plugins/opencode/src/index.ts`

- [ ] 3. 修复 compact 后记忆不保存 bug

  **What to do**:
  - 问题根因：`compactingHook`（hooks.ts:777）在 compact 后清空 sessionMessages，但 `processedMessageIds`（全局 Set）仍持有旧消息 ID。compact 后消息以新形式出现，但 ID 可能相同，导致 `sessionIdleHook` 跳过它们
  - **方案**: 将 `processedMessageIds` 从全局 `Set<string>` 改为按 session 隔离的 `Map<string, Set<string>>`，compact 时只清理对应 session 的 Set
  - 同时修改 `sessionIdleHook`（行1074 `.has()` 和行1134 `.add()`）适配 Map API

  **具体改动 1** — 类型定义修改 (`plugins/opencode/src/hooks.ts`):
  ```typescript
  // 替换行905: const processedMessageIds = new Set<string>();
  // 改为按 session 隔离:
  const processedMessageIds = new Map<string, Set<string>>();
  // 注意：行178是 saveKeywordDetectedSessions，不是 processedMessageIds！
  ```

  **具体改动 2** — compactingHook 清理 (`plugins/opencode/src/hooks.ts:777-783`):
  ```typescript
  if (input.sessionID) {
    sessionMessages.delete(input.sessionID);
    profileInjectedSessions.delete(input.sessionID);
    lastUserMsgCount.delete(input.sessionID);
    firstMessages.delete(input.sessionID);
    // FIX: compact 后只清理该 session 的消息去重集合，允许后续 idle 重新处理
    processedMessageIds.delete(input.sessionID);
    logDebug("compactingHook cleared processedMessageIds for session", { sessionID: input.sessionID });
  }
  ```

  **具体改动 3** — sessionIdleHook 适配 Map API (`plugins/opencode/src/hooks.ts`):
  ```typescript
  // 行1074 替换:
  // 旧: if (processedMessageIds.has(msgId)) continue;
  // 新:
  if (!processedMessageIds.has(input.sessionID)) {
    processedMessageIds.set(input.sessionID, new Set());
  }
  if (processedMessageIds.get(input.sessionID)!.has(msgId)) continue;

  // 行1134 替换:
  // 旧: processedMessageIds.add(msgId);
  // 新:
  processedMessageIds.get(input.sessionID)!.add(msgId);
  ```

  **Must NOT do**:
  - 不要改 compactingHook 的 ingest 逻辑
  - 不要改 autocontinueHook
  - 不要用 `processedMessageIds.clear()` 全量清空（会影响其他 session 的去重）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: None
  - **Blocked By**: None

  **References**:
  - `plugins/opencode/src/hooks.ts:774-783` — compactingHook 清理逻辑（需修改）
  - `plugins/opencode/src/hooks.ts:905` — processedMessageIds 定义（需改为 Map，非行178！行178是 saveKeywordDetectedSessions）
  - `plugins/opencode/src/hooks.ts:1074` — processedMessageIds.has(msgId)（需适配 Map）
  - `plugins/opencode/src/hooks.ts:1134` — processedMessageIds.add(msgId)（需适配 Map）

  **Acceptance Criteria**:
  - [ ] `cd plugins/opencode && npm run build` → PASS
  - [ ] processedMessageIds 类型为 `Map<string, Set<string>>`
  - [ ] compactingHook 使用 `processedMessageIds.delete(sessionId)` 而非 `.clear()`
  - [ ] sessionIdleHook 使用 `.get(sessionId)!.has(msgId)` 和 `.get(sessionId)!.add(msgId)`

  **QA Scenarios**:
  ```
  Scenario: processedMessageIds changed to Map per-session
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
      2. grep -n "processedMessageIds" dist/index.js | head -20
    Expected Result: Build succeeds, output shows Map usage (not global Set)
    Evidence: .omo/evidence/task-3-build.txt

  Scenario: Compact only clears target session, not all sessions
    Tool: Bash (grep source)
    Steps:
      1. grep "processedMessageIds.clear" plugins/opencode/src/hooks.ts
    Expected Result: Zero matches (should use .delete(sessionId) instead)
    Evidence: .omo/evidence/task-3-no-clear.txt
  ```

  **Commit**: YES
  - Message: `fix(plugin): isolate processedMessageIds per session for compact fix`
  - Files: `plugins/opencode/src/hooks.ts`

- [ ] 4. client.ts 扩展 listRecent() 支持 project_path

  **What to do**:
  - 扩展 `plugins/opencode/src/client.ts` 现有 `listRecent()` 方法（行301-306），添加 project_path 参数
  - 服务端已支持 `GET /v1/memories?limit=5&sort=updated_at&order=desc&project_path=xxx`

  **具体改动** (`plugins/opencode/src/client.ts`):
  ```typescript
  // 替换行301-306
  async listRecent(limit = 20, projectPath?: string): Promise<MemoryDto[]> {
    const params = new URLSearchParams({ limit: String(limit), offset: "0", sort: "updated_at", order: "desc" });
    if (projectPath) params.set("project_path", projectPath);
    const res = await this.request<ListResponse>(
      `/v1/memories?${params}`,
    );
    return res?.memories ?? [];
  }
  ```

  **Must NOT do**:
  - 不要新增 listMemories 方法（扩展已有 listRecent）
  - 不要改 tools.ts 中 listRecent 的调用（兼容，新增参数可选）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 7)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 5
  - **Blocked By**: Task 1 (POC pass)

  **References**:
  - `plugins/opencode/src/client.ts:301-306` — 现有 listRecent()（需修改）
  - `plugins/opencode/src/client.ts:83-98` — MemoryDto 类型（返回格式）
  - `omem-server/src/api/handlers/memory.rs:83-101` — ListQuery struct（确认参数支持）
  - `omem-server/src/store/lancedb.rs:2165-2177` — 排序支持 updated_at

  **Acceptance Criteria**:
  - [ ] `cd plugins/opencode && npm run build` → PASS
  - [ ] listRecent 签名变为 `listRecent(limit?, projectPath?)`
  - [ ] tools.ts:197 调用 `listRecent()` 仍兼容（无参数）

  **QA Scenarios**:
  ```
  Scenario: listRecent with project_path filter
    Tool: Bash (curl)
    Steps:
      1. curl "https://www.mengxy.cc/v1/memories?limit=5&sort=updated_at&order=desc&project_path=/mnt/d/dev/github/project/omem-server-source" -H "X-API-Key: $OMEM_API_KEY"
      2. Verify response has memories array with project_path matching
    Expected Result: Returns 200 with memories filtered by project_path
    Evidence: .omo/evidence/task-4-api-test.txt

  Scenario: Build succeeds
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: Zero errors
    Evidence: .omo/evidence/task-4-build.txt
  ```

  **Commit**: YES
  - Message: `feat(plugin): extend listRecent with project_path filter`
  - Files: `plugins/opencode/src/client.ts`

- [ ] 5. hooks.ts 新增 buildMemoryInjection()

  **What to do**:
  - 在 `plugins/opencode/src/hooks.ts` 中新增 `buildMemoryInjection()` 函数
  - 并行获取 Profile + 项目最新5条 + 搜索结果
  - 统一格式化为 `[CEREBRO-MEMORY]` 块
  - 包含 seenIds 去重、maxChars budget、API 超时

  **具体代码** (添加在 `FETCH_POLICY` 常量后面):
  ```typescript
  const INJECTION_MAX_CHARS = 4000;

  interface InjectionResult {
    text: string;
    profileCount: number;
    memoryCount: number;
    projectMemoryCount: number;
  }

  async function buildMemoryInjection(
    client: CerebroClient,
    projectPath: string | undefined,
    query: string,
    config: Partial<OmemPluginConfig>,
  ): Promise<InjectionResult> {
    const empty: InjectionResult = { text: "", profileCount: 0, memoryCount: 0, projectMemoryCount: 0 };

    // 并行获取三部分数据，各自带超时
    const [profile, projectMemories, searchResults] = await Promise.all([
      Promise.race([
        client.getInjection(),  // 画像是全局的，不传 projectPath
        new Promise<null>((resolve) => setTimeout(() => resolve(null), 3000)),
      ]).catch(() => null),
      Promise.race([
        client.listRecent(5, projectPath),
        new Promise<never[]>((resolve) => setTimeout(() => resolve([]), 2000)),
      ]).catch(() => []),
      Promise.race([
        client.searchMemories(query, 10, undefined, undefined, projectPath),
        new Promise<never[]>((resolve) => setTimeout(() => resolve([]), 3000)),
      ]).catch(() => []),
    ]);

    const sections: string[] = ["[CEREBRO-MEMORY]", ""];

    // Profile（服务端已返回 ## User Profile\n- slot: value 格式）
    if (profile?.content) {
      sections.push(profile.content);
      sections.push("");
    }

    // 去重集合
    const seenIds = new Set<string>();

    // 项目最新5条
    if (projectMemories.length > 0) {
      sections.push("## Recent Project Activity");
      for (const m of projectMemories) {
        seenIds.add(m.id);
        const age = formatRelativeAge(m.updated_at || m.created_at) || "unknown";
        const content = truncate(m.content, 200);
        sections.push(`- (${age}) ${content}`);
      }
      sections.push("");
    }

    // 搜索结果（去重）
    const dedupedResults = (searchResults || []).filter((r) => !seenIds.has(r.memory.id));
    if (dedupedResults.length > 0) {
      sections.push("## Relevant Memories");
      for (const r of dedupedResults) {
        const age = formatRelativeAge(r.memory.created_at) || "unknown";
        const content = truncate(r.memory.content, 300);
        sections.push(`- (${age}) ${content}`);
      }
      sections.push("");
    }

    sections.push("[/CEREBRO-MEMORY]");

    let text = sections.join("\n");
    // Token budget 控制 — 段落级裁剪（不在句子中间截断）
    if (text.length > INJECTION_MAX_CHARS) {
      // 找最后一个换行符位置，在行边界截断
      const cutoff = text.lastIndexOf('\n', INJECTION_MAX_CHARS);
      text = text.slice(0, cutoff > 0 ? cutoff : INJECTION_MAX_CHARS) + "\n…\n[/CEREBRO-MEMORY]";
    }

    return {
      text,
      profileCount: profile?.preference_count ?? 0,
      memoryCount: dedupedResults?.length ?? 0,
      projectMemoryCount: projectMemories.length,
    };
  }
  ```

  **Must NOT do**:
  - 不要删除现有 `buildContextBlock()`（等 Task 8 统一清理）
  - 不要删除 FETCH_POLICY
  - 不要删除 formatMemoryLine、categorize 等旧函数

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 4)
  - **Parallel Group**: Wave 2 (sequential after Task 4)
  - **Blocks**: Task 6
  - **Blocked By**: Task 4

  **References**:
  - `plugins/opencode/src/hooks.ts:247-253` — FETCH_POLICY（保留，在其后添加新函数）
  - `plugins/opencode/src/hooks.ts:186-196` — formatRelativeAge()（复用）
  - `plugins/opencode/src/hooks.ts:198-217` — truncate()（复用）
  - `plugins/opencode/src/client.ts:284-291` — getInjection() API（返回 profile content）
  - `plugins/opencode/src/client.ts:219-237` — searchMemories() API

  **Acceptance Criteria**:
  - [ ] `cd plugins/opencode && npm run build` → PASS
  - [ ] buildMemoryInjection 函数存在且类型正确
  - [ ] 包含 seenIds 去重逻辑
  - [ ] 包含 INJECTION_MAX_CHARS budget 截断

  **QA Scenarios**:
  ```
  Scenario: buildMemoryInjection compiles
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: Zero errors
    Evidence: .omo/evidence/task-5-build.txt
  ```

  **Commit**: NO (groups with Task 6)

- [ ] 6. hooks.ts 新增 chatMessageRecallHook()

  **What to do**:
  - 在 `plugins/opencode/src/hooks.ts` 中新增 `chatMessageRecallHook()` 函数
  - 替代旧 `autoRecallHook()`：首轮 parts.unshift 注入一次
  - 关键：先检查 query 非空再 injectedSessions.add()（防止 "hi" 类空 query 导致永远不再注入）

  **具体代码** (添加在 buildMemoryInjection 后面):
  ```typescript
  const injectedSessions = new Set<string>();

  export function chatMessageRecallHook(
    client: CerebroClient,
    containerTags: string[],
    tui: any,
    config: Partial<OmemPluginConfig> = {},
    getAgentName?: () => string,
    directory?: string,
  ) {
    return async (
      input: { sessionID: string; messageID?: string },
      output: { message: UserMessage; parts: Part[] },
    ) => {
      if (!input.sessionID) return;

      // 已注入过 → 跳过
      if (injectedSessions.has(input.sessionID)) return;

      const agentId = getAgentName?.() || process.env.OMEM_AGENT_ID || "opencode";
      const policy = resolveAgentPolicy(agentId, config);
      if (policy === "none") {
        injectedSessions.add(input.sessionID); // 标记防止重试
        return;
      }

      // 提取用户 query
      const textContent = output.parts
        .filter((p: any) => p.type === "text")
        .map((p: any) => p.text || (p as any).content || "")
        .join(" ")
        || (output.message as any).content
        || "";

      const query = extractUserRequest(textContent);

      // Smart Query Gate：寒暄检测，跳过无意义搜索（省 3s 搜索延迟）
      const TRIVIAL_PATTERNS = /^(hi|hello|hey|你好|嗨|嗯|ok|okay|好的|收到|\s*)$/i;
      if (!query || TRIVIAL_PATTERNS.test(query.trim())) {
        logDebug("chatMessageRecallHook: trivial query, will retry next turn", { sessionId: input.sessionID });
        return;
      }

      try {
        const injection = await buildMemoryInjection(client, directory, query, config);

        // [FIX B3] 检查实质内容，不仅仅是长度 —— 防止空壳注入
        const hasContent = (injection.profileCount ?? 0) > 0
          || (injection.memoryCount ?? 0) > 0
          || (injection.projectMemoryCount ?? 0) > 0;

        if (injection.text && hasContent && injection.text.length > 20) {
          // [FIX B2] injectedSessions.add() 移到成功注入之后 —— 防止 API 超时后永远不再注入
          injectedSessions.add(input.sessionID);

          output.parts.unshift({
            type: "text",
            text: injection.text,
            synthetic: true,
          } as any);

          showToast(tui, "🧠 Memory Injected",
            `${injection.profileCount} prefs · ${injection.projectMemoryCount} project · ${injection.memoryCount} relevant`,
            "success");
        } else if (!hasContent) {
          // 全部为空（API 超时或新用户无记忆）→ 不注入不标记，允许后续重试
          logDebug("chatMessageRecallHook: no content available, will retry next turn", {
            sessionId: input.sessionID,
            profileCount: injection.profileCount,
            memoryCount: injection.memoryCount,
            projectMemoryCount: injection.projectMemoryCount,
          });
          showToast(tui, "🧠 Memory Unavailable", "API timeout or no memories yet", "warning");
        }
      } catch (err) {
        logErr("chatMessageRecallHook failed", { error: String(err) });
        showToast(tui, "🧠 Memory Injection Failed", "Check connection", "error");
        // 注意：catch 也不标记 injectedSessions，允许下一轮重试
      }
    };
  }
  ```

  **关键修复说明（业务评审发现）**:
  - **[B2 FIX]** `injectedSessions.add()` 从注入逻辑**之前**移到**成功注入之后**。如果 API 全超时或注入失败，不标记 session，下一轮用户消息会重新尝试注入
  - **[B3 FIX]** 新增 `hasContent` 检查（profile/memory/project 三者是否全为0），防止空壳 `[CEREBRO-MEMORY]\n\n[/CEREBRO-MEMORY]` 被注入（34 chars > 20 的旧检查不够）
  - API 全超时或新用户无记忆时，显示 warning toast 提醒但不标记 session

  **同时修改 compactingHook** — compact 后清理 injectedSessions 以便重新注入:
  ```typescript
  // hooks.ts compactingHook 内，行787-790 的清理代码中添加：
  // (import 的 injectedSessions 需要在文件内可访问)
  injectedSessions.delete(input.sessionID);
  ```

  **[H1 增强] 后续补搜机制（可选，优先级 P1）**:
  如果发现长对话中"记忆断了"，可后续添加 N轮补搜逻辑：
  ```typescript
  // 在 chatMessageRecallHook 的 return 前添加：
  // 当 injectedSessions.has(sessionId) 时，检查是否需要补搜
  const turnCounts = new Map<string, number>();
  // 每10轮自动补搜一次
  if (injectedSessions.has(input.sessionID)) {
    const turns = (turnCounts.get(input.sessionID) || 0) + 1;
    turnCounts.set(input.sessionID, turns);
    if (turns % 10 === 0 && query) {
      const injection = await buildMemoryInjection(client, directory, query, config);
      if (injection.text && injection.text.length > 20) {
        output.parts.push({ type: "text", text: injection.text, synthetic: true } as any);
        showToast(tui, "🧠 Memory Refreshed", "Context updated", "info");
      }
    }
    return;
  }
  ```
  **注意**：此增强不作为本次改造的 Must Have，但作为后续优化方向记录。首轮改造聚焦于"首轮注入 + 后续靠 tool"的简洁方案。

  **Must NOT do**:
  - 不要删除 autoRecallHook（等 Task 8 统一清理）
  - 不要删 profileInjectedSessions/lastUserMsgCount（compactingHook+sessionIdleHook 仍引用）

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (sequential after Task 5)
  - **Blocks**: Task 8
  - **Blocked By**: Task 5

  **References**:
  - `plugins/opencode/src/hooks.ts:301-579` — autoRecallHook（参考旧逻辑，将被替代）
  - `plugins/opencode/src/hooks.ts:159-176` — extractUserRequest()（复用）
  - `plugins/opencode/src/hooks.ts:4-5` — import 类型
  - `plugins/opencode/src/hooks.ts:786-790` — compactingHook 清理代码（需添加 injectedSessions.delete）

  **Acceptance Criteria**:
  - [ ] `cd plugins/opencode && npm run build` → PASS
  - [ ] chatMessageRecallHook 导出函数存在
  - [ ] query 为空时不执行 injectedSessions.add()
  - [ ] **[B2]** injectedSessions.add() 仅在成功注入后执行（不在 try 之前）
  - [ ] **[B3]** hasContent 检查：profile/memory/project 全为0时不注入空壳
  - [ ] **[B3]** hasContent 为 false 时显示 warning toast 但不标记 session
  - [ ] compactingHook 包含 injectedSessions.delete()

  **QA Scenarios**:
  ```
  Scenario: chatMessageRecallHook compiles and exports
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: Zero errors
    Evidence: .omo/evidence/task-6-build.txt

  Scenario: [B2] injectedSessions.add() after successful injection
    Tool: Bash (grep source)
    Steps:
      1. grep -n "injectedSessions.add" plugins/opencode/src/hooks.ts
    Expected Result: add() calls are INSIDE the if(injection.text && hasContent) block, NOT before try
    Evidence: .omo/evidence/task-6-b2-verify.txt

  Scenario: [B3] hasContent check prevents empty shell injection
    Tool: Bash (grep source)
    Steps:
      1. grep -n "hasContent" plugins/opencode/src/hooks.ts
    Expected Result: hasContent variable exists and is checked before unshift
    Evidence: .omo/evidence/task-6-b3-verify.txt
  ```

  **Commit**: YES
  - Message: `feat(plugin): add buildMemoryInjection + chatMessageRecallHook`
  - Files: `plugins/opencode/src/hooks.ts`

- [ ] 7. keywords.ts 优化关键字列表

  **What to do**:
  - 扩展 `plugins/opencode/src/keywords.ts` 的 SAVE_KEYWORDS 列表
  - 优化 KEYWORD_NUDGE 文案

  **具体改动** (`plugins/opencode/src/keywords.ts`):
  ```typescript
  const SAVE_KEYWORDS: readonly string[] = [
    // English
    "remember", "save this", "don't forget", "keep in mind",
    "note that", "store this", "memorize",
    "make a note", "write this down", "jot this down",
    "for future reference", "bear in mind",
    "commit to memory", "take note",
    // Chinese
    "记住", "记一下", "保存", "记下来", "别忘了",
    "记好", "存一下", "记住了",
    "写下来", "记到", "存起来",
    // Tool-related
    "memory_store", "save memory", "store memory",
    "保存记忆", "存储记忆",
  ] as const;

  export function detectSaveKeyword(text: string): boolean {
    const lower = text.toLowerCase();
    return SAVE_KEYWORDS.some((kw) => lower.includes(kw));
  }

  export const KEYWORD_NUDGE =
    "[cerebro] The user wants you to remember this. Use the `memory_store` tool to save it now.";
  ```

  **Must NOT do**:
  - 不要改 detectSaveKeyword 的检测逻辑（includes 方式保持不变）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4, 5, 6)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 8
  - **Blocked By**: None

  **References**:
  - `plugins/opencode/src/keywords.ts:1-23` — 当前关键字列表（全部替换）

  **Acceptance Criteria**:
  - [ ] `cd plugins/opencode && npm run build` → PASS
  - [ ] SAVE_KEYWORDS 包含 "make a note", "写下来", "memory_store"

  **QA Scenarios**:
  ```
  Scenario: Keywords expanded and build passes
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: Zero errors
    Evidence: .omo/evidence/task-7-build.txt
  ```

  **Commit**: YES
  - Message: `feat(plugin): optimize keyword detection list`
  - Files: `plugins/opencode/src/keywords.ts`

- [ ] 8. index.ts hook 注册迁移 + 旧代码清理

  **What to do**:
  这是集成任务，将所有改造整合到一起：

  **Step 1: 修改 index.ts hook 注册**
  - 删除 `experimental.chat.system.transform` 注册（行144-152）
  - 修改 `chat.message` handler 合并 recall + keyword detection + message tracking
  - 更新 import（删除 autoRecallHook，添加 chatMessageRecallHook）

  **具体改动** (`plugins/opencode/src/index.ts`):

  Import 行（替换行8，注意移除旧 import）:
  ```typescript
  // 旧的（需完全替换）:
  // import { autoRecallHook, keywordDetectionHook, autocontinueHook, compactingHook, showToast as hooksShowToast } from "./hooks.js";
  // 新的:
  import { chatMessageRecallHook, autocontinueHook, compactingHook, showToast as hooksShowToast, sessionMessages, firstMessages } from "./hooks.js";
  ```
  **关键**: 移除 `autoRecallHook` 和 `keywordDetectionHook` 两个旧 import

  Recall hook 创建（替换行106）:
  ```typescript
  const chatMessageRecall = chatMessageRecallHook(cerebroClient, containerTags, tui, config, () => cachedAgentName || agentId, directory);
  ```

  Handler 注册（替换行144-153）:
  ```typescript
  // 删除 experimental.chat.system.transform 整个注册
  "chat.message": async (input: any, output: any) => {
    // 锁定 mainSessionId
    if (input.sessionID && !mainSessionLocked) {
      mainSessionId = input.sessionID;
      mainSessionLocked = true;
      logInfo("mainSessionId locked", { sessionId: input.sessionID });
    }
    // 1. 首轮记忆注入
    await chatMessageRecall(input, output);
    // 2. 关键词检测 + nudge
    const textContent = output.parts
      .filter((p: any) => p.type === "text" && !(p as any).synthetic)
      .map((p: any) => p.text || (p as any).content || "")
      .join(" ")
      || (output.message as any).content
      || "";
    if (!firstMessages.has(input.sessionID)) {
      firstMessages.set(input.sessionID, textContent);
    }
    if (detectSaveKeyword(textContent)) {
      output.parts.push({
        type: "text",
        text: KEYWORD_NUDGE,
        synthetic: true,
      } as any);
      logDebug("keyword nudge pushed via parts.push", { sessionId: input.sessionID });
    }
    // 3. Message tracking (给 compacting 用)
    const policy = resolveAgentPolicy(agentId, config);
    if (policy !== "none") {
      if (!sessionMessages.has(input.sessionID)) {
        sessionMessages.set(input.sessionID, []);
      }
      sessionMessages.get(input.sessionID)!.push({ role: "user", content: textContent });
    }
  },
  ```

  添加 import:
  ```typescript
  import { detectSaveKeyword, KEYWORD_NUDGE } from "./keywords.js";
  ```
  （注意：需要在 import 区域添加 keywords 的 import）

  还需要 import session-level 的 Map:
  ```typescript
  import { chatMessageRecallHook, autocontinueHook, compactingHook, showToast as hooksShowToast, sessionMessages, firstMessages } from "./hooks.js";
  ```
  （sessionMessages 和 firstMessages 需要从 hooks.ts 导出 — 在 hooks.ts 中确认它们是否已 export）

  **Step 2: 清理 hooks.ts 旧代码**
  - 删除 `autoRecallHook()` 函数（行301-579）
  - 删除 `buildContextBlock()` 函数（行265-299）
  - 删除 `categorize()` 函数（行219-233）
  - 删除 `formatMemoryLine()` 函数（行235-245）
  - 删除 `appendToSystem()` 函数（行16-22）
  - 删除 `keywordDetectionHook()` 函数体（行581-621）—— 注意：index.ts 的 chat.message handler 已内联 keyword 逻辑，此函数成为死代码
  - **保留**: FETCH_POLICY, profileInjectedSessions, lastUserMsgCount, lastProfileBlock（其他函数仍引用）
  - **保留**: sessionMessages, firstMessages, summarizedSessions, processedMessageIds（compacting/idle 用）
  - **必须添加 export**: `sessionMessages`（行180）和 `firstMessages`（行179）当前没有 export，Task 8 必须在行首添加 `export` 关键字，否则 index.ts import 编译失败
  - **清理 unused import**: 删除 keywordDetectionHook 后，hooks.ts 行4 的 `import { detectSaveKeyword, KEYWORD_NUDGE }` 如果不再被 hooks.ts 内部使用，也要删除（index.ts 直接从 keywords.js import）

  **Must NOT do**:
  - 不要删 FETCH_POLICY
  - 不要删 profileInjectedSessions/lastUserMsgCount/lastProfileBlock
  - 不要删 compactingHook, autocontinueHook, sessionIdleHook
  - 不要改 tools.ts

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (after Tasks 6, 7)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 6, 7

  **References**:
  - `plugins/opencode/src/index.ts:1-11` — imports（需修改）
  - `plugins/opencode/src/index.ts:106` — recallHook 创建（需替换）
  - `plugins/opencode/src/index.ts:144-157` — hook 注册（需修改）
  - `plugins/opencode/src/hooks.ts:16-22` — appendToSystem（删除）
  - `plugins/opencode/src/hooks.ts:219-299` — categorize + formatMemoryLine + buildContextBlock（删除）
  - `plugins/opencode/src/hooks.ts:301-579` — autoRecallHook（删除）

  **Acceptance Criteria**:
  - [ ] `cd plugins/opencode && npm run build` → PASS
  - [ ] index.ts 无 `system.transform` 注册
  - [ ] index.ts `chat.message` handler 包含 chatMessageRecall + keyword nudge
  - [ ] hooks.ts 无 autoRecallHook 函数

  **QA Scenarios**:
  ```
  Scenario: Full plugin build after migration
    Tool: Bash
    Steps:
      1. cd plugins/opencode && npm run build
    Expected Result: Zero errors, no warnings about missing imports
    Evidence: .omo/evidence/task-8-build.txt

  Scenario: No system.transform registration
    Tool: Bash (grep)
    Steps:
      1. grep -r "system.transform" plugins/opencode/src/
    Expected Result: Zero matches
    Evidence: .omo/evidence/task-8-no-system-transform.txt
  ```

  **Commit**: YES
  - Message: `refactor(plugin): migrate recall from system.transform to chat.message`
  - Files: `plugins/opencode/src/hooks.ts`, `plugins/opencode/src/index.ts`

- [ ] 9. 服务端 profile 格式改 markdown

  **What to do**:
  - 修改 `omem-server/src/profile_v2/injection.rs` 的格式化逻辑
  - 将 `<cerebro-profile>` XML 标签改为 markdown 格式
  - 修改 `omem-server/src/api/handlers/memory.rs` 的 XML 清理列表，添加新标签

  **具体改动 1** (`omem-server/src/profile_v2/injection.rs:114-126`):
  ```rust
  // 替换行114-126
  // 5. 格式化为 markdown
  let content = if selected.is_empty() {
      String::new()
  } else {
      let lines: Vec<String> = selected
          .iter()
          .map(|p| format!("- {}: {}", p.slot, p.value))
          .collect();
      format!("## User Profile\n{}", lines.join("\n"))
  };
  ```

  **具体改动 2** (`omem-server/src/api/handlers/memory.rs:2343-2380`):
  在 xml_patterns 列表中添加新标签，**同时修改清理逻辑以支持方括号格式**:
  ```rust
  // [FIX B1] 现有 xml_patterns 清理逻辑假设标签以 < 开头、用 </tag> 闭合
  // 无法正确处理 [CEREBRO-MEMORY]...[/CEREBRO-MEMORY] 方括号格式
  // 需要为方括号块增加独立的清理逻辑

  // 1. 在 xml_patterns 末尾保留旧标签（兼容旧数据）
  let xml_patterns = [
      "<system-reminder>",
      // ... 保留现有 ...
      "<cerebro-profile>",     // 保留旧标签清理
      "<cerebro-fetch-policy>",
      // ... 其余保留 ...
  ];

  // 2. 新增：方括号块清理（在 xml 清理之后执行）
  let bracket_patterns = [
      ("[CEREBRO-MEMORY]", "[/CEREBRO-MEMORY]"),
      // 未来可扩展其他方括号标签
  ];
  for (open, close) in &bracket_patterns {
      while let Some(start) = cleaned.find(open) {
          if let Some(end) = cleaned[start + open.len()..].find(close) {
              let block_end = start + open.len() + end + close.len();
              cleaned = format!("{}{}", &cleaned[..start], &cleaned[block_end..]);
          } else {
              // 无闭合标签 → 只删除开始标签行
              if let Some(line_end) = cleaned[start..].find('\n') {
                  cleaned = format!("{}{}", &cleaned[..start], &cleaned[start + line_end..]);
              } else {
                  cleaned.truncate(start);
              }
          }
      }
  }
  ```

  **关键修复说明（业务评审发现）**:
  - **[B1 FIX]** 原清理逻辑用 `&tag_name[1..]` 去掉首字符 `<` 再拼 `</tag>`，对方括号标签生成错误的闭合标签
  - 新增 `bracket_patterns` 独立处理 `[TAG]...[/TAG]` 格式：直接匹配开闭标签对，整体删除块内容
  - 与现有 `xml_patterns` 清理逻辑并行，互不影响

  **Must NOT do**:
  - 不要改 injection.rs 的选择逻辑（budget 分配等）
  - 不要改 injection.rs 的缓存逻辑
  - 不要删 memory.rs 中的旧 `<cerebro-profile>` 清理项（兼容旧数据）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Wave 2 tasks)
  - **Parallel Group**: Wave 3
  - **Blocks**: F1-F4
  - **Blocked By**: None

  **References**:
  - `omem-server/src/profile_v2/injection.rs:100-146` — 注入格式化完整逻辑
  - `omem-server/src/profile_v2/injection.rs:118-126` — 具体格式化代码（需修改）
  - `omem-server/src/api/handlers/memory.rs:2343-2357` — XML 清理列表（需添加新标签）

  **Acceptance Criteria**:
  - [ ] `cd omem-server && cargo build` → PASS
  - [ ] `cd omem-server && cargo test` → PASS
  - [ ] injection.rs 中无 `<cerebro-profile>` 字符串
  - [ ] **[B1]** memory.rs 新增 bracket_patterns 清理逻辑，支持 `[TAG]...[/TAG]` 格式
  - [ ] **[B1]** bracket_patterns 包含 `("[CEREBRO-MEMORY]", "[/CEREBRO-MEMORY]")`

  **QA Scenarios**:
  ```
  Scenario: Server builds with markdown profile format
    Tool: Bash
    Steps:
      1. cd omem-server && cargo build 2>&1
    Expected Result: Zero errors
    Evidence: .omo/evidence/task-9-cargo-build.txt

  Scenario: Server tests pass
    Tool: Bash
    Steps:
      1. cd omem-server && cargo test 2>&1
    Expected Result: All tests pass
    Evidence: .omo/evidence/task-9-cargo-test.txt

  Scenario: Profile injection returns markdown format
    Tool: Bash (curl)
    Steps:
      1. curl -s "https://www.mengxy.cc/v2/profile/inject" -H "X-API-Key: 4cf468b2-003c-4ae3-8274-2005389bffa6" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('content','')[:200])"
      2. Note: This tests after deployment, not during implementation
    Expected Result: Response starts with "## User Profile\n- slot: value" (after deploy)
    Evidence: .omo/evidence/task-9-profile-format.txt (post-deploy)
  ```

  **Commit**: YES
  - Message: `refactor(server): change profile inject format to markdown`
  - Files: `omem-server/src/profile_v2/injection.rs`, `omem-server/src/api/handlers/memory.rs`

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .omo/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cd plugins/opencode && npm run build` + `cd omem-server && cargo build` + `cargo clippy`. Review all changed files for: `as any`/`@ts-ignore` (new ones), empty catches, console.log in prod, commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state. Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Test cross-task integration. Test edge cases: empty profile, no project memories, API timeout. Save to `.omo/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — everything in spec was built, nothing beyond spec was built. Check "Must NOT do" compliance. Detect cross-task contamination. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

| Tasks | Message | Files | Pre-commit |
|-------|---------|-------|------------|
| 1 | `test(plugin): add POC for parts.unshift synthetic injection` | hooks.ts | npm run build |
| 2 | `fix(plugin): web server shutdown on window close` | index.ts | npm run build |
| 3 | `fix(plugin): clear processedMessageIds after compact` | hooks.ts | npm run build |
| 4 | `feat(plugin): extend listRecent with project_path filter` | client.ts | npm run build |
| 5-6 | `feat(plugin): add buildMemoryInjection + chatMessageRecallHook` | hooks.ts | npm run build |
| 7 | `feat(plugin): optimize keyword detection list` | keywords.ts | npm run build |
| 8 | `refactor(plugin): migrate recall from system.transform to chat.message` | hooks.ts, index.ts | npm run build |
| 9 | `refactor(server): change profile inject format to markdown` | injection.rs, memory.rs | cargo build |

---

## Success Criteria

### Verification Commands
```bash
cd plugins/opencode && npm run build    # Expected: no errors
cd omem-server && cargo build            # Expected: no errors
cd omem-server && cargo test             # Expected: all pass
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] Plugin builds clean
- [ ] Server builds clean
- [ ] POC verified synthetic:true works
- [ ] Web server stops on window close
- [ ] Compact 后记忆能保存

---

## Plan B（POC 失败时激活）

> 如果 Task 1 验证 `synthetic:true` 不可行（LLM 看不到 synthetic parts），则整个 `parts.unshift` 方案无法执行。
> 此 Plan B 保留 `system.transform` 注入路径，但复用精简的 `buildMemoryInjection()` 逻辑。

### Plan B 改动范围

| 原 Task | Plan B 改动 | 说明 |
|---------|------------|------|
| Task 1 | 跳过 | POC 失败，不做 |
| Task 2-3 | 不变 | Bug 修复与注入路径无关 |
| Task 4 | 不变 | client.ts 扩展复用 |
| Task 5 | 不变 | buildMemoryInjection() 逻辑完全复用 |
| **Task 6** | **替换为 systemTransformRecallHook** | 在 `system.transform` hook 中调用 buildMemoryInjection()，替代 chat.message |
| Task 7 | 不变 | keywords.ts 优化复用 |
| **Task 8** | **简化** | 只需修改 system.transform handler 内容，不删旧注册方式 |
| Task 9 | 不变 | 服务端格式修改复用 |

### Plan B Task 6 替换代码

```typescript
// 不新增 chatMessageRecallHook，改为在 system.transform 中调用 buildMemoryInjection
export function systemTransformRecallHook(
  client: CerebroClient,
  containerTags: string[],
  config: Partial<OmemPluginConfig> = {},
  directory?: string,
) {
  return async (
    input: { sessionID: string },
    output: { system: string[] },
  ) => {
    if (!input.sessionID) return;
    if (profileInjectedSessions.has(input.sessionID)) return;

    const query = ""; // system.transform 没有用户消息，使用空 query（只获取 profile + 项目记忆）
    const injection = await buildMemoryInjection(client, directory, query, config);

    const hasContent = (injection.profileCount ?? 0) > 0
      || (injection.projectMemoryCount ?? 0) > 0;

    if (injection.text && hasContent) {
      output.system[output.system.length - 1] += "\n\n" + injection.text;
      profileInjectedSessions.set(input.sessionID, 1);  // Map<string,number>，用 .set() 非 .add()
    }
  };
}
```

### Plan B 劣势
- 注入内容在 system prompt 末尾，LLM 注意力可能不如 parts.unshift
- 但 system.transform 是已验证的路径，零风险

### Plan B 成本
- 只需改 Task 6（~30行新代码）和 Task 8（简化），其余 Task 完全复用
- 预计额外 30 分钟
