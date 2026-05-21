# shouldRecall 优化改动记录 (v1.15.4 → v1.15.13)

> 生成时间：2026-05-21  
> 目的：供后续迭代计划执行时比对，避免误改

---

## 一、改动概览

| 指标 | 改前 | 改后 |
|------|------|------|
| 首轮 shouldRecall 耗时 | 5~9秒 | 802ms |
| 记忆召回率 | shouldRecall 几乎锁定 false | 四档信号+LLM prompt重写，召回率大幅提升 |
| LLM 判断倾向 | "不确定答no" | "宁可多召回" |
| 动态阈值 | 二档(llm_yes=0.40, 其他=0.60) | 四档(Strong/LLM=yes=0.35, Medium=0.40, Weak=0.45, None=0.50) |
| rate limit | 60秒 | 30秒 |

**7个文件改动，+813/-288行**

---

## 二、文件级改动清单

### 2.1 Rust 服务端

#### `omem-server/src/api/handlers/session_recalls.rs` (+359/-53)

**改动1：SignalStrength 四档枚举替代 bool**
- 新增 `SignalStrength` 枚举：`None / Weak / Medium / Strong`
- 函数签名 `has_recall_signals() -> bool` 改为 `-> SignalStrength`

**改动2：19个 LazyLock regex 分三档**
- STRONG_PATTERNS（7个）：中文历史引用、中文指代、中文密钥、中文检索意图、英文历史、英文指代、英文密钥
- MEDIUM_PATTERNS（7个）：中文偏好、中文项目、中文配置、中文决策、英文偏好、英文项目、英文配置
- WEAK_PATTERNS（5个）：中文求助+领域、中文错误、中文改造、英文求助、英文改造
- 重要：不包含编程通用词（config/key/data/function/variable）作为独立触发词

**改动3：LLM prompt 重写**
```rust
// 旧 prompt 核心逻辑
// - A类(yes): 明确引用过去的决策、偏好、项目细节
// - B类(no): 技术方案选择、架构讨论（新话题）
// - C类(no): 通用编程问题、闲聊
// - 规则：不确定时答no（后续有兜底搜索机制）

// 新 prompt 核心逻辑
// - 回答yes的场景：引用过去事件、个人偏好、特定项目讨论、配置部署、问题求解
// - 回答no的场景：仅通用知识问答、简单计算、完全无关的新话题闲聊
// - 核心原则：**宁可多召回**
// - 移除了虚假的"兜底搜索机制"承诺
```

**改动4：四档动态阈值矩阵**
```rust
// effective_min_score
// llm_yes || Strong → 0.35
// Medium → 0.40
// Weak → 0.45
// None → 0.50

// quality_gate
// llm_yes || Strong → 0.35
// Medium → 0.40
// Weak → 0.42
// None → 0.48
```

**改动5：skip_llm_gate 参数**
- `ShouldRecallRequest` 新增 `skip_llm_gate: Option<bool>`
- 首条消息跳过 LLM 判断，直接 llm_yes=true（省1-3秒）

**改动6：29个新增测试**
- SignalStrength 各档位测试（Strong 8个、Medium 7个、Weak 5个、None 3个）
- skip_llm_gate 序列化测试（2个）
- 总计 32 个 tests passed

**改动7：日志增强**
- 新增 `should_recall_thresholds` 日志（signal_level, llm_yes, effective_min_score, quality_gate）
- 新增 `recall_llm_skipped` 日志

#### `omem-server/src/config.rs` (+1/-1)
- rate limit 从 60秒改为 30秒

### 2.2 TypeScript 插件端

#### `plugins/opencode/src/hooks.ts` (+654/-244)

**改动1：recallCache Map 引入**
```typescript
export const recallCache = new Map<string, {
  profileBlock: string;
  recallResult: ShouldRecallResponse;
  profileData: { countText: string };
  timestamp: number;
}>();
```
- 内存级 LRU 缓存，上限 50 个 session
- key = sessionID（无 query 匹配）
- Phase A cache miss → 同步 await + 写缓存
- Phase A cache hit → 毫秒级读缓存 + Phase B 后台刷新

**改动2：memoryInjectionHook 重写为两阶段**
- Phase A：cache miss 时同步 await Promise.all([getProfile, shouldRecall])，cache hit 时直接读缓存
- Phase B：cache hit 时后台 fire-and-forget 重新请求 shouldRecall，刷新缓存供下一轮使用
- 注入逻辑统一化：cache hit 和 cache miss 共享同一个注入代码块

**改动3：首条消息 skip_llm_gate + refine_strategy: "loose"**
```typescript
// Phase A cache miss 时
skip_llm_gate: true,
refine_strategy: "loose" as any,
```

**改动4：buildProfileBlock 提取为独立导出函数**
- 从 memoryInjectionHook 内联代码提取为 `export function buildProfileBlock()`

**改动5：profile TTL 从 30分钟改为 10分钟**

**改动6：cache hit null 安全检查**
```typescript
// 旧：if (cached)  — crash when recallResult=null
// 新：if (cached && cached.recallResult)
```

**改动7：Phase B 使用 balanced 策略（非 loose）**
- Phase A: `refine_strategy: "loose"` + `skip_llm_gate: true` → 快但粗糙
- Phase B: `refine_strategy: refineStrategy`（默认 balanced）→ 慢但精准

**改动8：session end/compact 清理 recallCache**
- sessionIdleHook、compactingHook 中新增 `recallCache.delete(input.sessionID)`

#### `plugins/opencode/src/client.ts` (+1)
- `recall_overrides` 类型新增 `skip_llm_gate?: boolean`

#### `plugins/opencode/src/config.ts` (+3)
- 新增 refine 相关配置项（refineStrategy, refineMediumChars 等）

#### `plugins/opencode/src/index.ts` (+80/-13)
- system.transform hook 中 profile 预加载（fire-and-forget）
- 导出 recallCache、buildProfileBlock 等供 index.ts 使用
- sessionIdleHook 集成

#### `plugins/opencode/package.json` (+2/-2)
- 版本：1.15.4 → 1.15.13

---

## 三、部署历史

| 版本 | commit | 内容 |
|------|--------|------|
| v1.15.5 | 4069b3b | async cache 消除 5-8s 阻塞 |
| v1.15.6 | e585d30 | cache miss 同步 await + no-recall toast + 英文化 |
| v1.15.7 | af03505 | createRecallEvent 覆盖 cache miss 路径 |
| v1.15.10 | 0739755 | profile TTL 10min + cached toast tag |
| v1.15.11 | 82f6212 | shouldRecall 四档信号系统 + skip_llm_gate |
| v1.15.12 | 2128788 | refine_strategy: "none"（未生效，pipeline只认"loose"） |
| v1.15.13 | (未commit) | 修正 refine_strategy: "none" → "loose"，首轮 802ms |

---

## 四、缓存机制设计

### recallCache 工作流
```
消息1 → cache miss → await shouldRecall(skip_llm_gate=true, refine="loose") [802ms] → 写缓存
消息2 → cache hit → 读缓存(毫秒) → Phase B后台shouldRecall(balanced)刷新缓存
消息3 → cache hit → 读缓存(毫秒，用的是消息2的query结果) → Phase B刷新
```

### 特点
- 缓存 key = sessionID，无 query 匹配
- 存在"延迟一拍"问题：第N条消息注入的是第N-1条 query 的结果
- LRU 淘汰：size > 50 时删最老
- 无 TTL 过期（仅有 LRU 淘汰 + session end 清理）

---

## 五、⏳ 待评审优化建议

> 以下为月儿分析后的优化建议，标注 [待评审]，需要玄机/明镜评审后再实施

### [待评审-1] query 变化检测，话题切换时强制 cache miss
- **问题**：缓存 key 只有 sessionID，不关心 query 内容。话题切换时注入无关记忆
- **方案**：缓存存 query 文本，cache hit 时做关键词重合度判断，低于阈值强制 cache miss
- **工作量**：小（~20行）
- **风险**：重合度阈值需调优，过低无效，过高频繁 cache miss

### [待评审-2] recallCache 加 TTL 过期
- **问题**：recallCache 无 TTL，长时间闲聊的 session 缓存永远不刷新
- **方案**：cache hit 时检查 timestamp，超过 5 分钟强制 cache miss
- **工作量**：极小（3行）
- **风险**：几乎无风险

### [待评审-3] Phase B refine_strategy 可配置化
- **问题**：当前 Phase B 硬编码用 refineStrategy 配置值（默认 balanced），但 balanced 模式下 LLM 精炼耗时 6s+
- **方案**：Phase B 也用 "loose"，或者新增 Phase B 专用 refine 配置
- **工作量**：小
- **风险**：Phase B 精炼质量下降，但 Phase B 结果只供下一轮缓存使用

### [待评审-4] recallResult 类型规范化
- **问题**：`recallCache` 中 `recallResult` 可能是 null（之前导致 crash），当前用 `cached && cached.recallResult` 防御
- **方案**：将 `recallResult` 类型从 `ShouldRecallResponse` 改为 `ShouldRecallResponse | null`，并确保所有读取消费方处理 null
- **来源**：明镜终审非阻塞建议

### [待评审-5] similarity_threshold 范围校验
- **问题**：`effective_min_score` 的 `unwrap_or_else` 逻辑在 skip_llm_gate 时走 0.35 分支，但实际日志显示 0.30
- **方案**：添加 `similarity_threshold` 范围校验 `(0.0..=1.0).contains()`
- **来源**：明镜终审非阻塞建议

---

## 六、Git Stash 注意事项

- stash@{0}: hook 重构前暂存
- stash@{1}: GC 优化 WIP  
- 执行迭代计划时不要误清 stash

---

## 七、验证结果

| 测试 | 结果 |
|------|------|
| Rust 32 tests | ✅ 全部通过 |
| tsc 编译 | ✅ 零错误 |
| 服务端 health | ✅ 200 OK |
| 首轮 shouldRecall 耗时 | ✅ 9258ms → 802ms |
| llm_refine_skipped | ✅ strategy=loose 已跳过 |
| should_recall=true | ✅ 正常召回 |
| npm publish | ✅ @mingxy/cerebro@1.15.13 |
| OpenCode 缓存清理 | ✅ ~/.cache/opencode/packages/@mingxy/cerebro@latest 已删除 |
| 明镜终审 | ✅ APPROVE（3个非阻塞建议） |
