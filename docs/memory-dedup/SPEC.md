# Memory Dedup & Injection SPEC (v1)

> 状态：**待批复** —— 2026-08-18 会话调研固化（起因：记忆 `3a940943-8ec5-4762-b6f1-9e9ba9918a24` 质量评估）
> 配套 issue：GitHub Issues（ready-for-agent）
> 范式参照：docs/dreams/SPEC.md

---

## 0. 调研结论存档（本轮会话已验证的码证事实）

### 0.1 入库合并链（session_ingest 路径）

- CC hooks（Stop/PreCompact/PostCompact/detached）以 cursor 游标增量推消息；<100 字符碎语丢弃；服务端 sessions 表按 `content_hash` 去重——**消息级重复已防死，不在本 spec 范围**
- 服务端提取：per-session 锁 → 取同 session 已有 EMOTIONAL/WORK 记忆摘要（`fetch_session_work_memory`，**只按 session_id 过滤**）→ 沿 ContinuedBy 走链尾 → prompt 塞 `merged_summary`（l0+l1 拼接，≤2000 字符）提示 LLM 勿重复提取 → LLM 增量交并差重写 content → `apply_append` 覆盖 content/l0/l1/l2，tags 只增不减 → 超 3000 字符 `split_memory` 按 `## ` 标题边界裂两条挂 Continues 链
- 「一 session 一条 WORK 大事记 + 超长裂链」是**有意设计**（web 端浏览体验），不动摇

### 0.2 三层摘要的真实消费方

| 消费方 | 实际吃的字段 |
|---|---|
| MCP `memory_search` 返回 | content 全文，无截断 |
| SessionStart `[CEREBRO-MEMORY]` 注入 | content 全文（recent+search 两段，截断开关默认 0=不截） |
| 检索管道（embed/RRF/BM25/MMR/rerank） | content 全文 |
| web 列表页 | content 截 120 字 |
| ingest 去重提示 `build_merged_summary` | **l0+l1（唯一消费者）** |

**L0/L1/L2 在召回链是死字段**——只影响 web 展示与 ingest 自参考。

### 0.3 已确认的病灶

1. **L0 漂移**：`apply_append` 注释明写「Preserve the latest topic title」——每次追加覆盖为最新 topic 标题，多主题记忆的摘要层只剩最后一题
2. **段落级重复**：病根链 = L0 被覆盖 → merged_summary 只见最近主题 → 旧主题在去重提示里蒸发 → LLM 重复提取已捕获主题（实证：3a940943 中「欢迎页边框文字」整段重复两次）
3. **跨条重复零兜底**：session_ingest 只查本 session；reconciler（LLM 对账 merge/skip）走的是 `/v1/memories` smart 模式路径，session_ingest 未接；检索层 MMR 只治召回显示不治存储；**dream 一期按 SPEC §11 边界第 4 条明确不做 cerebro 记忆库整理**；web 手动 merge 是唯一兜底
4. **tags 单调膨胀**：追加时 `if !contains then push`，只增不减
5. **注入肥大**：SessionStart 注入吃 content 全文，多主题裂链记忆注入最多 3000 字/条

## Problem Statement

用户（记忆系统所有者）在 web 端查看记忆时发现同一会话记忆内出现整段重复内容；不同会话讨论同一主题时记忆池沉淀多条互相独立的重复条目；agent（月儿）开场被动注入的记忆全文过长且大部分与当轮任务无关。用户无法信任记忆池的整洁度，且缺乏自动化清理手段——目前只有 web 手动 merge 一条路。

## Solution

三层治理，全部在写入侧与注入侧，不动摇「一 session 一条大事记」的存储架构：

1. **段落级防新增**：ingest 去重提示的内容源从「最近几条的 l0+l1」改为「content 全部 `## ` 段头行集合」，旧主题不再随 L0 漂移蒸发
2. **段落级硬兜底**：LLM 重写产物与旧 content 的段头集合做代码级比对，完全重复的段丢弃——字符串比对，零成本零幻觉，不依赖 LLM 自觉
3. **跨条级防新建**：新 topic 提取后做全库向量相似查（复活已废弃的 `find_similar_work_memory` 的 cosine>0.72 思路），超阈值追加进已有条链尾而非新建
4. **注入瘦身（独立开关）**：SessionStart 注入改为「l0+l1 摘要 + memory id」，末尾一行全局提示 agent 需要详情时用 `memory_get(id)` 取全文——渐进式披露，被动注入给短、主动 search 给长

## User Stories

1. As a 记忆系统所有者, I want 同一条记忆内不出现重复段落, so that web 端阅读时不用反复看到已看过的内容
2. As a 记忆系统所有者, I want 换 session 讨论旧主题时新归纳追加进已有记忆而非另起新条, so that 记忆池条目数随主题数增长而非随 session 数增长
3. As a 记忆系统所有者, I want web 手动 merge 从唯一去重手段降级为最后兜底, so that 我不需要肉眼巡检记忆池
4. As an agent（月儿）, I want SessionStart 注入只给我主题摘要和 id, so that 我的上下文不被当轮无关的记忆全文稀释
5. As an agent, I want 注入末尾有「memory_get 取详情」的指引, so that 我知道详情通道且能自主决定何时展开
6. As an agent, I want 主动 memory_search 时仍返回全文, so that 我主动检索时一步到位不用二次往返
7. As a 记忆系统所有者, I want 注入摘要模式有配置开关且默认关闭, so that 改动不影响现有行为直到我验证满意
8. As a 记忆系统所有者, I want 段头比对兜底不依赖 LLM 自觉, so that LLM 输出不稳定时重复段仍被拦截
9. As a 记忆系统所有者, I want 跨条向量合并有可调阈值, so that 误合并（相似但不同主题）风险可控
10. As a 记忆系统所有者, I want tags 在追加时做上限截断, so that 标签集合不随追加次数无限膨胀
11. As a web 端用户, I want L0 摘要漂移问题至少被记录, so that 未来决定是否值得做全文合成摘要
12. As a 记忆系统所有者, I want cerebro 池离线整理（cerebro 版 dream）立项评估, so that 存量重复有清理路径而非只防新增

## Implementation Decisions

- **不动摇**：一 session 一条 WORK 大事记 + 3000 字裂链架构；`MemoryType::Pinned` 硬编码；reconciler smart 模式路径
- **刀1（merged_summary 内容源）**：`build_merged_summary` 从「l0_abstract + l1_overview 拼接」改为「content 的 `## ` 段头行集合（带 updated_at 截止时间戳）」，保持 2000 字符上限。理由：去重判断是主题级，需要的是主题清单而非血肉；全文喂回（3000字×5条=15000字）在 ingest 高频通道烧 LLM 费用不可取
- **刀2（段头比对硬校验）**：抽纯函数（旧 content 段头集合 × LLM 重写产物段头），完全重复的段直接丢弃；位于 apply 追加路径。仅拦「段头完全一致」的段，不做模糊匹配——宁漏勿误杀
- **刀3（跨 session 向量查）**：新 topic 的 l0 embed 后全库向量查，cosine 超阈值（初始 0.72，可配）→ 走 `walk_to_chain_tail` 追加进已有条；查不到才新建。误合并风险通过阈值与灰度观察控制。**此刀先独立分支验证再合**
- **刀5（digestMode 注入）**：插件端 hooks 格式化层改动（opencode 版 `buildMemoryInjection` 与 CC 版 session-start 对应处），新增 `injection.digestMode` 配置（默认 off）。on 时注入条目格式：`- (age) id=<uuid>` + l0 + l1，末尾一行「以上为摘要，需要某条完整详情时调用 memory_get 工具传该 id（工具未加载时先 ToolSearch 捞取）」。**被动注入给摘要、主动 search 给全文的不对称是原则**
- **MCP tools 端不改**：`memory_search` 返回全文保持现状
- **服务端 L0 摘要合成（全文多主题 → 合成标题）不做**：L0 在召回链是死字段，收益仅 web 展示；留给二期与 cerebro 池 dream 一并评估

## Testing Decisions

- 只测外部行为（给定输入 memory 集合/对话 → 断言产出/注入文本），不测内部状态
- **现成 seam 全用，零新增测试基建**：
  - 刀1/刀2：纯函数 seam（`build_merged_summary` 已是纯函数；刀2 抽新纯函数），参照 refine_service.rs 内既有截断/边界纯函数测试风格
  - 刀3：MockEmbed（零向量 1024 维）+ Memory 构造，refine_service/pipeline 的测试模块里有现成 MockEmbed 先例可抄
  - 刀5：buildMemoryInjection 已是可注入 fake client 的纯逻辑层，断言输出文本含 id/l0/l1 与提示行
- 先例：dream 一期 17/17 测试（fake Llm）、refine_service sentence-boundary 纯函数测试

## Out of Scope

- **cerebro 池离线整理（cerebro 版 dream）**：二期大活，独立 spec；dream SPEC §12.2 已有 LanceDB 防护模式清单 9 条待照抄
- **L0 全文合成摘要**：死字段不值得单独动
- **MCP memory_search 改摘要返回**：主动检索应一步到位
- **消息级去重**：cursor + content_hash 已防死
- **web 按 session 聚合视图**：详情页 RelationGraph 已有裂链可视化，列表页聚合另议
- **存储架构改动（拆条原子化）**：明确否决——一 session 一条大事记是有意设计

## Further Notes

- 调研起因：3a940943（7 主题合 1 记忆，version=6，含整段重复）质量评估
- 本轮修正的认知错误存档：①「ingest 合并条件太松」——实为深度设计 ②「dream 蒸馏兜底」——dream 一期只整理调用方提交的 CC 本地 md 记忆档，cerebro 池明确二期 ③「L0 失真伤害召回」——召回链不吃 L0/L1/L2
- fetch_session_work_memory 现实现为全拉 100 条内存过滤——糙但当前量级不痛，留观
- 注入体积账：现状 ~12000 字 ≈ 8000 token；digestMode 后 ~1800 字 ≈ 1200 token，省约 85%
- 环境注意：生产 .env 的 OMEM_DREAM_LLM_* 与 profile 通道计费策略（ingest 是高频通道）影响刀1/刀3 的 prompt 成本评估

## 待办（执行清单）

- [ ] 刀1：merged_summary 内容源改段头集合（小）
- [ ] 刀2：段头比对硬校验纯函数 + 集成（小）
- [ ] 刀3：跨 session 向量查防新建（中，独立分支验证阈值）
- [ ] 刀5：digestMode 注入开关（小，插件端 opencode+CC 两处）
- [ ] tags 追加上限截断（顺手，随刀1/2 带上）
- [ ] 二期立项评估：cerebro 池 dream（独立 spec）
- [ ] 全部完成后：codegraph sync → commit → push（顺序铁律）
