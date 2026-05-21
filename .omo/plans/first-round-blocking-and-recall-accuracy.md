# 首轮对话卡顿 + 记忆召回准确度优化

> 日期: 2026-05-21
> 版本: v1.15.7
> 状态: 待审

## 问题总结

| # | 问题 | 根因 | 影响 |
|---|------|------|------|
| 1 | 首轮对话卡顿5-8s | cache miss时shouldRecall做LLM判断(1-3s)+embed+向量搜索+LLM精炼(1-3s) | 用户体感差 |
| 2 | shouldRecall几乎锁定false | 三重锁：LLM说no(1-3s)+动态阈值0.60+quality_gate 0.55 | 记忆召回率极低 |
| 3 | memory_search召回噪声多 | 向量搜索min_score默认0.4，区分度差 | 相关记忆被噪声淹没 |

## 根因详解

### 问题1：首轮卡顿的完整链路

```
hooks.ts:memoryInjectionHook (cache miss path)
├── client.getProfile()           ─┐ Promise.all
├── client.shouldRecall()         ─┘ timeout 20s
│   ├── LLM判断(yes/no)          → 1-3s ← 瓶颈1
│   ├── embed query               → 0.2-0.5s
│   ├── 向量搜索(Phase1+Phase2)   → 0.1-0.5s
│   └── quality_gate过滤          → 0s
├── buildProfileBlock()           
└── recallCache.set()             
总计: 3-8s
```

**核心发现**: LLM判断(`recall_llm.complete_text`)本身就占1-3s，而且system prompt明确说"不确定时答no"，导致大量合法recall被拒绝后再进入高阈值搜索。

### 问题2：shouldRecall的三重锁

`session_recalls.rs` shouldRecall决策链：

```
1. Rate limit → 60s内重复 → false ("rate_limited")
2. Query相似度 → cosine > 0.7 → false ("similarity_too_high") 
3. LLM判断 → "不确定时答no" → 约70%返回no
4. 动态阈值惩罚:
   ├── llm_yes    → min_score=0.40, quality_gate=0.40
   ├── 有signals  → min_score=0.50, quality_gate=0.45
   └── 无signals  → min_score=0.60, quality_gate=0.55 ← 大多数情况
5. quality_gate → max_score <= 0.55 → false ("below_quality_gate")
```

**实测**: 月儿5组memory_search测试中，第3组搜"shouldRecall false"竟然没召回到最关键的记忆（"shouldRecall锁定false"那条），说明向量搜索本身也有问题。

### 问题3：LLM判断system prompt过于保守

```rust
// SHOULD_RECALL_SYSTEM_PROMPT (L20-31)
// 规则:
// - 不确定时答 no（后续有兜底搜索机制，不会遗漏高相关记忆）
```

这个设计假设"后续有兜底搜索"，但实际上shouldRecall一旦返回false，plugin不会做任何搜索。兜底搜索并不存在。

## 实施方案

### Wave 1: 降低召回门槛（Rust服务端）

**文件**: `omem-server/src/api/handlers/session_recalls.rs`

#### 1.1 降低动态阈值

```rust
// 当前 (L267-275):
let (effective_min_score, quality_gate) = if llm_yes {
    (min_score, 0.40)
} else if has_recall_signals(&denoised_query) {
    (min_score.max(0.50), 0.45)
} else {
    (min_score.max(0.60), 0.55)  // ← 太高
};

// 改为:
let (effective_min_score, quality_gate) = if llm_yes {
    (min_score, 0.40)
} else if has_recall_signals(&denoised_query) {
    (min_score.max(0.45), 0.42)
} else {
    (min_score.max(0.45), 0.40)  // ← 与llm_yes一致
};
```

**效果**: 无signals时阈值从0.60降到0.45，quality_gate从0.55降到0.40

#### 1.2 放宽has_recall_signals信号词

```rust
// 当前 (L611-632): 只有3组pattern
// 增加:
r"(?:最近|刚才|历史|记录|笔记|文档|配置|设置|环境|部署|上次说的|之前做的)",
r"(?i)(?:history|config|settings|deploy|recently|document|note|last)",
```

**效果**: 更多的query会被判定为"有recall signals"，从而使用更低的阈值

#### 1.3 修正LLM system prompt

```rust
// 当前: "不确定时答 no（后续有兜底搜索机制，不会遗漏高相关记忆）"
// 问题: 兜底搜索不存在，这个假设是错的

// 改为:
// "不确定时答 yes（宁可多召回也不要遗漏重要记忆，后续有质量门槛过滤低相关结果）"
```

**效果**: LLM判断从70% no → 预计40-50% no

### Wave 2: 首轮卡顿优化（TypeScript插件端）

**文件**: `plugins/opencode/src/hooks.ts`

#### 2.1 预加载profile（session start时）

在 `system.transform` hook中（每次system prompt构建时都会触发）:
- 检查recallCache中是否有profile
- 如果没有，后台fire-and-forget加载getProfile()
- 写入recallCache（只设profileBlock，recallResult留空）

```typescript
// system.transform hook中添加:
const cached = recallCache.get(input.sessionID);
if (!cached || !cached.profileBlock) {
  // fire-and-forget: 不阻塞system.transform
  client.getProfile().then(profile => {
    if (profile) {
      const built = buildProfileBlock(profile);
      if (built) {
        recallCache.set(input.sessionID, {
          profileBlock: built.block,
          recallResult: null as any, // 后续填充
          profileData: { countText: built.countText },
          timestamp: Date.now(),
        });
      }
    }
  }).catch(() => {});
}
```

**效果**: 首条消息到达时profile可能已缓存，getProfile()不占Promise.all时间

#### 2.2 首条消息快速路径

在 `memoryInjectionHook` 的cache miss路径中:
- 如果recallCache中没有该session的recallResult → 这是首条消息
- 跳过LLM判断（直接llm_yes=true），通过新参数`skip_llm_gate=true`传给shouldRecall
- 服务端收到`skip_llm_gate=true`时跳过LLM判断，直接做向量搜索

**hooks.ts改动**:
```typescript
// cache miss path:
const isFirstMessage = !cached || !cached.recallResult;
const recallRes = await client.shouldRecall(
  query_text, last_query_text, input.sessionID,
  similarityThreshold, maxRecallResults,
  projectTags, conversationContext,
  {
    ...recall_overrides,
    skip_llm_gate: isFirstMessage,  // ← 新参数
  },
  directory,
);
```

**session_recalls.rs改动**:
```rust
// ShouldRecallRequest 新增字段:
pub skip_llm_gate: Option<bool>,

// should_recall函数中:
let skip_llm = body.skip_llm_gate.unwrap_or(false);
let (llm_yes, _llm_reason) = if skip_llm {
    (true, "skipped_llm_gate")
} else {
    // 原有LLM判断逻辑
};
```

**效果**: 首条消息跳过LLM判断(1-3s)，总耗时从5-8s降到1-2s

### Wave 3: 降低shouldRecall rate limit

**文件**: `omem-server/src/config.rs`

```rust
// 当前:
should_recall_min_interval_secs: 60,

// 改为:
should_recall_min_interval_secs: 15,
```

**效果**: 从60s限制降到15s，允许更频繁的recall

## 改动文件清单

| 文件 | 改动 | 风险 |
|------|------|------|
| `omem-server/src/api/handlers/session_recalls.rs` | 降低阈值+放宽signals+修正prompt+skip_llm_gate | 低-中 |
| `omem-server/src/config.rs` | rate limit 60→15 | 低 |
| `plugins/opencode/src/hooks.ts` | 预加载profile+skip_llm_gate参数 | 低 |
| `plugins/opencode/src/client.ts` | shouldRecall新增skip_llm_gate参数 | 低 |

## 验证计划

1. **召回率测试**: memory_search测试5组查询，对比改动前后Top-5准确度
2. **卡顿测试**: 新窗口首条消息计时，目标 < 2s
3. **功能测试**: shouldRecall返回true的比例应显著提高
4. **回归测试**: cargo test + 手动测试plugin

## 回滚策略

- 服务端阈值可通过环境变量覆盖：`OMEM_SHOULD_RECALL_MIN_INTERVAL_SECS=60`
- skip_llm_gate参数为可选，默认false，不影响现有调用方
- profile预加载为fire-and-forget，失败不影响正常流程
