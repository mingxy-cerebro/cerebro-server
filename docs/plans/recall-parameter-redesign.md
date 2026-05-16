# Recall 参数体系重设计方案

> 综合灵犀(Metis)设计方案 + 玄机(Oracle)评审意见

## 一、当前问题

当前只有 2 个参数控制 recall 全链路：`maxRecallResults` + `similarityThreshold`。
链路中有 5 处硬编码倍率/阈值不可配，师尊无法通过 config 精准控制各阶段行为。

### 硬编码点清单

| 位置 | 硬编码 | 含义 |
|------|--------|------|
| pipeline.rs L145 | `limit * 3` | 初始搜索广度 |
| pipeline.rs L172 | `limit * 2` | topk_cap 候选上限 |
| pipeline.rs L308 | `max_results * 2` | Phase2 补充搜索倍率 |
| pipeline.rs L829 | `0.85` | MMR Jaccard 相似度惩罚阈值 |
| pipeline.rs L835 | `0.5` | MMR 相似惩罚系数 |
| pipeline.rs L892 | `15` | LLM 精炼最大评估数 |
| pipeline.rs L1008 | `200` | medium 级别截断字符数 |
| pipeline.rs L922 | `15s` | LLM 精炼超时 |

---

## 二、设计方案（综合灵犀+玄机）

### 设计原则

1. **三层递进**：Server env 默认值 → API 请求覆盖 → Plugin 端配置
2. **Pipeline/Recall 分离**：搜索级参数（fetch/topk/mmr）和 recall 级参数（phase2/llm_refine）命名分离
3. **向后兼容**：所有默认值 = 当前硬编码值，零行为变更
4. **每参数可配**：env var 设全局默认，API 请求可按次覆盖

### 环境变量（9 个）

#### 搜索级参数（影响所有 pipeline.search 调用）

| 环境变量 | 类型 | 默认值 | 作用 |
|----------|------|--------|------|
| `OMEM_SEARCH_FETCH_MULTIPLIER` | usize | 3 | `fetch_limit = max_results * N` |
| `OMEM_SEARCH_TOPK_CAP_MULTIPLIER` | usize | 2 | `topk_cap = max_results * N` |
| `OMEM_SEARCH_MMR_JACCARD_THRESHOLD` | f32 | 0.85 | Jaccard 相似度惩罚阈值 |
| `OMEM_SEARCH_MMR_PENALTY_FACTOR` | f32 | 0.5 | 相似记忆分数惩罚系数 |

#### Recall 级参数（只影响 should-recall）

| 环境变量 | 类型 | 默认值 | 作用 |
|----------|------|--------|------|
| `OMEM_RECALL_PHASE2_MULTIPLIER` | usize | 2 | Phase2 `limit = max_results * N` |
| `OMEM_RECALL_LLM_MAX_EVAL` | usize | 15 | LLM 单次评估最大候选数 |
| `OMEM_RECALL_REFINE_STRATEGY` | String | balanced | strict / balanced / loose |
| `OMEM_RECALL_REFINE_MEDIUM_CHARS` | usize | 200 | medium 级别截断字符数 |
| `OMEM_RECALL_LLM_REFINE_TIMEOUT_SECS` | u64 | 15 | LLM 精炼超时（秒） |

### refine_strategy 行为矩阵

| 策略 | high | medium | irrelevant |
|------|------|--------|------------|
| `strict` | 保留原文 | **丢弃** | 丢弃 |
| `balanced` | 保留原文 | 降级到 medium_chars 字符 | 丢弃 |
| `loose` | 保留原文 | 保留原文 | **保留原文+标注**（跳过 LLM 调用，省 token） |

> 玄机建议：`loose` 模式跳过 LLM 调用直接保留全部，避免浪费 token。

### API 请求参数扩展（ShouldRecallRequest 新增）

```rust
// 所有新字段 Option + #[serde(default)]
pub struct ShouldRecallRequest {
    // 已有
    similarity_threshold: Option<f32>,
    max_results: Option<usize>,
    // 新增
    fetch_multiplier: Option<usize>,
    topk_cap_multiplier: Option<usize>,
    mmr_jaccard_threshold: Option<f32>,
    mmr_penalty_factor: Option<f32>,
    phase2_multiplier: Option<usize>,
    llm_max_eval: Option<usize>,
    refine_strategy: Option<String>,
    refine_medium_chars: Option<usize>,
}
```

优先级：请求参数 > env var > 硬编码默认值

### Plugin 端配置扩展

```typescript
recall: {
    // 已有
    similarityThreshold: 0.4,
    maxRecallResults: 10,
    // 新增
    fetchMultiplier: 3,
    topkCapMultiplier: 2,
    mmrJaccardThreshold: 0.85,
    mmrPenaltyFactor: 0.5,
    phase2Multiplier: 2,
    llmMaxEval: 15,
    refineStrategy: 'balanced',
    refineMediumChars: 200,
}
```

---

## 三、改动文件清单

### Phase 1：Server 端参数化（P0 核心）

| 文件 | 改动 |
|------|------|
| `config.rs` | 新增 9 个 `OMEM_*` 字段 + `from_env()` |
| `pipeline.rs` | 新增 `SearchOverrides` struct；替换 8 处硬编码；`stage_mmr_diversity` 和 `stage_llm_refine` 改签名接收参数 |
| `session_recalls.rs` | `ShouldRecallRequest` 新增 Option 字段；Phase2 倍率改用参数；传递 overrides 到 pipeline |
| `server.rs` | `AppState` 注入 recall config（或从 config 字段读取） |

### Phase 2：Plugin 端透传

| 文件 | 改动 |
|------|------|
| `config.ts` | recall 配置新增 9 个字段 |
| `client.ts` | `shouldRecall()` 方法新增参数；`ShouldRecallRequest` 类型扩展 |
| `hooks.ts` | 读取新配置并传递给 `shouldRecall()` |
| `schema.json` | 更新配置 schema |

### 关键实现细节

1. **pipeline.rs `SearchOverrides` struct**：新增结构体传递覆盖参数
   ```rust
   pub struct SearchOverrides {
       pub fetch_multiplier: Option<usize>,
       pub topk_cap_multiplier: Option<usize>,
       pub mmr_jaccard_threshold: Option<f32>,
       pub mmr_penalty_factor: Option<f32>,
       pub llm_max_eval: Option<usize>,
       pub refine_strategy: Option<String>,
       pub refine_medium_chars: Option<usize>,
       pub refine_timeout_secs: Option<u64>,
   }
   ```

2. **stage 方法签名变更**：`stage_mmr_diversity` 和 `stage_llm_refine` 从 static fn 改为接收 overrides 参数
   - `stage_mmr_diversity(entries, limit)` → `stage_mmr_diversity(entries, limit, jaccard_threshold, penalty_factor)`
   - `stage_llm_refine(entries, ...)` → `stage_llm_refine(entries, ..., max_eval, strategy, medium_chars, timeout)`

3. **`loose` 模式跳过 LLM**：在 `stage_llm_refine` 开头判断 strategy=loose 时直接返回，不做 LLM 调用

4. **向后兼容保证**：所有新参数默认值 = 当前硬编码值。不设新 env var 时行为完全不变

---

## 四、实施顺序

| 步骤 | 内容 | 预估 |
|------|------|------|
| 1 | `config.rs` 新增 9 字段 | 30min |
| 2 | `pipeline.rs` SearchOverrides + 替换硬编码 | 1h |
| 3 | `session_recalls.rs` API 参数扩展 | 30min |
| 4 | Plugin 端透传（config + client + hooks） | 30min |
| 5 | `schema.json` 更新 | 15min |
| 6 | 明镜评审 + 修复循环 | 1-2h |
| 7 | 编译 + 部署 + 验证 | 30min |

---

## 五、P1 存储对齐 + P2 画像展示（与 P0 独立，可并行）

### P1：存储对齐（方案2 已 approved）
- should_recall handler 删内部存储逻辑（L370-480）
- RecallEvent 表新增 `injected_count` 字段
- Plugin hooks.ts 三个 return 路径传 `injected_count`
- 前端展示"注入N条·保留M条·精炼掉K条"

### P2：画像注入 web 可见
- RecallEvent 新增 `profile_content` 字段
- Plugin hooks.ts 存储时传 profile content
- 后端 PATCH 端点支持
- 前端 EventCard 展示画像内容

---

## 六、约束（师尊铁律）

- 每次改完代码必须让明镜评审，循环直至全部通过
- 不交叉编译：`cargo build --release -p omem-server --no-default-features`
- Plugin 发布：tsc → 升版本 → npm publish → rm cache
- 前端部署：scp dist → `/var/www/omem-web/`
- schema.json 必须更新
- P3 GC 修复已完成（8处 after_mutation），明镜已通过
