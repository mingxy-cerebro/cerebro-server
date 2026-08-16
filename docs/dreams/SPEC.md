# Dream 一期规格书(Dream Spec v1)

> 状态:**已批准开工** —— 2026-08-16 定稿法旨落地,批复:D-1 全量方案准+补 type 字段、D-2 准、ADR×4 认可
> 本文档是 dream 功能的唯一权威 spec,后续所有决策与变更记录在案(§9 变更日志)。

---

## 1. 背景与使命

让服务端利用 deepseek-v4-flash 通道,对调用方(Claude Code hook)提交的「记忆档全文 + 新 session 证据」做一次离线整理——去重、合并、更新、挖掘新条目,产出一份结构化的新记忆档。类比"睡觉做梦时大脑整理白天的记忆"。

**一期铁律:无状态引擎**——输入只读,绝不回写调用方数据;结果独立返回,服务端不留档。

## 2. 法旨原文(2026-08-16 定稿,逐字存档)

```
【流程令】
1. research skill:先建 docs/dreams/(或项目惯用 docs 位置),把本法旨全文 + 你侦察的三项报告固化成 spec 文档,后续所有决策与变更记录在案
2. domain-modeling:钉死领域词(Dream / DreamJob / 食材 payload / 结果 schema),写进 spec
3. codebase-design:handler 按现有 axum 模式设计深模块,接口定稿写进 spec
4. tdd:红绿重构实现,测试先行

【需求定稿(一期)】
一、API(套 imports 范式,axum authed_routes)
- POST /v1/dreams:收 {memory: 记忆档全文, sessions: [待消化文本数组], since: RFC3339},立即返回 job id,入异步任务
- GET /v1/dreams/{id}:轮询状态,completed 时附结果
- 超时参照现有 30s TimeoutLayer 之外的任务级超时(LLM read 90s × 重试3次,任务级预算你评估后写进 spec)

二、引擎
- 复用 profile_llm 通道(deepseek-v4-flash)+ complete_json<T>(),不新建通道
- 任务引擎职责:①记忆条目去重合并 ②旧值被 sessions 新证据推翻时更新 ③从 sessions 挖未落档的新条目(打 added 标)
- 铁律:输入只读,绝不回写调用方数据;结果独立返回,服务端不留档(无状态引擎)

三、输出 schema(纯 JSON,禁 markdown/YAML)
{entries: [{name, description, body, links: []}], stats: {merged, updated, added, dropped, total}}
每条 entry 需带来源类型标记(merged/updated/added),供调用方做 diff 呈报

四、SSE 钩子(一行的事)
- 任务完成时向 EventBus 发 dream.completed 事件,payload 带 stats——web 端二期才做管理界面,一期只埋事件流

五、边界(明确不做)
- 不做触发节奏(调用方 CC hook 管:上线=距上次≥6h且新增session≥2,兜底24h;测试期=1h/1session——全是本地参数,服务端无感)
- 不做审阅/回滚(CC 本地管)
- 不做 web 管理页(二期,等 dream 结果入库后)
- 不做 cerebro 记忆库整理(二期)

【验收】
spec 文档落地 → domain model 入档 → TDD 绿 → 手测一轮 POST/GET 循环 → 汇报。每步完成向我传话,重大决策先报后动。
```

## 3. 侦察报告(2026-08-16 只读侦察,已验证)

### 3.1 HTTP 路由框架
- **axum + tokio**。路由表集中注册:`omem-server/src/api/router.rs` 的 `build_router()`(router.rs:14)。
- 两块结构:`authed_routes`(全部 /v1 业务接口,挂 `auth_middleware` + 30s `TimeoutLayer`)+ `public_routes`(/health、/v1/tenants、github webhook、SSE `/v1/events`)。
- handler 按域分文件:`omem-server/src/api/handlers/`(memory.rs、merge.rs、imports.rs…),签名统一 `State(Arc<AppState>) + Extension(AuthInfo) + Json/Path/Query`。
- `AppState` 在 `omem-server/src/api/server.rs:22`,全局依赖注入中心(126 处引用)。
- 新增接口成本 = router.rs 一行 + handlers/ 一个文件。

### 3.2 异步任务机制
- 无持久化队列、无通用 job 框架。现有四种机制:
  - **LifecycleScheduler**(`src/lifecycle/scheduler.rs`):tokio::spawn 常驻循环,定时维护,支持手动触发/pause/status。
  - **imports 范式**(dream 直接套用):`POST /v1/imports` 立返 task id(uuid v4 + rfc3339 时间戳),后台 `tokio::spawn` + semaphore permit,`GET /v1/imports/{id}` 轮询。ImportTaskRecord 持久化在 space_store(LanceDB)。
  - **induction 范式**:trigger + runs 查询,带 cooldown。
  - **并发闸 + 事件**:import_semaphore(3) / reconcile_semaphore(1) / ingest_semaphore(10);EventBus(broadcast 256)发 SSE 进度事件。

### 3.3 deepseek-v4-flash 调用封装
- 即 **profile LLM 通道本尊**:`profile_llm_model="deepseek-v4-flash"`、`profile_llm_base_url="https://opencode.ai/zen/v1"`、provider="openai-compatible"(`src/config.rs:150-153`)。env 前缀 `OMEM_PROFILE_LLM_*`。
- 三层封装:
  - trait `LlmService`(`src/llm/service.rs:4`):单方法 `complete_text(system, user)`。
  - `OpenAICompatLlm`(`src/llm/openai_compat.rs`):reqwest 直调 chat/completions,temperature 0.1,重试 3 次指数退避,connect 10s / read 90s,支持 response_format 与 thinking 字段。
  - 工厂 `create_profile_llm_service`(`src/llm/mod.rs:43`):`profile_enabled=false` 或 key 空 → NoopLlm。
- 高层工具 `complete_json<T>()`(`src/llm/service.rs:57`):剥 `<think>` 标签/markdown 围栏 + JSON 修复(尾逗号/未转义字符)+ 一次带错误提示重试。
- **现状缺口**:`create_profile_llm_service` 的产物目前只注入 `ProfileV2Service`,AppState 无此通道 → dream 需将其同时注入 AppState(见 ADR-2)。
- 先例:`handlers/merge.rs` 的 `merge_memories` 走通 prompt → complete_json → 结果回传 全流程,是 dream 引擎的直接参照。

## 4. 领域模型

统一语言表。代码、spec、API 文档一律用这套词:

| 术语 | 定义 |
|------|------|
| **Dream** | 一次记忆整理任务的整体概念:"用新 session 证据消化既有记忆档"。 |
| **DreamRequest**(食材 payload) | POST /v1/dreams 的请求体:`{memory, sessions[], since}`。memory=记忆档全文(自由文本,通常 markdown);sessions=待消化文本数组(每项一段 session 摘要/记录);since=RFC3339 时间戳,标识本次食材的时间下界(元数据,引擎不参与推理,原样回显)。 |
| **DreamJob** | 一次 Dream 的运行记录:`{id, tenant_id, status, result?, error?, created_at, started_at?, completed_at?}`。生命周期:pending → running → completed / failed。 |
| **DreamStatus** | job 状态枚举:pending / running / completed / failed。 |
| **DreamResult** | 引擎产出:`{entries: DreamEntry[], stats: DreamStats}`。纯 JSON。 |
| **DreamEntry** | 新记忆档中的一条:`{name, description, type, body, links[], source}`。source ∈ {merged, updated, added, kept}。kept 条目允许极简形态 `{name, source}`(其余字段缺省,调用方补全,见 D-1b)。 |
| **EntrySource** | 条目来源标记:merged(多条旧条目合并)/ updated(旧条目被新证据修订)/ added(从 sessions 新挖)/ kept(原样保留,未受影响)。 |
| **EntryType** | 条目记忆类型,四类枚举:user(用户画像)/ feedback(工作指导)/ project(项目动态)/ reference(外部指针)。源档 frontmatter 的 type 透传保留;新挖条目(added)由 LLM 归类四选一。 |
| **DreamStats** | `{merged, updated, added, dropped, total}`。dropped=旧档中被判定淘汰的条数;total=entries 总数(含 kept)。 |
| **DreamJobStore** | 内存 job 表(DashMap),承载 DreamJob 生命周期。**不持久化**(ADR-1)。 |
| **dream.completed** | SSE 事件:job 终态(completed)时发布,payload 带 stats + job id。 |

不变式:
- `stats.total == entries.len()`
- `stats.merged + stats.updated + stats.added + kept数 == total`(kept 数不单独出现在 stats,由 total 反推)
- `entry.type ∈ {user, feedback, project, reference}`
- 引擎输入 `DreamRequest` 与输出 `DreamResult` 之间无任何服务端存储副作用。

## 5. API 契约

### 5.1 POST /v1/dreams(authed)

请求体(JSON):
```json
{
  "memory": "<记忆档全文,自由文本,非空>",
  "sessions": ["<session 文本 1>", "<session 文本 2>"],
  "since": "2026-08-15T00:00:00Z"
}
```

校验(失败 → 400 Validation):
- `memory` 非空;`sessions` 非空且 ≤ 50 条
- body 总大小 ≤ 2MB(axum 默认 DefaultBodyLimit,不额外放宽)
- `since` 可解析 RFC3339(可选字段,缺省合法)
- 服务端 dream_llm 通道未配置(NoopLlm)→ 400 "dream requires dream LLM"(见 §7 错误映射)
- 并发闸:排队中 job(pending/running)≥ 8 → 429 RateLimited

成功响应 202(语义:已受理,异步执行):
```json
{
  "id": "<uuid v4>",
  "status": "pending",
  "created_at": "<rfc3339>"
}
```

### 5.2 GET /v1/dreams/{id}(authed)

- job 存在且属当前 tenant → 返回 DreamJob 全量
- 不存在或属于他人 → 404(不泄露存在性)
- completed 态:
```json
{
  "id": "...", "tenant_id": "...", "status": "completed",
  "created_at": "...", "started_at": "...", "completed_at": "...",
  "result": {
    "entries": [{"name": "...", "description": "...", "type": "feedback", "body": "...", "links": [], "source": "kept"}],
    "stats": {"merged": 2, "updated": 1, "added": 3, "dropped": 1, "total": 12}
  }
}
```
- failed 态:`"error": "<原因摘要>"`,无 result。

### 5.3 SSE(dream.completed)

job 到达 completed 时经 EventBus 发布:
```json
{"event_type": "dream.completed", "tenant_id": "...", "data": {"job_id": "...", "stats": {...}}, "timestamp": "..."}
```
(failed 不发事件——一期无消费方,不值得噪音。)

## 6. 引擎设计

纯函数,深模块窄接口:

```rust
pub async fn run_dream(llm: &dyn LlmService, req: &DreamRequest) -> Result<DreamResult, OmemError>
```

- 唯一外部依赖 = llm 参数(测试注 fake);不碰 store、不碰AppState。
- 内部:构造 prompt(见 6.2)→ `complete_json::<DreamResult>()` → 校验不变式(stats 与 entries 自洽,不自洽则按 entries 重算 stats 兜底)。
- 任务级超时:`tokio::time::timeout(600s, ...)` 包裹整个 run_dream 调用(handler 层),超时 → job failed("dream engine timeout")。
  - 预算推演:LLM 单次调用最坏 read 90s × 内部重试 3 ≈ 270s;complete_json 失败重试一轮再 270s;合计最坏 ≈ 540s,取 600s 封顶。

### 6.1 职责(法旨三条)

1. **去重合并**:多条旧条目语义重复 → 合并为一条,source=merged
2. **证据更新**:旧条目断言被 sessions 新证据推翻/修订 → 更新内容,source=updated
3. **新知挖掘**:sessions 中含未落档的持久事实 → 新增条目,source=added,并由 LLM 归类 EntryType 四选一
4. (补)未受影响条目原样保留,source=kept(见 D-1)
5. (补)旧档中过时/无价值条目淘汰 → 不入 entries,计入 dropped
6. (补)源档 frontmatter 的 `type` 字段透传到 EntryType;源档无 type 的条目由 LLM 归类(ADR-5)

### 6.2 Prompt 策略

- system:角色定位(记忆整理师)+ 职责六条 + 输出 JSON schema 严格约束(禁 markdown/YAML/思考标签)+ 不变式自检要求 + type 透传/归类规则
- user:记忆档全文(含 frontmatter) + sessions 逐条编号 + since 回显
- 语言:输出跟随输入主体语言(中文档 → 中文 entries)

## 7. 模块设计(深模块)

```
omem-server/src/dream/mod.rs        — 领域类型 + DreamJobStore + run_dream 引擎 + spawn 生命周期
omem-server/src/dream/prompts.rs    — prompt 构造(独立可测)
omem-server/src/api/handlers/dreams.rs — create_dream / get_dream 薄 handler
omem-server/src/api/router.rs       — +2 行路由
omem-server/src/api/server.rs       — AppState + dream_llm / dream_jobs
omem-server/src/main.rs             — create_dream_llm_service 产物注入 AppState
omem-server/src/config.rs           — OMEM_DREAM_LLM_* 配置组
omem-server/src/llm/mod.rs          — create_dream_llm_service 工厂
omem-server/src/llm/openai_compat.rs — OpenAICompatLlm::new_dream 构造器
```

- **`src/dream/mod.rs` 是深模块**:外界只见 `DreamJobStore::spawn(llm, req, tenant_id) -> DreamJob` 与 `get(&id, &tenant_id) -> Option<DreamJob>` 两个操作;job 生命周期、超时、过期清理、事件发布全部封在里面。
- handler 极薄:校验 → 调 store → 回 JSON。
- 错误映射:OmemError 现有变体(Validation→400 / NotFound→404 / RateLimited→429 / Llm→500);dream_llm 未配置 → `OmemError::Validation("dream requires dream LLM (OMEM_DREAM_LLM_*)")`。
- job 过期:终态后 1h 由 detached tokio 任务自动从表中移除(sleep+remove,近乎零成本);表内上限 128 条硬顶(超出时清最早终态项),防内存无界。重启 = 表清零,调用方超时重试即可(引擎幂等,无副作用)。
- `# ponytail: 内存 job 表,重启丢任务;调用方(CC hook)有 24h 兜底重试,需要跨重启续跑时再落 SQLite`

## 8. 决策记录(ADR + 待确认项)

| # | 决策 | 理由 | 状态 |
|---|------|------|------|
| ADR-1 | job 状态存内存 DashMap,不落库 | 法旨"服务端不留档/无状态引擎"最直接实现;重启丢 job 无害——结果本就不留档,调用方重发即可;ponytail 最短路径 | 已定 |
| ADR-2 | ~~复用 profile_llm 通道~~ **已被 ADR-6 取代**:dream 走独立 `dream_llm_*` 配置组,AppState 持 `dream_llm: Option<Arc<dyn LlmService>>` | 初版复用 profile 通道;师尊急令改道(deepseek 官方),与 profile_llm 解耦互不影响 | 已修订(2026-08-16) |
| ADR-3 | 任务级超时 600s | §6 推演:最坏 540s + 余量 | 已定 |
| ADR-4 | dream_semaphore = Semaphore(1),队列上限 8 → 429 | 单 LLM 通道并行无益;3.4Gi 小机防堆积;imports 同款 semaphore 手法 | 已定 |
| D-1 | **entries = 完整新记忆档**(未变条目 source=kept 一并返回),而非"仅变更条目" | stats.total 独立于 m+u+a 存在,暗示全量;调用方拿 entries 可直接整体替换记忆档文件,免本地合并逻辑;diff 呈报靠 source 标记;与官方 platform dreams 产出完整新 store 同构 | **已批复(2026-08-16)** |
| D-1b | kept 条目免重写:kept 仅输出 `{name, source}` 两字段(name 与源档逐字一致),description/type/body/links 缺省;调用方从旧档按 name 原样补全 | D-1 全量重写在 30+ 条目记忆档下输出天然 >4k,叠加默认输出上限导致 JSON 被掐断(首夜事故);瘦身 80%+ 且记忆档越滚越大也扛得住 | **修订(2026-08-16 首夜事故后)** |
| D-2 | GET 归属校验:非本人 job 返回 404(imports 的 get_import 现无归属校验,dream 不抄这一点) | id 可枚举探测,跨租户 403 会泄露存在性 | **已批复(2026-08-16)** |
| ADR-5 | DreamEntry 保留 `type` 字段(user/feedback/project/reference 四类):源档 frontmatter type 透传,新挖条目 LLM 归类 | 调用方源档是带 type 的 Markdown frontmatter;全量返回若丢 type,本地补装时类型信息蒸发,记忆召回行为会变 | **已批复(师尊补刀,2026-08-16)** |
| ADR-6 | dream 引擎走 **deepseek 官方 API** 独立通道(`OMEM_DREAM_LLM_*` 配置组),不走 opencode zen 免费通道,不动 profile_llm 现状 | zen 免费通道并发受限且不稳定,dream 任务跑一半断=白梦;官方通道稳定;解耦后 profile 行为零变化 | **师尊急令(2026-08-16)** |

## 9. 变更日志

| 日期 | 变更 |
|------|------|
| 2026-08-16 | 初版:法旨固化 + 侦察报告 + 领域模型 + API 契约 + 模块设计 + ADR×4 + 待确认项×2 |
| 2026-08-16 | 批复落地:D-1/D-2 获准;ADR-5 新增(DreamEntry.type 保留,schema 四字段含 type,职责+prompt 补透传/归类规则) |
| 2026-08-16 | 师尊硬要求:§12 LanceDB OOM 防护策略入档(一期零接触 + 二期照抄模式清单);装载 omem-internals skill |
| 2026-08-16 | 师尊急令 ADR-6:dream 通道改道 deepseek 官方 API,新增 `OMEM_DREAM_LLM_*` 独立配置组,与 profile_llm 解耦(ADR-2 修订);§13 部署注意重写 |
| 2026-08-16 | 手测验收通过(§10 勾选)。记录:①手测借 siliconflow Qwen3-8B 打通管线,生产换 deepseek 官方 key;Qwen 对 updated 条目标记 source 但 body 未改写——生产首晚重点盯 deepseek 同场景,不改写则 prompt 需加强 ②drvfs(/mnt/d)上 LanceDB 写后读丢元数据,服务数据目录必须在 ext4(WSL home 或服务器),.dream-mt 手测目录已清理 |
| 2026-08-16 | 玄机评审落地:P1-1 /v1/events 挪入 authed_routes(EventSource 走 api_key query,存量消费方需加参数);P1-2 payload 字节上限 512KB(超限 400 Validation——项目错误表无 422 变体,以 400 承载);P2-2 dream LLM Noop 化启动警告日志;P2-1 TOCTOU/P2-4 evict 空跑 不修记录在案;P2-3 型号名核实转部署清单(§13) |
| 2026-08-16 | P2-2 落地偏差回正:`create_dream_llm_service` 原返回 NoopLlm 被 main 包成 Some,导致 key 未配时启动 warn 静音 + spawn 层 400 防线失效(Noop 跑成 failed job)。改为返回 `Option`,未配置= None → 受理层 400 + 启动 `dream LLM is Noop` warn 双达成 |
| 2026-08-16 | 首夜真梦梦碎修复(job 1d24164e failed: JSON EOF——输出被掐断):双刀落:①`new_dream` 通道加 `max_tokens: 8192`(deepseek 默认输出上限 ~4k,ChatRequest 透传,其余通道 None 不受影响);②kept 条目免重写(D-1b):prompt 要求 kept 仅输出 name+source 两字段,serde 层 description/type/body/links 加 default 兜缺省,调用方从旧档按 name 原样补全。输出体积降 80%+ |

## 10. 验收清单

- [x] spec 文档落地(本文档)
- [x] D-1 / D-2 获确认(2026-08-16 批复,D-1 附带 ADR-5 type 字段)
- [x] TDD 红→绿:引擎纯函数(fake Llm)、JobStore 生命周期、handler 校验、prompt 构造 —— 17/17 绿
- [x] `cargo test` 全绿;`cargo build` 通过 —— dream 17/17;全量 528 测 492 绿,36 挂为预存环境病(reqwest 0.13.2 测试进程无 TLS provider,失败模块 github/embed/reranker 零改动,改道前后失败数相同)
- [x] 手测:POST /v1/dreams → 轮询 GET 至 completed → result schema 与不变式自洽(total=5=merged1+updated1+added1+kept2),type 透传四类全对(LLM 真调用,45s)
- [x] SSE dream.completed 事件在 /v1/events 可观测(payload 带 job_id + stats)
- [x] 401(无 key)/404(空 id)错误路径验讫
- [ ] codegraph sync(顺序:pull → add → commit → push → sync,2026-08-16 定)

## 11. 边界(法旨五不做,重申)

1. 触发节奏:调用方 CC hook 自管(上线 ≥6h 且 ≥2 新 session,兜底 24h;测试期 1h/1 session)——服务端无感
2. 审阅/回滚:CC 本地管
3. web 管理页:二期
4. cerebro 自家记忆库整理:二期
5. (一期不做)结果入库:dream 结果只回调用方,不写 LanceDB/SQLite

## 12. LanceDB OOM 防护策略(师尊硬要求,2026-08-16 补)

> 服务器仅 3.4Gi 内存,历史两次事故同源:① 8/12 内存 93% 濒死(rebuild 风暴:30 分钟 scheduler 全量遍历 99 space,drop 7 索引从零重训)② 冷请求 11s(惰性 init 持全局锁干重活)。防护模式全部照抄现有代码,不发明新的。

### 12.1 一期:dream 零 LanceDB 接触

无状态引擎铁律的直接红利——不读库、不写库、不碰索引,天然免疫上述两类事故。

一期实际做的内存防护(全在 DreamJobStore 内):
- job 表硬顶 128 条(`MAX_JOBS`,超顶清最早终态项)
- 终态 job TTL 1h 自动清理
- 排队上限 8(`MAX_ACTIVE`,超出 429)
- `Semaphore(1)` 串行执行,LLM 通道不并发
- body ≤ 2MB(axum 默认 DefaultBodyLimit)

### 12.2 二期若结果入库,必须照抄的现有模式

| 模式 | 位置 | 作用 |
|------|------|------|
| 写路径版本 GC | `store/lancedb.rs:2582` `after_mutation()`(8 处 caller) | 每次 mutation 后检查,版本数超 `GC_VERSION_THRESHOLD`(2577)触发 GC,防版本爆炸 |
| 优化阈值闸 | `store/lancedb.rs:2807` `maybe_optimize()` | 版本 <50 时 no-op,不乱 compact |
| GC 防重入 | `LanceStore.gc_running: Arc<AtomicBool>` | GC 进行中不重入 |
| 版本修剪 | `store/lancedb.rs:2825` `prune_old_versions()` | 直接删旧版本 manifest |
| 分页遍历 | `lifecycle/scheduler.rs:309` batch_size=100 + offset | 全表操作分页拉取,不全量载入内存 |
| LRU store 缓存 | `store/manager.rs`(128 槽) | store 句柄常驻,防 LRU miss 抖动 |
| 并发闸 | AppState 各 `Semaphore` | import 3 / ingest 10 / reconcile 1,dream 若入库同款 |
| scalar 更新走原位 | `store/lancedb.rs:1723-1802` | 标量更新走 `table.update().column()`,不走 delete+re-insert(后者每次产生新版本,版本膨胀) |
| 重活离请求路径 | scheduler / 手动 trigger | optimize 类重活绝不放请求路径,不在锁内干重活 |

二期实现时,本表为验收检查项:每条在 PR 里对号。

### 12.3 已知 P2(记录不阻塞,2026-08-16 玄机自查轮)

| # | 事项 | 处置 |
|---|------|------|
| P2-1 | axum body 2MB 上限 ≫ deepseek-v4-flash 上下文窗,超大输入在 LLM 侧 400 → job failed(错误路径正确,体验糙)。真实场景(CC hook 记忆档几十 KB)不触 | 二期视需要 `DefaultBodyLimit` 收紧,一行 |
| P2-2 | run() 若 panic 则 job 卡 running 且无 TTL 清理(complete_json 全 Result 流,概率极低) | MAX_JOBS=128 硬顶兜底泄漏上界;不为此加 catch_unwind |
| P2-3 | 600s 任务预算复核:90s×3重试×2轮 complete_json ≈ 541s < 600s,余 59s;参数漂移最坏也是 failed(timeout) 干净收场,不悬挂 | 无需改动,账目存档 |

## 13. 部署注意

**dream 走独立配置组 `OMEM_DREAM_LLM_*`(ADR-6),生产 `/opt/omem/.env` 需补**(key 空时接口返回 400 "dream requires dream LLM"):

```
OMEM_DREAM_LLM_PROVIDER=openai-compatible
OMEM_DREAM_LLM_API_KEY=<deepseek 官方 key>
OMEM_DREAM_LLM_MODEL=deepseek-v4-flash
OMEM_DREAM_LLM_BASE_URL=https://api.deepseek.com
```

- model/base_url 与代码默认一致(deepseek 官方端点),显式写出防默认漂移;**默认型号名 `deepseek-v4-flash` 未经官方目录核实(P2-3)——部署换官方 key 时顺手核对 deepseek 官方模型表的真实型号名,不符则 env 覆盖 `OMEM_DREAM_LLM_MODEL`**。
- key 未配/拼错时服务能起但 /v1/dreams 返回 400;启动日志有 `dream LLM is Noop` 警告(P2-2),部署验收时 journalctl 里应看不到该行。
- **不动 profile_llm 现状**:profile induction 继续走原通道(或维持未配置),dream 与 profile 完全解耦。
- zen 免费通道(`https://opencode.ai/zen/v1`)明确不用于 dream:并发受限、不稳定,任务中断=整梦白跑。
