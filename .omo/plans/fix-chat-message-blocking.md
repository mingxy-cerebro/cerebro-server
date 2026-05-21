# 修复 chat.message 阻塞 — 用户输入延迟 5~8 秒

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**目标：** 消除 `memoryInjectionHook` 在 `chat.message` hook 中同步执行 `getProfile` + `shouldRecall` 导致的 5~8 秒用户输入阻塞。

**架构：** 引入 per-session 异步缓存——`chat.message` 入口立即读取缓存注入 `output.parts`（同步，毫秒级返回），然后后台发起 API 请求，结果写入缓存供下一轮消费。用户消息秒发，记忆注入延迟一轮（第二轮消息开始就有缓存了）。

**技术栈：** TypeScript，OpenCode Plugin SDK，不引入新依赖

---

## 背景

### 根因
`memoryInjectionHook` 运行在 `chat.message`（index.ts L147-152）。这个 hook 是**阻塞的**——OpenCode 等它返回后才渲染用户消息。hook 内有两个串行 HTTP 请求：
1. `client.getProfile()` — GET /v1/profile（~1-2秒）
2. `client.shouldRecall(...)` — POST /v1/should-recall 含 LLM 推理（~3-6秒）

hook-refactor（v1.15.0）之前，recall 走 `system.transform`，它在用户看到消息之后执行，延迟不可感知。现在改成 `chat.message` 就阻塞了。

### 附加问题：web 端每条消息都有召回记录
移除客户端门控后，每条消息都调 `shouldRecall`，导致 web 端看到大量召回记录（即使最终 `should_recall: false`）。客户端需要在 `shouldRecall` 返回不需要召回时，不调 `createRecallEvent`（或只调一次标记为 skipped）。

### 为什么不回退 system.transform？
hook-refactor 有正当理由（GLM system[0] 截断问题、parts.unshift + synthetic:true 方案）。保留 `chat.message` 注入路径，但让它不阻塞。

### 设计决策
- **异步缓存 + 一轮延迟**：后台发 API 请求，注入*上一轮*的缓存结果。第一条消息无注入（可接受——profile 有 30 分钟 TTL），第二条消息起秒级注入。
- **Profile 缓存与 recall 缓存分离**：Profile 已有 30 分钟 TTL（`profileInjectedSessions`），recall 缓存是新的。
- **后台 fetch 在 `chat.message` 入口立即触发**，但 `output.parts` 同步从缓存填充。
- **减少无意义召回记录**：`shouldRecall` 返回 `should_recall: false` 时不调 `createRecallEvent`，减少 web 端噪音。

---

## TODOs

- [x] **T1: 给 memoryInjectionHook 添加 per-session recall 结果缓存 + 异步获取逻辑**
  - 在模块级新增缓存结构：`Map<string, { profileBlock: string, recallResult: ShouldRecallResponse | null, timestamp: number }>`
  - 重构 `memoryInjectionHook`：拆分为「同步缓存读取 + 注入」和「异步后台获取 + 缓存写入」
  - 同步路径（不 await）：
    1. 读缓存 → 如果命中且有内容，立即构建 parts 注入 `output.parts`
    2. 如果缓存未命中（session 第一条消息），只注入 profile（如果有 TTL 缓存）或跳过
    3. **立即 return**，不 await 任何东西
  - 异步路径（fire-and-forget，不 await）：
    1. `Promise.all([client.getProfile(), client.shouldRecall(...)])` 并行请求
    2. 结果写入缓存 Map
    3. 如果 `shouldRecall` 返回 `should_recall: false`，不调 `createRecallEvent`
    4. 如果有新记忆，调 `createRecallEvent`
  - `compactingHook` 清理时同步清除缓存 Map 中对应 session
  - 保留 `injectedMemoryIds` 去重逻辑（从缓存中读取已注入 ID）

- [x] **T2: 验证编译 + 无回归**
  - `npx tsc` 零错误
  - 确认 `chat.message` hook 不 await 任何网络调用
  - 确认 recall 仍然生效（第一条无注入，第二条有缓存注入）

---

## Final Verification Wave

- [x] **F1: 编译 + 内容验证** — `quick`
  - `npx tsc` 零错误
  - Grep 验证：memoryInjectionHook 同步路径中无 `await client.shouldRecall`、无 `await client.getProfile`
  - Grep 验证：存在 fire-and-forget 异步模式
  - Grep 验证：模块级缓存 Map 存在
  - Grep 验证：`should_recall: false` 时不调 `createRecallEvent`

- [x] **F2: 玄机代码评审** — `oracle`
  - 评审：recall 准确性无回归（一轮延迟可接受）
  - 评审：无内存泄漏（compactingHook 清理缓存）
  - 评审：缓存 Map 线程安全（单线程 JS，无需担心）
  - 评审：web 端召回记录噪音减少
