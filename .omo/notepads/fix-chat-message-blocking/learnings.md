## Learnings

### 2026-05-21 Task: T0-verification
- src/index.ts 落后于 dist/index.js：缺少 memoryInjectionHook 导入、injectionStrategy 策略分支、soulWhisperSystemHook
- src/hooks.ts 中 memoryInjectionHook 存在于 L618，但未被 index.ts 导入
- dist/index.js (v1.15.4) 是生产运行版本，有完整的 injectionStrategy 分支逻辑
- src/config.ts 中没有 injectionStrategy 配置项，需要添加
- 插件发布到 npm @mingxy/cerebro v1.15.4

### Hook 注册结构（dist 版本）
- `chat.message`: strategy==="parts" ? keywordDetectionHook + memoryInjectionHook : keywordDetectionHook only
- `system.transform`: strategy==="parts" ? soulWhisperSystemHook : autoRecallHook + soulWhisperSystemHook
- `injectionStrategy` 配置默认值 "parts"，可选 "parts" | "system"

### 阻塞根因（修正后）
- **生产版本**（dist v1.15.4）：chat.message 注册 keywordDetectionHook + memoryInjectionHook
- memoryInjectionHook 在 chat.message 中 await client.getProfile() (~1-2s) + await client.shouldRecall() (~3-6s)
- 日志证实：01:52:28 start → 01:52:39 result = 11秒延迟
- **chat.message hook 是阻塞的**——OpenCode 等它返回后才执行 sessions.updateMessage()（保存用户消息到 DB → UI 渲染）

### OpenCode Hook 执行模型（玄机验证）
- Plugin.trigger 是 yield* Effect.promise() —— 顺序阻塞
- chat.message trigger 在 createUserMessage() 中，位于 sessions.updateMessage() 之前
- system.transform trigger 在 runLoop() 中，位于 LLM 调用之前
- 执行顺序：chat.message → 用户消息渲染 → system.transform → LLM
- **关键**：chat.message 阻塞用户消息保存/渲染；system.transform 阻塞 LLM 响应开始

### 源码与 dist 差异
- src/index.ts: 只有 keywordDetectionHook 在 chat.message（无网络调用）
- dist/index.js: 有 injectionStrategy 策略分支，parts 模式下 chat.message 包含 memoryInjectionHook（有网络调用）
- 计划的前提基于生产版本（dist），是正确的

### 2026-05-21 师尊实测验证
- 师尊重启 OpenCode 后实测 v1.15.5："确实不卡了"
- cerebro-profile 画像注入正常（可见完整 profile 数据）
- 异步缓存方案验证通过：chat.message 毫秒级返回，记忆注入延迟一轮可接受

### 2026-05-21 Task: 两阶段异步缓存重构

**核心改动：memoryInjectionHook 从同步阻塞改为 per-session 异步缓存**

1. **新增 `recallCache` (Map)** — 存储 `{ profileBlock, recallResult: ShouldRecallResponse, profileData, timestamp }`，在 compactingHook 三个清理点同步清除

2. **Phase A（同步路径）**：检查 `recallCache.get(sessionId)` → 有缓存立即构建 parts 注入 → 零 await → 用户消息不阻塞

3. **Phase B（异步路径）**：`Promise.all([getProfile, shouldRecall])` fire-and-forget → `.then()` 写入 recallCache 供下一轮消费 → 更新 `injectedMemoryIds` → `should_recall === true` 才调 `createRecallEvent`

4. **提取 `buildProfileBlock()`** — 从 profile 构建注入文本块，两个路径共用

5. **权衡**：session 第一条消息无缓存（cache miss），不注入记忆但也不阻塞；第二条消息起消费上一轮异步预取结果

6. **类型安全**：recallCache 的 `recallResult` 类型从 `any` 改为 `ShouldRecallResponse`，消除 implicit any 错误

7. **噪音减少**：`should_recall === false` 时不再调用 `createRecallEvent`，减少 web 端无意义记录
