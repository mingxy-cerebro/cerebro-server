# Fix: 子Agent绕过readonly策略产生记忆

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复三个Bug使子Agent（Metis/Momus/Oracle等）严格遵循readonly策略，不再产出碎片记忆。

**Architecture:** OpenCode plugin在同一个Node.js进程中为所有session（主窗口+子agent）共享同一个plugin实例。当前实现有三个缺陷：(1) `currentSessionId`闭包变量被子agent覆盖导致主窗口gate失效；(2) `agentId`是进程级常量，子agent真实身份不可见；(3) config加载错误被静默吞掉导致fallback到"readwrite"。

**Scope:** 仅修改 `plugins/opencode/src/` 下3个文件：`index.ts`、`hooks.ts`、`config.ts`。不涉及Rust后端或其他plugin。

## Files Changed

| File | Responsibility | Change Type |
|------|---------------|-------------|
| `plugins/opencode/src/index.ts` | Plugin入口，hook注册 | 修改: 区分主窗口/子agent session，传递真实agentId |
| `plugins/opencode/src/hooks.ts` | sessionIdleHook + policy gate | 修改: 从session metadata提取真实agentId，增强日志 |
| `plugins/opencode/src/config.ts` | 配置加载 + policy解析 | 修改: config加载错误不再静默 |

## Bug Overview

### Bug #1: `currentSessionId` 竞态覆盖
- `index.ts` L118: `let currentSessionId` 是闭包变量，所有session共享
- L131: `if (input.sessionID) currentSessionId = input.sessionID;` — 子agent的transform hook覆盖主窗口值
- L137: `sessionIdleHook(... () => currentSessionId ...)` — gate依赖这个被污染的值

### Bug #2: 子Agent真实身份不可见
- `index.ts` L116: `const agentId = process.env.OMEM_AGENT_ID || "opencode";` — 进程级常量
- `hooks.ts` L608: `resolveAgentPolicy(agentId || "", config)` — 永远看到"opencode"
- 子agent（Metis/Momus等）的session事件中无法获取真实agent名

### Bug #3: config.json加载错误被静默吞掉
- `config.ts` L160-162: catch块完全静默
- `resolveAgentPolicy` fallback链: `agentMemoryPolicy[name] ?? defaultPolicy ?? "readwrite"`
- 如果config加载失败，defaultPolicy=undefined → fallback到"readwrite" → readonly被绕过

---

## Tasks

### Task 1: Fix config.ts — config加载错误日志 + 默认策略保护
**File:** `plugins/opencode/src/config.ts`

- [ ] Step 1: 在 `loadPluginConfig()` 的catch块中添加 `logWarn` 日志
  - 位置: L160-162
  - 将空catch改为:
    ```typescript
    } catch (err) {
      // Config file doesn't exist or is invalid, use defaults
      const { logInfo } = await import("./logger.js");
      logInfo("cerebro config file not loaded, using defaults", { error: String(err) });
    }
  ```
  - 注意：由于logger.js是同步导入已有（config.ts顶部的import），直接用import的logInfo即可，无需dynamic import。先检查config.ts是否已import logger。
  - **实际上config.ts没有import logger**。所以用 `console.warn("[cerebro] config file not loaded, using defaults:", err instanceof Error ? err.message : String(err));` 更安全，避免循环依赖。

- [ ] Step 2: 修改 `resolveAgentPolicy` 添加defaultPolicy保护
  - 位置: L200-205
  - 当前: `return config.agentMemoryPolicy?.[agentName] ?? config.defaultPolicy ?? "readwrite";`
  - 改为: 在fallback到"readwrite"前打日志，但不改默认行为（这是最安全的fallback）:
    ```typescript
    export function resolveAgentPolicy(
      agentName: string,
      config: Partial<OmemPluginConfig>,
    ): AgentPolicy {
      const explicit = config.agentMemoryPolicy?.[agentName];
      if (explicit) return explicit;
      if (config.defaultPolicy) return config.defaultPolicy;
      console.warn("[cerebro] no policy configured for agent:", agentName, "— defaulting to readwrite");
      return "readwrite";
    }
    ```
  - 这保证了config加载失败时有可见日志，但不改变现有默认行为

- [ ] Step 3: 运行 `cargo test -p omem-server` 确认没有破坏现有测试（config变更只影响TS plugin）
  - 等等，这是TypeScript插件，不是Rust。应该运行: `cd plugins/opencode && npm run build`
  - 确认build通过

### Task 2: Fix index.ts — 区分主窗口/子agent session
**File:** `plugins/opencode/src/index.ts`

- [ ] Step 4: 将 `currentSessionId` 改为 `mainSessionId`，只在首次设置时赋值
  - 位置: L118
  - 将 `let currentSessionId: string | undefined;` 改为 `let mainSessionId: string | undefined;`
  - 这样语义更清晰：这是"主窗口的session ID"，不是"当前session ID"

- [ ] Step 5: 修改 transform hook，只在mainSessionId未设置时赋值（锁定首个session为主窗口）
  - 位置: L130-132
  - 当前:
    ```typescript
    "experimental.chat.system.transform": async (input: any, output: any) => {
      if (input.sessionID) currentSessionId = input.sessionID;
      return recallHook(input, output);
    },
    ```
  - 改为:
    ```typescript
    "experimental.chat.system.transform": async (input: any, output: any) => {
      if (input.sessionID && !mainSessionId) mainSessionId = input.sessionID;
      return recallHook(input, output);
    },
    ```
  - **关键设计决策**: 首个触发transform的session就是主窗口。OpenCode启动时主窗口先于子agent，所以这是可靠的。

- [ ] Step 6: 更新所有引用 `currentSessionId` 的地方改为 `mainSessionId`
  - L135: `compactingHook(... () => currentSessionId ...)` → `() => mainSessionId`
  - L136: `buildTools(... getSessionId: () => currentSessionId ...)` → `() => mainSessionId`
  - L137: `sessionIdleHook(... () => currentSessionId ...)` → `() => mainSessionId`

- [ ] Step 7: 在index.ts中添加一个per-session agentId映射表
  - 位置: L116-118区域，在agentId定义之后
  - 添加:
    ```typescript
    // Map sessionId → agentId for sub-agents
    const sessionAgentMap = new Map<string, string>();
    ```
  - 在transform hook中尝试从input提取agent信息:
    ```typescript
    "experimental.chat.system.transform": async (input: any, output: any) => {
      if (input.sessionID) {
        if (!mainSessionId) mainSessionId = input.sessionID;
        // Extract agent info from session if available
        const agentName = input.agentID || input.agentName || input.properties?.agentID;
        if (agentName && agentName !== agentId) {
          sessionAgentMap.set(input.sessionID, agentName);
        }
      }
      return recallHook(input, output);
    },
    ```
  - **注意**: 需要调研OpenCode SDK的 `chat.system.transform` input对象是否包含agentID字段。如果不包含，这个方案需要调整。

- [ ] Step 8: 构建验证: `cd plugins/opencode && npm run build`

### Task 3: Fix hooks.ts — sessionIdleHook提取真实agentId
**File:** `plugins/opencode/src/hooks.ts`

- [ ] Step 9: 修改 `sessionIdleHook` 函数签名，增加 `getSessionAgentId` 参数
  - 位置: L535-546
  - 添加新参数:
    ```typescript
    export function sessionIdleHook(
      cerebroClient: CerebroClient,
      _containerTags: string[],
      tui: any,
      sdkClient: any,
      _ingestMode: "smart" | "raw" = "smart",
      threshold: number = 0,
      getMainSessionId?: () => string | undefined,
      isAutoStoreEnabled?: (sessionId: string | undefined) => boolean,
      agentId?: string,
      config: Partial<OmemPluginConfig> = {},
      getSessionAgentId?: (sessionId: string) => string | undefined,  // NEW
    ) {
    ```

- [ ] Step 10: 在sessionIdleHook内部，policy gate之前提取真实agentId
  - 位置: L607-612（policy gate区域）
  - 修改:
    ```typescript
    // Resolve effective agentId: prefer session-specific mapping over process-level default
    const effectiveAgentId = (getSessionAgentId?.(sessionID)) || agentId || "opencode";
    const policy = resolveAgentPolicy(effectiveAgentId, config);
    if (policy !== "readwrite") {
      logInfo("sessionIdleHook blocked by policy", { agentId: effectiveAgentId, policy, sessionId: sessionID });
      return;
    }
    ```
  - 同时把sessionIngest调用中的agentId改为effectiveAgentId:
    ```typescript
    await cerebroClient.sessionIngest(conversationMessages, sessionID, effectiveAgentId, sessionTitle, projectName);
    ```

- [ ] Step 11: 更新index.ts中sessionIdleHook调用，传入getSessionAgentId
  - 位置: L137
  - 改为:
    ```typescript
    event: sessionIdleHook(
      cerebroClient, containerTags, tui, client,
      config.ingest.ingestMode, config.ingest.autoCaptureThreshold,
      () => mainSessionId, isAutoStoreEnabled, agentId, config,
      (sid: string) => sessionAgentMap.get(sid),  // getSessionAgentId
    ),
    ```

- [ ] Step 12: 构建验证: `cd plugins/opencode && npm run build`

### Task 4: 防御性增强 — 即使agentId不可提取也要靠mainSessionId gate挡住
**File:** `plugins/opencode/src/hooks.ts`

- [ ] Step 13: 加强getMainSessionId gate逻辑
  - 位置: L558-561
  - 当前逻辑: `if (mainId && sessionID !== mainId) return;`
  - 这其实已经能挡住子agent（如果mainSessionId没被覆盖的话）
  - Bug #1的修复（Task 2）已经确保mainSessionId不被覆盖
  - 但为安全起见，增加日志:
    ```typescript
    if (getMainSessionId) {
      const mainId = getMainSessionId();
      if (mainId && sessionID !== mainId) {
        logInfo("sessionIdleHook skipped: non-main session", { sessionId: sessionID, mainSessionId: mainId });
        return;
      }
    }
    ```

- [ ] Step 14: 最终构建 + 手动验证
  - `cd plugins/opencode && npm run build`
  - 确认无TypeScript错误

---

## Verification

- [ ] Step 15: 验证所有改动
  - `cd plugins/opencode && npm run build` 通过
  - 检查 `resolveAgentPolicy("metis", { agentMemoryPolicy: { metis: "readonly" }, defaultPolicy: "readonly" })` 返回 "readonly"
  - 检查 `resolveAgentPolicy("opencode", { agentMemoryPolicy: {}, defaultPolicy: "readonly" })` 返回 "readonly"
  - 确认 `mainSessionId` 不会被后续session覆盖
  - 确认config加载失败时有console.warn输出

## Key Decisions

1. **首个session = 主窗口**: OpenCode启动时主窗口transform先于子agent触发，`!mainSessionId` gate可锁定首个session。这是最简单可靠的方案。
2. **sessionAgentMap fallback**: 如果OpenCode SDK不传递agentID字段，sessionAgentMap可能为空。此时仍依赖Bug#1的修复（mainSessionId不被覆盖）+ getMainSessionId gate来挡住子agent。
3. **不改默认fallback行为**: `resolveAgentPolicy` 最终仍fallback到"readwrite"，但加warn日志。改默认行为可能影响已有用户。
4. **不增加外部依赖**: 仅用console.warn避免循环依赖问题。

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| 首个session可能不是主窗口 | 极低概率。OpenCode启动流程保证主窗口先于子agent。 |
| OpenCode SDK不传递agentID字段 | 有fallback: mainSessionId gate已足够挡住子agent。agentId提取是增强功能。 |
| sessionAgentMap内存泄漏 | 低风险。plugin生命周期=进程生命周期，Map大小=活跃session数。 |
