# Changelog

## [0.4.0] - 2026-05-21

### Phase 6a: 独立画像系统 (Profile V2)

新增独立于 Memory 管线的偏好画像系统，基于 SQLite 存储，支持 LLM 自动归纳偏好、14 个预定义偏好槽、12 个 REST API 端点。

#### 新增模块: `profile_v2/`

| 文件 | 行数 | 功能 |
|------|------|------|
| `types.rs` | 167 | 数据类型: Preference, InductionRun, InductionLock, ProfileChangelog, ProfileVersion |
| `slots.rs` | 78 | 14 个预定义 slot + `is_valid_slot_name()` 验证 |
| `migration.rs` | 107 | 5 张 SQLite 表 DDL: preferences, induction_runs, induction_locks, changelog, versions |
| `store.rs` | 630 | SQLite 完整 CRUD + DashMap 查询缓存 (TTL=1800s) |
| `service.rs` | 105 | ProfileV2Service 核心 + ProfileConfig |
| `induction.rs` | 328 | 10 步归纳引擎 (LLM 归纳 + induction lock 防并发) |
| `injection.rs` | 171 | 注入协议 (DashMap 缓存 + TTL 过期) |
| `mod.rs` | 7 | 模块声明与导出 |

#### 新增 API (12 个端点)

```
GET    /v2/profile/preferences          查询偏好列表 (支持 slot/scope/project_path 过滤)
POST   /v2/profile/preferences          手动创建偏好
GET    /v2/profile/preferences/:id      获取单条偏好 (含租户隔离校验)
PUT    /v2/profile/preferences/:id      更新偏好
DELETE /v2/profile/preferences/:id      软删除偏好
POST   /v2/profile/induction            手动触发归纳
POST   /v2/profile/injection            生成注入预览 (不缓存)
GET    /v2/profile/changelog            查看偏好变更历史
GET    /v2/profile/changelog/:id        查看单条变更详情
GET    /v2/profile/versions             查看偏好版本历史
GET    /v2/profile/versions/:id         查看单条版本详情
GET    /v2/profile/stats                偏好统计信息
```

#### 新增配置 (12 个环境变量)

```
OMEM_PROFILE_LLM_PROVIDER          LLM 提供商 (默认复用主 LLM)
OMEM_PROFILE_LLM_API_KEY           Profile LLM API Key
OMEM_PROFILE_LLM_BASE_URL          Profile LLM API 地址
OMEM_PROFILE_LLM_MODEL             Profile LLM 模型名
OMEM_PROFILE_INDUC_ENABLED         是否启用归纳 (默认 true)
OMEM_PROFILE_INDUC_MIN_TEXTS       触发归纳最小文本数 (默认 3)
OMEM_PROFILE_INDUC_MIN_INTERVAL_SECS  归纳最小间隔秒数 (默认 300)
OMEM_PROFILE_INJECTION_ENABLED     是否启用注入 (默认 true)
OMEM_PROFILE_CACHE_TTL_SECS        缓存 TTL 秒数 (默认 1800)
OMEM_PROFILE_AUTO_INDUC_ENABLED    ingest 后自动触发归纳 (默认 true)
OMEM_PROFILE_MAX_CANDIDATES        单次归纳最大候选数 (默认 10)
OMEM_PROFILE_LLM_RESPONSE_FORMAT   LLM 响应格式 (可选)
```

#### 修改文件

| 文件 | 改动 | 说明 |
|------|------|------|
| `config.rs` | +66 行 | 新增 12 个 profile_* 配置字段 |
| `llm/mod.rs` | +10 行 | `create_profile_llm_service()` 工厂函数 |
| `llm/openai_compat.rs` | +35 行 | `new_profile()` 构造器 |
| `api/handlers/profile_v2.rs` | +521 行 | 12 个 handler 实现 |
| `api/handlers/mod.rs` | +6 行 | profile_v2 模块注册 |
| `api/handlers/memory.rs` | +2 行 | pipeline 创建时传递 induction_engine |
| `api/router.rs` | +24 行 | /v2/profile 路由组 (9 条路由) |
| `api/server.rs` | +6 行 | AppState 新增 3 字段: profile_v2_service, induction_engine, injection_builder |
| `api/mod.rs` | +9 行 | setup_app() 测试辅助函数初始化 profile_v2 组件 |
| `api/handlers/stats.rs` | +23 行 | 测试中 AppState 添加 profile_v2 字段 |
| `main.rs` | +25 行 | ProfileStore + InductionEngine + InjectionBuilder 初始化序列 |
| `lib.rs` | +1 行 | profile_v2 模块声明 |
| `lifecycle/scheduler.rs` | +113 行 | 3 个定时任务: dormant 检查, deleted 清理, 过期 lock 清理 |
| `ingest/pipeline.rs` | +21 行 | induction_engine 字段 + detached spawn 归纳触发 |

#### 架构设计

- **存储**: SQLite (5 张表)，不使用 LanceDB，避免 OOM 风险
- **归纳**: 独立 LLM 实例 (可配置)，induction lock 防止同一租户并发归纳
- **注入**: DashMap 缓存 + TTL 过期，支持租户+项目粒度
- **集成**: ingest pipeline 通过 detached tokio::spawn 异步触发，不阻塞主管线
- **安全**: get_preference API 包含租户隔离校验，防止跨租户信息泄露

#### 测试

- profile_v2 模块 24 个内联测试全部通过
- cargo check: 0 error, 0 warning

#### 未完成 (Phase 6b scope)

- Plugin 端 (OpenCode) 调用 injection API 并追加到 system prompt
- v1 → v2 数据迁移
- 归纳 spawn 并发 semaphore 限制

---

## [0.3.2] - 2026-05-19

### 新增
- `shouldRecall` 四档信号系统 (skip/weak/normal/strong) + skip_llm_gate 首轮加速
- 灵魂低语 Soul Whisper — 工具调用时注入记忆提醒
- Phase 3 memory isolation — project_path 全链路适配
- 启动时自动为已有 tenant seed categories
- UpdateMemoryBody 支持 category 和 project_path 字段修改
- Session recall 表独立 GC + 启动优化递归扫子目录，内存从 35% 降至 5%

### 修复
- refine_strategy none→loose, pipeline 只认 loose 跳过 LLM 精炼, 9s→802ms
- 首条消息跳过 LLM 精炼 (refine), 6.5s→0.5s
- backfill 跳过 preferences 类别记忆
- scalar update path 补充 project_path column 写入
- 移除 gc_lock 解决 OOM 写饥饿 + recall 表 GC
- 配置编译目录到项目外，避免 CodeGraph 索引 target 目录
