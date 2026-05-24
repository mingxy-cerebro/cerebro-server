# 🔮 Cerebro 完整架构文档 — 从注入到遗忘

> 端到端数据流：Plugin → REST API → Ingest/Retrieve/Lifecycle → Storage

---

## 一、系统总览

Cerebro 是一个 AI Agent 共享持久记忆系统，由 **Rust 后端** (axum 0.8 + LanceDB 0.27) + **TypeScript 插件** 组成。

### 数据流全景图

```
用户消息 → Plugin Hooks → Cerebro REST API → [召回/存储] → Plugin 注入 → LLM 响应
                                    ↓
                            [生命周期调度器]
                            Weibull衰减 → Tier升降 → 自动遗忘
```

### 三大核心管道

| 管道 | 入口 | 功能 |
|------|------|------|
| **Ingest Pipeline** | `POST /v1/memories/session-ingest` | 对话消息 → 结构化记忆 |
| **Retrieve Pipeline** | `POST /v1/should-recall` → `search_memories` | 向量+BM25混合检索 |
| **Lifecycle Pipeline** | `LifecycleScheduler` (每日午夜) | 衰减评估 → Tier升降 → 遗忘 |

---

## 二、Plugin 端完整流程

### 2.1 Hook 注册表

| OpenCode 事件 | Hook 函数 | 作用 |
|---|---|---|
| `experimental.chat.system.transform` | `autoRecallHook` | **核心召回**：每轮判断是否召回+注入画像 |
| `chat.message` | `keywordDetectionHook` | **消息采集**：收集用户消息+检测保存关键词 |
| `experimental.session.compacting` | `compactingHook` | **压缩前保存**：采集session消息+丰富压缩提示 |
| `experimental.compaction.autocontinue` | `autocontinueHook` | **压缩后存储**：将压缩摘要存为记忆 |
| `event` | `sessionIdleHook` | **空闲存储**：session idle 时批量 ingestion |

### 2.2 全局状态管理

```
projectNameCache            : Map<string, string>          // rootPath → projectName
saveKeywordDetectedSessions : Set<string>                  // 检测到保存关键词的session
firstMessages               : Map<string, string>          // sessionID → 首条用户消息
sessionMessages             : Map<string, Array<{role,content}>>  // sessionID → 采集的消息
profileInjectedSessions     : Map<string, number>          // sessionID → 上次画像注入时间戳
lastProfileBlock            : Map<string, {content,count}> // sessionID → 缓存的画像内容
lastUserMsgCount            : Map<string, number>          // sessionID → 上次处理时的消息数
summarizedSessions          : Set<string>                  // 已捕获压缩摘要的session
processedMessageIds         : Set<string>                  // 已通过idle ingestion处理的消息ID
```

### 2.3 自动召回流程 (autoRecallHook)

```
用户消息到达
    │
    ▼
┌─ Gate 0: 无 sessionID → 返回
├─ Gate 1: Agent policy = "none" → 返回
├─ Gate 2: 无新用户消息 (userMsgCount ≤ lastCount) → 返回
│
├─ PROFILE: TTL 门控 (默认5分钟)
│   ├─ 过期 → GET /v2/profile/inject (V2 API, 最多3次重试)
│   └─ 缓存 → 从 lastProfileBlock 恢复
│
├─ Gate 3: 无用户消息 (压缩后瞬态) → 返回
├─ QUERY: extractUserRequest() 提取查询
│   └─ 匹配14种系统注入模式 → 返回 (跳过系统消息)
│
├─ CONTEXT: 构建对话上下文 (最近3条用户消息, <private>过滤, 200字符截断)
│
├─ RECALL: POST /v1/should-recall (20秒超时)
│   ├─ should_recall = false → 仅注入画像, 返回
│   └─ should_recall = true → 继续
│
├─ BUDGET: 剩余预算 = maxContentChars(30000) - 画像字符数
│
├─ BUILD: buildContextBlock() → 按 category 分组, 预算分配
│   └─ 每条记忆: score占比 × 预算 = 分配长度
│
├─ INJECT: output.system[last] +=
│   ├─ <cerebro-context>...</cerebro-context>
│   ├─ FETCH_POLICY (提示用 memory_get 获取详情)
│   ├─ <cerebro-profile>...</cerebro-profile>
│   └─ KEYWORD_NUDGE (如检测到保存关键词)
│
└─ EVENT: POST /v1/recall-events (分析追踪)
```

### 2.4 消息采集流程 (keywordDetectionHook)

```
每条用户消息
    │
    ├─ firstMessages: 首条消息存入 (用于后续查询回退)
    ├─ Keyword检测: "记住"/"save this"/"别忘了" 等中英文模式
    │   └─ 匹配 → saveKeywordDetectedSessions.add(sessionID)
    ├─ Policy gate: "none" → 跳过采集
    └─ sessionMessages[sessionID].push({role:"user", content:text})
        └─ 消息数 >= threshold → 等待 session.idle 触发 ingestion
```

### 2.5 Session Idle 存储 (sessionIdleHook)

```
session.idle 事件触发
    │
    ├─ 10秒防抖定时器
    ├─ isCapturing 互斥锁
    ├─ SDK获取所有session消息
    ├─ 过滤: 跳过已处理(processedMessageIds) + 插件启动前消息
    ├─ 阈值检查: conversationMessages.length < threshold → 返回
    ├─ 解析agent名 + 项目名
    ├─ Policy gate: 非"readwrite" → 返回
    └─ POST /v1/memories/session-ingest (60秒超时)
        └─ 标记已处理消息ID
```

### 2.6 Agent Policy 系统

三级策略: `"none"` | `"readonly"` | `"readwrite"` (默认: readwrite)

| Hook | `none` | `readonly` | `readwrite` |
|---|---|---|---|
| autoRecallHook | 跳过 | 完整召回 | 完整召回 |
| keywordDetectionHook | 跳过采集 | 采集消息 | 采集消息 |
| compactingHook | 仅清理 | 丰富提示+清理 | 完整ingest+清理 |
| autocontinueHook | 跳过 | 跳过 | 存储摘要 |
| sessionIdleHook | 跳过 | 跳过 | 完整session ingest |

---

## 三、服务端 Ingest Pipeline (11阶段)

### 3.0 入口: session_ingest handler

**源文件**: `api/handlers/memory.rs`

```
POST /v1/memories/session-ingest
    │
    ├─ 验证: 消息数 ≤ 40, 总字符 ≤ 30K
    ├─ 获取 session lock (DashMap per-session Mutex)
    ├─ 获取个人空间 LanceStore
    ├─ 获取已有 emotional + work 记忆摘要
    │
    ├─ LLM 提取: build_session_extract_prompt_with_memories()
    │   └─ 包含已有记忆摘要, 实现 merge/supersede
    │
    ├─ 对每个提取结果 (分类为 EMOTIONAL 或 WORK):
    │   │
    │   ├─ EMOTIONAL 路径:
    │   │   └─ 查找已有情感记忆 (scope="private", session_id匹配)
    │   │   └─ 找到 → append 新内容到已有记忆 (≤3000字符)
    │   │   └─ 未找到 → 创建新的 private 记忆
    │   │   └─ 标签: "私密"/"vulnerable"/"playful"/"reconciliation" (LLM自动)
    │   │
    │   └─ WORK 路径 (精炼流程, 详见 3.3):
    │       └─ find_similar_work_memory() → cosine > 0.72 且 session_id匹配
    │       └─ 找到相似 → collect_chain_memories() (BFS遍历关系链)
    │       └─ refine_and_replace() → LLM精炼 → 创建新记忆 + 删除旧记忆
    │       └─ 未找到 → 创建新的 public 记忆
    │
    └─ 返回 202 Accepted (异步处理)
```

### 3.3 WORK 记忆精炼流程 (Refine Service)

**源文件**: `ingest/refine_service.rs` (344行) + `ingest/refine_prompt.rs` (85行)

当 session_ingest 提取出 WORK 类型的记忆时，会执行精炼流程：

```
新WORK事实到达
    │
    ▼
Step 1: 相似记忆查找 (find_similar_work_memory)
    │ embed(topic.l0_abstract) → 查询向量
    │ store.find_memories_by_session_id(session, 100) → 同session记忆
    │ 过滤 scope != "private" → WORK记忆
    │ 逐一计算 cosine(query_vec, mem_vec)
    │ cosine > 0.72 → 返回最相似的那条
    │
    ▼ 找到相似记忆
Step 2: 收集关系链 (collect_chain_memories)
    │ BFS遍历 Continues/ContinuedBy 关系 (最大深度5)
    │ 返回链上所有 Memory (包含root)
    │
    ▼
Step 3: LLM精炼 (refine_and_replace)
    │ 输入准备:
    │   ├─ 取链上最多3条记忆 (MAX_CHAIN_FOR_REFINE=3), 按时间倒序
    │   ├─ 每条截断 ≤3000字符 (MAX_SINGLE_MEMORY_CHARS)
    │   └─ 总输入 ≤8000字符 (MAX_INPUT_CHARS), 超限则只保留最新1条
    │
    │ 调用 LLM (REFINE_SYSTEM_PROMPT):
    │   System: 精炼引擎 — 去重+压缩到30-60%
    │   User: "## Topic: {topic}\n### Existing Memory #1\n...\n### New Information\n{new_fact}"
    │   返回: { refined_content, l0_abstract, l1_overview, l2_content }
    │
    │ 后处理:
    │   ├─ 按句子边界截断 (。！？\n): refined≤3000, l1≤150, l2≤300
    │   ├─ 继承最优 tier (core>working>peripheral)
    │   ├─ 继承最高 importance
    │   └─ 继承合并 tags (排序去重)
    │
    │ 存储操作:
    │   ├─ 创建新 Memory (新UUID, 精炼后的内容)
    │   │   embed(refined_content) → 新向量
    │   │   store.create(new_memory, vector)
    │   └─ 物理删除所有旧记忆
    │       store.batch_hard_delete_by_ids(old_ids)
    │       → NOT 软删除, 是硬删除!
    │
    ▼
  返回精炼后的新 Memory
```

#### 精炼 LLM Prompt (完整)

```
You are a memory refinement engine. Read one or more existing memory entries about
the same topic, plus a new fact, then produce a SINGLE refined, deduplicated memory.

## ABSOLUTE RULES

### Rule 1: Language Preservation (MANDATORY)
- YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT. NEVER translate.

### Rule 2: Deduplication (CORE TASK)
- Remove duplicate/redundant information across all sections.
- If multiple sections describe the same event/decision, MERGE using LATEST timestamp.
- Keep ONLY: final conclusions, key decisions, important outcomes, critical data points.
- Remove: intermediate steps, verbose process details, outdated information.

### Rule 3: Format Preservation
- Maintain `## YYYY-MM-DD HH:MM Topic` section structure.
- Chronological order (oldest first).

### Rule 4: Precision Over Recall
- Better to lose minor details than keep redundant content.
- Target: compress to 30-60% of original total length.

## OUTPUT FORMAT
{
  "refined_content": "Deduplicated content in section format",
  "l0_abstract": "Topic label (≤100 chars)",
  "l1_overview": "Arrow format: A→B→C→result (≤150 chars)",
  "l2_content": "Key facts only (≤300 chars)"
}

## l1_overview FORMAT (MANDATORY)
Arrow notation: verb phrase→verb phrase→result
Example: "diagnosed bug→traced to handler→fixed→verified→deployed v1.16.10"
```

#### 关键设计要点

| 要点 | 说明 |
|------|------|
| **物理删除** | 精炼后旧记忆被 `batch_hard_delete_by_ids` 彻底删除, 不是软归档 |
| **Tier继承** | 链上最高tier被继承 (core>working>peripheral), 精炼不降级 |
| **Importance继承** | 取链上最高importance, 精炼不降权 |
| **Tags合并** | 所有旧记忆的tags合并去重, 保留所有标签 |
| **向量重建** | 用精炼后的 `refined_content` 重新embed, 不是用l0_abstract |
| **截断策略** | 按句子边界 (。！？\n) 截断, 无边界时强制字符截断+省略号 |
| **链深度限制** | BFS最大深度5, 防止无限遍历 |
| **输入预算** | 总输入≤8000字符, 单条≤3000字符, 最多3条旧记忆 |

### 3.1 Smart Ingest 完整11阶段

```
Stage 0: Session Storage
    └─ SessionStore::bulk_create() → SHA256去重 → 存储
    └─ Raw 模式在此终止

Stage 1: Message Selection
    └─ 取最后20条, ≤200KB

Stage 2: Pre-Filter (Meta Operations)
    └─ 20种正则模式过滤工具输出/构建日志/Git操作
    └─ <private>标签内容总是保留

Stage 3: Privacy Strip
    └─ <private>...</private> → [REDACTED]

Stage 4: Fully-Private Filter
    └─ 全部私密的消息直接丢弃
    └─ 全部消息都私密 → 终止慢路径

Stage 5: LLM Fact Extraction
    └─ FactExtractor::extract() → 最多15条事实
    └─ 置信度 < 3 丢弃, 质量评分 (0.1-1.0)
    └─ 9种元数据清理: System channel, 压缩标记, dcp标签等

Stage 6: Noise Filter
    └─ 38种正则模式 (问候/拒绝/诊断/感谢/系统日志)
    └─ 向量原型对比 (cosine ≥ 0.82 → 噪声)
    └─ 学习噪声向量累积 (最多200)

Stage 7: Admission Control (6维评分)
    └─ Utility(0.15) + Confidence(0.15) + Novelty(0.10)
       + Recency(0.10) + TypePrior(0.30) + SemanticQuality(0.20)
    └─ Balanced预设: admit≥0.65, reject<0.50

Stage 8: Privacy Tagging (后提取)
    └─ 14种正则检测: IP/密码/API Key/Token/SSH/数据库URL/邮箱/手机/信用卡/身份证
    └─ 设置 visibility=private, 添加"私密"标签

Stage 9: Reconciliation (最复杂)
    └─ 9a: gather_existing() → 向量+全文搜索 (最多50条)
    └─ 9b: batch_self_dedup() → LLM批量去重
    └─ 9c: exact_match_dedup() → hash + 子串 + Jaccard(0.6)
    └─ 9d: fast_session_merge() → 同session Jaccard(0.5)合并
    └─ 9e: LLM reconciliation → 7种决策 (见下表)
    └─ 9f: 执行决策 → LanceDB create/update/archive

Stage 10: Profile Induction
    └─ 触发 InductionEngine 后台任务
```

### 3.2 Reconciliation 7种决策

| 决策 | 动作 | Pinned保护 |
|------|------|-----------|
| **CREATE** | 创建新记忆 | — |
| **MERGE** | 替换已有记忆内容 | 降级为CREATE |
| **SKIP** | 丢弃事实 | — |
| **SUPERSEDE** | 归档旧记忆+创建新记忆 | 降级为CREATE |
| **SUPPORT** | 已有记忆 confidence+=0.1 | — |
| **CONTEXTUALIZE** | 创建新记忆+关联已有 | — |
| **CONTRADICT** | 项目类→SUPERSEDE; 其他→创建+矛盾关联 | — |

---

## 四、Retrieve Pipeline (15阶段)

```
Stage 1:  parallel_search    — 向量搜索 + BM25 并行 (tokio::join!)
Stage 2:  tag_boost          — 标签重叠加权 (如有 tags_filter)
Stage 3:  rrf_fusion         — 倒数排名融合 (3条腿加权)
Stage 4:  rrf_normalize      — Min-Max 归一化到 [0,1]
Stage 5:  min_score_filter   — 低于阈值丢弃 (默认0.15)
Stage 6:  topk_cap           — 截断到 limit × topk_multiplier
Stage 7:  expand_relations   — 展开关联记忆 (≤20, score=min×0.8)
Stage 8:  cross_encoder_rerank — 外部重排: 60%rerank + 40%pre-rerank
Stage 9:  bm25_floor         — 保护高BM25结果不被重排破坏
Stage 10: decay_boost        — Weibull衰减引擎调整分数
Stage 11: importance_weight  — score × (0.7 + 0.3 × importance)
Stage 12: length_normalization — score ÷ log₂(len/500+1), 偏好简洁
Stage 13: hard_cutoff        — 低于0.005丢弃
Stage 14: mmr_diversity      — Jaccard去重 (阈值0.85, 惩罚0.5)
Stage 15: llm_refine         — LLM判定 high/medium/irrelevant
```

### RRF 融合公式

```
rrf_score = (W_vector / W_total) × 1/(K+rank_v)
          + (W_bm25  / W_total) × 1/(K+rank_b)
          + (W_tag   / W_total) × 1/(K+rank_t)

默认: W_vector=0.7, W_bm25=0.3, W_tag=0.2, K=60
Pinned记忆 ×1.5 加成
```

### BM25 Floor 保护

原始 BM25 分数 ≥ 0.75 时, 重排后分数不低于 pre-rerank 分数的 95%。防止 cross-encoder 破坏强关键词匹配。

### MMR 多样性

CJK 文本用字符 bigram, 其他用空格分词。Jaccard > 0.85 时施加 penalty factor (0.5) 降权。

### LLM Refine 策略

- `"loose"` — 跳过LLM, 返回全部
- `"balanced"` — 保留 high+medium, 丢弃 irrelevant
- `"strict"` — 仅保留 high

评估上限 `llm_max_eval` (默认15), 超时15秒。

---

## 五、Should-Recall 5层门控

```
Layer 1: Query Sanitization
    └─ 清洗XML系统注入残留 → 空结果则跳过

Layer 2: Per-Session Rate Limiting
    └─ 全局HashMap追踪: 同session最小30秒间隔
    └─ 86400秒自动清理过期条目

Layer 3: Similarity Gate
    └─ 嵌入当前查询+上次查询 → cosine > 0.7 → 跳过 (防重复)

Layer 4: LLM Gate
    └─ 去噪查询(去HTML/代码块/截断200字符) → recall_llm
    └─ 返回 {should_recall, keywords[]}
    └─ LLM出错 → 回退为 should_recall=true (乐观策略)

Layer 5: Quality Gate (检索后)
    └─ 信号强度判定:
        STRONG: "之前"/"上次"/"remember" → 阈值0.35
        MEDIUM: "我喜欢"/"这个项目"/"deploy" → 阈值0.40
        WEAK: "怎么实现"/"refactor this" → 阈值0.42
        NONE: 通用问题 → 阈值0.48
```

### 两阶段搜索

1. **Phase 1**: 项目范围搜索 (project_tags filter)
2. **Phase 2**: 全局回退搜索 (limit × phase2_multiplier 补充)

两阶段通过 `HashSet<String>` 去重。

---

## 六、Profile V2 系统

### 三组件架构

```
InductionEngine (偏好提取)
    │
    ├─ LLM从记忆中提取偏好 → ProfileStore
    ├─ 10步流程: 锁获取→冷却检查→候选计数→LLM提取→验证→冲突解决→写入→快照→释放锁
    ├─ 冲突解决: 精确匹配 或 40%+关键词重叠 → confidence += 0.15 (封顶0.95)
    └─ 可用偏好槽位: 14个内置 + custom:* 自定义

ProfileStore (持久存储)
    └─ SQLite数据库, 每个tenant独立
    └─ 偏好字段: slot_name, slot_value, confidence, scope(global/project), status(active/dormant/deleted)

InjectionBuilder (缓存+注入)
    └─ DashMap缓存 (key=tenant:project_path, TTL=1800秒)
    └─ 获取 global(≤20) + project(≤10) 偏好
    └─ Token预算裁剪 (≤500字符)
    └─ 输出 <cerebro-profile>...</cerebro-profile> XML块
```

### 6.1 InductionEngine 偏好提取 (10步完整流程)

**源文件**: `profile_v2/induction.rs` (363行)

```
触发时机:
  1. session_ingest handler — 存储 memory 后 fire-and-forget
  2. POST /v2/profile/induction/trigger — 手动触发
  3. LifecycleScheduler — 周期性维护

10步流程:
  ┌─ Step 1: 启用检查 + 归纳锁 (SQLite mutex, TTL=600秒)
  ├─ Step 2: 冷却期检查 (默认600秒, 防止频繁触发)
  │   └─ get_induction_runs(tenant, 1) → 最近一次 run 的 elapsed < cooldown → skip
  ├─ Step 3: 获取锁 + 创建 InductionRun 记录
  │   └─ acquire_induction_lock(tenant, 600) + create_induction_run(...)
  ├─ Step 4: 候选不足检查 (默认阈值=5条)
  │   └─ candidate_texts.len() < threshold → skip, 释放锁
  ├─ Step 5: LLM归纳调用 (60秒超时)
  │   └─ system: INDUCTION_SYSTEM_PROMPT (偏好归纳引擎)
  │   └─ user: "以下是从用户行为中提取的N条记忆：\n...\n请从中提取用户偏好"
  │   └─ 返回: Vec<InductedPreference> (slot, value, confidence, scope)
  ├─ Step 6: 验证每个提取结果
  │   ├─ slot名称合法 (14内置 + custom:* 小写字母数字下划线)
  │   ├─ confidence ∈ [0.0, 1.0]
  │   ├─ scope ∈ {"project", "global"}
  │   └─ value 非空
  ├─ Step 7: 冲突解决 (与已有偏好对比)
  │   ├─ 精确匹配: existing.value == new.value → 强化
  │   ├─ 关键词重叠: extract_keywords() → 2-gram(CJK) + 3+字母英文
  │   │   └─ overlap / union > 0.4 → 强化
  │   └─ 强化: confidence += 0.15 (封顶 0.95)
  │   └─ 不匹配: 创建新偏好 (source="observed")
  ├─ Step 8: 写入偏好 (upsert_preference)
  │   └─ 每次写入都记录 ProfileChangelog (action: "reinforced"/"created")
  ├─ Step 9: 保存 version 快照
  │   └─ save_version(ProfileVersion { snapshot: JSON, preference_count })
  └─ Step 10: 释放锁 + 更新 run 状态 + invalidate_cache
```

### 6.2 偏好槽位定义

**源文件**: `profile_v2/slots.rs` (78行)

| 槽位名 | 显示名 | 多值 | 描述 |
|--------|--------|------|------|
| `communication_style` | 沟通风格 | 否 | 用户偏好的沟通方式 |
| `tone` | 语气偏好 | 否 | 用户偏好的语气 |
| `code_style` | 代码风格 | 否 | 用户偏好的代码编写风格 |
| `error_handling` | 错误处理 | 否 | 用户偏好的错误处理方式 |
| `naming_convention` | 命名规范 | 否 | 用户偏好的变量/函数命名风格 |
| `testing_strategy` | 测试策略 | 否 | 用户偏好的测试方法 |
| `workflow_preference` | 工作流偏好 | 否 | 用户偏好的开发工作流程 |
| `commit_style` | 提交风格 | 否 | 用户偏好的git commit风格 |
| `emoji_preference` | Emoji偏好 | 否 | 用户对emoji使用的偏好 |
| `self_reference` | 自称方式 | 否 | AI自称方式偏好 |
| `address_style` | 称呼方式 | 否 | AI称呼用户的方式偏好 |
| `language` | 语言 | **是** | 用户偏好的编程语言 |
| `framework_preference` | 框架偏好 | **是** | 用户偏好的开发框架 |
| `preferred_tools` | 工具偏好 | **是** | 用户偏好的开发工具 |
| `custom:*` | 自定义 | — | 格式: `custom:lowercase_digits_underscore` |

共 **14个内置槽位** (11单值 + 3多值) + 无限自定义槽位。

### 6.3 LLM归纳 Prompt (完整)

```
你是偏好归纳引擎。从用户的行为记忆中提取偏好。每条偏好对应一个slot和一个具体值。
仅从提供的记忆中提取，不编造。输出JSON数组。

可用slot: communication_style, tone, code_style, error_handling,
  naming_convention, testing_strategy, workflow_preference, commit_style,
  emoji_preference, self_reference, address_style, language,
  framework_preference, preferred_tools, custom:*（自定义slot格式）

输出格式:
[{"slot":"slot_name","value":"偏好描述","confidence":0.0到1.0,"scope":"project或global"}]

规则:
- confidence: 0.5-0.9（从单条记忆推断0.5-0.6，多条一致0.7-0.9）
- scope: 涉及特定项目用project，跨项目通用用global
- 每条记忆最多提取3条偏好
- 没有明确偏好的记忆跳过
- value必须在150字以内，同时保留关键细节（命令模板、文件路径、工具名、配置值等）
- 好的value: 「用PowerShell编译：powershell.exe -Command "& { ... }"」
- 差的value: 「使用PowerShell调用Maven编译」——太笼统无操作价值
- 去重：如果多条记忆指向同一偏好，合并为一条输出，取信息最完整的描述
```

### 6.4 关键词提取算法 (冲突检测用)

```rust
fn extract_keywords(text: &str) -> HashSet<String> {
    // CJK字符: 连续两个汉字组成 bigram
    //   例: "沟通风格" → {"沟通", "通风", "风格"}
    // 英文: 3+字母的单词转小写
    //   例: "PowerShell compile" → {"powershell", "compile"}
}
```

冲突判定: `overlap_count / union_count > 0.4` → 视为同一偏好 → 强化而非重复创建。

### Profile API 端点

| 端点 | 功能 |
|------|------|
| `GET /v2/profile/inject` | 构建注入块 (供plugin使用) |
| `POST /v2/profile/induction/trigger` | 手动触发偏好提取 |
| `GET /v2/profile/preferences` | 列出偏好 |
| `POST /v2/profile/preferences` | 创建偏好 (explicit, confidence=0.9) |
| `PUT /v2/profile/preferences/{id}` | 更新偏好 |
| `DELETE /v2/profile/preferences/{id}` | 软删除偏好 |

---

## 七、Lifecycle 管道

### 7.1 LifecycleScheduler

**调度模式**:
- `interval_secs > 0`: 固定间隔运行
- `interval_secs = 0` (默认): 每日午夜 Asia/Shanghai (UTC+8)

**执行顺序**:
```
对每个tenant:
    ├─ 1. Tier评估: 每100条批量处理
    └─ 2. 三轮遗忘: TTL → Superseded → Stale
    ├─ 3. Optimize所有LanceStore
    ├─ 4. Optimize session存储
    └─ 5. Profile维护:
        ├─ check_dormant_preferences (90天未强化 → dormant)
        ├─ cleanup_deleted_preference (180天dormant → 硬删除)
        └─ cleanup_expired_locks
```

### 7.2 Weibull 衰减模型

```
composite = max(
    w_recency × recency + w_frequency × frequency + w_intrinsic × intrinsic,
    floor(tier)
)

权重: recency=0.4, frequency=0.3, intrinsic=0.3

recency = exp(-λ × days^β)
  λ = ln(2) / (half_life × exp(importance_mod × importance))
  默认: half_life=30天, importance_mod=1.5
  β: Core=0.8 (慢衰), Working=1.0 (指数), Peripheral=1.3 (快衰)

frequency = (1 - exp(-access/5)) × (0.5 + 0.5 × recentness_bonus)
  recentness_bonus = exp(-avg_gap / 30)

intrinsic = importance × confidence

floor: Core=0.9, Working=0.7, Peripheral=0.5
stale: composite < 0.3
```

**Importance对半衰期的影响**: importance=0.9, mod=1.5 → `hl_eff = 30 × exp(1.35) ≈ 116天`

### 7.3 Tier 升降规则

```
Peripheral → Working:  access ≥ 3 AND composite ≥ 0.4
Working → Core:  access ≥ 10 AND composite ≥ 0.7 AND importance ≥ 0.8
Working → Peripheral:  composite < 0.15 OR (age > 60天 AND access < 3)
Core → Working:  composite < 0.15 AND access < 3
```

**关键设计**: Core → 只降到Working (不直接降到Peripheral), floor保证Core最低0.9分

### 7.4 三轮遗忘策略

| 轮次 | 策略 | 条件 | 动作 |
|------|------|------|------|
| Pass 1 | TTL到期 | "今天"→2天, "明天"→2天, "下周"→10天, "这个月"→35天 | **硬删除** |
| Pass 2 | 被取代归档 | `superseded_by`非空 且 >30天 | `state=Archived` |
| Pass 3 | 过时清理 | `composite<0.3` 且 非Pinned 且 `access<5` | **硬删除** (≤50条/轮) |

---

## 八、存储层

### 8.1 LanceDB 表结构

#### memories 表 (35列)

| 列 | 类型 | 用途 |
|----|------|------|
| `id` | Utf8 | UUID主键 |
| `content` | Utf8 | 原始记忆内容 |
| `l0_abstract` | Utf8 | Level-0: 短摘要/标题 |
| `l1_overview` | Utf8 | Level-1: 中等摘要 |
| `l2_content` | Utf8 | Level-2: 完整内容 |
| `vector` | FixedSizeList(Float32, 1024) | 嵌入向量 |
| `category` | Utf8 | 分类: preferences/events/cases等 |
| `memory_type` | Utf8 | Insight/Session/Pinned/Fact/Procedure |
| `state` | Utf8 | Active/Archived/Deleted |
| `tier` | Utf8 | Core/Working/Peripheral |
| `importance` | Float32 | 0.0-1.0 重要性 |
| `confidence` | Float32 | 0.0-1.0 置信度 |
| `access_count` | Int32 | 访问次数 |
| `tags` | Utf8 (JSON) | 标签数组序列化 |
| `scope` | Utf8 | global/project/private |
| `session_id` | Utf8 | 绑定session |
| `project_path` | Utf8 | 项目隔离路径 |
| `relations` | Utf8 (JSON) | 记忆关联数组 |
| `superseded_by` | Utf8 | 替代记忆ID |
| `cluster_id` | Utf8 | 聚类分配 |
| `visibility` | Utf8 | global/private/shared:X |
| `space_id` | Utf8 | 所属空间 |
| `version` | UInt64 | 自动递增版本号 |

### 8.2 索引策略

| 类型 | 列 | 用途 |
|------|-----|------|
| BTree | id, cluster_id, created_at, updated_at | 精确查找+范围查询 |
| Bitmap | state, category, tier | 低基数码过滤 |
| FTS (ngram 2-4) | content, l0_abstract | CJK全文搜索 |
| IVF-HNSW-SQ | vector | 向量近似搜索 (≥10万行才创建) |

### 8.3 GC 管道 (after_mutation)

```
每30个LanceDB版本触发 (AtomicBool防并发):
    ├─ Prune: 10分钟安全窗口, delete_unverified=true
    ├─ Compact: 合并小碎片文件
    ├─ Index Merge: threshold=128, 合并新数据到索引
    ├─ Recall表 GC: parallel prune + compact (recall_events + recall_items)
    └─ 孤儿索引清理: 对比manifest UUID vs 磁盘目录, 删除不存在的
```

### 8.4 StoreManager LRU 缓存

- 最大20个 `LanceStore` 实例缓存
- 淘汰策略: 最久未访问 (`last_accessed` 时间戳)
- 多空间权重: Personal=1.0, Team=0.8, Organization=0.6

---

## 九、完整端到端数据流

```
[用户消息]
    │
    ▼
chat.message (keywordDetectionHook)
    │ 收集消息到 sessionMessages[] + 检测保存关键词
    │
    ▼
system.transform (autoRecallHook)
    │ Profile TTL检查 → GET /v2/profile/inject → 缓存
    │ POST /v1/should-recall → 5层门控
    │   ├─ Query Sanitization → Rate Limit → Similarity Gate
    │   ├─ LLM Gate (recall_llm)
    │   └─ Quality Gate (信号强度 → 动态阈值)
    │ 向量+BM25并行搜索 → 15阶段检索管道 → 结果
    │ 构建 <cerebro-context> + <cerebro-profile>
    │ output.system[last] += 注入内容
    │
    ▼
[LLM 生成响应]
    │
    ▼
session.idle (sessionIdleHook)
    │ 10秒防抖 → SDK采集新消息
    │ POST /v1/memories/session-ingest (60s超时)
    │
    ▼
IngestPipeline (11阶段)
    │ SessionStorage → MessageSelection → PreFilter
    │ → PrivacyStrip → FullyPrivateFilter
    │ → LLM提取(≤15条) → NoiseFilter(38正则+向量原型)
    │ → AdmissionControl(6维评分) → PrivacyTagging
    │ → Reconciliation(7决策) → ProfileInduction
    │ 存入LanceDB → after_mutation GC (每30版本)
    │
    ▼
LifecycleScheduler (每日午夜)
    │ Weibull衰减评估 → Tier升降 → 三轮遗忘
    │ GC: Prune → Compact → Index Merge → 孤儿清理
    │ Profile维护: dormant检测 → 清理过期锁
    │
    ▼
[记忆被遗忘或被提升为Core]
```

---

## 十、Client API 调用参考

| Plugin 方法 | 服务端端点 | 超时 | 调用方 |
|---|---|---|---|
| `getInjection(path)` | `GET /v2/profile/inject?project_path=X` | 15s | autoRecallHook |
| `shouldRecall(...)` | `POST /v1/should-recall` | 20s | autoRecallHook |
| `createRecallEvent(...)` | `POST /v1/recall-events` | 10s | autoRecallHook |
| `searchMemories("*", 20)` | `GET /v1/memories/search?q=*&limit=20` | 20s | compactingHook |
| `ingestMessages(msgs, opts)` | `POST /v1/memories` (with messages) | 15s | compacting/autocontinue |
| `sessionIngest(msgs, sid, ...)` | `POST /v1/memories/session-ingest` | 60s | sessionIdleHook |

---

## 十一、已知架构问题

1. **注入策略**: `system[last] +=` 方式导致第一轮注入被LLM注意力忽略
2. **检索管道过长**: 15阶段链路, 部分阶段可合并
3. **Profile Induction**: fire-and-forget 可能跟批量 ingestion 撞车
4. **Session Lock Leak**: DashMap 锁永不自动清理 (仅scheduler每60秒清理24h过期的)
5. **Background spawn 无信号量限制**: pipeline慢路径的 `tokio::spawn` 无并发上限
6. **shareing.rs/memory.rs 过大**: 分别2072/1853行, 需要拆分
