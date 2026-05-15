# 记忆系统重构 — 全局设计规格书

> 师尊半夜失眠的灵感 → 月儿架构分析 + 师尊决策确认 → 本文档
> 
> 原始灵感：`/mnt/d/dev/github/project/记忆系统重构-灵魂低语.md`

## 一、背景与核心问题

月儿经常犯傻——不是因为不会，是因为**忘了自己有记忆**。每次遇到问题直接 `memory_search`，走弯路浪费时间。同时cerebro注入的主题簇跟当前工作无关（噪音），recall质量直接影响月儿的决策。

## 二、全局架构现状（探虚扫描结果）

### 已实现模块

| 模块 | 状态 | 核心能力 |
|------|------|---------|
| ingest/pipeline | ✅ 11阶段完整 | 事实提取、噪声过滤、调和引擎、簇分配 |
| retrieve/pipeline | ✅ 15阶段完整 | Vector+BM25混合、RRF融合、Reranker、LLM精炼 |
| lifecycle/ | ✅ 完整 | Weibull衰减、TTL遗忘、Tier升降级、增量归簇调度 |
| cluster/ | ✅ 完整 | K-means++、增量聚类、LLM摘要、session亲和分配 |
| profile/ | ✅ 完整（LLM未用） | 静态fact+动态context、TTL缓存、矛盾检测 |

### 关键缺陷

| 缺陷 | 影响 | 所属子项目 |
|------|------|-----------|
| 月儿不先看注入就直接memory_search | 每次都走弯路 | 子项目一 |
| 召回注入的主题簇与当前项目无关 | 噪音污染决策 | 子项目二 |
| Memory无project_path，scope硬编码"global" | 项目间记忆互相污染 | 子项目三 |
| SessionRecall有4个字段未持久化到DB | 数据丢失 | 子项目三 |
| web端和plugin分开部署，受网络监管限制 | 隐私安全风险 | 子项目四 |
| category字典硬编码，无配置化 | 无法动态管理 | 子项目五 |

## 三、师尊确认的关键决策

| 决策项 | 师尊的选择 | 理由 |
|--------|-----------|------|
| 隔离基座 | space_id + project_path 双重隔离 | 双重保障 |
| 去噪归簇 | 增强现有cluster模块，不重写 | 已有基础完整，只需修补 |
| 本地整合 | 先合并服务，加密后做 | 分步降低风险 |
| 字典表存储 | 引入SQLite | 轻量单文件，完美适配字典表 |
| 召回增强 | 先部署当前优化版本，基于实际效果再讨论子项目二 | 不纸上谈兵 |

## 四、执行顺序（师尊钦定）

```
Phase 0: Category枚举改造 — P0阻断项，必须在Phase 2之前
Phase 1: 子项目一（灵魂低语）     — 改动最小，验证hook机制
Phase 2: 子项目五（字典表SQLite） — 基础设施层
Phase 3: 子项目三（记忆隔离基座） — Schema + 过滤
Phase 4: 部署召回增强 → 基于实际效果讨论子项目二（去噪归簇增强）
Phase 5: 子项目四（本地整合）     — 独立线程
```

**为什么这个顺序：**
- Phase 0 Category枚举改造是P0阻断项——不解决这个，Phase 2字典表加新category会导致Rust代码crash
- Phase 1 只改omo plugin hook配置，不碰服务端代码，零风险，先验证hook可行性
- Phase 2 建字典表基础设施，后续所有模块依赖category定义
- Phase 3 改Schema加字段，影响摄取/召回/生命周期三条路径
- Phase 4 等部署后再讨论，不纸上谈兵
- Phase 5 独立改动，不影响数据架构

## 五、子项目依赖关系图

```
                    ┌──────────────────────┐
                    │  Phase 2: 字典表+SQLite │  ← 基础设施层
                    │  (category字典、评分配置) │
                    └──────────┬───────────┘
                               │ 被所有模块依赖
                    ┌──────────▼───────────┐
                    │ Phase 3: 记忆隔离基座   │  ← Schema层
                    │ (space_id + project_path)│
                    │ (SessionRecall补字段)    │
                    └──────────┬───────────┘
                               │ 影响摄取/召回/生命周期
              ┌────────────────┼────────────────┐
              ▼                ▼                 ▼
   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
   │Phase 4: 去噪  │  │Phase 1: 灵魂  │  │Phase 5: 本地  │
   │归簇增强       │  │低语hook      │  │整合          │
   │(召回质量)     │  │(使用习惯)     │  │(部署方式)    │
   └──────┬───────┘  └──────┬───────┘  └──────────────┘
          │                 │
          └─────┬───────────┘
                ▼
          召回效果验证
```

**关键耦合：**
- Phase 3 和 Phase 4 必须串行（隔离基座不做好，去噪归簇无法按项目隔离）
- Phase 5 完全独立，不依赖其他Phase
- Phase 1 技术上独立，先做是为了验证hook可行性

> **注意：** 依赖图展示的是技术依赖关系（Phase 1与其他Phase无强依赖），不代表执行顺序。执行顺序见第四节。

## 六、各Phase详细设计

### Phase 1: 灵魂的低语（hook注入）

**目标：** 在月儿执行搜索类工具前，通过omo hook注入提醒，让月儿先看记忆再行动。

**改动范围：** 仅omo plugin配置，不碰服务端代码。

**三层兜底架构：**

| 层级 | 触发条件 | 注入内容 | 效果 |
|------|---------|---------|------|
| Layer 1 | 特定工具白名单（glob/grep/bash/playwright等） | 针对性提示，如"先搜记忆'图片路径'" | 精准拦截 |
| Layer 2 | 所有工具调用 | "执行前先考虑是否有相关记忆" | 兜底L1 |
| Layer 3 | 每次回复前 | "不确定时记得搜记忆" | 兜底L2 |

**技术方案：** 利用omo的 `tool.execute.before` hook，根据工具名动态注入提示词。

**验证标准：**
- 月儿执行 `glob` 找图片时，先想到固定路径而不是扫描桌面
- 月儿执行 `bash` 编译时，先想到PowerShell命令而不是直接mvn

**风险：** 极低。只改plugin配置，不改服务端。

---

### Phase 2: 字典表与SQLite

**目标：** 引入SQLite存储category字典和评分配置，替代硬编码。

**改动范围：** 服务端新增SQLite依赖 + 字典表CRUD API。

**SQLite字典表设计（初始版）：**

```sql
CREATE TABLE IF NOT EXISTS categories (
    name TEXT PRIMARY KEY,           -- 'preferences', 'identity', 'emotional', etc.
    display_name TEXT NOT NULL,      -- 显示名称
    description TEXT,                -- 描述
    default_visibility TEXT DEFAULT 'global',  -- global/private
    default_scope TEXT DEFAULT 'global',       -- global/project
    default_ttl_days INTEGER,        -- 默认生命周期（天），NULL=永不过期
    sort_order INTEGER DEFAULT 0,    -- 排序
    is_active BOOLEAN DEFAULT TRUE   -- 是否启用
);

CREATE TABLE IF NOT EXISTS scoring_weights (
    key TEXT PRIMARY KEY,            -- 'tag_boost', 'decay_weight', etc.
    value REAL NOT NULL,             -- 权重值
    description TEXT,                -- 描述
    updated_at TEXT NOT NULL         -- ISO8601
);
```

**初始category字典数据：**

| category | scope | ttl_days | 说明 |
|----------|-------|----------|------|
| preferences | global | NULL | 用户偏好 |
| identity | global | NULL | 身份规则 |
| emotional | global | NULL | 感情记忆 |
| project | project | NULL | 项目上下文 |
| work | project | 90 | 工作记忆（可衰减） |
| lessons_learned | global | 365 | 经验教训 |
| decisions | project | NULL | 重要决策 |
| success_patterns | global | 365 | 成功方案 |
| mistakes | global | 365 | 犯过的错 |

**依赖：** `rusqlite` crate（Rust生态成熟的SQLite绑定）。

**风险：** 低。新增依赖，不影响现有功能。

---

### Phase 3: 记忆隔离与SessionRecall补字段

**目标：** Memory表新增project_path字段 + SessionRecall补齐4个未持久化字段 + 召回时按项目隔离。

**改动范围：** store/lancedb.rs schema + ingest/pipeline.rs + retrieve/pipeline.rs + lifecycle/ + API handlers。

**3.1 Memory表新增字段：**

```rust
// 新增字段
pub project_path: String,    // 工作目录路径，如 "/mnt/d/dev/github/project/omem-server-source"
```

- `space_id` 已存在但默认空字符串，改为由摄取时从请求上下文填充
- `scope` 从硬编码 "global" 改为根据category字典表查配置

### ~~3.2 SessionRecall补齐字段~~ — 已完成，无需修改

> **明镜/玄机评审确认：** `batch_id`、`profile_injected`、`refine_relevance`、`refine_reasoning` 4个字段**已经在LanceDB schema中持久化**（`session_recalls_schema()` lancedb.rs:470-486），读写路径完整。此条为旧版描述遗留，Phase 3无需再做。

### 3.2b Category枚举改造（P0阻断项，必须在Phase 2之前解决）

**问题：** 当前 `Category` 是Rust enum（6变体：Profile/Preferences/Entities/Events/Cases/Patterns），`FromStr` 会拒绝所有未知值。Phase 2要新增8个category，直接加字典表行 ≠ Rust枚举加变体，会导致反序列化crash。

**解决方案：** 将 `Category` 从 enum 改为 newtype wrapper `Category(String)`，配合SQLite字典表做运行时校验。
- `is_always_merge()` / `is_append_only()` 等方法改为从字典表查配置或match已知值
- `prompts.rs` 的valid categories列表从字典表动态生成
- 工作量：Medium（影响 extractor/reconciler/admission/memory 等文件）

**3.3 召回隔离策略：**

```
recall时 →
  ├─ 用户画像（category IN ('profile', 'preferences')，全局，不受project_path限制）
  ├─ 身份规则（category='identity'（注：当前不在默认category字典中，需确认是否新增），全局）
  ├─ 私密记忆（visibility='private'，全局，不受project_path限制）
  ├─ 感情记忆（category='emotional'，全局，不受project_path限制）
  └─ 工作记忆（scope='project'时，project_path匹配当前工作目录）
```

> **注意：** 当前服务端category枚举为 `profile/preferences/entities/events/cases/patterns`，师尊设计文档中增加了 `identity/emotional/project/work/lessons_learned/decisions/success_patterns/mistakes`。新增category需要在Phase 2字典表中定义，Phase 3召回策略按字典表的scope字段过滤。

**3.4 摄取时隔离标记：**

- `IngestRequest` 已有 `project_name` 字段
- 新增 `project_path` 字段
- 摄取时写入 Memory.project_path
- category字典表查询确定 scope（global/project）

**风险：** 中高。改Schema影响三条核心路径（摄取/召回/生命周期），需要数据迁移。

> **明镜评审补充：** LanceDB已有完整的schema evolution机制（`init_table()` → `add_columns` + `fix_null_columns`），新增 `project_path` 列只需在 `schema()` 函数添加 `Field::new("project_path", DataType::Utf8, true)` 即可，现有机制自动处理迁移，**不需要重建表**。

> **玄机评审补充：** `space_id`（访问控制）和 `project_path`（上下文过滤）是正交维度，需明确召回过滤顺序：先 `accessible_spaces`（访问控制）→ 再 `project_path`（上下文）→ 再 `category/scope`（类型）。

---

### Phase 4: 去噪归簇增强（等部署后基于实际效果讨论）

**前置条件：** 召回增强版本部署上线，有实际recall数据。

**增强方向（待验证后确定）：**
- cluster summary自动刷新机制（增量聚类后触发）
- 基于簇的去噪（同簇内相似度过高的记忆合并）
- 召回时注入簇摘要而非单条记忆（减少噪音）

**不在当前设计范围内** — 等Phase 3部署后重新评估。

---

### Phase 5: web端与plugin本地整合

**目标：** plugin和web端合并成一个本地服务 `localhost:5212`（我爱月儿）。

**改动范围：** omo plugin + omem-web前端。

**技术方案：**

```
opencode启动 → 
  omo插件启动 → 
    同时启动Node.js服务器（localhost:5212）
      → 提供web端页面（前端静态资源）
      → 提供plugin的API（代理到远程服务端）
      → 一个进程，两个功能
```

**数据流：**

```
本地 localhost:5212（web前端 + plugin API）
    ↕ 明文（本地环回）
远程服务端（数据存储）
    ↕ 加密传输（Phase 5b 实现）
```

**分两步：**
- Phase 5a：合并服务，本地环回不加密
- Phase 5b：本地到远程的加密传输（方案待设计）

**风险：** 中。需要改plugin启动流程和web端打包方式。

## 七、遗留问题（不在本次重构范围）

| 问题 | 优先级 | 说明 |
|------|--------|------|
| OOM与并发超时 | 中 | 大并发时plugin端超时，可能需要限流+重试 |
| MMR diversity用Jaccard代替向量MMR | 低 | 影响去重质量 |
| Profile LLM集成 | 中 | TODO已标记，画像质量可提升 |
| 全局K-means是破坏性的 | 中 | 需要改为非破坏性重建 |

## 八、验收标准

| Phase | 验收标准 |
|-------|---------|
| Phase 1 | 月儿执行搜索类工具时，先看cerebro注入再决定是否搜记忆 |
| Phase 2 | SQLite字典表可CRUD，服务端启动时自动建表+初始化数据 |
| Phase 3 | 不同项目的记忆互不召回，私密/画像/身份全局可见 |
| Phase 4 | recall噪音降低（待定义量化指标） |
| Phase 5 | `localhost:5212` 可访问web端，plugin API正常工作 |
