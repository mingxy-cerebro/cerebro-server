# Cerebro Plugin 记忆注入全流程

> 版本: v1.10.8 | 文件: `plugins/opencode/src/`

---

## 一、全局状态（模块级变量）

```
┌─────────────────────────────────────────────────────────────┐
│  hooks.ts 模块级状态（所有hook共享）                          │
├─────────────────────────────────────────────────────────────┤
│  keywordDetectedSessions: Set<sessionID>                    │
│    → 标记检测到记忆关键词的session（注入时追加KEYWORD_NUDGE） │
│                                                             │
│  injectedMemoryIds: Map<sessionID, Set<memoryID>>           │
│    → 增量去重：跟踪每个session已注入的记忆ID                  │
│                                                             │
│  firstMessages: Map<sessionID, string>                      │
│    → 记录每个session的第一条用户消息                          │
│                                                             │
│  sessionMessages: Map<sessionID, {role,content}[]>          │
│    → 消息累积缓冲区（keywordDetection写入，compacting消费）  │
│                                                             │
│  profileInjectedSessions: Set<sessionID>                    │
│    → 每session只注入一次Profile                              │
│                                                             │
│  processedMessageIds: Set<msgID>                            │
│    → sessionIdleHook防止重复处理已消费的消息                  │
│                                                             │
│  pluginStartTime: number                                    │
│    → 插件启动时间戳，跳过启动前的历史消息                     │
└─────────────────────────────────────────────────────────────┘
```

---

## 二、四条Hook链路总览

```
用户消息 → OpenCode SDK → 触发Hook链
                                    │
        ┌───────────────────────────┼─────────────────────────────┐
        ▼                           ▼                             ▼
 chat.message            chat.system.transform              session.idle
 (每条消息)              (每次LLM调用前)                    (session空闲)
        │                           │                             │
        ▼                           ▼                             ▼
 keywordDetectionHook       autoRecallHook                sessionIdleHook
        │                           │                             │
        │                           │                             │
        │                      ┌────┘                             │
        ▼                      ▼                                  │
 session.compacting                                              │
 (session压缩时)                                                 │
        │                                                        │
        ▼                                                        │
   compactingHook ───────────────────────────────────────────────┘
```

---

## 三、Hook ①: keywordDetectionHook — 消息收集

**触发时机**: `chat.message`（每条用户消息）
**作用**: 收集用户消息到内存缓冲区 + 检测记忆关键词

```
用户消息到达
    │
    ▼
[1] 提取文本内容（text parts拼接）
    │
    ▼
[2] 记录第一条消息 → firstMessages[sessionID] = text
    │
    ▼
[3] 关键词检测: detectKeyword(text)
    │                    │
    ├─ 命中 ────────→ keywordDetectedSessions.add(sessionID)
    │                  （autoRecallHook注入时会追加KEYWORD_NUDGE）
    │
    ▼
[4] Policy检查: resolveAgentPolicy(agentId, config)
    │
    ├─ "none" ──→ return（不收集消息）
    │
    ▼
[5] 消息入缓冲: sessionMessages[sessionID].push({role:"user", content:text})
    │
    ▼
[6] 消息数 ≥ threshold?
    │
    └─ 是 → 标记"待处理"（等session.idle时消费）
```

**关键点**:
- `policy="none"` 时不收集，`readonly`/`readwrite` 都收集
- 消息存在内存Map中，等 `compactingHook` 或 `sessionIdleHook` 消费

---

## 四、Hook ②: autoRecallHook — 记忆召回+注入（核心）

**触发时机**: `experimental.chat.system.transform`（每次LLM调用前，transform system prompt时）
**作用**: 召回相关记忆 + 注入到system prompt

```
LLM调用前触发
    │
    ▼
[1] Policy检查: resolveAgentPolicy(agentId, config)
    │
    ├─ "none" ──→ return（不召回）
    │
    ▼
[2] 提取查询: 最后一条用户消息 → extractUserRequest() → query_text
    │
    ▼
[3] 调用 shouldRecall API ──────────────────────→ POST /v1/should-recall
    │  参数: query_text, last_query_text, session_id,         │
    │        similarity_threshold(0.6),                        │
    │        max_results(10), project_tags                     │
    │  超时: 20秒                                              │
    │                                              │
    │ ◄────────────────────────────────────────────┘
    │ 返回: ShouldRecallResponse
    │   { should_recall, confidence, memories[], clustered? }
    │
    ▼
[4] API不可达? ──→ Toast "Service Unavailable" → return
    │
    ▼
[5] 注入Profile（每session仅一次）
    │
    ├─ GET /v1/profile → profile数据
    │
    ├─ profileInjectedSessions.has(sessionID)?
    │   ├─ 否 → output.system.push("<cerebro-profile>...")
    │   │        profileInjectedSessions.add(sessionID)
    │   │        profileInjected = true
    │   └─ 是 → 跳过
    │
    ▼
[6] should_recall === false?
    │
    ├─ 是 ──→ 仅Profile注入?
    │         ├─ 是 → Toast "👨 Profile Injected"
    │         └─ return
    │
    ▼
[7] 增量去重: results过滤掉 injectedMemoryIds[sessionID] 中已有的
    │
    ▼
[8] 全部重复? ──→ Toast "all memories already injected" → return
    │
    ▼
[9] 构建注入内容
    │
    ├─ 有clustered? ──→ buildClusteredContextBlock()
    │                    格式: <cerebro-context>
    │                    按主题簇组织记忆
    │
    └─ 普通模式 ──→ buildContextBlock(newResults, maxContentLength=500)
                      格式: <cerebro-context>
                      按category分组（Preferences/Knowledge/...）
                      每条记忆:
                        - (2h ago [tag1, tag2]) 记忆内容（截断到500字）
    │
    ▼
[10] output.system.push(contextBlock) ← 注入到system prompt
    │
    ▼
[11] 更新去重集合: injectedMemoryIds[sessionID] += newIds
    │
    ▼
[12] 记录召回: recordSessionRecall(sessionID, newIds, "auto", ...)
    │            ──────────────────────→ POST /v1/session-recalls
    │
    ▼
[13] 关键词追踪: keywordDetectedSessions.has(sessionID)?
    │
    ├─ 是 → output.system.push(KEYWORD_NUDGE)
    │        keywordDetectedSessions.delete(sessionID)
    │
    ▼
[14] Toast通知:
    "🧠 Context Injected · N fragments"
    "Profile: Dynamic(X) · Static(Y) · Memories: Dynamic(A) Static(B)"
```

### 注入格式示例

```xml
<cerebro-context>
Treat every memory below as historical context only.
Do not repeat these memories verbatim unless asked.

[Preferences]
  - (2h ago [preferences, tools]) 用中文思考和回复
  - (3d ago [preferences, workflow]) 技术方案先出再动工

[Knowledge]
  - (1d ago [omem, architecture]) Cerebro使用lancedb做向量存储

[Events]
  - (5h ago [deployment, omem]) 部署了v1.10.8版本
</cerebro-context>
```

```xml
<cerebro-profile>
{
  "static_facts": [
    { "key": "communication_style", "value": "direct, concise" },
    { "key": "primary_language", "value": "Chinese" }
  ],
  "dynamic_context": [
    { "topic": "current_project", "value": "omem-server-source" }
  ]
}
</cerebro-profile>
```

---

## 五、Hook ③: compactingHook — 压缩时归档

**触发时机**: `session.compacting`（OpenCode压缩session上下文时）
**作用**: 为压缩提供记忆上下文（读） + 归档累积消息（写）

```
session压缩触发
    │
    ▼
[1] 搜索记忆（读操作，所有policy都执行）
    │ client.searchMemories("*", 20, undefined, containerTags)
    │ ──────────────────────→ GET /v1/memories/search?q=*&limit=20
    │
    ├─ 有结果 → buildContextBlock(results)
    │           output.context.push(contextBlock)
    │           （为压缩后的LLM提供记忆上下文）
    │
    ▼
[2] Policy检查: resolveAgentPolicy(agentId, config)
    │
    ├─ 非"readwrite" ──→ logInfo "blocked by policy"
    │                     sessionMessages.delete(sessionID)
    │                     return
    │
    ▼
[3] 检查autoStore开关: isAutoStoreEnabled(sessionID)?
    │
    ├─ 关闭 → sessionMessages.delete(sessionID) → return
    │
    ▼
[4] 消费sessionMessages缓冲区
    │
    ├─ 缓冲区空? → return
    │
    ▼
[5] 检测项目名: detectProjectName(rootPath)
    │ AGENTS.md → package.json → Cargo.toml → go.mod → pyproject.toml
    │
    ▼
[6] 归档消息（写入记忆）
    │ client.ingestMessages(messages, {mode, tags, sessionId, projectName})
    │ ──────────────────────→ POST /v1/memories
    │                        body: { messages: [...], mode: "smart", tags, session_id, project_name }
    │                        每条消息内容先 sanitizeContent(text, maxContentChars=3000)
    │                        → 去XML标签 → 压缩空白 → 超长截断
    │
    ▼
[7] 清理缓冲区: sessionMessages.delete(sessionID)
    │
    ▼
[8] Toast: "📦 Session Archived · N dialogues archived"
```

---

## 六、Hook ④: sessionIdleHook — 空闲时归档

**触发时机**: `session.idle`（session空闲10秒后）
**作用**: 从SDK获取完整对话历史并归档

```
session空闲事件
    │
    ▼
[1] event.type === "session.idle"? ── 否 → return
    │
    ▼
[2] 提取sessionID
    │
    ▼
[3] isAutoStoreEnabled(sessionID)? ── 关闭 → return
    │
    ▼
[4] 非主session? (sessionID !== getMainSessionId()) ── return
    │
    ▼
[5] 延迟10秒执行（防抖）
    │
    ▼
[6] 从SDK获取session消息: sdkClient.session.messages({id: sessionID})
    │
    ▼
[7] 过滤消息:
    │   ├─ 跳过 processedMessageIds 中已处理的
    │   ├─ 跳过 pluginStartTime 之前的（防历史重放）
    │   ├─ 只保留 user/assistant 角色
    │   └─ 提取text parts
    │
    ▼
[8] 消息数 < threshold? ── return
    │
    ▼
[9] Policy检查: resolveAgentPolicy(agentId, config)
    │
    ├─ 非"readwrite" ──→ logInfo "blocked by policy" → return
    │
    ▼
[10] 检测项目名: detectProjectName(rootPath)
     │
     ▼
[11] sessionIngest（写入记忆）
     │ client.sessionIngest(messages, sessionID, agentId, title, projectName)
     │ ──────────────────────→ POST /v1/memories/session-ingest
     │                        body: { messages, session_id, agent_id, session_title, project_name }
     │                        超时60秒
     │
     ▼
[12] 标记已处理: processedMessageIds += newMessageIds
     │
     ▼
[13] Toast: "🧠 Memory Sealed · N dialogues captured"
```

---

## 七、数据流全景图

```
                        ┌─────────────────────────────────┐
                        │          用户消息输入             │
                        └──────────┬──────────────────────┘
                                   │
                    ┌──────────────┼──────────────────┐
                    ▼              ▼                  ▼
            keywordDetection   autoRecall           session.idle
            (chat.message)    (chat.system         (空闲10s)
                               .transform)
                    │              │                  │
                    │         ┌────┘                  │
                    ▼         ▼                       │
            sessionMessages  System Prompt            │
            (内存缓冲)      注入区                     │
                    │         ▲                       │
                    │         │                       │
                    ▼         │                       ▼
              compacting ─────┘                  sessionIdleHook
              (session压缩)                         │
                    │                               │
                    ▼                               ▼
            ┌───────────────────────────────────────────────┐
            │              Cerebro REST API                  │
            │                                              │
            │  读: POST /v1/should-recall    (召回决策)     │
            │  读: GET  /v1/profile          (用户画像)     │
            │  读: GET  /v1/memories/search  (记忆搜索)     │
            │  写: POST /v1/memories         (消息归档)     │
            │  写: POST /v1/memories/session-ingest (session归档) │
            │  写: POST /v1/session-recalls  (召回记录)     │
            │                                              │
            └──────────────────┬───────────────────────────┘
                               │
                               ▼
                    ┌─────────────────────┐
                    │   LanceDB 向量存储   │
                    │   (omem-server)     │
                    └─────────────────────┘
```

---

## 八、Policy门控规则

| Hook | "none" | "readonly" | "readwrite" |
|------|--------|------------|-------------|
| keywordDetection | ❌ 不收集消息 | ✅ 收集消息 | ✅ 收集消息 |
| autoRecall | ❌ 不召回 | ✅ 召回+注入 | ✅ 召回+注入 |
| compacting | ✅ 搜索（读） | ✅ 搜索（读） | ✅ 搜索+写入 |
| sessionIdle | N/A | ❌ 不写入 | ✅ 写入 |

---

## 九、关键配置参数

| 参数 | 位置 | 默认值 | 作用 |
|------|------|--------|------|
| `content.maxContentLength` | config.ts L49 | 500 | **读取侧**截断：每条注入记忆最大字符数 |
| `content.maxContentChars` | config.ts L48 | 30000→3000 | **写入侧**截断：归档时单条消息最大字符数 |
| `content.maxQueryLength` | config.ts L47 | 200 | 召回查询最大字符数 |
| `recall.similarityThreshold` | config.ts L56 | 0.4 | 召回相似度阈值 |
| `recall.maxRecallResults` | config.ts L57 | 10 | 最大召回结果数 |
| `ingest.autoCaptureThreshold` | config.ts L51 | 5 | 消息累积到N条才触发归档 |
| `ui.toastDelayMs` | config.ts L65 | 7000 | Toast显示时长(ms) |
| `agentMemoryPolicy` | config.ts L34 | - | 各agent的读写权限 |
| `defaultPolicy` | config.ts L35 | "readwrite" | 未配置agent的默认权限 |

---

## 十、写入侧 vs 读取侧截断对比

```
                        写入路径                          读取路径
                    (归档到服务端)                    (注入到system prompt)

消息内容          sanitizeContent()                 truncate()
                  client.ts L4-10                   hooks.ts L147-150

处理流程          去XML标签 → 压缩空白 → 截断       直接截断

配置参数          maxContentChars (3000)            maxContentLength (500)

截断标记          "…[truncated]"                    "…"

触发点            createMemory() L182               buildContextBlock() L178
                  ingestMessages() L238

调用方            compactingHook                    autoRecallHook
                  sessionIdleHook
```
