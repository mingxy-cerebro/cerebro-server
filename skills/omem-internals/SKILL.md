---
name: omem-internals
version: 1.0.0
description: |
  Cerebro (omem-server) 内部架构知识库——agent迭代时自动加载，避免重新摸索代码。

  触发场景：
  - 修改 ingest pipeline / session_ingest / reconciler / refine
  - 修改 retrieve pipeline / 检索逻辑
  - 修改 profile_v2 偏好系统
  - 修改 lifecycle tier / decay / scheduler
  - 修改 L0/L1/L2 字段使用
  - 修改 split / create_continuation / relations 链
  - 修改任何 LLM prompt（SESSION_EXTRACT / REFINE / INDUCTION）
  - 部署 omem-server / 排查生产问题
  - "omem 迭代" / "omem 改" / "记忆系统" / "ingest" / "retrieve" / "refine" / "split" / "tier" / "profile"

  覆盖：写入路径×2、检索11阶段、7-decision reconcile、偏好归纳、L0/L1/L2语义、
  split触发点、配置项、服务端部署、已知问题。
keywords:
  - omem
  - cerebro
  - memory-system
  - ingest-pipeline
  - retrieve-pipeline
  - session-ingest
  - reconciler
  - refine
  - profile-v2
  - tier-lifecycle
---

# Cerebro 内部架构

> Apache-2.0 共享持久记忆系统。Rust (axum 0.8 + lancedb 0.27) + TypeScript 插件。
> 本文档覆盖**写入+检索+生命周期+偏好**四条核心链路的内部实现，供 agent 迭代时直接引用，不重新摸索。

## 服务端部署

| 项 | 值 |
|----|-----|
| URL | `https://www.mengxy.cc` |
| 服务器 | `root@47.93.199.242:/opt/omem/omem-server` |
| 服务 | `systemctl restart omem` |
| 健康检查 | `GET /health` → 200 |
| 手动触发 lifecycle | `POST /v1/lifecycle/trigger` |
| 当前版本 | v0.3.2 release |
| 部署流程 | `cargo build --release` → `scp` binary → `systemctl restart omem` |

## 关键配置（.env.deploy + config.rs）

| 配置 | 值 | 说明 |
|------|-----|------|
| `OMEM_LLM_MODEL` | `big-pickle` via opencode.ai/zen | 主 LLM（提取/精炼/reconcile）|
| `OMEM_RECALL_LLM_MODEL` | `Qwen/Qwen3-8B` via 硅基流动 | recall 判断 |
| `OMEM_EMBED_MODEL` | `Qwen/Qwen3-Embedding-0.6B` via 硅基流动 | 向量化 |
| `profile_induction_threshold` | 5 | 候选<5跳过归纳 |
| `profile_induction_cooldown_secs` | 600 | 归纳冷却 |
| `profile_cache_ttl_secs` | 1800 | injection 缓存 |
| `profile_injection_budget_tokens` | 3000 | 注入字符上限 |
| `profile_max_global_preferences` | 20 | 全局偏好上限 |
| `profile_max_project_preferences` | 10 | 项目偏好上限 |

---

## 5 个 LLM Prompt（位置+用途）

| Prompt | 位置 | 用途 |
|--------|------|------|
| **SESSION_EXTRACT** | `omem-server/src/ingest/prompts.rs:746-848` | conversation → topics[] (EMOTIONAL/WORK) |
| **REFINE (ingest)** | `omem-server/src/ingest/refine_prompt.rs:16-32` | 合并已存在 WORK 记忆 |
| **REFINE (retrieve)** | `omem-server/src/retrieve/prompts.rs:15-35` | 检索时判断相关性（**完全不同 prompt**）|
| **INDUCTION** | `omem-server/src/profile_v2/induction.rs:15-53` | 记忆 → 偏好 slot |
| **INJECTION** | `omem-server/src/profile_v2/injection.rs:34-142`（**无 LLM**）| 偏好 → agent context |

---

## 两条独立写入路径

### 路径 A: IngestPipeline.ingest() — `POST /v1/memories` + messages

**入口**: `omem-server/src/api/handlers/memory.rs:172` create_memory
**核心**: `omem-server/src/ingest/pipeline.rs:83-323`

```
session_store.bulk_create          ← Raw 模式止步于此
→ select_messages (≤20条, ≤200KB)
→ filter meta_operation (RegexSet 20+ 模式)
→ strip_private_content (<private> tag)
→ FactExtractor.extract (LLM, max 15 facts, confidence≥3)
→ NoiseFilter.is_noise (cosine>0.82, max 200 学习向量)
→ AdmissionControl.evaluate (5维评分)
→ detect_private_content (IP/密码/key 自动 private)
→ Reconciler.reconcile (7-decision, LLM)
→ trigger_induction (async, 阈值5+冷却600s)
```

### 路径 B: session_ingest() — `POST /v1/memories/session-ingest`

**入口**: `omem-server/src/api/handlers/memory.rs:1439-2484`

```
fetch_session_emotional_memory / fetch_session_work_memory
→ walk_to_chain_tail (沿 ContinuedBy 找链尾)
→ SESSION_EXTRACT (LLM) → topics[]
→ for each topic:
   ├─ EMOTIONAL (scope=private):
   │   topic 匹配 → append (≤3000) / skip
   │   不匹配 → create new
   └─ WORK (scope=public):
      ├─ scope!=private 路径:
      │   find_similar_work_memory (embedding cosine>0.72)
      │   collect_chain_memories (BFS depth≤5)
      │   refine_and_replace (LLM REFINE prompt)
      │   → content > MAX_SINGLE_MEMORY_CHARS(3000):
      │     find_split_point + create_continuation + relations.push(ContinuedBy)
      │     store.update(refined, vec) ← 原子写入
      │     add_continued_by_relation (祖先链)
      │   → content ≤ 3000: 直接 update
      └─ fallback 路径 (无 existing_work_memory):
         append >3000 → create_continuation
→ INDUCTION(ind_texts) → preferences (async)
```

**关键区别**：路径 A 走 Reconciler 7-decision；路径 B 走 EMOTIONAL/WORK 分流 + REFINE 精炼。

---

## Reconciler 7-Decision

**位置**: `omem-server/src/ingest/reconciler.rs:53-`

执行顺序：
1. `gather_existing` — 向量搜已有记忆
2. `batch_self_dedup` — 新 facts 互 dedup
3. `exact_match_dedup` — hash+substring 硬匹配
4. `fast_session_merge` — 同 session 快速合并
5. `compute_fuzzy_pairs` — O(n²) 模糊对
6. LLM reconcile → 7 decisions

**决策**：
- `CREATE` — 新信息
- `MERGE` — 丰富已有（pinned 降级 CREATE）
- `SKIP` — 重复/低质
- `SUPERSEDE` — 矛盾更新（archive 旧）
- `SUPPORT` — 确认（提 confidence）
- `CONTEXTUALIZE` — 情境补充（创建关联）
- `CONTRADICT` — 直接矛盾

**Category 规则**: profile 永远 MERGE；events/cases 只 CREATE/SKIP；其余支持全 7 决策。

---

## Intelligence（post-import 异步重提取）

**位置**: `omem-server/src/ingest/intelligence.rs:140-`

```
raw_messages → detect_content_type
→ extract_atomic / extract_sections / extract_document
→ reconcile (持 reconcile_semaphore=1)
```

用于文件导入后的二次精炼。

---

## L0/L1/L2 字段语义

### L0 (l0_abstract) — 搜索核心

**生成**:
- 路径 A: FactExtractor → `l0_abstract`（≤100 chars）
- 路径 B: SESSION_EXTRACT → `topic` 字段 → `l0_abstract`

**使用**:
- LanceDB **FTS 索引**（`store/lancedb.rs:2107-2113`）— 全文搜索主键
- 检索 length normalization（`retrieve/pipeline.rs:828-832`）— `len_ratio = l0_abstract.len() / 500.0`
- tier_history / get_tier_changes 标题展示
- NoiseFilter 用 l0_abstract embedding 判噪音

### L1 (l1_overview) — 箭头时间线

**约束**:
- SESSION_EXTRACT: `verb phrase→verb phrase→result` ≤150 chars（`prompts.rs:837`）
- REFINE: `arrow notation: verb→verb→result` ≤150 chars（`refine_prompt.rs:31`）
- 示例: `diagnosed bug→traced to handler→fixed→verified→deployed`

**使用**:
- **retrieve REFINE 候选展示**（`pipeline.rs:1001-1004`）— LLM 判断相关性时看 L1+截断 content
- create_continuation 继承（>150 截断）

### L2 (l2_content) — 几乎只写不读

**约束**: ≤300 chars，结构化 key-value
**使用**: 代码中**几乎没有直接读**——历史遗留字段。检索时不用，注入时不用。

### ⚠️ create_continuation 继承问题

**位置**: `memory.rs:1946-1955`

```rust
child.l0_abstract = parent.l0_abstract.clone()
child.l1_overview = parent.l1_overview.clone()  // >150 截断
child.l2_content = parent.l2_content.clone()    // >500 截断
```

**问题**: split 后父子 L0/L1/L2 完全相同但 content 不同 → L0/L1/L2 跟内容脱节，FTS 召回失真。

---

## Split 触发点（3 处，仅 WORK 路径会 split）

| 位置 | 触发条件 | 行为 |
|------|---------|------|
| `memory.rs:2040` EMOTIONAL append | `new_content > 3000` | **不 split，跳过 append** |
| `memory.rs:2142` WORK refine 后 | `content > MAX_SINGLE_MEMORY_CHARS(3000)` | split via create_continuation |
| `memory.rs:2263` WORK fallback append | `new_content > 3000` | split via create_continuation |

**`MAX_SINGLE_MEMORY_CHARS = 3000`** 定义于 `omem-server/src/ingest/refine_service.rs:13`

### create_continuation（memory.rs:1926-2011）

```rust
child = Memory {
    content: child_content,           // 第二半
    l0/l1/l2: parent.clone(),         // ⚠️ 继承问题见上
    source: "auto-split",
    relations: [Continues → parent.id],
    ...
}
state.embed.embed(&[child_content])   // 重新向量化
store.create(&child, vec)
```

### add_continued_by_relation（memory.rs:2394-2438）

反向关系：parent.relations.push(ContinuedBy → child.id)
- MAX_RETRIES = 3
- 幂等检查：relation 已存在则 skip

---

## ⚠️ 已知问题：REFINE 压缩太狠 → split 不触发

**根因**: `refine_prompt.rs:16-32` REFINE_SYSTEM_PROMPT Rule 2:
> "Compress: Target output = 40-60% of total input length. Minimum 25%."

**死锁链**:
1. `refine_service.rs:194-199` 输入 existing_content 先截到 3000 字符
2. LLM 按 40-60% 压缩 → 输出 1200-1800
3. 永远 < 3000 → `refine_service.rs:249` split 永远不触发
4. → 没 child 记忆 → 没 Continues/ContinuedBy 关系
5. → 跨记忆关联断裂 + L0/L1 信息密度下降

**对比**: 老账号（tenant `c60beb98-7aab-4985-8c1d-29ffd6aff75a`）200 条记忆中 7 条有 relations，content len max=2998；当前账号（`4cf468b2-...`）200 条几乎无 relations。

**备份**: `omem-server/src/ingest/refine_prompt.rs.bak` 是精简前版本（Rule 4: 50-80%，Keep ALL，禁跨 section 合并）。

---

## fetch_session_work_memory 过滤陷阱

**位置**: `memory.rs:2705-2710`

```rust
m.session_id == sid
&& m.scope != "private"
&& m.category != "preferences"
&& m.source == "session_ingest"   // ← 关键
```

**陷阱**: auto-split child `source = "auto-split"`，**不会被 fetch 选中**！只有原始 session_ingest 记忆才会被加载为 existing_work_memory。

---

## Profile V2（偏好系统，SQLite）

### 表结构（`profile_v2/migration.rs:5-64`）

```sql
preferences(
    id, tenant_id, slot, value, confidence,
    scope,         -- global | project
    project_path,  -- project scope 时填
    source,        -- observed | induced
    status,        -- active | dormant | reinforce | deleted
    last_reinforced_at, created_at, updated_at,
    UNIQUE(tenant_id, slot, value, project_path)
)
profile_versions(id, tenant_id, snapshot, preference_count, created_at)
profile_changelog(id, tenant_id, preference_id, action, old_value, new_value, source, created_at)
induction_runs(id, tenant_id, status, candidate_count, extracted_count, error, started_at, completed_at)
induction_locks(id, tenant_id, created_at, ttl_secs)  -- 默认 600s
```

### Preference Slot 枚举（`induction.rs:18-33`）

```
communication_style | tone | code_style | error_handling
naming_convention | testing_strategy | workflow_preference
commit_style | emoji_preference | self_reference | address_style
language | framework_preference | preferred_tools | custom:*
```

**value 硬限**: ≤150 字符，超过丢弃。

### Induction 流程（`induction.rs:75-`）

```
检查 enabled + 归纳锁
→ 检查冷却（cooldown_secs=600）
→ acquire_induction_lock(ttl=600s)
→ create_induction_run
→ candidate_count < threshold(5)? skip
→ LLM INDUCTION_SYSTEM_PROMPT → Vec<InductedPreference>
→ 验证 slot/confidence/scope/value
→ 去重（同 slot+value 或 40%+ keyword overlap）
→ 写入 preferences 表
→ invalidate_cache
```

### Injection（`injection.rs:34-142`，**无 LLM**）

```
cache_key = tenant_id:project_path, TTL=1800s
→ 全局偏好（scope=global, status=active）按 confidence 降序 ≤20
→ 项目偏好（scope=project, project_path 匹配）≤10
→ 合并 + 按 confidence 降序
→ Token 预算裁剪：总字符 ≤3000
→ 格式化 markdown: "## User Profile\n- slot: value\n..."
→ 写缓存
```

### PreferenceStatus 流转

```
active ←→ reinforce（被多次确认）
active → dormant（last_reinforced_at 超过 dormant_days=90）
* → deleted（软删除）
```

scheduler.rs `check_dormant_preferences` 定期检查 dormant。

---

## Retrieve 11 阶段 Pipeline

**位置**: `omem-server/src/retrieve/pipeline.rs`

```
1.  stage_parallel_search       — vector + BM25 并行
2.  stage_tag_boost             — tag 匹配加分
3.  stage_rrf_fusion            — Reciprocal Rank Fusion
4.  stage_rrf_normalize         — 归一化
5.  stage_min_score_filter      — 最低分过滤
6.  stage_topk_cap              — 取 TopK
7.  stage_expand_relations      — 关系扩展
8.  stage_cross_encoder_rerank  — 可选 reranker
9.  stage_bm25_floor            — BM25 兜底
10. stage_decay_boost           — Weibull 衰减加权
11. stage_importance_weight     — importance 调权
12. stage_length_normalization  — 长度归一（用 l0_abstract）
13. stage_hard_cutoff           — 硬截断
14. stage_mmr_diversity         — MMR 去重
15. stage_llm_refine            — LLM 相关性判断（可选）
```

### Decay 公式（`lifecycle/decay.rs`）

```
composite = 0.4·recency + 0.3·frequency + 0.3·intrinsic

recency    = exp(-λ·t^β),  λ=ln2/hl_eff,  hl_eff=hl·exp(μ·importance)
frequency  = (1 - exp(-count/5)) · (0.5 + 0.5·recentness_bonus)
intrinsic  = importance · confidence

β: Core=0.8 (sub-exp), Working=1.0, Peripheral=1.3 (super-exp)
floor: Core=0.9, Working=0.7, Peripheral=0.5
```

`compute_composite` 带 floor clamp（搜索排序用）；`compute_raw_composite` 无 clamp（tier 降级用）。

---

## Lifecycle Tier（已修复降级，v0.3.2+）

**位置**: `omem-server/src/lifecycle/tier.rs:74-114`

```
Peripheral → Working:  access≥3 AND composite≥0.4
Working → Core:        access≥10 AND composite≥0.7 AND importance≥0.8
Working → Peripheral:  raw_composite<0.15 OR days_since_access>60
Core → Working:        raw_composite<0.15 OR days_since_access>60
```

**Scheduler**（`scheduler.rs:303-369` evaluate_tiers）:
- batch_size=100 分页遍历
- 跳过非 Active + **v0.3.2 起不再跳过 private**（已修复）
- 触发条件：`run_on_start=true` 启动即跑；之后每日上海午夜

**access_count 递增**（4 处 fire-and-forget）:
- `memory.rs:428` 同 space 搜索后
- `memory.rs:573` 跨 space 搜索后
- `session_recalls.rs:553` recall 后
- `memory.rs:660` GET /v1/memories/{id}（同步+1+立即 evaluate_tier）

---

## Store 层（LanceDB）

### StoreManager（`store/manager.rs`）

```rust
StoreManager {
    base_uri: String,
    cache: Mutex<HashMap<String, CacheEntry>>,        // LRU, max 20
    session_cache: Mutex<HashMap<String, SessionCacheEntry>>,
}
```

- `get_store(tenant_id) -> Arc<LanceStore>` — 最常用（~42 处）
- `get_accessible_stores(tenant_id, spaces)` — 跨 space 搜索
- LRU 驱逐：cache > 20 时淘汰最旧

### LanceStore.update（`store/lancedb.rs:1723-1802`）

两条路径：
- **vector 更新**: delete + re-insert（向量不能 update via expressions）
- **scalar 更新**: native `table.update().column(...)`（避免 version bloat）

每次 update 自动 `version += 1`，`updated_at = now()`。

### batch_bump_access_count（`lancedb.rs:1808`）

```sql
UPDATE table SET access_count = access_count + 1, last_accessed_at = now()
WHERE id IN (...)
```

---

## 调试速查

### 验证服务

```bash
curl https://www.mengxy.cc/health
curl -H "X-API-Key: $KEY" https://www.mengxy.cc/v1/stats
```

### 触发 lifecycle

```bash
curl -X POST -H "X-API-Key: $KEY" https://www.mengxy.cc/v1/lifecycle/trigger
```

### 查 tier 变更

```bash
curl -H "X-API-Key: $KEY" "https://www.mengxy.cc/v1/tier-changes?filter=demote&limit=10"
curl -H "X-API-Key: $KEY" "https://www.mengxy.cc/v1/tier-changes?filter=promote&limit=10"
```

### 查服务器日志

```bash
ssh root@47.93.199.242 "journalctl -u omem -n 100 --no-pager"
```

### 部署新版本

```bash
cargo build --release -p omem-server
scp target/release/omem-server root@47.93.199.242:/opt/omem/omem-server
ssh root@47.93.199.242 "systemctl restart omem"
curl https://www.mengxy.cc/health
```

---

## 常见迭代场景

### 改 LLM prompt

1. 定位（5 个 prompt 位置见上表）
2. 改 const 字符串
3. `cargo build --release` → scp → restart
4. 触发一次 ingest/search 验证日志

### 调 split 阈值

改 `refine_service.rs:13` `MAX_SINGLE_MEMORY_CHARS`。**注意**：prompt 里的压缩比也要同步调，否则 LLM 输出永远不到阈值。

### 调 tier 降级条件

改 `tier.rs:74-114` `evaluate_tier`。**注意**：scheduler 跑过的记忆不会自动重评，需要等下次 cycle 或手动 trigger。

### 调偏好归纳

改 `induction.rs:15-53` `INDUCTION_SYSTEM_PROMPT` 或 `config.rs` 阈值。**注意**：cooldown 600s 内重复触发无效。

### 加新 preference slot

1. `induction.rs:18-33` slot 枚举加描述
2. 注入自动支持（injection.rs 不区分 slot 类型）

---

## 文件大小警告

| 文件 | 行数 | 建议 |
|------|------|------|
| `api/handlers/sharing.rs` | 2072 | 拆 share_ops/auto_share/org |
| `api/handlers/memory.rs` | 2772 | 拆 memory_crud/search/session_ingest |
| `ingest/reconciler.rs` | 1822 | 保持，逻辑紧密 |
| `store/lancedb.rs` | 3648 | 拆 scalar/vector/session_ops |

---

## 测试

```bash
cargo test -p omem-server                    # 全部
cargo test -p omem-server lifecycle          # 仅 lifecycle
cargo test -p omem-server api::tests         # API 集成
cargo clippy                                 # lint
```

373 inline tests / 49 files。Mock 模式：TestEmbedder(1024-dim 固定向量) + TestLlm(返回 `{"memories":[]}`)。

---

## 参考文档

- `docs/PIPELINE.md` — 记忆流水线架构
- `docs/SHARING.md` — 共享架构
- `docs/API.md` — REST API 参考
- `docs/architecture.md` — 整体架构
- `omem-server/src/AGENTS.md` — Rust 源码导航
- `omem-server/src/api/AGENTS.md` — HTTP 层
- `omem-server/src/ingest/AGENTS.md` — 11 阶段 ingest pipeline
