# Phase 3 Plugin 适配 + 老数据补全

## TL;DR

> **核心目标**: opencode plugin 端传递 `project_path` 到所有 ingest/search/recall API；老数据 project_path=NULL 需补全实际路径（否则 plugin 加上隔离后召回不好使）。
>
> **前置**: Phase 3 后端已完成 (Memory.project_path + WHERE 过滤 + sanitize)
>
> **交付物**:
> - opencode client 层加 `projectPath` 参数
> - opencode 所有 hooks 调用点传递 `project_path`
> - opencode 所有 tools 传递 `project_path`（含 description 更新）
> - 老数据补全 API（按 tenant 批量更新 project_path）
> - package.json description 更新
>
> **预估工作量**: Medium（~400行，6文件）
> **范围**: 仅 opencode plugin + 后端老数据补全

---

## Context

### 调研结论

**目录来源**:
- **opencode**: `input.directory`（插件初始化时 OpenCode 传入项目根目录）
- hooks 里从 `sessionInfo?.data?.directory` 获取
- tools 里从 `process.env.OMEM_PROJECT_DIR` 获取（index.ts L157 已设此环境变量）

**关键发现**:
- `detectProjectName()` 从目录提取名称（如 "cerebro"），丢失了完整路径
- 现有 `IngestOptions.projectName` 和 `sessionIngest.projectName` 只传 `project_name`，不传 `project_path`
- `index.ts:157` 已设 `output.env.OMEM_PROJECT_DIR = directory` → tools 可用

**需要适配的 tool**:
- `memory_store` → `client.createMemory()` → POST /v1/memories（后端已有 project_path）
- `memory_search` → `client.searchMemories()` → GET /v1/memories/search（后端已有 project_path query param）
- `memory_ingest` → `client.ingestMessages()` → POST /v1/memories（后端已有 project_path）
- `memory_get` / `memory_update` / `memory_delete` / `memory_list` / `memory_stats` / `memory_profile` → 不涉及 project_path 过滤

**project_path 过滤设计**:
- 后端 WHERE 条件: `(project_path IS NULL OR project_path = '{path}')`
- 这意味着 `project_path=NULL` 的老记忆**始终会被召回**，不受过滤影响
- 私密记忆 (visibility=private) 和全局记忆 (scope=global) 也走同样的 WHERE，所以**只要有 project_path 或为 NULL，都能被搜到**
- 关键：**不传 project_path 时，后端不做过滤，返回所有记忆**（向后兼容）
- 老数据补全是为了**精确隔离**，不是为了让它们可被搜索（它们已经可以了）

**老数据问题**:
- 当前 API Key 下所有旧记忆的 `project_path=NULL`
- Plugin 端加上 project_path 隔离后，WHERE `(project_path IS NULL OR project_path = '/foo')` 会包含 NULL 记忆
- 但更好的方案是将旧数据补全为实际项目路径，实现精确隔离
- 需要一个迁移接口：按 tenant 查询 project_path=NULL 的记忆，批量更新

---

## Work Objectives

### Must Have
1. **opencode client 层**: IngestOptions/shouldRecall/searchMemories/sessionIngest/ingestMessages/createMemory 支持 `projectPath` 参数
2. **opencode hooks**: compactingHook/autocontinueHook/sessionIdleHook/keywordDetectionHook/autoRecallHook 传递 project_path
3. **opencode tools**: memory_store/memory_search/memory_ingest 传递 project_path
4. **tool description 更新**: 所有涉及 project_path 的 tool 的 description 需精准描述工具使用方式和参数说明
5. **老数据补全**: 后端提供批量更新 API，将 project_path=NULL 的记忆补全为指定路径
6. **package.json**: 更新 description 反映 project_path 隔离能力
7. 向后兼容: 不传 projectPath 时行为不变

### Must NOT Have (Guardrails)
- 不做 openclaw / claude-code / mcp 适配
- 不修改后端 domain/store 逻辑（Phase 3 已完成）
- 不修改 OpenCode SDK 接口
- 不添加新的依赖包
- 不修改 plugin 配置结构（config.ts）

---

## TODOs

- [x] 1. opencode Client 层加 projectPath 参数

  **What to do**:
  - `opencode/src/client.ts`:
    - `IngestOptions` 加 `projectPath?: string`
    - `createMemory()`: 参数加 `projectPath?: string`, body 加 `project_path: projectPath`
    - `ingestMessages()`: body 加 `project_path: opts.projectPath`
    - `sessionIngest()`: 参数加 `projectPath?: string`, body 加 `project_path: projectPath`
    - `shouldRecall()`: 参数加 `projectPath?: string`, body 加 `project_path: projectPath`
    - `searchMemories()`: 参数加 `projectPath?: string`, URL params 加 `project_path`

  **Must NOT**: 不删除 `projectName`，保持兼容

  **Recommended Agent Profile**: `quick`

- [x] 2. opencode Hooks 传递 project_path

  **What to do**:
  - `opencode/src/hooks.ts`:
    - 需要让各 hook 能访问到 `directory`（来自 `input.directory`）。当前部分 hook 通过 `sessionInfo?.data?.directory` 获取。
    - `autoRecallHook`: 需要接收 `directory` 参数（或从闭包），传给 `shouldRecall()` 的 `projectPath`
    - `compactingHook`: 已有 `sessionInfo?.data?.directory`，直接作为 `opts.projectPath` 传给 `ingestMessages()`
    - `autocontinueHook`: 同 compactingHook
    - `keywordDetectionHook`: 传 projectPath（从 `OMEM_PROJECT_DIR` 环境变量获取）
    - `sessionIdleHook`: 已有 `sessionInfo?.data?.directory`，传给 `sessionIngest()` 的 `projectPath`
  - `opencode/src/index.ts`:
    - 将 `directory` 传入 `autoRecallHook` 的参数/闭包

  **目录获取策略**:
  - hooks: 优先用 `sessionInfo?.data?.directory`，fallback 用闭包中的 `directory`
  - tools: 用 `process.env.OMEM_PROJECT_DIR`

  **Must NOT**: 不修改 config.ts

  **Recommended Agent Profile**: `unspecified-high`

- [x] 3. opencode Tools 传递 project_path + Description 更新

  **What to do**:
  - `opencode/src/tools.ts`:
    - `ToolContext` 加 `getProjectPath?: () => string | undefined`
    - `memory_store` tool:
      - execute: 调 `client.createMemory()` 时传 `projectPath`
      - **description 更新**: 加入 project_path 隔离说明——"Memories are automatically scoped to the current project via project_path. Set scope='global' for cross-project memories. Private memories (visibility='private') are always visible only to the creating agent regardless of project_path."
    - `memory_search` tool:
      - execute: 调 `client.searchMemories()` 时传 `projectPath`
      - **description 更新**: 加入说明——"Searches are automatically filtered by the current project_path. Global-scope memories and memories without a project_path are always included in results."
    - `memory_ingest` tool:
      - execute: 调 `client.ingestMessages()` 时 `opts.projectPath`
      - **description 更新**: 加入 project_path 说明
    - **所有 tool description**: 精准描述工具使用方式，参数含义，传什么值
  - `opencode/src/index.ts`:
    - `buildTools` 调用处加 `getProjectPath: () => directory`

  **Tool Description 设计原则**:
  - 明确告诉 agent project_path 是自动传递的，agent 不需要手动处理
  - 说明 scope='global' 记忆跨项目可见
  - 说明 visibility='private' 记忆对其他 agent 不可见
  - 说明不传 project_path 时返回所有记忆

  **Recommended Agent Profile**: `quick`

- [x] 4. 后端: 私密记忆强制 project_path=None + 老数据补全 API

  **What to do**:

  **4a. 私密记忆强制无路径** (memory.rs):
  - `create_memory` handler (L280): 当 `visibility == "private"` 时，强制 `memory.project_path = None`
  - `create_memory` handler (messages ingest 路径，L199-213): 同理，当创建的记忆最终 visibility=private 时，不写 project_path
  - `session_ingest` handler (L1619): 同理
  - 逻辑: 私密记忆是全局的，不应被 project_path 隔离
  - 向后兼容: visibility 非 private 时，project_path 行为不变

  **4b. 老数据补全 API** (memory.rs + router.rs):
  - 添加一个批量更新接口：`POST /v1/memories/backfill-project-path`
    - 请求 body: `{ project_path: "/mnt/d/dev/project/foo" }`
    - 逻辑: 更新当前 tenant 下所有 `project_path IS NULL AND visibility != 'private'` 的记忆，设置 `project_path` 为传入值
    - 返回: `{ updated_count: N }`
  - 安全: 需要 sanitize_project_path() 验证
  - 注意: 跳过 visibility=private 的记忆，因为私密记忆不应有 project_path

  **Recommended Agent Profile**: `quick`

- [x] 5. package.json description 更新

  **What to do**:
  - `plugins/opencode/package.json`:
    - 更新 `description` 字段，反映 project_path 隔离能力
    - 当前: "Cerebro persistent memory plugin for OpenCode — auto-recall, auto-capture, 9 memory tools with clustering"
    - 改为: "Cerebro persistent memory plugin for OpenCode — auto-recall, auto-capture, 9 memory tools with clustering, project-scoped memory isolation"
    - 更新 `keywords` 加 "project-isolation"

  **Recommended Agent Profile**: `quick`

---

## Final Verification

- [x] F1. **Build 验证**
  - `cd plugins/opencode && npm run build` — PASS
  - `cargo check` (老数据 API) — PASS

- [x] F2. **代码审查**
  - 所有 opencode ingest/search/recall 传递 project_path
  - 不传时行为不变（向后兼容）
  - 私密记忆 (visibility=private) 不带 project_path
  - 老数据补全 API 安全可用（跳过 private 记忆）
  - Tool descriptions 精准完整
  - package.json 已更新

---

## Commit Strategy

- **C1**: `feat(plugin-opencode): add projectPath to client methods and IngestOptions` — client.ts
- **C2**: `feat(plugin-opencode): wire project_path through hooks and tools, update tool descriptions` — hooks.ts, tools.ts, index.ts
- **C3**: `fix(api): private memories should not have project_path` — memory.rs (create_memory, session_ingest)
- **C4**: `feat(api): add backfill-project-path endpoint for old data migration` — memory.rs, router.rs
- **C5**: `chore(plugin-opencode): update package.json description` — package.json
