# 记忆召回增强 & Profile刷新 实施方案（v2 修正版）

> **For agentic workers:** REQUIRED SUB-SKILL: Use omem-iteration skill 进行开发。所有子agent委派必须 `run_in_background=true`。

**目标：** 实现完整的召回增强（标签加分 + relation扩展 + LLM精炼）+ profile TTL刷新，让召回更精准、更完整、画像更及时。

**架构：** 三个独立子系统 — (A) should_recall handler增强 + retrieve pipeline扩展 (B) plugin端 profile TTL刷新 (C) plugin传递对话上下文给服务端

---

## ⚠️ v2 修正记录（对比v1）

| # | v1问题 | v2修正 | 严重度 |
|---|--------|--------|--------|
| 1 | Handler文件写成 `api/handlers/memory.rs` | 实际是 **`api/handlers/session_recalls.rs`** | 🔴 P0 |
| 2 | 假设 should_recall 用 `RetrievalPipeline` | **实际没用！** handler直接调 `store.vector_search` + `store.fts_search`，手写两阶段搜索 | 🔴 P0 |
| 3 | Pipeline阶段写成6步 | 实际已有 **12个stage**，需精确定位插入位置 | 🟡 P1 |
| 4 | LLM精炼与cross-encoder rerank关系不明 | 明确：**两者叠加**。cross-encoder调权重，LLM精炼判断相关性+压缩 | 🟡 P1 |
| 5 | p95 < 200ms 约束不现实 | 改为：**LLM精炼超时5s跳过，整体p95不回退超过原有+2s** | 🟡 P1 |
| 6 | 执行顺序 A→B→C | 改为 **B→C→A**（C是A的前置依赖） | 🟡 P1 |
| 7 | 缺少 `SearchRequest` 改动 | 新增：`SearchRequest` 加 `tag_weight`、`conversation_context` 字段 | 🟡 P1 |

---

## 完整召回架构（师尊2026-05-10讨论确认）

```
用户消息 → plugin提取关键词tags + 最近2-3轮对话上下文
→ 服务端 /v1/should-recall：
  ① 向量搜索（语义匹配）
  ② BM25全文（关键词匹配）
  ③ 标签匹配加分（精准加权）← 新增
  ④ RRF三路融合（向量 + BM25 + 标签）← 改造
  ⑤ relation深度关联拉取（解决分割记忆问题）← 新增
  ⑥ LLM精炼（带对话上下文，精准判断相关性）← 新增
→ 返回精炼结果给plugin
→ 注入system prompt（完整content，不做l1/l2压缩）
```

### 关键决策（师尊确认）
- 注入content（完整内容）而非l1/l2压缩版，对AI价值最大
- 需要精准筛选减少噪音，不能全量注入
- 标签匹配做加分不做过滤（避免漏召回）
- plugin给服务端传最近2-3轮对话上下文，让LLM精准判断
- 高相关保留完整content，中相关用l1概述，无关丢弃
- 结果质量标准：相关、完整（relation合并）、不矛盾、有reasoning
- **cross-encoder rerank 调权重排序，LLM精炼做最终相关性判断+内容裁剪，两者叠加不互斥**

---

## 文件结构（修正后）

| 文件 | 变更 | 所属计划 |
|------|------|----------|
| `omem-server/src/api/handlers/session_recalls.rs` | ③标签加分 ⑤relation扩展 ⑥LLM精炼 | A |
| `omem-server/src/retrieve/pipeline.rs` | 新增3个stage + 三路RRF | A |
| `omem-server/src/retrieve/trace.rs` | 新增trace阶段（无需改结构） | A |
| `omem-server/src/store/lancedb.rs` | `get_memories_by_ids`（L1919已验证存在） | A（只读） |
| `omem-server/src/domain/memory.rs` | 只读：`Memory.tags`、`Memory.relations` | A（只读） |
| `omem-server/src/domain/relation.rs` | 只读：`MemoryRelation`、`RelationType` | A（只读） |
| `omem-server/src/retrieve/prompts.rs` | 新建：LLM精炼prompt模板 | A（新建） |
| `plugins/opencode/src/hooks.ts` | TTL刷新 + 传递对话上下文 | B+C |
| `plugins/opencode/src/client.ts` | shouldRecall增加 `conversationContext` + `tags` 参数 | C |

---

### 现有 should_recall LLM门控层分析（师尊反馈2）

当前 `session_recalls.rs` 的 `should_recall` handler 有 **3层前置过滤**：

```
请求进入
→ ① Rate limiting（per-session间隔控制）
→ ② Similarity check（与上次query余弦相似度 >0.7 则跳过，避免重复召回）
→ ③ LLM yes/no 门控（SHOULD_RECALL_SYSTEM_PROMPT，判断是否需要搜记忆）
   → yes: 执行向量+BM25搜索
   → no: 直接返回空
```

**本版改造后的分析**：

| 层级 | 作用 | 改造后是否保留 | 理由 |
|------|------|--------------|------|
| ① Rate limiting | 防止频繁调用 | ✅ 保留 | 与召回质量无关，纯成本控制 |
| ② Similarity check | 防止相似query重复召回 | ✅ 保留 | 避免连续对话重复触发，节省embed+搜索开销 |
| ③ LLM yes/no 门控 | 粗筛"要不要搜" | ✅ 保留 | **关键**：如果no，直接跳过向量搜索+BM25+LLM精炼，节省大量资源。LLM精炼在搜索**之后**，无法替代搜索**之前**的门控。去掉会导致每次对话都触发完整搜索pipeline，成本剧增 |

**结论**：三层前置过滤全部保留。LLM yes/no 是"要不要搜"（粗筛，省搜索成本），LLM精炼是"搜到了哪些有用"（细筛，提结果质量）。两者互补不互斥。

**修改原则**：
- `SHOULD_RECALL_SYSTEM_PROMPT`（session_recalls.rs:19）**不修改**
- LLM yes/no 判断逻辑（L205-216）**不修改**
- 只有 LLM 判定 yes 之后，才走新增强的 pipeline（tag boost → relation expand → LLM refine）

---

## 执行顺序（修正后）

**B → C → A**（由简到难，C是A的前置依赖）

1. **B** 先做：profile TTL（独立快速，10分钟出活）
2. **C** 再做：plugin传上下文（简单，且A需要 `conversation_context` 参数）
3. **A** 最后做：服务端增强（最复杂，需要B+C的上下文熟悉度）

### 评审规则（师尊2026-05-11确认）
- 每完成一个任务（B/C/A），必须调用玄机(oracle)评审
- 评审不通过 → 修复 → 再次评审 → 通过后才进入下一个任务
- 评审重点：代码质量、回归风险、prompt边界、测试覆盖

---

## 计划A：服务端召回增强

### 背景（修正）

**⚠️ 关键发现：`should_recall` handler 并未使用 `RetrievalPipeline`！**

当前 `session_recalls.rs:127-401` 的 `should_recall` handler 直接调用：
- `store.vector_search()` — 向量搜索
- `store.fts_search()` — BM25全文搜索
- 手写两阶段搜索（project-first, global fallback）
- 无 RRF 融合、无 rerank、无 tag boost、无 relation 扩展

而 `retrieve/pipeline.rs` 的 `RetrievalPipeline` 已有完整12阶段pipeline，但 `should_recall` 没用它。

**两种实现路径：**
- **路径1（推荐）**：让 `should_recall` 改用 `RetrievalPipeline`，在pipeline中添加新stage
- **路径2**：在 `should_recall` handler中直接实现3个新阶段，不动pipeline

**v2选择路径1**：将 should_recall 重构为使用 RetrievalPipeline，在pipeline中扩展新stage。理由：pipeline有完整trace、测试框架、score规范化，比handler内手写更可靠。

### 当前 Pipeline 12阶段（pipeline.rs L122-173）

```
① stage_parallel_search (L189)    — 并行 vector + BM25
② stage_rrf_fusion (L258)         — 2路RRF融合（vector_weight=0.7, bm25_weight=0.3）
③ stage_rrf_normalize (L318)      — 归一化到[0,1]
④ stage_min_score_filter (L360)    — 低于阈值丢弃
⑤ stage_topk_cap (L392)           — 截断到 limit*2
⑥ stage_cross_encoder_rerank (L427) — 外部reranker（可选）
⑦ stage_bm25_floor (L467)         — 高BM25结果保底
⑧ stage_decay_boost (L494)        — Weibull时间衰减
⑨ stage_importance_weight (L518)  — importance字段加权
⑩ stage_length_normalization (L540) — 长内容惩罚
⑪ stage_hard_cutoff (L575)        — 绝对分数截止
⑫ stage_mmr_diversity (L607)      — Jaccard多样性 + topK
```

### 新增3阶段插入位置

```
① stage_parallel_search
→ 【新增】② stage_tag_boost（标签匹配加分）
→ ③ stage_rrf_fusion（改造为3路：vector + BM25 + tag）
→ ④ stage_rrf_normalize
→ ⑤ stage_min_score_filter
→ ⑥ stage_topk_cap
→ 【新增】⑦ stage_expand_relations（关联记忆拉取）
→ ⑧ stage_cross_encoder_rerank
→ ⑨ stage_bm25_floor
→ ⑩ stage_decay_boost
→ ⑪ stage_importance_weight
→ ⑫ stage_length_normalization
→ ⑬ stage_hard_cutoff
→ ⑭ stage_mmr_diversity
→ 【新增】⑮ stage_llm_refine（LLM精炼，最终返回前）
```

### 任务

#### A0：重构 should_recall 使用 RetrievalPipeline（新增任务）
- [ ] A0-1：在 `AppState` 中确认 `RetrievalPipeline` 是否已实例化（检查 `api/server.rs` 的 `AppState` 字段）
- [ ] A0-2：如果没有，在 `AppState` 中添加 `pipeline: Arc<RetrievalPipeline>` 字段
- [ ] A0-3：在 `main.rs` 或 `server.rs` 的初始化流程中构造 `RetrievalPipeline`
- [ ] A0-4：重构 `should_recall` handler，替换手写搜索为 `pipeline.search(request)` 调用
- [ ] A0-5：保持 project_tags 两阶段搜索语义（project-first, global fallback）— 在 `SearchRequest` 中用 `tags_filter` 字段传递
- [ ] A0-6：`cargo test` 验证重构不破坏现有召回行为

#### A1：确认数据模型（只读验证）
- [ ] A1-1：确认 `Memory.tags: Vec<String>` (memory.rs L43) ✅ 已验证
- [ ] A1-2：确认 `Memory.relations: Vec<MemoryRelation>` (memory.rs L49) ✅ 已验证
- [ ] A1-3：确认 `MemoryRelation { relation_type, target_id, context_label }` (relation.rs L6-10) ✅ 已验证
- [ ] A1-4：确认 `RelationType` 有6变体：Supersedes/Contextualizes/Supports/Contradicts/Continues/ContinuedBy ✅ 已验证
- [ ] A1-5：确认 `get_memories_by_ids` (lancedb.rs L1919) ✅ 已验证

#### A2：实现 stage_tag_boost
- [ ] A2-1：在 `SearchRequest` 中确认 `tags_filter: Option<Vec<String>>` ✅ 已存在 (pipeline.rs L23)
- [ ] A2-2：新增 `tag_weight: f32` 字段到 `RetrievalPipeline`（默认0.2），添加 builder 方法
- [ ] A2-3：实现 `stage_tag_boost` 函数：
  1. 如果 `request.tags_filter` 有值，遍历候选记忆
  2. 计算标签重叠度：`overlap_count / request_tags_count`
  3. 生成第三路排序信号（tag rank list）
  4. 返回 `TagBoostResults`（包含 tag rank 信息）
- [ ] A2-4：添加 `StageTrace` 条目

#### A3：改造 RRF 融合为三路
- [ ] A3-1：修改 `stage_rrf_fusion` 签名，接收 `TagBoostResults` 作为第三路输入
- [ ] A3-2：在 fusion 循环中增加 tag 路径：`tag_weight / (rrf_k + tag_rank + 1)`
- [ ] A3-3：更新 `ParallelResults` 结构或新增 `ThreeWayResults` 包含 tag 路数据
- [ ] A3-4：保持 `vector_weight=0.7, bm25_weight=0.3` 默认不变，新增 `tag_weight=0.2`（三路权重归一化）

#### A4：实现 stage_expand_relations
- [ ] A4-1：在 `stage_topk_cap` 之后、`stage_cross_encoder_rerank` 之前插入
- [ ] A4-2：从当前 top-N 结果收集所有 `memory.relations` 中的 `target_id`
- [ ] A4-3：用 `store.get_memories_by_ids(ids)` (L1919) 批量拉取
- [ ] A4-4：去重：排除已存在于结果中的记忆 ID
- [ ] A4-5：追加到候选列表，score 设为当前最低分的 0.8（让后续rerank重新打分）
- [ ] A4-6：上限：每次查询最多20条关联记忆
- [ ] A4-7：添加 `StageTrace` 条目

#### A5：实现 stage_llm_refine

**调用方式**（参照现有 `recall_llm` + `complete_json` 模式）：
- 使用 `state.recall_llm`（独立LLM实例，已有 `OMEM_RECALL_LLM_*` 配置）
- 调用 `complete_json::<RefineResponse>(llm, system, user)` — 自动 JSON repair + retry
- **不使用** `complete_text`（需要结构化JSON输出）
- **不修改** 现有 `SHOULD_RECALL_SYSTEM_PROMPT`（session_recalls.rs:19），新prompt完全独立
- `RetrievalPipeline` 需注入 `Arc<dyn LlmService>`（构造时传入 `recall_llm`）

**超时兜底**：用 `tokio::time::timeout(Duration::from_secs(5), ...)` 包裹，超时则跳过精炼直接返回原始结果

- [ ] A5-1：在 `stage_mmr_diversity` 之后，最终返回之前
- [ ] A5-2：接收参数：候选记忆列表 + 用户查询 + `conversation_context`（来自C计划）
- [ ] A5-3：构造 prompt（调用 `retrieve/prompts.rs` 中的函数，详见A6）
- [ ] A5-4：调用 `complete_json::<RefineResponse>` 获取结构化结果
- [ ] A5-5：高相关：保留完整 content
- [ ] A5-6：中相关：只保留 `l1_overview`（如无概述，保留content前200字）
- [ ] A5-7：无关：丢弃
- [ ] A5-8：超时5秒兜底：超时跳过精炼直接返回原始结果
- [ ] A5-9：添加 `StageTrace` 条目
- [ ] A5-10：`RetrievalPipeline` 新增 `llm: Arc<dyn LlmService>` 字段，通过 `with_llm()` builder 注入

#### A6：新建 retrieve/prompts.rs（⚠️ 师尊重点审查）

**隔离原则**：
- 新文件 `omem-server/src/retrieve/prompts.rs`，不碰 `ingest/prompts.rs`
- 不修改 `session_recalls.rs` 中的 `SHOULD_RECALL_SYSTEM_PROMPT`
- 不修改任何已有 prompt 常量

**Prompt 设计（完整定义）**：

```rust
// === retrieve/prompts.rs ===

use serde::{Deserialize, Serialize};

/// LLM精炼返回的单条记忆评判结果
#[derive(Deserialize)]
pub struct RefineItem {
    /// 记忆ID（必须与输入中的id匹配）
    pub id: String,
    /// 相关性等级：high / medium / irrelevant
    pub relevance: String,
    /// 判断理由（简短一句话）
    pub reasoning: String,
}

/// LLM精炼返回的整体结构
#[derive(Deserialize)]
pub struct RefineResponse {
    pub items: Vec<RefineItem>,
}

const REFINE_SYSTEM_PROMPT: &str = r#"You are a memory relevance judge. Given a user's current conversation context and a list of candidate memories, judge each memory's relevance to the conversation.

## Your Task
For each memory, output exactly one relevance level with a brief reasoning.

## Relevance Levels
- **high**: Directly answers or closely relates to the user's current question/conversation. Contains specific facts, code, config, or context the user would need.
- **medium**: Tangentially related or provides useful background context, but not directly needed for the current question.
- **irrelevant**: No meaningful connection to the current conversation.

## Rules
1. Judge ONLY based on the user's current question and conversation context provided.
2. Do NOT judge based on general topic similarity — be specific about whether the memory helps answer THIS question.
3. Preserve the user's original language — respond in the same language as the user's question.
4. Keep reasoning to ONE short sentence.
5. Output valid JSON only, no markdown fences.

## Output Format
{"items": [{"id": "<memory_id>", "relevance": "high|medium|irrelevant", "reasoning": "<one sentence>"}]}
"#;

/// 构造精炼 user prompt
pub fn build_refine_user_prompt(
    memories: &[(String, &str, Option<&str>)], // (id, content, l1_overview)
    query: &str,
    conversation_context: Option<&[String]>,
) -> String {
    let mut prompt = String::with_capacity(2048);

    prompt.push_str("## User's Current Question\n");
    prompt.push_str(query);
    prompt.push('\n');

    if let Some(ctx) = conversation_context {
        if !ctx.is_empty() {
            prompt.push_str("\n## Recent Conversation Context\n");
            for msg in ctx {
                prompt.push_str("- ");
                prompt.push_str(msg);
                prompt.push('\n');
            }
        }
    }

    prompt.push_str("\n## Candidate Memories\n");
    for (id, content, l1) in memories {
        prompt.push_str(&format!("[id:{}] ", id));
        if let Some(overview) = l1 {
            // 提供概述+截断content避免过长
            prompt.push_str(&format!("(overview: {}) ", overview));
            let truncated = if content.len() > 300 { &content[..300] } else { content };
            prompt.push_str(truncated);
        } else {
            let truncated = if content.len() > 500 { &content[..500] } else { content };
            prompt.push_str(truncated);
        }
        prompt.push('\n');
    }

    prompt.push_str("\nJudge each memory's relevance. Output JSON only.");
    prompt
}
```

**边界规则**：
- 记忆content截断：有l1_overview时content截300字，无l1时截500字（控制prompt长度）
- conversation_context 最多3条，每条截200字
- 总prompt长度控制在 4096 token 以内（超过则截断记忆条数）
- 一次最多评判15条记忆（MMR diversity后的结果通常 <= limit）

- [ ] A6-1：创建 `omem-server/src/retrieve/prompts.rs`
- [ ] A6-2：定义 `RefineResponse` + `RefineItem` 结构（Deserialize）
- [ ] A6-3：定义 `REFINE_SYSTEM_PROMPT` 常量（完整如上）
- [ ] A6-4：实现 `build_refine_user_prompt()` 函数（完整如上）
- [ ] A6-5：在 `retrieve/mod.rs` 中 `pub mod prompts;` 导出
- [ ] A6-6：单元测试：prompt构造 — 验证输出格式、截断逻辑、空context处理

#### A7：修改 ShouldRecallRequest
- [ ] A7-1：在 `session_recalls.rs` 的 `ShouldRecallRequest` 中新增：
  ```rust
  pub conversation_context: Option<Vec<String>>,  // 最近2-3轮对话
  ```
  注意：`tags` 参数已存在（`project_tags: Option<Vec<String>>`），无需新增
- [ ] A7-2：将 `conversation_context` 传递到 `SearchRequest` 或直接到 `stage_llm_refine`

#### A8：Trace 支持
- [ ] A8-1：3个新阶段各添加 `StageTrace` 条目（结构无需修改，只需填充）

#### A9：测试
- [ ] A9-1：`stage_tag_boost` 单元测试：有tags / 无tags / 空tags
- [ ] A9-2：`stage_expand_relations` 单元测试：有relations / 无relations / 超上限
- [ ] A9-3：`stage_llm_refine` 单元测试：mock `NoopLlm` + 手写JSON响应，测试 high/medium/irrelevant 分流 + 超时跳过
- [ ] A9-4：`stage_rrf_fusion` 三路测试：验证权重正确、三路归一化
- [ ] A9-5：`retrieve/prompts.rs` 单元测试：prompt构造输出格式、截断逻辑、空context
- [ ] A9-6：集成测试：should_recall 端到端（需mock embed + llm）
- [ ] A9-7：回归测试：`SHOULD_RECALL_SYSTEM_PROMPT` 不被修改，原有 should_recall yes/no 判断不受影响

#### A10：验证
- [ ] A10-1：`cargo build` 通过
- [ ] A10-2：`cargo test` 通过
- [ ] A10-3：`cargo clippy` 无新增 warning

#### A11：提交
- [ ] `feat(retrieve): enhance recall with tag boost, relation expansion, LLM refine`

### 约束

- 关联扩展上限：每次查询最多20条
- LLM精炼超时：5秒，超时跳过精炼直接返回
- 标签加分权重：默认0.2，可通过 `with_tag_weight()` builder 配置
- **延迟约束（修正）**：LLM精炼超时5s兜底，整体p95不回退超过 **原有+2秒**
- relation扩展追加的 score = 当前最低分 × 0.8（让rerank重排）
- 三路RRF权重归一化：`vector_w + bm25_w + tag_w` 归一化后使用
- **LLM调用隔离**：
  - 使用 `recall_llm`（独立实例），不占用 `primary_llm`
  - 调用 `complete_json::<RefineResponse>`，不用 `complete_text`
  - prompt 定义在 `retrieve/prompts.rs`，不碰 `ingest/prompts.rs`
  - 不修改 `session_recalls.rs:19` 的 `SHOULD_RECALL_SYSTEM_PROMPT`
  - 精炼prompt中记忆content截断（有l1截300字/无l1截500字），控制总token < 4096
  - 一次最多评判15条记忆
- **回归防护**：现有 should_recall 的 yes/no LLM判断、两阶段搜索、project_tags 逻辑必须保持不变

---

## 计划B：Profile TTL 刷新

### 背景

- 当前行为：`profileInjectedSessions` 是 `Set<string>`（hooks.ts L138）
- sessionID 一旦加入，profile 永不重新注入
- Session结束清理（L498/L545）清 `sessionMessages` 但 **不清** `profileInjectedSessions`
- Profile 内容随记忆摄入而变化，但 plugin 端看不到更新
- 目标：TTL（5分钟）过期后重新拉取并注入 profile

### 任务

- [ ] **B1：Set 改 Map** — `profileInjectedSessions` 从 `Set<string>` 改为 `Map<string, number>`，value是上次注入的时间戳
- [ ] **B2：TTL 检查逻辑** — `autoRecallHook` 中（L292附近），把 `!profileInjectedSessions.has(sessionID)` 改为：
  ```typescript
  const lastInjected = profileInjectedSessions.get(input.sessionID);
  const ttlExpired = !lastInjected || (Date.now() - lastInjected > 5 * 60 * 1000);
  ```
  TTL 过期则重新拉取 profile 并注入，更新时间戳
- [ ] **B3：去重 profile block** — 重新注入时，**按数组元素匹配**（明镜P1）：遍历 `output.system`，找到包含 `<cerebro-profile>` 标签（L294确认格式）的那个元素，替换为新内容。**禁止全局正则替换**
- [ ] **B4：调试日志** — 首次注入 vs TTL刷新 分别打不同日志
- [ ] **B5：Toast 控制** — 首次注入显示 toast（L310），TTL刷新不显示（避免刷屏）
- [ ] **B6：`npx tsc --noEmit` 验证**
- [ ] **B7：提交** — `feat(plugin): TTL-based profile refresh`

### 约束

- TTL 默认 5 分钟，可通过 plugin config 配置
- profile 拉取失败时用缓存版本，不阻塞召回
- 只首次注入显示 toast

---

## 计划C：Plugin传递对话上下文

### 背景

LLM精炼（A5）需要对话上下文来精准判断记忆相关性。当前 `shouldRecall` 只传 `query_text` + `last_query_text`（client.ts L321-338），不传对话历史。

### 当前 shouldRecall 签名（client.ts L321）

```typescript
async shouldRecall(
  query_text: string,
  last_query_text: string | undefined,
  session_id: string,
  similarity_threshold?: number,
  max_results?: number,
  project_tags?: string[],
): Promise<ShouldRecallResponse | null>
```

### 任务

- [ ] **C1：client.ts 增加 `conversationContext` 参数** — 新增可选参数 `conversationContext?: string[]`（最近2-3轮对话），序列化到请求体
- [ ] **C2：hooks.ts 提取对话上下文** — 在 `autoRecallHook` 中（L273附近），从 `sessionMessages` Map 提取最近2-3条用户消息作为上下文，传给 `shouldRecall`
- [ ] **C3：hooks.ts 提取关键词tags** — 从当前用户消息提取关键词，作为 `tags` 参数传给 `shouldRecall`（注意：`project_tags` 已存在，这里指的是用户消息关键词，可能需要新增参数或复用 `project_tags`）
- [ ] **C4：`npx tsc --noEmit` 验证**
- [ ] **C5：提交** — `feat(plugin): pass conversation context and tags to recall API`

### 约束

- 上下文最多3轮对话，超出截断
- 关键词提取用简单分词（空格/标点分割 + 过滤停用词），不上LLM（避免延迟）

---

## 评审记录

### 🦅 玄机（架构）P0
- relation expansion 放在 topk_cap 之后、rerank 之前（已采纳）
- profile TTL 客户端轮询方案正确（已确认）

### 🪞 明镜（代码质量）
- P1: profile去重必须按数组元素匹配（已写入B3）
- P2: `get_memories_by_ids` 已存在（A4改为验证）

### 💡 灵犀（需求分析）
- 外包给探虚，已斩 💀

### 📋 v2 修正（月儿评审）
- P0: should_recall 未使用 RetrievalPipeline，需重构（新增A0任务）
- P0: handler文件路径修正为 session_recalls.rs
- P1: LLM精炼 vs cross-encoder 关系明确：叠加
- P1: 延迟约束修正为 p95 不回退超过原有+2秒
- P1: 执行顺序改为 B→C→A

---

## 无人值守规则
- 执行过程中**禁止向师尊提问**，遇到歧义自行决策并记录
- 所有子agent委派必须 `run_in_background=true`
- 遇到编译错误自行修复，不阻塞
- 每个计划完成后独立commit，不等待师尊确认

---

## 部署检查清单

完成后：

- [ ] `cargo build --release` 通过
- [ ] `npx tsc --noEmit` 通过
- [ ] 部署服务端二进制到生产
- [ ] plugin `npm version patch && npm publish`
- [ ] plugin日志验证：profile TTL刷新正常
- [ ] 服务端日志验证：扩展pipeline trace 完整输出
- [ ] 监控 p95 召回延迟 — 不应回退超过原有+2秒
