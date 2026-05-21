# Phase 7: Hooks 重构 — 消除卡顿 + V2 Inject API

**状态**: 评审通过，待实施 🔄
**批准时间**: 2026-05-21
**评审**: 灵犀(Metis) + 玄机(Oracle) 已通过

## 根因
`injectionStrategy="parts"` 把 `memoryInjectionHook`（await shouldRecall + getProfile）放在 `chat.message` hook，阻塞 UI。
v1.8.1 把重操作放在 `system.transform`，只延迟 LLM 响应不卡 UI。

## 目标
1. 重操作搬回 `system.transform`（和 v1.8.1 一样不卡）
2. 用 V2 profile inject API（`GET /v2/profile/inject`）直接返回可注入文本
3. 砍掉 `memoryInjectionHook`（chat.message 里的重操作）
4. 砍掉灵魂低语
5. 保留 shouldRecall 精准召回机制

## 核心设计决策

### Profile TTL 门控（师尊确认）
- Profile 注入有 **客户端 TTL 门控**（默认 5 分钟，可配置）
- TTL 内：**profile 不注入，context 照常注入**
- TTL 过期：重新调 `getInjection()` 获取最新 profile 并注入
- 服务端缓存 ≠ 客户端注入 TTL：服务端避免重复计算，客户端避免重复注入

### Web 端数据同步（师尊确认）
- **no-recall 路径删除 createEventAndReturn**：没召回就不产生事件
- 只有 `shouldRecall=true` 且有实际 context 注入时才创建 recall event
- Web 端严格对齐：有注入 → 有事件，无注入 → 无事件

### Toast（师尊确认）
- Toast 延迟时间已可在 config.json 配置（`ui.toastDelayMs`），不做额外改动
- 统一两个 `showToast` 函数为一个（删除 index.ts 的，保留 hooks.ts 的）

## 变更清单（5 个文件）

### 1. `client.ts` — 新增 getInjection()

```typescript
async getInjection(projectPath?: string): Promise<{
  content: string;           // "<cerebro-profile>\n  · slot — value\n</cerebro-profile>"
  preference_count: number;
  estimated_tokens: number;
} | null> {
  const params = projectPath ? `?project_path=${encodeURIComponent(projectPath)}` : "";
  return this.request("GET", `/v2/profile/inject${params}`);
}
```

- 调用 `GET /v2/profile/inject?project_path=xxx`
- 返回的 `content` 已是完整的 `<cerebro-profile>` XML 块
- 有服务端缓存（cache_key = tenant_id:project_path）
- **加 try-catch**：V2 API 失败时静默跳过 profile，不影响后续 shouldRecall

### 2. `hooks.ts` — 重构 autoRecallHook，删除冗余函数

#### 2a. 重构 autoRecallHook（L328）profile 注入部分

**替换 profile 获取逻辑**：
```typescript
// 旧：client.getProfile() + 手动构建 profileBlock（约 40 行）
// 新：带 TTL 门控的 getInjection()

const profileTtlMs = config.profile?.ttlMs ?? 300000; // 默认 5 分钟
const lastInjected = profileInjectedSessions.get(input.sessionID);
const profileTtlExpired = !lastInjected || (Date.now() - lastInjected > profileTtlMs);

let profileBlock = "";
let profileInjected = false;
let profileCountText = "";

if (profileTtlExpired) {
  try {
    const injection = await client.getInjection(directory || process.env.OMEM_PROJECT_DIR);
    if (injection?.content) {
      profileBlock = injection.content;
      profileCountText = `${injection.preference_count} preferences`;
      profileInjected = true;
      profileInjectedSessions.set(input.sessionID, Date.now());
    }
  } catch (e) {
    logErr("autoRecallHook getInjection failed, skipping profile", { error: String(e) });
    // profile 注入失败不阻塞 shouldRecall
  }
}
// shouldRecall 每条消息都调，不受 TTL 影响
```

#### 2b. 删除 no-recall 路径的 createEventAndReturn

**旧**（L494-503）：
```typescript
if (!shouldRecallRes.should_recall) {
  if (profileBlock) { appendToSystem(output.system, profileBlock); }
  if (profileInjected && isFirstInjection) {
    await createEventAndReturn(0, 0, 0); // ← 删除这行
    showToast(...);
  }
  return;
}
```

**新**：
```typescript
if (!shouldRecallRes.should_recall) {
  if (profileBlock) { appendToSystem(output.system, profileBlock); }
  // 无召回 → 不创建 event → Web 端看不到
  // 首次 profile 注入仍弹 toast
  if (profileInjected && !lastInjected) {
    showToast(tui, "👨 Profile Injected", `${profileCountText} · no memory recall needed`, "success", toastDelayMs);
  }
  return;
}
```

#### 2c. 删除的函数/变量

| 删除项 | 行号 | 原因 |
|--------|------|------|
| `buildProfileBlock()` | L626-647 | V2 API 已返回格式化内容 |
| `memoryInjectionHook()` | L649-975+ | 重操作不再放在 chat.message |
| `soulWhisperToolTracker()` | L1645-1677 | 灵魂低语整体删除 |
| `buildWhisperText()` | L1679-1693 | 灵魂低语整体删除 |
| `pendingToolCalls` Map | L1643 | 灵魂低语整体删除 |
| `recallCache` Map | L179-184 | 只给 memoryInjectionHook 用，删除后无用 |
| `injectedSessions` Set | L175 | 只给 memoryInjectionHook 用 |

#### 2d. 保留的函数/变量

| 保留项 | 原因 |
|--------|------|
| `profileInjectedSessions` Map | TTL 门控 + isFirstInjection 判断 |
| `injectedMemoryIds` Map | autoRecallHook 去重使用 |
| `sessionMessages` Map | autoRecallHook + keywordDetectionHook |
| `firstMessages` Map | autoRecallHook + keywordDetectionHook |
| `saveKeywordDetectedSessions` Set | 关键词检测 + KEYWORD_NUDGE |
| `showToast` | autoRecallHook 通知 |
| `appendToSystem` | 注入到 system prompt |
| `buildContextBlock` / `buildClusteredContextBlock` | context 构建 |

### 3. `index.ts` — 简化 hook 注册

#### 3a. 清理 import（L7）

**旧**：
```typescript
import { autoRecallHook, memoryInjectionHook, autocontinueHook, compactingHook, keywordDetectionHook, sessionIdleHook, soulWhisperToolTracker, pendingToolCalls, buildWhisperText, recallCache, profileInjectedSessions, buildProfileBlock } from "./hooks.js";
```

**新**：
```typescript
import { autoRecallHook, autocontinueHook, compactingHook, keywordDetectionHook, sessionIdleHook } from "./hooks.js";
```

#### 3b. 删除 index.ts 本地 showToast（L45-61）

删除 index.ts 中的 `showToast` 函数定义。启动连接 toast 改用 hooks.ts 导出的 `showToast`（需 export）。

#### 3c. 删除 soulWhisperSystemHook（L122-142）

整个闭包删除。

#### 3d. 删除 strategy 分支（L144-165）

**旧**：
```typescript
const strategy = config.injectionStrategy ?? "parts";
const chatMessageHook = strategy === "parts" ? ... : ...;
const systemTransformHook = strategy === "parts" ? ... : ...;
```

**新**：
```typescript
// system.transform：autoRecallHook（重操作，不卡 UI）
const recallHook = autoRecallHook(cerebroClient, containerTags, tui, config, () => cachedAgentName || agentId, directory);

// chat.message：keywordDetectionHook（轻量，不卡）
```

#### 3e. 删除 recallCache 预热逻辑（L182-201）

**整段删除**：
```typescript
// 删掉这段 ↓
if (sid) {
  const cached = recallCache.get(sid);
  if (!cached || !cached.profileBlock) {
    cerebroClient.getProfile().then(profile => {
      const built = buildProfileBlock(profile);
      ...
    }).catch(() => {});
  }
}
```

#### 3f. 删除 tool.execute.before 注册（L209）

```typescript
// 删掉 ↓
"tool.execute.before": (() => { const tracker = soulWhisperToolTracker(config); return tracker; })(),
```

#### 3g. 简化后的 hook 注册

```typescript
return {
  config: ...,
  "experimental.chat.system.transform": async (input: any, output: any) => {
    if (input.sessionID && !mainSessionLocked) {
      mainSessionId = input.sessionID;
      mainSessionLocked = true;
    }
    return recallHook(input, output);
  },
  "chat.message": keywordDetectionHook(cerebroClient, containerTags, config.ingest.autoCaptureThreshold, tui, config.ingest.ingestMode, config, agentId),
  "experimental.session.compacting": compactingHook(...),
  "experimental.compaction.autocontinue": autocontinueHook(...),
  tool: buildTools(...),
  event: sessionIdleHook(...),
  // tool.execute.before 删除
  "shell.env": ...,
};
```

### 4. `config.ts` — 配置清理 + 新增

#### 4a. 删除
- `injectionStrategy?: "parts" | "system"` — 不再需要双策略
- `soulWhisper` 整个接口和配置块 — 灵魂低语整体删除

#### 4b. 新增

```typescript
profile?: {
  ttlMs?: number;  // Profile 注入 TTL，默认 300000（5 分钟）
};
```

#### 4c. DEFAULTS 更新

```typescript
const DEFAULTS: OmemPluginConfig = {
  // ... 现有默认值
  profile: { ttlMs: 300000 },
  // 删除 injectionStrategy 和 soulWhisper
};
```

### 5. `schema.json` — 同步更新

#### 5a. 删除
- `soulWhisper` 整个配置节（enabled、tools、excludeTools、maxToolNames）
- `injectionStrategy` 字段

#### 5b. 新增
```json
"profile": {
  "type": "object",
  "description": "Profile 注入配置",
  "properties": {
    "ttlMs": {
      "type": "number",
      "description": "Profile 注入 TTL（毫秒），TTL 内跳过 profile 注入但 context 照常注入",
      "default": 300000
    }
  }
}
```

## compactingHook 清理

- 删除 `injectedSessions.delete(input.sessionID)` — Set 已删除
- 删除 `recallCache.delete(input.sessionID)` — Map 已删除
- 删除 `pendingToolCalls.delete(input.sessionID)` — Map 已删除
- 保留 `injectedMemoryIds.delete(input.sessionID)` — 去重 Map 仍需清理

## 不改的文件
- `tools.ts`：`memory_profile` 工具仍用 V1 `getProfile()`（展示完整 profile 给用户看）
- `keywords.ts`、`logger.ts`、`privacy.ts`、`tags.ts`、`tui.tsx`：不变
- Rust 服务端：V2 inject API 已实现，不需要改动

## 验证
1. `cd plugins/opencode && npm run build` — 编译通过（确认无引用已删除符号）
2. 重启 OpenCode → 新对话 → 不卡顿
3. 第一条消息：profile 注入 + shouldRecall 正常工作
4. 第二条消息（5 分钟内）：profile 不注入，context 照常注入
5. 5 分钟后：profile 重新注入
6. Web 端 session 页面：只有 shouldRecall=true 的消息才有 recall event，no-recall 不产生事件
7. 关键词检测正常（save/remember 触发）
8. compactingHook 触发后无 ReferenceError

## 风险
- V2 inject API 服务端缓存 TTL 可能需要调整
- 首次请求（cache miss）仍有延迟，但不卡 UI（在 system.transform 中）
- output.system 生命周期需确认：当前假设每次 system.transform 调用传入新的 output 对象（非跨消息累积）。如果累积则 profile 会重复叠加——需验证

## 关键背景
- **V2 inject API**：`GET /v2/profile/inject?project_path=xxx`
- **返回格式**：`{ content: string, preference_count: number, estimated_tokens: number }`
- **content 示例**：`<cerebro-profile>\n  · coding_style — TypeScript strict mode, no any\n  · language — Chinese\n</cerebro-profile>`
- **Rust handler**：`omem-server/src/api/handlers/profile_v2.rs` → `get_injection()`
- **Rust builder**：`omem-server/src/profile_v2/injection.rs` → `build_injection()`

## 评审记录
- **灵犀(Metis)**：发现 2 个 P0（编译报错 + TTL 缺失）、3 个 P1（残留清理）、2 个 P2
- **玄机(Oracle)**：发现 4 个 P1（isFirstInjection 断裂 + 数据同步 + 状态清理 + 缓存）、3 个 P2
- **师尊决策**：TTL 默认 5 分钟可配置 | no-recall 不产生 event | toast delay 可配置不动
