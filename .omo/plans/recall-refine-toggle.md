# Recall 精炼开关方案

## 一、问题描述

recall pipeline 耗时过长，实测数据如下：

### 实测结果（2026-05-24 12:29 curl to server）

| 阶段 | 时间戳 | 耗时 | 说明 |
|------|--------|------|------|
| request start | 12:29:38.157 | — | — |
| LLM Gate | 12:29:40.052 | **1.9s** | recall_llm 判断是否需要召回 |
| Embed+阈值判断 | 12:29:40.118 | **0.07s** | 向量化+signal_level判断 |
| Phase1 pipeline.search | 12:29:48.438 | **8.3s** | 含向量搜索+BM25+RRF+**LLM refine** |
| Phase2 pipeline.search | (无结束日志，curl测) | **~11.5s** | 含向量搜索+BM25+RRF+**LLM refine** |
| **总耗时** | — | **21.76s** | — |

### 瓶颈定位

**LLM refine 占总耗时 ~90%（Phase1 8.3s + Phase2 11.5s）**

- Phase1: pipeline.search 处理项目记忆，5个候选送LLM精炼，LLM返回5个判定（1 kept, 4 dropped），耗时 8.3s
- Phase2: pipeline.search 处理全局记忆，同样调LLM精炼，耗时 11.5s
- 其余阶段（向量搜索、BM25、RRF融合、衰减、MMR去重）加起来不到 1s

### 日志证据

```
Phase1 llm_refine_summary: input=5, evaluated=5, high=0, medium=2, irrelevant=3, kept=1, dropped=4
refine_strategy="balanced"
```

## 二、现有代码分析

### `refine_strategy` 已有三档

| 值 | 行为 | LLM调用 |
|---|---|---|
| `"loose"` | 跳过LLM，返回全部候选 | 0次 |
| `"balanced"` | LLM判定 high/medium/irrelevant，irrelevant丢弃 | 1次/pipeline.search |
| `"strict"` | LLM判定后 medium 也丢弃 | 1次/pipeline.search |

### loose 的安全性验证

`pipeline.rs:957` — `if refine_strategy == "loose"` 直接 return 全部候选（candidates），discarded 为空 vec。

**下游影响**：
1. handler 层把 `refine_relevance`/`refine_reasoning` 透传给响应，loose 时为 None → **不影响搜索结果**
2. handler 层把 `discarded` 透传给响应，loose 时为空 → **不影响返回数量**
3. handler 层用 `memories.is_empty()` 判断 `should_recall` → loose 有结果就 true → **逻辑正确**
4. **唯一差异**：不丢弃 irrelevant 记忆，可能返回一些相关度低的结果。但这是用户主动选择关闭精炼的预期行为。

**结论：loose 安全可用，无坑。**

### Plugin 端现有配置

```
config.ts:31  refineStrategy: "strict" | "balanced" | "loose"  // 类型定义
config.ts:78  refineStrategy: "balanced"                        // 默认值
hooks.ts:302  config.recall?.refineStrategy ?? "balanced"       // 读取
hooks.ts:398  refine_strategy: refineStrategy                   // 传给API
```

用户在 `~/.config/cerebro/config.json` 中配置：
```json
{
  "recall": {
    "refineStrategy": "loose"    // 改这里即可跳过精炼
  }
}
```

## 三、方案

### 结论：不需要改代码

**Plugin 端已有 `refineStrategy` 配置，默认 `"balanced"`，用户改为 `"loose"` 即可跳过精炼。**

服务端也完整支持 `loose` 模式，无任何坑。

### 用户操作

编辑 `~/.config/cerebro/config.json`：
```json
{
  "recall": {
    "refineStrategy": "loose"
  }
}
```

### 预期效果

| 指标 | balanced (当前) | loose (改后) |
|------|----------------|-------------|
| LLM refine 调用 | 2次 (Phase1+Phase2) | 0次 |
| 总耗时 | ~21s | ~2s (仅 LLM Gate + 向量搜索) |
| 返回结果 | 精炼后丢弃 irrelevant | 返回全部候选 |
| refine_relevance | 有 (high/medium/irrelevant) | 无 (None) |

### 风险

- 关闭精炼后，返回结果可能包含一些 irrelevant 记忆
- 这是用户主动配置的权衡：速度 vs 精度
- 随时可改回 `"balanced"` 恢复精炼

## 四、后续优化方向（本次不做）

如果未来需要"既要速度又要精度"：
1. Phase1/Phase2 并行执行（当前串行）
2. LLM Gate 和 Embed 并行
3. 使用更快的 recall_llm 模型
