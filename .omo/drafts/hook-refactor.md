# Draft: Cerebro 插件 Hook 重构方案

> 2026-05-20 · 月儿整理

## 重构目标

把记忆注入从 `system.transform` 改回 `chat.message`（参考 opencode-mem），灵魂低语留在 `system.transform`（跟 DCP 一致）。

## 当前问题

- `system.transform` 的 `output.system` 是字符串数组，DCP 先执行把 system 变成2个元素
- 智谱 GLM API 可能只处理 system[0]，丢弃 system[1]
- v1.14.1~v1.14.6 反复打补丁，append 到 system[0]、early return 前加 profile 注入，但治标不治本

## 重构方案

### 功能分工

| 功能 | Hook | 注入方式 | 参考 |
|------|------|---------|------|
| 记忆注入（profile + context + fetch-policy） | `chat.message` | `parts.unshift` + `synthetic: true` | opencode-mem |
| 灵魂低语（soulWhisper） | `system.transform` | `system[0] +=` | DCP |
| 提醒get（KEYWORD_NUDGE） | `system.transform` | `system[0] +=` | DCP |
| 压缩时记忆恢复（compactingHook） | `session.compacting` | `output.context` | 不变 |

### Part 结构（参考 opencode-mem）

```typescript
const contextPart: Part = {
  id: `prt-cerebro-${Date.now()}`,
  sessionID: input.sessionID,
  messageID: output.message.id,
  type: "text",
  text: memoryBlock,          // <cerebro-system-reminder> + markdown
  synthetic: true,            // 让首次检测跳过这个 part
} as any;
output.parts.unshift(contextPart);
```

### 注入格式

```
<cerebro-system-reminder>

## 👤 用户画像
- 偏好1
- 偏好2

## 🧠 记忆上下文
### 📋 主题簇（聚合记忆）
#### 簇标题 (整合自N条记忆) ★★★
> 簇摘要
**核心要点：**
- ● [id:xxx] 内容

### 📌 补充信息
- [id:xxx] 内容

## 📌 提醒
记忆ID可通过 memory_get("id") 获取完整内容
</cerebro-system-reminder>
```

## DCP nudgeForce=strong 调研结果

### 核心机制

一行关键代码：
```typescript
const targetRole = config.compress.nudgeForce === "strong" ? "user" : "assistant"
```

- `soft`（默认）：催促注入到 assistant 消息，权威性低
- `strong`：催促注入到 **user 消息**，LLM 把它当人类指令（RLHF 训练结果）

### 6大生效机制

1. **伪装成环境标签** — `<system-reminder>` 标签，系统提示说"不要输出"，制造权威感
2. **系统提示铺垫** — 先在 system prompt 植入"保持高质量上下文是你的责任"，建立义务感
3. **Strong 模式利用 user 权威** — 注入到 user 消息 = 人类命令
4. **具体可操作** — 告诉 LLM 哪些 (bN) 压缩块存在、哪些消息高优先级
5. **升级层次** — Turn(建议) → Iteration(指令) → Context Limit(强制 MUST)
6. **干净反馈环** — LLM 执行 compress 后 anchor 立即清除，正强化

### 对灵魂低语的启示

灵魂低语要成功：
1. 用 `system.transform` 注入到 `system[0]`
2. 内容用 `<system-reminder>` 标签包裹
3. 文案具体可操作——告诉 LLM 当前有哪些工具可用
4. 语气可升级

## opencode-mem 注入方式调研结果

### Hook 注册

```typescript
"chat.message": async (input, output) => {
  // input: { sessionID, agent?, model?, messageID?, variant? }
  // output: { message: UserMessage, parts: Part[] }
}
```

### injectOn="first" 策略

- 检查 session 中是否有非 synthetic 的 user 消息
- `synthetic: true` 的 part 对首次检测**不可见**
- 压缩后也会重新注入一次

### 我们需要调整的点

opencode-mem 用 `injectOn="first"`（只注入一次），但我们的 cerebro 需要持续更新：
- 方案A：改成 `injectOn="always"`（每次都注入，但 dedup 避免重复）
- 方案B：保持 `injectOn="first"` 但通过 keywordDetection 机制在关键词触发时重新注入
- 方案C：混合策略——profile 用 first，context 用 always

### 状态管理

两个 hook 之间需要共享的状态：
- `sessionMessages` — 消息历史（keywordDetectionHook 已经在用）
- `injectedMemoryIds` — 已注入的记忆ID（dedup）
- `profileInjectedSessions` — profile TTL 管理
- `firstMessages` — 首条消息缓存
- `keywordDetectedSessions` — 关键词检测触发

这些状态都在模块级 Map 中，两个 hook 都能访问。

## 三方评审结论

### 玄机评审（GO with conditions）

**结论**：方向正确，但有3个必须解决的技术风险：
1. hook 执行顺序不确定——需确认 `chat.message` 是否在 `system.transform` 之前执行
2. synthetic part 压缩后可能被清除——需确认 compacting 后 synthetic part 是否保留
3. injectOn="first" 不适用动态记忆——我们不是 strict first-only，需要 keyword 触发

### 灵犀灵感（方案 C+ 推荐）

**推荐**：profile first+TTL, context always+dedup, soul whisper always
- Profile：首条注入 + 30min TTL 过期后可重注入
- Context：每次都注入但严格 dedup（比 supermemory 更好）
- 灵魂低语：always 注入到 system[0]
- 让后端 shouldRecall 做智能门控（空/无关结果不注入）

**5个暗坑**：执行顺序、synthetic 清除、TTL+always 冲突、标签清理、向后兼容

### Supermemory 调研（权威参考）

**核心发现**：
- **strict first-only**：只在首条消息注入一次，之后不更新
- **无 dedup**：不做任何去重，靠 injectedSessions Set 保证每 session 一次
- **无动态更新**：依赖 LLM 通过 tool search 主动查询
- **compacting 3阶段**：监测→注入prompt→摘要后恢复（540行完整实现）
- **Part 结构**：`{ id, sessionID, messageID, type: "text", text, synthetic: true }`
- **[SUPERMEMORY] 块格式**：Profile + Recent Context + Project Knowledge + Relevant Memories

### 最终方案决定

| 维度 | supermemory | 我们的方案 | 理由 |
|------|------------|-----------|------|
| injectOn | strict first-only | **first + keyword 触发** | 比supermemory更灵活 |
| Profile | first-only | **first + TTL 30min** | 长session需要刷新 |
| dedup | 无 | **保留 injectedMemoryIds** | 比supermemory更优 |
| 动态更新 | 无 | **keyword触发 + shouldRecall门控** | 已有机制更好 |
| compacting | 3阶段完整流程 | **适配新格式** | 不需要重写 |

### 灵魂低语文案方向（参考DCP三重结构）

1. **身份绑定**：告诉LLM "你是配置了Cerebro的AI助手"
2. **义务感**："保持高质量上下文是你的责任"
3. **具体操作**："记忆ID可通过 memory_get('id') 获取完整内容"

## CompactingHook 升级计划（移植 supermemory 三阶段策略）

### 现状
当前 compactingHook 只在 `session.compacting` 时往 `output.context` 注入记忆，功能单一。

### 升级目标：移植 supermemory 三阶段

**阶段 1：监测触发**（可能不需要，OpenCode 已有 `session.compacting` hook）
- OpenCode 自己触发 compacting，我们被动接收
- 但可以增加"摘要保存为记忆"的后处理

**阶段 2：增强 compaction prompt**（核心升级）
当前：只注入记忆块
升级：注入 6 段式 compaction prompt，让 LLM 摘要时保留关键信息：
```
1. User Requests（用户原始请求）
2. Final Goal（最终目标）
3. Work Completed（已完成的工作）
4. Remaining Tasks（剩余任务）
5. MUST NOT Do（关键约束）
6. Project Knowledge（从 Cerebro 实时拉取记忆补充）
```

**阶段 3：摘要后处理**（新增）
- 将会话摘要保存为 Cerebro 记忆（带 `[Session Summary]` 前缀）
- 不需要 supermemory 的"自动发 Continue"（OpenCode 自己处理恢复）

### 与 supermemory 的关键差异
- supermemory 主动监测+触发 compacting，我们被动接收 `session.compacting` 事件
- supermemory 保存到自己的 API，我们保存到 Cerebro 服务端
- supermemory 用 containerTag 做 namespace，我们用 project_path 做 scope

## 灵犀+玄机方案验证结果（关键结论）

### 技术验证（三方一致确认）
1. **Part.synthetic 官方支持** — SDK `TextPart` 有 `synthetic?: boolean`，不需要 `as any`
2. **GLM API 兼容** — parts 作为 user message 发送，不受 system[0] 截断影响
3. **Hook 执行顺序** — chat.message 先于 system.transform 执行，不冲突

### 双轨注入架构
- **chat.message**: keywordDetectionHook（已有）+ memoryInjectionHook（新，替代 autoRecallHook）
- **system.transform**: 仅灵魂低语（精简，<200字）

### 降级策略
- 配置开关 `config.injectionStrategy = "parts" | "system"`，默认 parts
- 如果 GLM 不识别 synthetic parts，切换到 system[0] 全量合并模式

### CompactingHook 升级要点
- 6段式 compaction prompt（用户请求/最终目标/已完成/未完成/禁止事项/项目知识）
- 摘要保存为 Cerebro 记忆（[Session Summary] 前缀）
- 多轮 compacting 去重：摘要内容 hash + sessionID 组合，30秒冷却期
- injectedMemoryIds 在 compacting 后保留（不 clear），避免重复注入

## 风险点（已评审，有应对策略）

1. ~~injectOn 策略选择~~ → **已决定：first + keyword 触发**
2. parts.unshift 内容量 → 灵犀建议 shouldRecall 门控（空/无关不注入）
3. ~~hook 执行顺序~~ → 玄机指出需确认，方案中需包含POC验证步骤
4. compactingHook → 需同步适配新格式
5. synthetic part 压缩后行为 → 方案中需包含POC验证步骤

## Scope 边界

### IN（本轮必做）
- opencode 插件 hook 重构：autoRecallHook 从 system.transform 迁移到 chat.message
- 灵魂低语 + 提醒get 保留在 system.transform
- compactingHook 升级：增强 compaction prompt + 摘要保存
- injectOn first + keyword 触发策略
- dedup injectedMemoryIds
- 2个 POC 验证（Part 类型 + GLM 兼容性）
- fallback 降级策略

### OUT（本轮不做）
- 后端 shouldRecall API 改动
- openclaw/mcp 插件同步迁移
- 新增 REST API 端点
- 语义触发（只保持当前 keyword 列表）
- config.ts 大规模 schema 变更（只加 injectStrategy 字段）
- compactingHook 的 poll 机制重构（保留现有 poll，只在回调后增强）

### 测试策略
- POC 验证优先：先验证 Part 类型 + GLM 兼容性，通过后再迁移
- 重构后 agent QA 回归（手动测试 + Playwright）
- 无 TDD（插件无测试框架，不做测试基础设施搭建）

## 代码位置

- 插件源码：`plugins/opencode/src/`
- hooks.ts：autoRecallHook, keywordDetectionHook, compactingHook
- index.ts：hook 注册 + soulWhisper + wrappedRecallHook
- 参考实现 opencode-mem：`/mnt/d/dev/github/project/opencode-mem/src/`
- 参考 DCP：`/mnt/d/dev/github/project/opencode-dcp/`

## npm 信息

- 包名：`@mingxy/cerebro`
- 当前版本：v1.14.6
- 认证用户：mingxy
- token 已更新到 ~/.npmrc
