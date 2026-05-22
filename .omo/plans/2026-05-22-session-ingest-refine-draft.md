# Session Ingest 精炼去重 — 讨论草稿

> 状态：讨论中，未提交方案。师尊说"很多细节都没讨论呢"。

---

## 核心问题

session_ingest路径**不走Reconciler**（7决策引擎），只有handler层简陋的section追加逻辑（memory.rs:1700-1900），导致记忆越积越冗长、重复。

---

## 已确定的设计决策

### 1. 改造范围
- **不动Reconciler** — Reconciler只走import路径，工作正常
- **改造session_ingest handler层** — 替换简陋的section追加为智能精炼

### 2. 精炼范围：关联链级
- 精炼时拉取同session同主题的记忆 + Continues/ContinuedBy relation链
- 链中所有记忆参与精炼

### 3. 超3000字：继续split
- 精炼后仍超3000字，按section分割成多条记忆 + Continues/ContinuedBy relation

### 4. l0/l1/l2同步更新（师尊确认）
- 精炼时LLM同时输出4个字段（content + l0 + l1 + l2）
- l0 = 主题标签（概括全貌，如"autoRecallHook注入系统问题诊断与修复"）
- l1 = 故事线脉络（箭头格式，如"发现问题→定位根因→修复→验证→发布"）
- l2 = 关键事实摘要（可检索的决策/结论/数据，≤300字）
- content = 完整去重降噪后的记忆（section格式保留，WORK≤500字）

### 4.1 l1箭头脉络格式（师尊确认）
l1固定用 **`节点A→节点B→节点C→结果`** 格式，覆盖所有场景：
- 修bug: 发现问题→定位根因→修复→验证→发布
- 新功能: 需求分析→方案设计→实现→测试→上线
- 架构改造: 识别问题→方案对比→选定方案→分阶段落地→效果验证
- 调研: 提出问题→收集资料→对比分析→得出结论
- 决策: 背景→选项A vs B→选定A→理由
- 项目进度: 启动→Phase1完成→Phase2进行中→卡在X

**两个地方都要加l1格式**：
1. SESSION_EXTRACT prompt（首次提取）
2. 精炼prompt（有旧记忆时重新生成）

### 4.2 精简字数（师尊确认）
| 字段 | 改造前 | 改造后 |
|------|--------|--------|
| summary(content) | WORK≤800字, EMOTIONAL≤500字 | WORK≤500字, EMOTIONAL≤300字 |
| l1_overview | ≤150字 | ≤150字（箭头脉络格式） |
| l2_content | ≤500字 | ≤300字（只保留结论/决策） |

### 4.3 偏好提取已独立（师尊同步）
- 偏好(preferences)提取已单独提出去，不走session_ingest的LLM
- 有独立的 `preference_slots.rs` + `preference_slot_guard`
- 精炼只针对WORK类记忆，偏好不需要精炼

### 5. 精炼输出格式
```json
{
  "refined_content": "去重降噪后完整content（section格式）",
  "l0_abstract": "主题标签",
  "l1_overview": "故事线概述(≤150字)",
  "l2_content": "关键事实摘要(≤500字)"
}
```

### 6. 精炼触发时机
- 第1次ingest（无旧记忆）→ 不走精炼，直接创建
- 第2次ingest（有旧记忆）→ 走精炼
- 第3次ingest → 再次精炼（精炼后的记忆 + 新fact → 精炼 → 去重降噪）
- 循环，记忆始终精简

### 6.1 Per-topic独立精炼（师尊确认）
SESSION_EXTRACT返回多个topic时，每个topic独立判断：

```
for each topic:
  ① embed(topic.l0_abstract)
  ② 搜索同session的WORK记忆，找cosine > 0.7的
  ③ 相似度高 → 收集relation链 → LLM精炼 → 存精炼结果
     相似度低 → 直接创建新记忆
```

示例：一次ingest提取出"bug修复"+"架构改造提案"
- bug修复 → cosine=0.85 匹配旧记忆 → 精炼
- 架构改造 → cosine=0.3 新话题 → 直接创建

**不需要改SESSION_EXTRACT**，只改handler层per-topic的追加逻辑。

---

## 讨论中 / 未决定的细节

### A. 语义相似度前置检查（师尊确认）
**思路**：不是每次都调LLM精炼，先看新内容跟旧记忆相似度
```
新fact进来
  ↓
语义相似度检查
  ↓
相似度高 → 同主题延续 → LLM精炼去重
相似度低 → 新话题 → 直接追加，不走LLM
```

**师尊确认**：
1. **比较对象**：先用新topic.l0_abstract vs 旧记忆.l0_abstract做embedding cosine。如果效果不好再换content
2. **阈值**：0.7
3. **多topic**：每个topic独立判断，各自检查各自旧记忆相似度

### B. 成本影响
- session_ingest路径：1次LLM → 可能2次LLM（+1次精炼）
- import路径：不变
- 有语义前置检查后，相似度低的场景不调LLM，节省成本
- 精炼只对"有旧记忆 + 同主题"触发

### C. 精炼prompt设计
- 待设计：输入什么、输出格式、去重规则
- 参考：Reconciler的RECONCILE_SYSTEM_PROMPT（7决策），但输出格式不同

### D. handler层改造范围
- 当前：memory.rs:1700-1900（WORK追加 + EMOTIONAL追加）
- 改造后：找到旧记忆 → 语义检查 → 走精炼 or 直接追加
- 简化：不再做section匹配/替换，精炼结果直接存

### E. OOM/成本保护
- 精炼输入长度限制（旧记忆content合计不超过N字）
- 链太长时只取最近N条
- 单条记忆超长时截断

### I. 精炼失败处理
- LLM精炼超时/返回空/解析失败 → **回退到原有追加逻辑**（不丢数据）
- 宁可多存也不丢数据

### J. 归簇砍掉时机
- 精炼先上线，归簇后面单独砍
- 砍归簇就是删代码：找所有引用范围，全部删掉

### G. 私密记忆不精炼（师尊确认）
- scope="private" / EMOTIONAL类记忆 → 不走精炼逻辑
- 保持原有的追加逻辑不变
- 原因：私密记忆涉及情感/亲密内容，精炼可能丢失情感细节

### H. 召回精炼（师尊确认）
- **保留**代码，不删除
- **plugin端添加开关**（配置项），**默认false**
- 开启后才在召回时走精炼
- 不用的用户不浪费LLM调用，需要的用户可以自己开

---

## 代码位置参考

### session_ingest入口
- `omem-server/src/api/handlers/memory.rs:1406` — handler入口
- `memory.rs:1539` — LLM提取topic（build_session_extract_prompt_with_memories）
- `memory.rs:1600-1670` — 记忆创建逻辑
- `memory.rs:1673-1697` — apply_append函数（l0/l1/l2覆盖更新）
- `memory.rs:1700-1790` — EMOTIONAL记忆追加
- `memory.rs:1792-1900` — WORK记忆追加（含3000字split）

### Reconciler（不动）
- `omem-server/src/ingest/reconciler.rs` — 完整的7决策引擎
- 只走import路径（POST /v1/imports）
- session_ingest **不调用Reconciler**（已验证：memory.rs中无reconcil关键字）

### Embedding服务
- `omem-server/src/embed/service.rs` — EmbedService trait
- 已有向量搜索能力，可直接用于语义相似度检查

### F. 归簇直接砍掉（师尊确认）
**原因**：源头精炼保证高质量后，归簇的"去重合并"功能完全多余
- 归簇的全部成本都省了：k-means embedding + LLM cluster_summary + 定期重跑
- **plugin端（hooks.ts）的clustered路径也要一起砍掉**
- should-recall API的clustered字段也要清理
- 这是一个更大的改造范围，可能分阶段执行

---

## 灵犀(Metis)审查结论（师尊定案）

### 师尊定案
1. **物理删除！不要逻辑删除！** superseded_by方案作废，直接store.delete()
2. **Prompt必须写完整**，写完三堂会审
3. **Session锁**：师尊让月儿和玄机定 → 全程持锁
4. **字数限制**：500/800是提取时summary限制，不改。精炼后content不设硬上限，3000字split
5. **OOM保护**：输入总长≤8000字，链≤3条，单条≤3000字

### 灵犀原建议（部分采纳）
1. ~~物理删除改标记覆盖~~ → 师尊改为直接物理删除 ✅
2. 阈值0.65 + session_id双条件 ✅
3. ~~session锁拆分~~ → 改为全程持锁 ✅
4. 继承旧记忆tier/importance/tags ✅
5. BFS加环路检测+深度限制 ✅
6. 字数硬截断 ✅
7. 精炼入口双重guard ✅

---

## 下一步

等师尊继续讨论细节，确定：
1. 语义前置检查的具体实现方案
2. 精炼prompt的设计
3. 其他细节

确认完毕后再更新正式计划提交。
