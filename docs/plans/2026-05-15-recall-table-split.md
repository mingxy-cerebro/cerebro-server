# 记忆召回表拆分 — 实施计划

> Phase: 独立于全局5 Phase计划，可先行启动
> 
> 背景：当前 `session_recalls` 单表平铺，同次召回N条记忆重复存储共享字段。
> 目标：拆为 `recall_events` + `recall_items`，should_recall 自动存储（含discarded），web端可看精炼前后对比。

---

## 一、现状

### 当前 session_recalls 单表（13字段）

```
create_session_recall handler (line 418-467):
  → 每条记忆一行，batch_id相同但共享字段（query_text等）全部重复
  → plugin调完should_recall后，再调create_session_recall传入memory_ids
  → discarded（被LLM精炼掉的）没有持久化，直接丢了
```

### 数据断层

```
should_recall → pipeline精炼 → 返回high/medium → plugin → create_session_recall → 只存保留的
                                                  ↓ discarded（irrelevant）
                                                  丢了！web端看不到
```

---

## 二、目标架构

### recall_events 表（1行/召回事件）

| 字段 | LanceDB类型 | Nullable | 说明 |
|------|------------|----------|------|
| id | Utf8 | false | 事件ID（UUID，替代原batch_id） |
| session_id | Utf8 | false | 会话ID |
| recall_type | Utf8 | false | auto / manual |
| query_text | Utf8 | false | 查询文本 |
| max_score | Float32 | false | 最高相似度 |
| llm_confidence | Float32 | false | LLM置信度 |
| profile_injected | Boolean | false | 是否注入画像 |
| kept_count | UInt32 | false | 保留的记忆数 |
| discarded_count | UInt32 | false | 被精炼掉的记忆数 |
| tenant_id | Utf8 | false | 租户 |
| created_at | Utf8 | false | RFC3339时间戳 |

### recall_items 表（1行/记忆）

| 字段 | LanceDB类型 | Nullable | 说明 |
|------|------------|----------|------|
| id | Utf8 | false | UUID主键 |
| event_id | Utf8 | false | 逻辑FK → recall_events.id |
| memory_id | Utf8 | false | 记忆ID |
| score | Float32 | false | 精炼前的原始得分 |
| refine_relevance | Utf8 | true | high / medium / irrelevant / candidate |
| refine_reasoning | Utf8 | true | LLM推理说明 |
| is_kept | Boolean | false | true=保留注入 / false=被精炼掉 |
| tenant_id | Utf8 | false | 租户 |
| created_at | Utf8 | false | RFC3339时间戳 |

### 数据流（改造后）

```
should_recall handler:
  → pipeline精炼 → 得到 results(kept) + discarded
  → 自动创建 1条 recall_event
  → 自动创建 N条 recall_items（kept + discarded 全存）
  → is_kept=true 表示保留，is_kept=false 表示被精炼掉
  → 返回给plugin的响应不变
```

---

## 三、代码改动清单

### 3.1 store/lancedb.rs — 新增schema + CRUD

**新增struct：**

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecallEvent {
    pub id: String,
    pub session_id: String,
    pub recall_type: String,
    pub query_text: String,
    pub max_score: f32,
    pub llm_confidence: f32,
    pub profile_injected: bool,
    pub kept_count: u32,
    pub discarded_count: u32,
    pub tenant_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecallItem {
    pub id: String,
    pub event_id: String,
    pub memory_id: String,
    pub score: f32,
    pub refine_relevance: Option<String>,
    pub refine_reasoning: Option<String>,
    pub is_kept: bool,
    pub tenant_id: String,
    pub created_at: String,
}
```

**新增schema函数：**
- `recall_events_schema() -> Arc<Schema>`
- `recall_items_schema() -> Arc<Schema>`

**新增CRUD方法：**
- `create_recall_event(event: &RecallEvent) -> Result<()>`
- `batch_create_recall_items(items: &[RecallItem]) -> Result<()>`
- `list_recall_events(tenant_id, session_id, limit, offset) -> Result<Vec<RecallEvent>>`
- `list_recall_items_by_event(tenant_id, event_id) -> Result<Vec<RecallItem>>`
- `delete_recall_events_by_session(tenant_id, session_id) -> Result<()>`（级联删items）
- `list_recall_groups(tenant_id) -> Result<Vec<SessionGroupRaw>>`（改为从events聚合）

**新增LanceStore字段：**
- `recall_events_table: Table`
- `recall_items_table: Table`

**已有常量（直接复用）：**
- `RECALL_EVENTS_TABLE: &str = "recall_events"` — 已定义
- `RECALL_ITEMS_TABLE: &str = "recall_items"` — 已定义

### 3.2 api/handlers/session_recalls.rs — 改造should_recall

**should_recall handler（line 135-416）改造：**

在返回响应前，新增自动存储逻辑：

```rust
// 新增：自动存储recall event + items
let event_id = uuid::Uuid::new_v4().to_string();
let kept_count = memories.len() as u32;
let discarded_count = /* 从pipeline获取 */;

let event = RecallEvent {
    id: event_id.clone(),
    session_id: body.session_id.clone(),
    recall_type: "auto".to_string(),
    query_text: body.query_text.clone(),
    max_score: memories.iter().map(|m| m.score).fold(0.0_f32, f32::max),
    llm_confidence: confidence.unwrap_or(0.0),
    profile_injected: false, // 由plugin在create时设置，此处为默认值
    kept_count,
    discarded_count,
    tenant_id: auth.tenant_id.clone(),
    created_at: chrono::Utc::now().to_rfc3339(),
};
store.create_recall_event(&event).await?;

// 存kept items
let mut items = Vec::new();
for m in &memories {
    items.push(RecallItem {
        id: uuid::Uuid::new_v4().to_string(),
        event_id: event_id.clone(),
        memory_id: m.memory.id.clone(),
        score: m.score,
        refine_relevance: m.refine_relevance.clone(),
        refine_reasoning: m.refine_reasoning.clone(),
        is_kept: true,
        tenant_id: auth.tenant_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    });
}
// 存discarded items
for d in &discarded {
    items.push(RecallItem {
        id: uuid::Uuid::new_v4().to_string(),
        event_id: event_id.clone(),
        memory_id: d.memory.id.clone(),
        score: d.score,
        refine_relevance: d.refine_relevance.clone(),
        refine_reasoning: d.refine_reasoning.clone(),
        is_kept: false,
        tenant_id: auth.tenant_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    });
}
store.batch_create_recall_items(&items).await?;
```

**需要pipeline返回discarded数据：**

当前 `RetrievalPipeline.search()` 返回 `SearchResults { results, discarded, trace }`，但 `should_recall` handler（line 354-364）只取了results，丢弃了discarded。需要保留discarded。

### 3.3 API兼容性

| 端点 | 改动 | 兼容性 |
|------|------|--------|
| `POST /v1/should-recall` | 内部自动存event+items，返回值不变 | ✅ 100%兼容 |
| `GET /v1/session-recalls` | 改为JOIN events+items返回平铺结构 | ✅ 兼容旧格式 |
| `GET /v1/session-recalls/groups` | 改为从recall_events聚合 | ✅ 兼容旧格式 |
| `POST /v1/session-recalls` | 保留但降级为补充接口（更新profile_injected等） | ✅ 兼容 |
| `DELETE /v1/session-recalls/session/{id}` | 改为级联删events+items | ✅ 兼容 |
| **新增** `GET /v1/recall-events` | 事件列表（web端用） | 🆕 新端点 |
| **新增** `GET /v1/recall-events/{id}/items` | 事件下的所有items（含discarded） | 🆕 新端点 |

### 3.4 router.rs — 新增路由

```rust
// 新增
.get("/v1/recall-events", list_recall_events)
.get("/v1/recall-events/{id}/items", list_recall_event_items)
```

### 3.5 init_table() — 自动建表

`LanceStore::new()` 已有自动建表逻辑，新增两张表的初始化：
- 检查表是否存在 → 不存在则创建
- schema evolution：检查缺失列并添加

---

## 四、数据迁移

**旧 `session_recalls` 表直接删除，不做向后兼容。**

新数据全部写入两张新表。旧 `session_recalls` 的历史数据不做迁移（历史数据价值低，不值得花时间）。

**清理范围：**
- 删除 `session_recalls` 表常量及相关schema/CRUD代码
- 删除 `SessionRecall` struct
- 删除 `session_recalls_table` 字段从 `LanceStore`

---

## 五、web端适配（omem-web）

### 页面结构设计（师尊钦定）

```
时间线（垂直）
├── 召回事件卡片（可点击展开/折叠）
│   ├── 头部：触发词（query_text，超过N字截断，hover显示全部）
│   ├── 展开/折叠
│   └── 展开后：
│       ├── 记忆内容展示（Tab切换：精炼 / 原始）
│       │   ├── Tab「精炼」：显示refine_relevance等级 + refine_reasoning推理说明
│       │   └── Tab「原始」：显示记忆的原始content（Markdown渲染）
│       ├── 精炼等级颜色区分（保留）
│       │   ├── 🟢 高相关（is_kept=true, relevance=high）
│       │   ├── 🟡 中相关（is_kept=true, relevance=medium）
│       │   └── 🔴 被精炼掉（is_kept=false, relevance=irrelevant）
│       ├── 私密记忆处理
│       │   ├── visibility=private → 默认隐藏，显示🔒图标
│       │   └── 点击🔒 → 输入Vault密码 → 解锁显示内容
│       └── 底部统计
│           ├── similarity_score 进度条（0~100%）
│           └── llm_confidence 进度条（0~100%）
```

### 交互细节

| 组件 | 行为 |
|------|------|
| 触发词 | 超过80字截断，鼠标hover显示tooltip完整内容 |
| 展开/折叠 | 点击卡片头部切换，默认折叠 |
| 精炼/原始 | Tab切换，默认显示「精炼」tab（refine_relevance + reasoning），点击切换到「原始」tab（记忆原始content） |
| 精炼等级 | 用颜色Badge区分（🟢=high / 🟡=medium / 🔴=irrelevant），配合Tab一起展示 |
| 私密记忆 | 默认隐藏内容，显示🔒+「私密记忆」，需输入Vault密码解锁 |
| 进度条 | similarity_score和llm_confidence各一个百分比进度条 |
| 记忆内容 | Markdown渲染，私密内容解锁后才显示 |

### 新增API调用

| 端点 | 用途 |
|------|------|
| `GET /v1/recall-events` | 事件列表（时间线） |
| `GET /v1/recall-events/{id}/items` | 事件下所有items（含discarded） |
| `GET /v1/memories/{id}` | 展开原始记忆详情（已有） |
| `POST /v1/vault/verify` | 私密记忆密码验证（已有） |

### TypeScript 类型定义

```typescript
interface RecallEvent {
  id: string
  session_id: string
  recall_type: "auto" | "manual"
  query_text: string
  max_score: number
  llm_confidence: number
  profile_injected: boolean
  kept_count: number
  discarded_count: number
  created_at: string
}

interface RecallItem {
  id: string
  event_id: string
  memory_id: string
  score: number
  refine_relevance: "high" | "medium" | "irrelevant" | "candidate" | null
  refine_reasoning: string | null
  is_kept: boolean
  created_at: string
}
```

---

## 六、验证标准

1. `cargo build` 通过
2. `cargo test` 通过
3. `should_recall` 调用后自动创建 event + items
4. web端能看到每次召回的完整画面（kept + discarded）
5. 旧API端点兼容不变
6. 新端点 `/v1/recall-events` 正常工作

---

## 七、风险

| 风险 | 等级 | 应对 |
|------|------|------|
| should_recall内部写DB增加延迟 | 低 | 异步写入或spawn，不阻塞响应 |
| LanceDB两张表跨表查询性能 | 中 | events表加btree索引(session_id, created_at) |
| discarded数据量大 | 低 | 设置保留上限（如每次最多存50条discarded） |
| 旧plugin调create_session_recall产生重复 | 低 | 检测event是否已存在，已存在则跳过 |
