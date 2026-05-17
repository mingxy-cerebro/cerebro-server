# 记忆系统重构 — 全局设计规格书

> 师尊半夜失眠的灵感 → 月儿架构分析 + 师尊决策确认 → 本文档
> 
> 原始灵感：`/mnt/d/dev/github/project/记忆系统重构-灵魂低语.md`
> 
> **开发流程：** 各Phase实施时使用 `/omem-iteration` skill 进行迭代管理（需求→实施→验证→部署）。

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
| 偏好散落在Memory中，碎片化/冲突/粒度不一 | 画像注入质量差 | 子项目六（画像系统） |

## 三、师尊确认的关键决策

| 决策项 | 师尊的选择 | 理由 |
|--------|-----------|------|
| 隔离基座 | space_id + project_path 双重隔离 | 双重保障 |
| 去噪归簇 | 增强现有cluster模块，不重写 | 已有基础完整，只需修补 |
| 本地整合 | 先合并服务，加密后做 | 分步降低风险 |
| 字典表存储 | 引入SQLite | 轻量单文件，完美适配字典表 |
| 召回增强 | 先部署当前优化版本，基于实际效果再讨论子项目二 | 不纸上谈兵 |
| 画像系统 | 偏好/画像从Memory管线剥离，建独立Profile聚合系统 | Memory向量搜索找冲突不可靠 |
| 画像存储 | 服务端SQLite | web端管理 + 多客户端一致性 |
| 旧数据迁移 | 不自动迁移，新系统从零积累 | 旧数据质量差，直接迁移带垃圾 |
| 偏好粒度 | 一条偏好15-30字，一句话一个维度 | 直接决定注入token消耗 |
| 偏好隔离 | 存储时分离（project scope / global scope） | 举证→归纳→晋升的两阶段设计 |
| 偏好晋升 | scope晋升：project → global，confidence跟随reinforce增长 | 没有晋升=情境和偏好混为一谈 |
| 偏好否定 | 两步处理：删除/降权相反偏好 + 创建否定偏好 | 归纳引擎支持 action:remove |
| Slot类型 | 分单值slot和多值slot，定义中标注cardinality | 单值冲突取高conf，多值可共存 |
| 归纳LLM | OpenCode Zen deepseek-v4-flash（免费），关thinking | 与服务端收费LLM区分 |
| 跨项目匹配 | 走LLM判断（批量处理控制成本） | 纯文本匹配无法处理"简洁"vs"不废话" |

## 四、执行顺序（师尊钦定）

```
Phase 0: Category枚举改造 — P0阻断项，必须在Phase 2之前
Phase 1: 子项目一（灵魂低语）     — 改动最小，验证hook机制
Phase 2: 子项目五（字典表SQLite） — 基础设施层
Phase 3: 子项目三（记忆隔离基座） — Schema + 过滤
Phase 4: 部署召回增强 → 基于实际效果讨论子项目二（去噪归簇增强）
Phase 5: 子项目四（本地整合）     — 独立线程
Phase 6a: 子项目六a（画像系统—新建模块）— SQLite + 归纳引擎 + /v2/路由
Phase 6b: 子项目六b（画像系统—切换注入源+旧模块下线）— 观察1-2周后执行
```

**为什么这个顺序：**
- Phase 0 Category枚举改造是P0阻断项——不解决这个，Phase 2字典表加新category会导致Rust代码crash
- Phase 1 只改omo plugin hook配置，不碰服务端代码，零风险，先验证hook可行性
- Phase 2 建字典表基础设施，后续所有模块依赖category定义
- Phase 3 改Schema加字段，影响摄取/召回/生命周期三条路径
- Phase 4 等部署后再讨论，不纸上谈兵
- Phase 5 独立改动，不影响数据架构
- Phase 6a 新建profile_v2模块，使用/v2/profile/路由前缀，不碰旧模块
- Phase 6b 观察1-2周后切换注入源到新SQLite，废弃旧profile模块

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

   ┌──────────────────────────────────────┐
   │ Phase 6a: 新建profile_v2 + /v2/路由   │
   │ Phase 6b: 切换注入源 + 旧模块下线      │
   │ 依赖: Phase 2 (SQLite) + Phase 3 (project_path) │
   └──────────────────────────────────────┘
```

**关键耦合：**
- Phase 3 和 Phase 4 必须串行（隔离基座不做好，去噪归簇无法按项目隔离）
- Phase 5 完全独立，不依赖其他Phase
- Phase 1 技术上独立，先做是为了验证hook可行性
- Phase 6a → 6b 必须串行（6a新模块就绪 → 观察 → 6b切换注入源）
- Phase 6a 依赖 Phase 2（SQLite基础设施）+ Phase 3（project_path字段）

> **注意：** 依赖图展示的是技术依赖关系（Phase 1与其他Phase无强依赖），不代表执行顺序。执行顺序见第四节。

## 六、各Phase详细设计

### Phase 0: Category枚举改造（P0阻断项）

> **注意：** 本节从Phase 3章节独立出来，与执行顺序保持一致。

**问题：** 当前 `Category` 是Rust enum（6变体：Profile/Preferences/Entities/Events/Cases/Patterns），`FromStr` 会拒绝所有未知值。Phase 2要新增8个category，直接加字典表行 ≠ Rust枚举加变体，会导致反序列化crash。

**解决方案：** 将 `Category` 从 enum 改为 newtype wrapper `Category(String)`，配合SQLite字典表做运行时校验。

**分两步实施（避免一次性改类型+改数据源）：**

1. **Phase 0：纯newtype改造** — `Category(String)` + 硬编码match兜底 + 手动实现`Serialize`/`Deserialize`
   - `is_always_merge()` / `is_append_only()` / `is_temporal_versioned()` / `is_merge_supported()` 改为 match 已知字符串 + fallback 默认值
   - 手动实现 serde（`#[serde(rename_all = "lowercase")]` 对enum自动生效，newtype需手动实现），确保LanceDB中已有的 `"preferences"` 等字符串存储数据反序列化不断裂
   - `prompts.rs` 的valid categories列表暂保持硬编码
   - 影响文件：`category.rs`, `reconciler.rs`, `extractor.rs`, `admission.rs`, `memory.rs`, `domain/` + 对应测试
   - 工作量：Medium（132处引用，18个文件，但多为 use 导入 + 测试代码，真正改逻辑的集中在2-3个文件）

2. **Phase 2：字典表驱动** — Phase 0 SQLite就绪后，行为方法改为从字典表查配置
   - 注意：`is_always_merge()` 等方法当前调用处是同步代码（如 `reconciler.rs:486`），改为 async 或引入内存缓存
   - `prompts.rs` 的valid categories列表从字典表动态生成

**Serde兼容性关键细节：**

```rust
// 改造前（enum，serde自动处理）
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category { Profile, Preferences, ... }

// 改造后（newtype，需手动实现以兼容LanceDB已有数据）
pub struct Category(String);

impl Serialize for Category {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_lowercase())
    }
}

impl<'de> Deserialize<'de> for Category {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Category(s.to_lowercase()))
    }
}
```

---

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

**依赖：** `rusqlite` crate，使用 `bundled` feature（项目有musl静态构建需求，bundled避免系统SQLite版本不一致问题）。在 tokio runtime 中用 `spawn_blocking` 包装同步SQLite操作。

**风险：** 低。新增依赖，不影响现有功能。

---

### Phase 3: 记忆隔离与SessionRecall补字段

**目标：** Memory表新增project_path字段 + 召回时按项目隔离。

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

**3.3 召回隔离策略：**

```
recall时 →
  ├─ 用户画像（category IN ('profile', 'preferences')，全局，不受project_path限制）
  ├─ 身份规则（category='identity'（注：当前不在默认category字典中，需确认是否新增），全局）
  ├─ 私密记忆（visibility='private'，全局，不受project_path限制）
  ├─ 感情记忆（category='emotional'，全局，不受project_path限制）
  └─ 工作记忆（scope='project'时，project_path匹配当前工作目录）
```

**旧数据兼容**：现有数据 `project_path` 全部为NULL，召回时NULL视为global（不过滤），确保旧数据不丢失。

> **玄机评审补充：** `space_id`（访问控制）和 `project_path`（上下文过滤）是正交维度，需明确召回过滤顺序：先 `accessible_spaces`（访问控制）→ 再 `project_path`（上下文）→ 再 `category/scope`（类型）。

**3.4 摄取时隔离标记：**

- `IngestRequest` 已有 `project_name` 字段
- 新增 `project_path` 字段
- 摄取时写入 Memory.project_path
- category字典表查询确定 scope（global/project）

**风险：** 中。LanceDB已有schema evolution机制（`init_table()` → `add_columns` + `fix_null_columns`），新增列不需要重建表。影响三条核心路径但机制成熟。

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

**端口配置：** 通过 `OMEM_LOCAL_PORT` 环境变量配置，默认 `5212`，避免端口冲突。

**风险：** 中。需要改plugin启动流程和web端打包方式。

---

### Phase 6a: 独立画像系统 — 新建模块

**目标：** 将偏好/画像从Memory管线剥离，建立独立的偏好存储、归纳引擎和精炼注入协议。

**依赖：** Phase 2（SQLite基础设施）+ Phase 3（project_path字段）。

**改动范围：** 新增 `profile_v2/` 模块 + 新增 `/v2/profile/*` 路由组。**不碰旧 `profile/` 模块。**

#### 6a.1 核心设计理念

**举证→归纳→晋升的两阶段模型：**
- Memory管线存储**观察证据**（project scope，有project_path）
- 归纳引擎从证据中提取**偏好结论**（可能升级为global scope）
- 注入协议只注入精炼结论，不注入原始证据

**精炼原则：** 一条偏好 = 一个slot的一个值 = 15-30字中文。按token预算反推上限（全局20条+项目10条 ≈ 750字 ≈ 500 tokens）。

#### 6a.2 数据模型（SQLite）

##### 偏好表 `preferences`

```sql
CREATE TABLE IF NOT EXISTS preferences (
    id              TEXT PRIMARY KEY,            -- UUID v7
    tenant_id       TEXT NOT NULL,               -- 租户ID（= API Key）
    slot            TEXT NOT NULL,               -- 偏好维度（如 communication_style, code_style）
    value           TEXT NOT NULL,               -- 偏好值（15-30字中文）
    scope           TEXT NOT NULL DEFAULT 'project', -- 'project' | 'global'
    project_path    TEXT,                        -- project scope时必填，global scope时NULL
    confidence      REAL NOT NULL DEFAULT 0.5,   -- 置信度 0.0~1.0
    status          TEXT NOT NULL DEFAULT 'active', -- 'active' | 'dormant'
    source          TEXT NOT NULL DEFAULT 'observed', -- 'observed' | 'explicit'
    reinforce_count INTEGER NOT NULL DEFAULT 1,  -- 强化次数
    last_reinforced_at TEXT NOT NULL,            -- ISO8601
    created_at      TEXT NOT NULL,               -- ISO8601
    updated_at      TEXT NOT NULL               -- ISO8601
);

-- 防止同一租户+slot+scope+project_path产生重复active偏好
CREATE UNIQUE INDEX IF NOT EXISTS idx_prefs_unique_active 
    ON preferences(tenant_id, slot, scope, COALESCE(project_path, ''), status) 
    WHERE status = 'active';

CREATE INDEX IF NOT EXISTS idx_prefs_tenant_scope ON preferences(tenant_id, scope, status);
CREATE INDEX IF NOT EXISTS idx_prefs_tenant_project ON preferences(tenant_id, project_path, status);
CREATE INDEX IF NOT EXISTS idx_prefs_tenant_slot ON preferences(tenant_id, slot, scope);
CREATE INDEX IF NOT EXISTS idx_prefs_dormant ON preferences(tenant_id, status, last_reinforced_at);
```

##### 画像版本表 `profile_versions`

```sql
CREATE TABLE IF NOT EXISTS profile_versions (
    id              TEXT PRIMARY KEY,
    tenant_id       TEXT NOT NULL,
    version         INTEGER NOT NULL,
    snapshot        TEXT NOT NULL,               -- JSON快照
    preference_count INTEGER NOT NULL,
    trigger_reason  TEXT NOT NULL,               -- 'induction' | 'manual' | 'decay_cleanup'
    created_at      TEXT NOT NULL,
    UNIQUE(tenant_id, version)
);
```

##### 变更日志 `profile_changelog`

```sql
CREATE TABLE IF NOT EXISTS profile_changelog (
    id              TEXT PRIMARY KEY,
    tenant_id       TEXT NOT NULL,
    version_id      TEXT NOT NULL REFERENCES profile_versions(id),
    change_type     TEXT NOT NULL,               -- 'added'|'updated'|'conflict_resolved'|'promoted'|'demoted'|'dormant'|'deleted'
    slot            TEXT NOT NULL,
    old_scope       TEXT,                        -- 变更前scope（晋升时有用）
    new_scope       TEXT,                        -- 变更后scope
    old_value       TEXT,
    new_value       TEXT,
    old_confidence  REAL,
    new_confidence  REAL,
    reason          TEXT NOT NULL,
    created_at      TEXT NOT NULL
);
```

##### 归纳运行记录 `induction_runs`

```sql
CREATE TABLE IF NOT EXISTS induction_runs (
    id                  TEXT PRIMARY KEY,
    tenant_id           TEXT NOT NULL,
    project_path        TEXT,
    trigger_type        TEXT NOT NULL,           -- 'session_end'|'threshold'|'manual'|'cross_project'
    memories_processed  INTEGER NOT NULL DEFAULT 0,
    candidates_extracted INTEGER NOT NULL DEFAULT 0,
    preferences_added   INTEGER NOT NULL DEFAULT 0,
    conflicts_resolved  INTEGER NOT NULL DEFAULT 0,
    llm_tokens_used     INTEGER NOT NULL DEFAULT 0,
    duration_ms         INTEGER NOT NULL DEFAULT 0,
    error               TEXT,
    created_at          TEXT NOT NULL
);
```

##### 归纳并发锁 `induction_locks`

```sql
CREATE TABLE IF NOT EXISTS induction_locks (
    tenant_id       TEXT PRIMARY KEY,
    locked_at       TEXT NOT NULL,
    lock_ttl_seconds INTEGER NOT NULL DEFAULT 600, -- TTL增至600s，防止长归纳被并发打破
    last_renewed_at TEXT,                         -- 锁续约时间（归纳引擎运行期间每60s续约）
    holder_id       TEXT NOT NULL
);
```

##### SQLite文件位置与S3策略

```
本地：{OMEM_DATA_DIR}/profile/{tenant_id}/profile.db
S3：  仅做备份（定期上传），不在S3上直接读写
      运行时始终使用本地文件，S3用于持久化和灾备恢复
```

> **部署模型：** 当前为单实例部署，per-tenant SQLite文件方案完美适配。如未来需多实例，归纳锁需改为基于共享存储的分布式锁。

#### 6a.3 Slot定义

Slot分为**单值slot**（`cardinality: single`，如communication_style只能有一个值）和**多值slot**（`cardinality: multi`，如language可以同时有中文和英文）。

| slot | cardinality | 说明 |
|------|-------------|------|
| communication_style | single | 沟通风格 |
| language | multi | 偏好语言 |
| tone | single | 语气偏好 |
| code_style | single | 代码风格 |
| framework_preference | multi | 框架选择 |
| error_handling | single | 错误处理偏好 |
| naming_convention | single | 命名规范 |
| testing_strategy | single | 测试策略 |
| workflow_preference | single | 工作流偏好 |
| commit_style | single | 提交风格 |
| preferred_tools | multi | 偏好工具链 |
| emoji_preference | single | emoji使用 |
| self_reference | single | 自称偏好 |
| address_style | single | 称呼偏好 |

> slot不硬编码为枚举，存储为TEXT。归纳引擎优先匹配预定义列表，未匹配的自动创建为 `custom:*` 格式。

#### 6a.4 偏好生命周期（状态机）

```
                    ┌──────────────────────────────────────────┐
                    │                                          │
                    ▼                                          │
  ┌─────────┐  归纳引擎  ┌─────────┐  reinforce   ┌─────────┐│
  │ Memory  │ ────────→ │ active  │ ───────────→ │ active  ││
  │ (观察)  │           │ (0.5)   │  +0.15       │ (↑conf) ││
  └─────────┘           └────┬────┘              └────┬────┘│
                             │                        │       │
                    跨项目出现│                        │       │
                    scope晋升│                        │       │
                             ▼                        │       │
                       ┌──────────┐                   │       │
                       │ active   │                   │       │
                       │ (global) │◄──────────────────┘       │
                       │ (+0.2)   │  同项目reinforce          │
                       └────┬─────┘                          │
                            │                                │
               90天无reinforce│                               │
               且conf<0.3    │                                │
                            ▼                                │
                       ┌──────────┐                          │
                       │ dormant  │ ─── reinforce ───────────┘
                       │ (休眠)   │     唤醒，conf→0.5
                       └────┬─────┘
                            │
               180天无       │
               reinforce    │
                            ▼
                       ┌──────────┐
                       │ DELETED  │ (物理删除，changelog保留记录)
                       └──────────┘

  用户明确表达"我偏好X"
       │
       ▼
  ┌──────────┐
  │ active   │  直接创建为 global, confidence=0.9, source=explicit
  │ (global) │
  │ (0.9)    │
  └──────────┘
```

**Confidence规则（纯基于reinforce频率，不做自然衰减）：**

| 操作 | 效果 |
|------|------|
| 初始观察（project） | confidence = 0.5 |
| 用户明确表达（global） | confidence = 0.9 |
| 同项目reinforce | confidence += 0.15（上限0.95） |
| 跨项目晋升 | scope→global, confidence += 0.2（上限0.95） |
| dormant唤醒 | confidence → 0.5 |

**衰减时间线：**

```
Day 0:     最后一次reinforce
Day 90:    若 confidence < 0.3 → dormant
Day 270:   dormant → 物理删除
```

> explicit来源的偏好 confidence=0.9，永远不会触发dormant。

> **设计决策：** confidence不做自然衰减（如Weibull/线性衰减），完全依赖reinforce频率作为"新鲜度"指标。好处是简单可预测，坏处是长期不被reinforce但仍然有效的低confidence偏好可能被误判为dormant——这是可接受的，因为真正重要的偏好会被反复reinforce。

#### 6a.5 归纳引擎

##### 触发机制

| 触发类型 | 触发条件 | LLM调用 |
|----------|----------|---------|
| Session结束 | 每次session ingest完成后（**受冷却期限制**） | 小（仅分析本session新偏好候选） |
| 阈值触发 | 累计≥5条新偏好候选 且距上次归纳≥10分钟 | 中（批量处理候选列表） |
| 跨项目检查 | 每次归纳完成后，检查是否有project偏好可晋升（**批量上限20对**） | 小（LLM语义比对） |
| 手动触发 | API调用 `POST /v2/profile/induction/trigger` | 完整归纳 |

> **冷却机制（所有触发共享）：** 同一租户两次归纳间隔最少 `OMEM_PROFILE_INDUCTION_COOLDOWN_SECS`（默认600秒）。session结束触发也受此限制，避免高频空跑。
>
> **并发控制：** 归纳引擎通过 `induction_locks` 表实现租户级互斥。归纳运行期间每60秒续约锁，防止长归纳被超时打破。

##### LLM配置

```bash
OMEM_PROFILE_LLM_PROVIDER=openai_compat
OMEM_PROFILE_LLM_API_KEY=${OPENCODE_ZEN_API_KEY}    # OpenCode Zen（免费）
OMEM_PROFILE_LLM_BASE_URL=https://opencode.ai/zen/v1
OMEM_PROFILE_LLM_MODEL=deepseek-v4-flash             # 关闭thinking，快速响应
```

> 与服务端收费LLM（`OMEM_LLM_*`）区分。默认复用recall LLM配置，若设置了 `OMEM_PROFILE_LLM_*` 则独立配置。

##### 冲突解决优先级

```
1. explicit > observed      （用户明确声明 > 行为归纳）
2. 新近 > 陈旧              （最近的行为 > 很久以前的行为）
3. confidence高 > 低        （高置信度 > 低置信度）
4. global > project         （全局偏好 > 项目偏好，除非项目有更新证据）
5. reinforce_count多 > 少   （被强化次数多的更可靠）
```

##### 用户否定偏好处理

用户说"我不要emoji"时：
1. 查找相反偏好（"用emoji"），删除或降权
2. 创建否定偏好（"不用emoji"），source=explicit

##### Ingest管线集成

```
现有流程：admission → extract → intelligence → noise → privacy → reconcile → store → done
新流程追加：    store → done
                        │
                        ▼
              trigger_induction()（异步，不阻塞ingest，受冷却期限制）
```

#### 6a.6 注入协议

##### Token预算分配

```
总预算：≈ 500 tokens（约750字中文）

全局偏好：~300 tokens, ≤20条（按confidence降序）
项目偏好：~150 tokens, ≤10条（按confidence降序）
元信息：  ~20 tokens
余量：    ~30 tokens
```

##### 注入格式

```markdown
<cerebro-profile version="42" generated-at="2026-05-17T10:30:00Z">
## 用户画像

### 全局偏好
- 沟通风格：专业简洁，直接给答案不绕弯
- 语言：中文沟通
- 代码风格：整洁可读优于过度聪明
- 自称：月儿/本帝
- 称呼：师尊

### 项目偏好 [/path/to/project]
- 框架：Rust用axum 0.8
- 存储：LanceDB向量+SQLite结构化
</cerebro-profile>
```

##### 注入选择逻辑

```
1. 全局偏好：scope='global', status='active', 按confidence DESC取top 20
2. 项目偏好：scope='project', project_path匹配, status='active', 按confidence DESC取top 10
3. 冲突检测：项目偏好 > 全局偏好（同slot不同value时，全局偏好排除）
4. Token裁剪：超出预算时移除confidence最低的
```

##### 注入时机

| 时机 | 说明 |
|------|------|
| Session开始时 | 插件调用 `GET /v2/profile/inject?project_path=xxx` |
| 归纳完成后 | 异步更新注入缓存 |
| 手动刷新 | `GET /v2/profile/inject?refresh=true` |

##### 注入缓存策略

```
缓存TTL: 30分钟（OMEM_PROFILE_CACHE_TTL_SECS）
缓存失效触发条件：
  1. TTL到期（被动失效）
  2. 归纳完成后主动更新缓存
  3. CRUD操作（POST/PUT/DELETE）后主动失效缓存 ← 防止用户删除偏好后缓存仍注入旧数据
一致性模型：最终一致（归纳进行中的窗口可能读到旧缓存，对偏好类低频变化数据可接受）
```

#### 6a.7 API设计（/v2/profile/ 路由前缀）

##### 偏好CRUD

```
GET    /v2/profile/preferences                    # 查询偏好列表
GET    /v2/profile/preferences/{id}               # 查询单条
POST   /v2/profile/preferences                    # 手动创建（source=explicit, conf=0.9）
PUT    /v2/profile/preferences/{id}               # 手动更新
DELETE /v2/profile/preferences/{id}               # 删除
```

##### 注入端点

```
GET    /v2/profile/inject?project_path=xxx        # 获取注入文本（插件调用）
```

##### 归纳管理

```
POST   /v2/profile/induction/trigger              # 手动触发归纳
GET    /v2/profile/induction/runs/{run_id}        # 查询归纳状态
GET    /v2/profile/induction/runs                 # 归纳历史
```

##### 画像管理

```
GET    /v2/profile                                # 画像快照
GET    /v2/profile/versions                       # 版本历史
GET    /v2/profile/versions/{version}             # 特定版本
GET    /v2/profile/changelog                      # 变更日志
GET    /v2/profile/stats                          # 统计概览
```

> **路由策略：** Phase 6a 使用 `/v2/profile/` 前缀，与旧 `GET /profile` 完全隔离。旧端点继续运行不受影响。

#### 6a.8 模块结构

```
omem-server/src/
  profile_v2/                    # 新的独立画像模块
    mod.rs                       # 模块导出
    service.rs                   # ProfileService: 核心业务逻辑
    store.rs                     # ProfileStore: SQLite存储层
    induction.rs                 # InductionEngine: 归纳引擎
    injection.rs                 # InjectionBuilder: 注入协议
    slots.rs                     # Slot定义和匹配
    migration.rs                 # SQLite表初始化/迁移
    types.rs                     # 类型定义
```

#### 6a.9 现有模块改造（Phase 6a 范围）

| 现有模块 | 改造内容 |
|----------|----------|
| `main.rs` | 启动流程增加 `ProfileStore::init()`, `ProfileService::new()` |
| `api/server.rs` | `AppState` 新增 `profile_v2_service: Arc<ProfileService>` |
| `api/router.rs` | 注册 `/v2/profile/*` 路由组 |
| `api/handlers/` | 新增 `profile_v2.rs` handler文件 |
| `ingest/pipeline.rs` | pipeline完成后异步触发 `trigger_induction()` |
| `lifecycle/scheduler.rs` | 新增定期任务：dormant检查、dormant→deleted清理 |

**Phase 6a 不碰以下模块**（Phase 6b 再处理）：
- ~~`api/handlers/profile.rs`~~ — 旧handler不动
- ~~`ingest/preference_slots.rs`~~ — 旧偏好提取逻辑不动
- ~~`domain/profile.rs`~~ — 旧类型不动
- ~~`profile/service.rs`~~ — 旧画像服务不动

#### 6a.10 生命周期调度器集成

```
现有定期任务：decay / forgetting / tier
新增定期任务：
  - profile_dormant_check: 每6小时检查 dormant 候选
  - profile_cleanup:       每6小时清理已 dormant 180天的偏好
```

#### 6a.11 风险

| 风险 | 缓解措施 |
|------|----------|
| LLM归纳幻觉 | 初始confidence=0.5，偏好需reinforce才晋升，保留用户手动删除能力 |
| 归纳成本 | 候选为空时跳过，使用免费zen LLM，单次≤2000 tokens |
| 语义匹配精度 | confidence保守增长，冲突解决有明确优先级规则 |
| 冷启动 | 无偏好时不注入不报错，自然积累 |

---

### Phase 6b: 画像系统 — 切换注入源 + 旧模块下线

**前置条件：** Phase 6a 上线运行1-2周，新画像系统稳定。

**目标：** 将插件注入源从旧ProfileService切换到新profile_v2，废弃旧模块。

#### 6b.1 切换步骤

1. **插件切换注入端点**：从 `GET /profile` 切换到 `GET /v2/profile/inject`
2. **验证注入质量**：对比新旧注入内容，确认新系统偏好精炼有效
3. **标记旧模块 deprecated**：
   - `profile/service.rs` 标记 `#[deprecated]`
   - `domain/profile.rs` 标记 `#[deprecated]`
   - `ingest/preference_slots.rs` 标记 `#[deprecated]`
4. **移除旧端点**：删除 `GET /profile` 旧handler
5. **清理旧模块**：移除 `profile/` 目录，`preference_slots.rs` 降级为纯候选收集器或移除

#### 6b.2 路由迁移

```
旧路由：GET /profile          → Phase 6b 删除
新路由：GET /v2/profile/*     → Phase 6b 移除 /v2/ 前缀，变为 GET /profile/*
```

#### 6b.3 回滚方案

如果新注入源出现质量问题：
1. 插件切回旧端点 `GET /profile`
2. 新profile_v2继续后台归纳积累（不影响）
3. 修复后重新切换

## 七、遗留问题（不在本次重构范围）

| 问题 | 优先级 | 说明 |
|------|--------|------|
| OOM与并发超时 | 中 | 大并发时plugin端超时，可能需要限流+重试 |
| MMR diversity用Jaccard代替向量MMR | 低 | 影响去重质量 |
| 全局K-means是破坏性的 | 中 | 需要改为非破坏性重建 |

## 八、评审记录

### 明镜评审（架构+完整性）

| 级别 | 问题 | 处理 |
|------|------|------|
| P0 | 新旧画像系统并行运行冲突 | ✅ Phase 6拆分为6a/6b，路由前缀隔离 |
| P1 | Phase 0章节归属在Phase 3内，与执行顺序矛盾 | ✅ 独立为Phase 0章节 |
| P1 | rusqlite未说明feature选择 | ✅ 补充bundled feature说明 |
| P1 | SQLite文件S3场景读写策略不明确 | ✅ 明确S3仅做备份，运行时本地文件 |
| P1 | confidence衰减规则不完整 | ✅ 明确为纯reinforce频率模型，不做自然衰减 |
| P1 | induction_locks缺乏锁续约 | ✅ 增加续约机制（每60s续约） |
| P2 | preferences表缺UNIQUE约束 | ✅ 增加partial unique index |
| P2 | profile_changelog缺new_scope字段 | ✅ 补充new_scope字段 |
| P2 | Phase 5端口硬编码 | ✅ 补充端口配置项 |

### 玄机评审（技术可行性）

| 级别 | 问题 | 处理 |
|------|------|------|
| P0 | 路由冲突 | ✅ /v2/profile/ 前缀隔离 |
| P0 | 双画像并存矛盾 | ✅ 6a新旧隔离，6b切换后下线旧 |
| P1 | Category newtype serde兼容性 | ✅ 补充手动Serialize/Deserialize实现 |
| P1 | 行为方法async化 | ✅ Phase 0纯newtype+硬编码match，Phase 2再改数据源 |
| P1 | 旧数据project_path=NULL召回过滤 | ✅ 补充NULL兜底说明 |
| P1 | 归纳引擎session触发缺冷却 | ✅ 所有触发共享冷却期 |
| P1 | 注入缓存CRUD后未失效 | ✅ 补充缓存失效触发条件 |
| P2 | SQLite多实例部署锁失效 | ✅ 明确单实例部署模型 |

## 九、验收标准

| Phase | 验收标准 |
|-------|---------|
| Phase 0 | Category改为newtype，所有132处引用编译通过，LanceDB旧数据反序列化正常 |
| Phase 1 | 月儿执行搜索类工具时，先看cerebro注入再决定是否搜记忆 |
| Phase 2 | SQLite字典表可CRUD，服务端启动时自动建表+初始化数据，rusqlite bundled编译通过 |
| Phase 3 | 不同项目的记忆互不召回，旧数据（project_path=NULL）正常召回为global |
| Phase 4 | recall噪音降低（待定义量化指标） |
| Phase 5 | `localhost:5212` 可访问web端，plugin API正常工作 |
| Phase 6a | profile_v2模块独立运行，/v2/profile/端点可用，归纳引擎异步触发，注入文本≤500 tokens |
| Phase 6b | 插件注入源切换到新系统，旧模块标记deprecated，回滚方案验证通过 |

## 十、配置项

### 现有配置（不变）

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `OMEM_PORT` | `8080` | HTTP listen port |
| `OMEM_LLM_PROVIDER` | (空) | 主LLM provider |
| `OMEM_LLM_MODEL` | `gpt-4o-mini` | 主LLM模型 |
| `OMEM_RECALL_LLM_*` | (空) | Recall LLM独立配置 |

### Phase 6 新增配置

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `OMEM_PROFILE_ENABLED` | `true` | 画像系统总开关 |
| `OMEM_PROFILE_DB_DIR` | `{data}/profile/` | SQLite存储目录 |
| `OMEM_PROFILE_LLM_PROVIDER` | (空，复用recall) | 归纳LLM provider |
| `OMEM_PROFILE_LLM_API_KEY` | (空，复用recall) | 归纳LLM API Key |
| `OMEM_PROFILE_LLM_BASE_URL` | (空，复用recall) | 归纳LLM Base URL |
| `OMEM_PROFILE_LLM_MODEL` | (空，复用recall) | 归纳LLM模型 |
| `OMEM_PROFILE_INDUCTION_ENABLED` | `true` | 归纳引擎开关 |
| `OMEM_PROFILE_INDUCTION_MIN_CANDIDATES` | `3` | 最小候选数触发归纳 |
| `OMEM_PROFILE_INDUCTION_COOLDOWN_SECS` | `600` | 归纳冷却期（秒，所有触发共享） |
| `OMEM_PROFILE_INJECTION_BUDGET_TOKENS` | `500` | 注入token预算 |
| `OMEM_PROFILE_MAX_GLOBAL` | `20` | 全局偏好上限 |
| `OMEM_PROFILE_MAX_PROJECT` | `10` | 项目偏好上限 |
| `OMEM_PROFILE_DORMANT_DAYS` | `90` | 转入dormant天数 |
| `OMEM_PROFILE_DELETE_DAYS` | `180` | dormant后删除天数 |
| `OMEM_PROFILE_CACHE_TTL_SECS` | `1800` | 注入缓存TTL |
| `OMEM_LOCAL_PORT` | `5212` | Phase 5 本地服务端口 |
