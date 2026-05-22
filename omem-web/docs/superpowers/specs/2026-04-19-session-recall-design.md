# Omem 第三次迭代设计文档：Session 维度记忆注入记录

**日期**: 2026-04-19
**版本**: v0.3.0
**状态**: 草案，待审批

---

## 1. 背景与目标

### 1.1 背景

当前 omem plugin 的 `autoRecallHook` 在每个 OpenCode session 开始时自动从记忆库搜索相关记忆，并注入到 system prompt 中作为上下文。这些被注入的记忆对 assistant 的回复质量至关重要，但目前没有任何记录机制来追踪：
- 哪些记忆被注入了？
- 基于什么消息(query)注入的？
- 注入的记忆与最终回复的关联是什么？

### 1.2 目标

1. **智能注入**: 改进 `autoRecallHook`，实现每轮对话自动检测是否需要查找记忆（A+D 结合方案）
2. **服务端**: 记录每次 recall 触发时注入的记忆，提供 Web 查询接口
3. **前端**: 按 OpenCode session 维度展示注入记录，支持查看详情
4. **体验**: 两个页面（列表页 + 详情页）设计精良，信息展示完整

---

## 2. 数据模型

### 2.1 新建表: `session_recalls`

| 字段 | 类型 |  nullable | 说明 |
|------|------|-----------|------|
| `id` | UUID | false | 主键 |
| `session_id` | string | false | OpenCode session ID (e.g. `ses_xxx`) |
| `session_name` | string | true | session 名称，当前取 query 前 50 字符 |
| `tenant_id` | string | false | 租户 ID，用于权限隔离 |
| `agent_id` | string | true | 来源 agent (e.g. `opencode`) |
| `query` | string | false | 触发 recall 的 query (session 第一条消息) |
| `injected_memories` | JSON | false | `SearchResult[]` 数组，含 memory 完整字段 + score |
| `context_block` | string | true | 注入的完整文本块 (原始 `<omem-context>` 内容) |
| `memory_count` | int | false | 注入的记忆数量 |
| `created_at` | timestamp | false | 创建时间 |

### 2.2 数据结构详情

`injected_memories` JSON 结构：

```json
[
  {
    "memory": {
      "id": "uuid",
      "content": "string",
      "l2_content": "string",
      "category": "string",
      "memory_type": "string",
      "state": "string",
      "tags": ["string"],
      "source": "string",
      "tenant_id": "string",
      "agent_id": "string",
      "created_at": "ISO8601",
      "updated_at": "ISO8601"
    },
    "score": 0.95
  }
]
```

---

## 3. 服务端 API 设计

### 3.1 新增 API

#### `POST /v1/should-recall`

Plugin 端调用，判断当前消息是否需要 recall。

**请求体:**
```json
{
  "session_id": "ses_xxx",
  "current_message": "这段代码怎么优化？",
  "last_query": "怎么部署项目？",
  "injected_memory_ids": ["uuid1", "uuid2"]
}
```

**响应:**
```json
{
  "should_recall": true,
  "query": "代码优化",
  "reason": "话题从部署变为代码优化",
  "similarity": 0.32
}
```

**实现逻辑:**
1. 计算 `current_message` 与 `last_query` 的语义相似度（Qwen Embedding）
2. 相似度 < 0.7 → 用轻量 LLM 判断是否需要 recall
3. 返回判断结果 + 推荐 query

#### `POST /v1/session-recalls`

Plugin 端调用，保存一次 recall 注入记录。

**请求体:**
```json
{
  "session_id": "ses_xxx",
  "session_name": "omem-web 前端 bug 修复...",
  "query": "这段代码有什么问题？",
  "injected_memories": [...],
  "context_block": "<omem-context>...",
  "agent_id": "opencode"
}
```

**响应:**
```json
{
  "id": "uuid",
  "status": "saved"
}
```

#### `GET /v1/session-recalls`

Web 端查询列表，支持分页和搜索。

**查询参数:**
- `page` (int, default: 1)
- `page_size` (int, default: 20, max: 100)
- `session_id` (string, optional) - 精确匹配
- `query` (string, optional) - 模糊搜索 query 内容

**响应:**
```json
{
  "items": [
    {
      "id": "uuid",
      "session_id": "ses_xxx",
      "session_name": "omem-web 前端 bug 修复...",
      "query": "这段代码有什么问题？",
      "memory_count": 5,
      "created_at": "2026-04-19T14:32:00Z"
    }
  ],
  "total": 100,
  "page": 1,
  "page_size": 20
}
```

#### `GET /v1/session-recalls/:id`

Web 端查询单条详情。

**响应:**
```json
{
  "id": "uuid",
  "session_id": "ses_xxx",
  "session_name": "omem-web 前端 bug 修复...",
  "tenant_id": "...",
  "agent_id": "opencode",
  "query": "这段代码有什么问题？",
  "injected_memories": [...],
  "context_block": "<omem-context>...",
  "memory_count": 5,
  "created_at": "2026-04-19T14:32:00Z"
}
```

### 3.2 权限控制

- 所有 API 通过 `X-API-Key` header 认证
- `tenant_id` 从 API Key 解析
- 列表和详情查询均按 `tenant_id` 过滤

---

## 4. 智能注入机制（A + D 结合方案）

### 4.1 三层触发策略

**第一层：话题变化检测（方案A）**
- 每轮对话计算当前消息与上次 recall query 的语义相似度
- 用 omem 服务端 Qwen Embedding 模型计算余弦相似度
- 相似度 > 0.7：话题没变，跳过
- 相似度 < 0.7：进入第二层

**第二层：LLM 精确判断（方案D）**
- 轻量 prompt 让 LLM 判断当前消息是否需要新记忆
- 分析当前消息 + 已注入记忆摘要
- 回答 yes/no

**第三层：增量注入**
- 维护 `injectedMemoryIds` 集合
- 新的 recall 结果与已注入记忆去重
- 只注入新增记忆

### 4.2 快速规则过滤

- 单字回复（"好"、"ok"）：直接跳过
- 代码块/文件操作：高概率需要 recall，优先触发
- 连续相似消息：缓存相似度结果

## 5. Plugin 改造

### 5.1 智能触发改造

**去掉 `injectedSessions` 限制**，改为每轮对话都检测是否需要 recall。

**代码位置:** `omem-server-source/plugins/opencode/src/hooks.ts`

**改造逻辑:**

```typescript
// 1. 调用服务端 should-recall API 判断是否需要 recall
const shouldRecall = await client.shouldRecall({
  session_id: input.sessionID,
  current_message: textContent,
  last_query: lastQueries.get(input.sessionID),
  injected_memory_ids: injectedMemoryIds.get(input.sessionID) || [],
});

if (shouldRecall?.should_recall) {
  // 2. 执行 recall
  const results = await client.searchMemories(
    shouldRecall.query,
    MAX_RECALL_RESULTS,
    undefined,
    containerTags,
  );
  
  // 3. 去重：过滤已注入的记忆
  const existingIds = injectedMemoryIds.get(input.sessionID) || new Set();
  const newResults = results.filter(r => !existingIds.has(r.memory.id));
  
  if (newResults.length > 0) {
    // 4. 注入新记忆
    const block = buildContextBlock(newResults);
    output.system.push(block);
    
    // 5. 更新已注入记忆集合
    newResults.forEach(r => existingIds.add(r.memory.id));
    injectedMemoryIds.set(input.sessionID, existingIds);
    
    // 6. 保存 recall 记录
    await client.saveSessionRecall({
      session_id: input.sessionID,
      session_name: truncate(shouldRecall.query, 50),
      query: shouldRecall.query,
      injected_memories: newResults,
      context_block: block,
      agent_id: "opencode",
    });
  }
}
```

### 5.2 Client 新增方法

```typescript
// 判断是否需要 recall
async shouldRecall(data: {
  session_id: string;
  current_message: string;
  last_query?: string;
  injected_memory_ids?: string[];
}): Promise<{ should_recall: boolean; query: string; reason: string } | null> {
  return this.post("/v1/should-recall", data);
}

// 保存 recall 记录
async saveSessionRecall(data: {
  session_id: string;
  session_name?: string;
  query: string;
  injected_memories: SearchResult[];
  context_block?: string;
  agent_id?: string;
}): Promise<{ id: string; status: string } | null> {
  return this.post("/v1/session-recalls", data);
}
```

### 4.2 Client 新增方法

在 `OmemClient` 中新增 `saveSessionRecall` 方法：

```typescript
async saveSessionRecall(data: {
  session_id: string;
  session_name?: string;
  query: string;
  injected_memories: SearchResult[];
  context_block?: string;
  agent_id?: string;
}): Promise<{ id: string; status: string } | null> {
  return this.post("/v1/session-recalls", data);
}
```

---

## 5. 前端设计

### 5.1 新增路由

| 路由 | 页面 | 说明 |
|------|------|------|
| `/sessions` | Session Recall 列表页 | 展示所有 recall 记录 |
| `/sessions/:id` | Session Recall 详情页 | 展示单条 recall 的完整信息 |

### 5.2 侧边栏

新增菜单项：
- 图标: `📦` 或 `GitBranch`
- 标签: "会话注入"
- 路径: `/sessions`

### 5.3 页面A: Session Recall 列表页 (`/sessions`)

**布局:** 卡片式网格布局（类似空间管理页风格）

**每卡片内容:**
- **顶部**: session 名称（`session_name`），若为空则显示 `session_id` 截断
- **中间**: 
  - query 摘要（前 80 字符）
  - 注入记忆数量 badge
  - 相关性分数范围（最低 ~ 最高 score）
- **底部**: 时间 + 标签（agent_id）
- **点击**: 进入详情页

**搜索/筛选:**
- 搜索框: 按 `session_name` 或 `query` 模糊搜索
- 分页: 底部页码

### 5.4 页面B: Session Recall 详情页 (`/sessions/:id`)

**布局:** 左右分栏或上下分区

**头部区域:**
- 返回按钮 ←
- session 名称（大号字体）
- session_id（小号，灰色）
- 时间 + agent badge

**Query 区域:**
- 标题: "触发消息"
- 卡片展示 `query` 完整内容
- 样式: 引用块风格，左侧有竖线装饰

**注入记忆列表区域:**
- 标题: "注入的记忆 (N 条)"
- 每条记忆用卡片展示:
  - **顶部**: category badge + memory_type badge + score badge（颜色按分数梯度）
  - **内容**: `content`（支持 markdown 渲染）
  - **展开后**: 展示完整字段
    - `l2_content`（详细内容）
    - `tags`（标签列表）
    - `source`
    - `created_at`
    - `agent_id`
  - **操作**: "查看原记忆" 链接（跳转 `/memories/:memoryId`）

**Context Block 区域（可折叠）:**
- 标题: "注入的原始上下文"
- 折叠面板，展示 `context_block` 完整文本
- 代码块样式，带复制按钮

---

## 6. 技术实现要点

### 6.1 LanceDB JSON 字段

LanceDB 支持 `FixedSizeList` 和 `List` 类型。`injected_memories` 使用 JSON string 存储，查询时反序列化。

### 6.2 服务端路由与 LLM 调用

参考现有 `/v1/memories` 路由实现，在 `router.rs` 中新增：

```rust
.route("/v1/should-recall", post(handlers::should_recall))
.route("/v1/session-recalls", get(handlers::list_session_recalls).post(handlers::create_session_recall))
.route("/v1/session-recalls/:id", get(handlers::get_session_recall))
```

**LLM 调用实现（SiliconFlow）：**

`POST /v1/should-recall` handler 内部逻辑：

1. **Embedding 相似度计算**：
   - 调用 SiliconFlow Embedding API（复用现有 `EMBEDDING_API_KEY`）
   - 请求体：`{ "model": "BAAI/bge-large-zh-v1.5", "input": ["text1", "text2"] }`
   - 计算余弦相似度

2. **LLM 判断**：
   - 如果相似度 < 0.7，调用 SiliconFlow Chat Completions API
   - 请求体：
     ```json
     {
       "model": "Qwen/Qwen3.5-4B",
       "messages": [
         {"role": "system", "content": "你是一个对话分析助手..."},
         {"role": "user", "content": "判断当前消息是否需要回忆..."}
       ],
       "max_tokens": 50
     }
     ```
   - 解析 yes/no 回答

### 6.3 前端组件复用

- 复用现有 `Card`, `Badge`, `Skeleton` 等 shadcn/ui 组件
- 复用 `apiClient`（自动注入 X-API-Key）
- 新增 `SessionRecallList` 和 `SessionRecallDetail` 页面组件

---

## 7. 实现顺序

### Phase 1: 智能注入机制
1. **服务端**: 
   - 新增 `POST /v1/should-recall` API（embedding 相似度 + LLM 判断）
   - 新增 `POST /v1/session-recalls` API（保存记录）
   - 新建 `session_recalls` 表
2. **Plugin**: 
   - 去掉 `injectedSessions` 限制
   - 每轮调用 `shouldRecall` 判断
   - 增量注入 + 保存记录

### Phase 2: 前端展示
3. **前端**: 
   - 新增 `/sessions` 路由和侧边栏菜单
   - 实现 Session Recall 列表页
   - 实现 Session Recall 详情页

### Phase 3: 测试优化
4. **测试**: 端到端验证
5. **优化**: 调整相似度阈值、LLM prompt、缓存策略

---

## 8. 待确认事项

1. **相似度阈值**: 方案A的语义相似度阈值默认 0.7，是否需要可调？
2. **LLM 模型**: 方案D的轻量判断用哪个模型？（建议 Qwen 1.8B 或本地小模型）
3. **Session 名称**: 当前 plugin API 无法获取 OpenCode 的 session 标题，方案是用 query 前 50 字符作为默认名称。
4. **数据保留策略**: `session_recalls` 表是否需要定期清理？

---

*文档版本: 草案 v0.1*
*等待师尊审批...*
