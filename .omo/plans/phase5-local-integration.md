# Phase 5 — web端与plugin本地整合

## TL;DR

> **Quick Summary**: Plugin内置Node.js HTTP服务器(localhost:5212) serve omem-web静态资源，新增4个管理页面（分类字典、项目筛选、私密记忆、偏好画像），实现应用层加密传输。
> 
> **Deliverables**:
> - Plugin内置HTTP服务器模块（Node.js http.createServer，零新依赖）
> - omem-web全量嵌入plugin（15个现有view + 4个新管理页）
> - 分类字典CRUD管理页
> - 项目筛选器组件（记忆列表页下拉）
> - 私密记忆管理页
> - 偏好画像管理页（只读展示）
> - 应用层加密模块（AES-256-GCM）
> - API地址从plugin config动态注入
> 
> **Estimated Effort**: Large
> **Parallel Execution**: YES - 3 waves + Final
> **Critical Path**: T1(HTTP服务器) → T3(入口集成) → T2(构建流程) → T9(端到端集成)

---

## Context

### Original Request
从设计文档 `docs/superpowers/specs/2026-05-15-memory-system-rewrite-design.md` L303-336。Plugin和web端合并成一个本地服务`localhost:5212`，同时新增4个web管理页面对应服务端新功能。

### Interview Summary
**Key Discussions**:
- 范围：5a(部署整合) + 5b(加密传输) + 4个web管理页，一步到位
- 嵌入范围：全量嵌入（所有15个现有view + 4个新管理页）
- 服务整合：Plugin内置Node.js HTTP服务器，只serve静态文件
- API方案：前端API走域名（nginx暴露），地址从plugin config注入（window.__OMEM_API_URL__）
- 加密：应用层AES-256-GCM加密请求体
- 测试：TDD
- HTTP服务器：Node.js内置http模块，零新依赖
- 项目筛选器：前端下拉组件（不是独立页面）
- 偏好画像：只读展示（编辑API在Phase 6a才建）

**Research Findings**:
- Plugin: OmemPlugin()函数式入口，注册5个hooks + 9个tools。无HTTP服务器。
- Web: React 19 + Vite 8 + Tailwind 3 + shadcn/ui，18个页面路由
- Plugin config: config.ts有connection.apiUrl（默认www.mengxy.cc），三级覆盖
- API覆盖：分类CRUD全部存在，私密记忆部分存在，偏好画像只有GET /v1/profile
- shadcn/ui v4风格：Card用原生div + data-slot
- Plugin现有view: 15个目录，不做任何改动

### Metis Review
**Identified Gaps (addressed)**:
- 加密方案：师尊坚持包含AES-256-GCM
- 全量vs子集：全量嵌入
- API代理：不做，走域名，从plugin config注入
- SPA fallback：必须处理所有路由
- Plugin降级：HTTP启动失败不影响核心功能
- PoC验证：Phase 5a第一步验证OpenCode框架允许plugin内启动HTTP服务器

---

## Work Objectives

### Core Objective
Plugin内置HTTP服务器serve全量omem-web + 新增4个管理页面 + 应用层加密传输。

### Concrete Deliverables
- `plugins/opencode/src/web-server.ts` — HTTP服务器模块
- `omem-web/src/views/categories/` — 分类字典管理页
- `omem-web/src/views/memories/components/project-filter.tsx` — 项目筛选器
- `omem-web/src/views/private-memories/` — 私密记忆管理页
- `omem-web/src/views/preferences/` — 偏好画像管理页
- `omem-web/src/api/categories.ts` — 分类API client
- `omem-web/src/api/private-memories.ts` — 私密记忆API client
- `omem-web/src/api/preferences.ts` — 偏好API client
- `omem-web/src/utils/encryption.ts` — AES-256-GCM加密模块

### Definition of Done
- [ ] `curl http://localhost:5212/` → 200 (index.html)
- [ ] `curl http://localhost:5212/settings/categories` → 200 (SPA fallback)
- [ ] Plugin config中的apiUrl正确注入到前端
- [ ] 4个新页面可通过URL直接访问
- [ ] 分类字典可CRUD操作
- [ ] 记忆列表页可按project_path筛选
- [ ] 偏好画像页面可查看（只读）
- [ ] 加密传输功能正常工作

### Must Have
1. Plugin内置HTTP服务器(localhost:5212)，零新npm依赖
2. omem-web全量嵌入plugin（vite build产物）
3. SPA fallback正确处理所有路由
4. API地址从plugin config.connection.apiUrl动态注入
5. 分类字典CRUD管理页
6. 项目筛选器（记忆列表页下拉组件）
7. 私密记忆管理页
8. 偏好画像管理页（只读展示）
9. 应用层加密（AES-256-GCM）前端模块
10. HTTP服务器启动失败降级（不影响plugin核心功能）
11. HTTP服务器端口通过OMEM_LOCAL_PORT配置（默认5212）

### Must NOT Have (Guardrails)
1. 不在Rust服务端新增任何端点或代码
2. 不在plugin package.json添加运行时dependencies
3. 不改动omem-web现有15个view的代码
4. 不实现偏好编辑功能（Phase 6a才建后端API）
5. 不做API反向代理（API走域名）
6. 不做数据分析/可视化图表/导出功能
7. 不改Plugin核心hooks/tools逻辑

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (omem-web有Vitest配置)
- **Automated tests**: TDD (RED → GREEN → REFACTOR)
- **Framework**: Vitest + @testing-library/react for components
- **E2E**: Playwright for page-level verification

### QA Policy
Every task MUST include agent-executed QA scenarios.
- **Plugin HTTP server**: Bash (curl) — status codes, MIME types, SPA fallback, content
- **Frontend components**: Vitest — render, interact, assert DOM
- **Frontend pages**: Playwright — navigate, fill forms, assert content
- **Encryption**: Bash (node script) — encrypt/decrypt roundtrip

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation — 3 tasks, MAX PARALLEL):
├── Task 1: Plugin HTTP服务器模块 [deep]
├── Task 2: omem-web构建流程改造 [quick]
└── Task 3: 前端API client动态baseURL [quick]

Wave 2 (Frontend pages — 5 tasks, MAX PARALLEL):
├── Task 4: 分类字典管理页 (depends: 3) [visual-engineering]
├── Task 5: 项目筛选器组件 (depends: 3) [quick]
├── Task 6: 私密记忆管理页 (depends: 3) [visual-engineering]
├── Task 7: 偏好画像管理页 (depends: 3) [quick]
└── Task 8: 应用层加密模块 (depends: none) [deep]

Wave 3 (Integration — 2 tasks):
├── Task 9: Plugin入口集成 + 端到端验证 (depends: 1, 2, 3, 8) [deep]
└── Task 10: 构建流水线 + 发布 (depends: 9) [quick]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Real manual QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
```

### Dependency Matrix

| Task | Blocked By | Blocks |
|------|-----------|--------|
| T1 | None | T9 |
| T2 | None | T9, T10 |
| T3 | None | T4, T5, T6, T7 |
| T4 | T3 | T9 |
| T5 | T3 | T9 |
| T6 | T3 | T9 |
| T7 | T3 | T9 |
| T8 | None | T9 |
| T9 | T1, T2, T3, T4, T5, T6, T7, T8 | T10 |
| T10 | T9 | F1-F4 |

### Agent Dispatch Summary

- **Wave 1**: 3 — T1 → `deep`, T2 → `quick`, T3 → `quick`
- **Wave 2**: 5 — T4 → `visual-engineering`, T5 → `quick`, T6 → `visual-engineering`, T7 → `quick`, T8 → `deep`
- **Wave 3**: 2 — T9 → `deep`, T10 → `quick`
- **FINAL**: 4 — F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

- [ ] 1. Plugin HTTP服务器模块

  **What to do**:
  - 创建 `plugins/opencode/src/web-server.ts`
  - 使用Node.js内置`http.createServer()`实现静态文件服务器
  - 实现SPA fallback：非文件路径的GET请求返回index.html
  - 实现MIME类型识别（.html, .js, .css, .json, .svg, .png, .ico等）
  - 实现config注入：将`config.connection.apiUrl`替换到index.html中的`window.__OMEM_API_URL__`占位符
  - 实现graceful shutdown：plugin退出时关闭HTTP服务器
  - 实现降级：HTTP服务器启动失败时log warning但不影响plugin核心功能
  - 端口通过`OMEM_LOCAL_PORT`环境变量配置（默认5212）

  **Must NOT do**:
  - 不引入任何npm依赖（不用Express/Hono/serve-static）
  - 不实现API反向代理
  - 不修改plugin核心hooks/tools逻辑

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2, T3)
  - **Blocks**: T9
  - **Blocked By**: None

  **References**:
  **Pattern References**:
  - `plugins/opencode/src/index.ts:63-90` — OmemPlugin()入口，返回对象结构
  - `plugins/opencode/src/config.ts:48-53` — DEFAULTS.connection.apiUrl配置
  - `plugins/opencode/src/config.ts:130-160` — loadPluginConfig()三级覆盖逻辑

  **API/Type References**:
  - `plugins/opencode/src/config.ts:7-13` — OmemPluginConfig.connection接口

  **WHY Each Reference Matters**:
  - `index.ts:63-90`: 理解plugin生命周期，HTTP服务器在哪里启动/关闭
  - `config.ts:48-53`: apiUrl默认值，需要注入到前端
  - `config.ts:130-160`: 配置读取逻辑，HTTP服务器需要的配置项

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: HTTP服务器启动并serve静态文件
    Tool: Bash (curl)
    Preconditions: plugin构建完成，dist/目录有vite build产物
    Steps:
      1. 启动plugin（模拟OmemPlugin()调用）
      2. curl -s -o /dev/null -w "%{http_code}" http://localhost:5212/
      3. curl -s http://localhost:5212/ | head -5
    Expected Result: HTTP 200, HTML包含window.__OMEM_API_URL__
    Failure Indicators: Connection refused, 404, 缺少__OMEM_API_URL__
    Evidence: .omo/evidence/task-1-http-server-start.txt

  Scenario: SPA fallback处理所有路由
    Tool: Bash (curl)
    Preconditions: HTTP服务器已启动
    Steps:
      1. curl -s -o /dev/null -w "%{http_code}" http://localhost:5212/settings/categories
      2. curl -s -o /dev/null -w "%{http_code}" http://localhost:5212/memories/123
      3. curl -s -o /dev/null -w "%{http_code}" http://localhost:5212/nonexistent/route
    Expected Result: 全部返回200 + index.html内容
    Failure Indicators: 任何路由返回404
    Evidence: .omo/evidence/task-1-spa-fallback.txt

  Scenario: 端口冲突时降级运行
    Tool: Bash
    Preconditions: 5212端口已被占用
    Steps:
      1. 用nc占用5212端口
      2. 启动plugin
      3. 检查plugin核心功能（memory tools）是否正常
    Expected Result: plugin正常启动，log显示HTTP服务器启动失败warning，核心功能不受影响
    Failure Indicators: plugin启动崩溃或核心功能异常
    Evidence: .omo/evidence/task-1-graceful-degradation.txt
  ```

  **Commit**: YES (groups with T2, T3)
  - Message: `feat(plugin): add HTTP server and web build integration`
  - Files: `plugins/opencode/src/web-server.ts`
  - Pre-commit: `cd plugins/opencode && npm run build`

- [ ] 2. omem-web构建流程改造

  **What to do**:
  - 修改 `omem-web/vite.config.ts`：添加构建后复制到plugin目录的配置
  - 修改 `omem-web/index.html`：添加`<script>window.__OMEM_API_URL__ = "__OMEM_API_URL__";</script>`占位符
  - 创建构建脚本 `scripts/build-plugin-web.sh`：
    1. `cd omem-web && npm run build`
    2. 复制 `omem-web/dist/` → `plugins/opencode/web/` 
    3. 验证产物完整性
  - 修改 `plugins/opencode/package.json`：添加`"web"` files字段或scripts
  - 确保静态资源使用content hash（vite默认行为）

  **Must NOT do**:
  - 不改动omem-web现有15个view的代码
  - 不引入新的构建工具依赖
  - 不修改vite的base路径（保持"/"）

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T3)
  - **Blocks**: T9, T10
  - **Blocked By**: None

  **References**:
  **Pattern References**:
  - `omem-web/vite.config.ts` — 当前vite配置（react plugin + proxy + @别名）
  - `omem-web/package.json` — 构建命令 `tsc -b && vite build`
  - `omem-web/index.html` — SPA入口HTML
  - `plugins/opencode/package.json` — plugin包结构

  **WHY Each Reference Matters**:
  - `vite.config.ts`: 理解当前构建配置，需要添加什么
  - `package.json`: 理解构建命令和产物
  - `index.html`: 需要添加API URL占位符
  - `plugin package.json`: 需要添加web静态资源到发布包

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 构建产物正确生成并复制
    Tool: Bash
    Preconditions: omem-web依赖已安装
    Steps:
      1. 运行 scripts/build-plugin-web.sh
      2. ls plugins/opencode/web/index.html
      3. ls plugins/opencode/web/assets/ | wc -l
    Expected Result: index.html存在, assets/目录有JS/CSS文件
    Failure Indicators: 文件不存在或为空
    Evidence: .omo/evidence/task-2-build-output.txt

  Scenario: index.html包含API URL占位符
    Tool: Bash
    Preconditions: 构建完成
    Steps:
      1. grep "__OMEM_API_URL__" plugins/opencode/web/index.html
    Expected Result: 找到window.__OMEM_API_URL__占位符
    Failure Indicators: 占位符不存在
    Evidence: .omo/evidence/task-2-api-url-placeholder.txt

  Scenario: 静态资源使用content hash
    Tool: Bash
    Preconditions: 构建完成
    Steps:
      1. ls plugins/opencode/web/assets/index-*.js
    Expected Result: 文件名包含hash（如index-a1b2c3d4.js）
    Failure Indicators: 文件名为index.js（无hash）
    Evidence: .omo/evidence/task-2-content-hash.txt
  ```

  **Commit**: YES (groups with T1, T3)
  - Message: `feat(plugin): add HTTP server and web build integration`
  - Files: `omem-web/vite.config.ts`, `omem-web/index.html`, `scripts/build-plugin-web.sh`, `plugins/opencode/package.json`
  - Pre-commit: `bash scripts/build-plugin-web.sh`

- [ ] 3. 前端API client动态baseURL

  **What to do**:
  - 修改 `omem-web/src/api/client.ts`：
    - baseURL改为读取`window.__OMEM_API_URL__`，fallback到`"/"`
    - 开发环境（无`window.__OMEM_API_URL__`）继续用vite proxy
  - 确保TypeScript编译不报错（window类型扩展）
  - 现有API调用零改动

  **Must NOT do**:
  - 不修改现有API调用代码
  - 不引入新的axios配置

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2)
  - **Blocks**: T4, T5, T6, T7
  - **Blocked By**: None

  **References**:
  **Pattern References**:
  - `omem-web/src/api/client.ts` — 当前axios实例，baseURL="/"

  **WHY Each Reference Matters**:
  - `client.ts`: 唯一需要改动的API client文件，baseURL动态化

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 动态baseURL正确读取
    Tool: Bash (node script)
    Steps:
      1. 设置 window.__OMEM_API_URL__ = "https://test.example.com"
      2. import client.ts
      3. assert client.defaults.baseURL === "https://test.example.com"
    Expected Result: baseURL匹配注入值
    Failure Indicators: baseURL仍为"/"
    Evidence: .omo/evidence/task-3-dynamic-baseurl.txt

  Scenario: fallback到默认值
    Tool: Bash (node script)
    Steps:
      1. 不设置window.__OMEM_API_URL__
      2. import client.ts
      3. assert client.defaults.baseURL === "/"
    Expected Result: baseURL fallback到"/"
    Failure Indicators: undefined或空字符串
    Evidence: .omo/evidence/task-3-fallback-baseurl.txt
  ```

  **Commit**: YES (groups with T1, T2)
  - Message: `feat(plugin): add HTTP server and web build integration`
  - Files: `omem-web/src/api/client.ts`
  - Pre-commit: `cd omem-web && npx tsc --noEmit`

- [ ] 4. 分类字典管理页

  **What to do**:
  - 创建 `omem-web/src/api/categories.ts` — 分类API client（GET/POST/PUT/DELETE /v1/categories, 别名管理）
  - 创建 `omem-web/src/types/categories.ts` — 类型定义（Category, CategoryConfig, CategoryAlias）
  - 创建 `omem-web/src/views/categories/` 目录：
    - `categories-page.tsx` — 主页面（分类列表 + CRUD操作）
    - `category-form.tsx` — 新增/编辑表单组件
    - `alias-manager.tsx` — 别名管理子组件
  - 在 `omem-web/src/App.tsx` 添加路由：`/settings/categories`
  - 在 `omem-web/src/components/layout/` 的导航中添加入口
  - TDD：先写Vitest测试，再写实现

  **Must NOT do**:
  - 不做数据分析/可视化图表
  - 不修改现有view代码
  - 不添加后端API端点（全部已存在）

  **Recommended Agent Profile**:
  - **Category**: `visual-engineering`
  - **Skills**: [`frontend-design`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T5, T6, T7, T8)
  - **Blocks**: T9
  - **Blocked By**: T3 (API client动态baseURL)

  **References**:
  **Pattern References**:
  - `omem-web/src/views/clusters/cluster-management.tsx` — CRUD管理页参考模式
  - `omem-web/src/views/settings/settings-page.tsx` — 设置页布局模式
  - `omem-web/src/api/cluster.ts` — API client模块参考
  - `omem-web/src/components/ui/` — shadcn/ui v4组件库（Card用div+data-slot）

  **API/Type References**:
  - Rust `omem-server/src/api/router.rs` — 确认所有分类API端点：
    - GET /v1/categories, POST /v1/categories
    - GET /v1/categories/{name}, PUT /v1/categories/{name}, DELETE /v1/categories/{name}
    - GET /v1/categories/aliases, POST /v1/categories/aliases, DELETE /v1/categories/aliases/{alias}
  - Rust `omem-server/src/domain/category.rs:42-50` — CategoryConfig结构参考

  **WHY Each Reference Matters**:
  - `cluster-management.tsx`: 最相似的CRUD管理页，复制列表+表单模式
  - `settings-page.tsx`: 分类页挂在settings下的导航模式
  - `cluster.ts`: API client函数导出模式
  - `ui/` 组件: 确保使用v4风格（div+data-slot而非Card组件）

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 分类列表加载和展示
    Tool: Playwright
    Steps:
      1. 访问 http://localhost:5212/settings/categories
      2. 等待页面加载完成
      3. 断言页面包含分类列表表格/卡片
      4. 断言至少显示已有分类名称
    Expected Result: 分类列表正确渲染
    Failure Indicators: 空白页、网络错误、无列表数据
    Evidence: .omo/evidence/task-4-categories-list.png

  Scenario: 新增分类
    Tool: Playwright
    Steps:
      1. 访问 /settings/categories
      2. 点击"新增分类"按钮
      3. 填写表单：name="test-category", display_name="Test Category"
      4. 点击提交
      5. 断言新分类出现在列表中
    Expected Result: 新分类成功创建并显示
    Failure Indicators: 提交失败、列表不更新
    Evidence: .omo/evidence/task-4-create-category.png

  Scenario: 空状态展示
    Tool: Playwright
    Steps:
      1. Mock空API响应
      2. 访问 /settings/categories
      3. 断言显示友好的空状态提示
    Expected Result: "暂无分类数据"提示
    Failure Indicators: 崩溃或无内容
    Evidence: .omo/evidence/task-4-empty-state.png
  ```

  **Commit**: YES (groups with T5, T6, T7)
  - Message: `feat(web): add category, project filter, private memory, and preference management pages`
  - Files: `omem-web/src/views/categories/`, `omem-web/src/api/categories.ts`, `omem-web/src/types/categories.ts`
  - Pre-commit: `cd omem-web && npx vitest run`

- [ ] 5. 项目筛选器组件

  **What to do**:
  - 创建 `omem-web/src/views/memories/components/project-filter.tsx` — 下拉筛选组件
  - 组件功能：获取可用project_path列表 → 下拉选择 → 更新URL query参数
  - 记忆列表页集成：在memory-list.tsx中添加ProjectFilter组件
  - API：通过GET /v1/stats/tags或现有API获取project_path列表
  - TDD：先写组件测试

  **Must NOT do**:
  - 不创建独立管理页面
  - 不修改记忆列表页的现有筛选逻辑（tag、time等）
  - 不添加后端API

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T4, T6, T7, T8)
  - **Blocks**: T9
  - **Blocked By**: T3

  **References**:
  **Pattern References**:
  - `omem-web/src/views/memories/memory-list.tsx` — 记忆列表页，添加筛选器的位置
  - `omem-web/src/components/ui/select.tsx` — shadcn/ui下拉组件

  **API/Type References**:
  - `omem-server/src/api/handlers/stats.rs` — 确认project_path列表获取方式

  **WHY Each Reference Matters**:
  - `memory-list.tsx`: 理解筛选器UI位置和现有筛选逻辑
  - `select.tsx`: shadcn/ui v4下拉组件用法

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 项目筛选器渲染和交互
    Tool: Playwright
    Steps:
      1. 访问 /memories
      2. 找到项目筛选器下拉
      3. 点击展开
      4. 选择一个项目
      5. 断言URL query参数更新（?project_path=xxx）
    Expected Result: 筛选器正常工作，记忆列表按项目过滤
    Failure Indicators: 筛选器不显示或选择无反应
    Evidence: .omo/evidence/task-5-project-filter.png

  Scenario: "全部项目"选项
    Tool: Playwright
    Steps:
      1. 先选择一个项目
      2. 切换回"全部"选项
      3. 断言project_path参数消失
    Expected Result: 显示所有记忆
    Failure Indicators: 筛选器卡住
    Evidence: .omo/evidence/task-5-project-filter-all.png
  ```

  **Commit**: YES (groups with T4, T6, T7)
  - Files: `omem-web/src/views/memories/components/project-filter.tsx`
  - Pre-commit: `cd omem-web && npx vitest run`

- [ ] 6. 私密记忆管理页

  **What to do**:
  - 创建 `omem-web/src/api/private-memories.ts` — 私密记忆API client
  - 创建 `omem-web/src/views/private-memories/` 目录：
    - `private-memories-page.tsx` — 主页面（私密记忆列表 + 解密查看）
    - `decrypt-viewer.tsx` — 解密内容查看组件
  - 在App.tsx添加路由：`/private-memories`
  - 在导航添加入口
  - 复用现有vault密码验证流程
  - TDD

  **Must NOT do**:
  - 不实现加密/解密前端逻辑（Phase 3b已在服务端实现，前端只展示解密后的内容）
  - 不修改vault模块代码

  **Recommended Agent Profile**:
  - **Category**: `visual-engineering`
  - **Skills**: [`frontend-design`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T4, T5, T7, T8)
  - **Blocks**: T9
  - **Blocked By**: T3

  **References**:
  **Pattern References**:
  - `omem-web/src/views/vault/vault-memories.tsx` — vault页面参考（密码验证流程）
  - `omem-web/src/views/memories/memory-detail.tsx` — 记忆详情展示模式

  **API/Type References**:
  - Phase 3b计划中的API端点：
    - GET /v2/memories?include_private=true — 获取私密记忆列表
    - GET /v2/memories/{id} — 获取单条（加密内容，需前端解密）
    - POST /v1/vault/verify — vault密码验证

  **WHY Each Reference Matters**:
  - `vault-memories.tsx`: 密码验证UI流程可直接复用
  - `memory-detail.tsx`: 记忆内容展示布局参考

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 私密记忆列表展示
    Tool: Playwright
    Steps:
      1. 访问 /private-memories
      2. 如需密码验证，输入vault密码
      3. 断言私密记忆列表渲染
    Expected Result: 私密记忆列表正确显示
    Failure Indicators: 密码验证失败或列表为空
    Evidence: .omo/evidence/task-6-private-memories-list.png

  Scenario: 解密内容查看
    Tool: Playwright
    Steps:
      1. 在列表中点击一条私密记忆
      2. 断言内容解密后正确展示
    Expected Result: 解密内容可读
    Failure Indicators: 内容显示为密文或乱码
    Evidence: .omo/evidence/task-6-decrypt-viewer.png
  ```

  **Commit**: YES (groups with T4, T5, T7)
  - Files: `omem-web/src/views/private-memories/`, `omem-web/src/api/private-memories.ts`
  - Pre-commit: `cd omem-web && npx vitest run`

- [ ] 7. 偏好画像管理页（只读）

  **What to do**:
  - 创建 `omem-web/src/api/preferences.ts` — 偏好API client（GET /v1/profile + Phase 6a端点）
  - 创建 `omem-web/src/views/preferences/` 目录：
    - `preferences-page.tsx` — 主页面（偏好列表 + 画像快照展示）
    - `preference-card.tsx` — 单条偏好展示卡片
  - 在App.tsx添加路由：`/settings/preferences`
  - 在导航添加入口
  - **只读展示**：展示当前偏好列表和画像信息，无编辑功能
  - TDD

  **Must NOT do**:
  - 不实现编辑功能（Phase 6a才建后端编辑API）
  - 不修改现有profile页面

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T4, T5, T6, T8)
  - **Blocks**: T9
  - **Blocked By**: T3

  **References**:
  **Pattern References**:
  - `omem-web/src/views/profile/profile-page.tsx` — 现有画像页面（参考展示模式）
  - `omem-web/src/views/settings/settings-page.tsx` — 设置页布局

  **API/Type References**:
  - GET /v1/profile — 现有画像API（只读）
  - Phase 6a计划中的端点（如已实现）：GET /v2/profile/preferences

  **WHY Each Reference Matters**:
  - `profile-page.tsx`: 理解现有画像展示格式，新页面保持一致
  - `settings-page.tsx`: 偏好页挂在settings下的导航模式

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 偏好列表只读展示
    Tool: Playwright
    Steps:
      1. 访问 /settings/preferences
      2. 断言偏好列表正确渲染
      3. 断言无编辑按钮（只读模式）
    Expected Result: 偏好信息正确展示，无编辑入口
    Failure Indicators: 出现编辑表单或修改按钮
    Evidence: .omo/evidence/task-7-preferences-readonly.png

  Scenario: 空偏好展示
    Tool: Playwright
    Steps:
      1. Mock空API响应
      2. 访问 /settings/preferences
      3. 断言显示友好空状态
    Expected Result: "暂无偏好数据"提示
    Failure Indicators: 崩溃或空白
    Evidence: .omo/evidence/task-7-empty-preferences.png
  ```

  **Commit**: YES (groups with T4, T5, T6)
  - Files: `omem-web/src/views/preferences/`, `omem-web/src/api/preferences.ts`
  - Pre-commit: `cd omem-web && npx vitest run`

- [ ] 8. 应用层加密模块

  **What to do**:
  - 创建 `omem-web/src/utils/encryption.ts` — AES-256-GCM加密/解密模块
  - 使用Web Crypto API（浏览器原生，零依赖）：
    - `encrypt(plaintext, key)` → {iv, ciphertext, tag}
    - `decrypt({iv, ciphertext, tag}, key)` → plaintext
    - `generateKey()` → AES-256-GCM key
  - 创建axios请求拦截器：对特定API请求自动加密请求体
  - 创建axios响应拦截器：自动解密加密响应
  - 密钥管理：从服务端获取per-tenant密钥（GET /v1/tenant/encryption-key）
  - TDD

  **Must NOT do**:
  - 不引入crypto-js或任何加密npm包（使用Web Crypto API）
  - 不加密所有请求（只加密标记为需要加密的）
  - 不修改Rust服务端代码

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with T4, T5, T6, T7)
  - **Blocks**: T9
  - **Blocked By**: None

  **References**:
  **External References**:
  - Web Crypto API: `crypto.subtle.encrypt/decrypt` — 浏览器原生AES-GCM支持
  - Phase 3b计划的密钥管理API（per-tenant key）

  **WHY Each Reference Matters**:
  - Web Crypto API是零依赖方案，所有现代浏览器支持
  - Phase 3b的密钥API决定密钥获取流程

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 加密解密roundtrip
    Tool: Bash (node script with --experimental-global-webcrypto)
    Steps:
      1. generateKey() → key
      2. encrypt("test content 测试内容", key) → encrypted
      3. decrypt(encrypted, key) → plaintext
      4. assert plaintext === "test content 测试内容"
    Expected Result: 加密后解密得到原文，中文内容正确
    Failure Indicators: 解密失败或中文乱码
    Evidence: .omo/evidence/task-8-encrypt-roundtrip.txt

  Scenario: 密钥不匹配解密失败
    Tool: Bash (node script)
    Steps:
      1. generateKey() → key1
      2. generateKey() → key2
      3. encrypt("test", key1) → encrypted
      4. decrypt(encrypted, key2)
    Expected Result: 抛出解密错误（DOMException: OperationError）
    Failure Indicators: 解密成功（密钥不匹配却通过了）
    Evidence: .omo/evidence/task-8-key-mismatch.txt
  ```

  **Commit**: YES (groups with T4, T5, T6, T7)
  - Files: `omem-web/src/utils/encryption.ts`
  - Pre-commit: `cd omem-web && npx vitest run`

- [ ] 9. Plugin入口集成 + 端到端验证

  **What to do**:
  - 修改 `plugins/opencode/src/index.ts`：
    - 在OmemPlugin()返回前启动HTTP服务器
    - 传入config参数给web-server模块
    - 添加shutdown handler（plugin退出时关闭HTTP服务器）
  - 端到端验证：
    - 构建omem-web → 复制到plugin → 构建plugin → 启动 → 验证
    - 测试所有新页面可访问
    - 测试SPA fallback
    - 测试config注入
  - TDD：集成测试

  **Must NOT do**:
  - 不修改hooks/tools逻辑
  - 不改变plugin返回值结构

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3
  - **Blocks**: T10
  - **Blocked By**: T1, T2, T3, T4, T5, T6, T7, T8

  **References**:
  **Pattern References**:
  - `plugins/opencode/src/index.ts:63-90` — OmemPlugin()入口
  - `plugins/opencode/src/config.ts` — 配置加载

  **WHY Each Reference Matters**:
  - `index.ts`: 需要在这里启动HTTP服务器并传入config

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: Plugin启动HTTP服务器并serve全量web
    Tool: Bash
    Preconditions: omem-web已构建，产物在plugin目录
    Steps:
      1. 运行 scripts/build-plugin-web.sh
      2. 构建plugin: cd plugins/opencode && npm run build
      3. 模拟启动plugin
      4. curl http://localhost:5212/ → 200
      5. curl http://localhost:5212/settings/categories → 200
      6. curl http://localhost:5212/settings/preferences → 200
      7. curl http://localhost:5212/private-memories → 200
    Expected Result: 所有页面可访问
    Failure Indicators: 任何页面返回404
    Evidence: .omo/evidence/task-9-e2e-integration.txt

  Scenario: Config注入验证
    Tool: Bash
    Steps:
      1. 设置 OMEM_API_URL=https://test.example.com
      2. 启动plugin
      3. curl -s http://localhost:5212/ | grep "__OMEM_API_URL__"
    Expected Result: HTML中包含 https://test.example.com
    Failure Indicators: 仍为占位符或默认值
    Evidence: .omo/evidence/task-9-config-injection.txt

  Scenario: HTTP服务器降级
    Tool: Bash
    Steps:
      1. 占用5212端口
      2. 启动plugin
      3. 验证memory tools仍正常工作（curl API）
    Expected Result: plugin核心功能不受影响
    Failure Indicators: plugin崩溃
    Evidence: .omo/evidence/task-9-degradation.txt
  ```

  **Commit**: YES (groups with T10)
  - Message: `feat(plugin): integrate HTTP server and build pipeline`
  - Files: `plugins/opencode/src/index.ts`
  - Pre-commit: `cd plugins/opencode && npm run build`

- [ ] 10. 构建流水线 + 发布

  **What to do**:
  - 完善 `scripts/build-plugin-web.sh` 脚本：
    - 清理旧产物
    - 安装依赖 → 构建omem-web → 复制到plugin
    - 构建plugin → 验证产物
  - 添加CI命令到 `omem-web/package.json` 和 `plugins/opencode/package.json`
  - 验证npm pack产物包含web静态资源
  - 文档化构建流程

  **Must NOT do**:
  - 不添加CI/CD pipeline（后续再做）
  - 不修改npm发布配置

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 (after T9)
  - **Blocks**: F1-F4
  - **Blocked By**: T9

  **References**:
  **Pattern References**:
  - `plugins/opencode/package.json` — 现有构建命令
  - `omem-web/package.json` — 现有构建命令

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY):**

  ```
  Scenario: 完整构建流水线
    Tool: Bash
    Steps:
      1. rm -rf plugins/opencode/web/ omem-web/dist/
      2. bash scripts/build-plugin-web.sh
      3. cd plugins/opencode && npm run build
      4. npm pack --dry-run | grep "web/"
    Expected Result: 产物包含web/目录的静态文件
    Failure Indicators: web/目录缺失或为空
    Evidence: .omo/evidence/task-10-build-pipeline.txt
  ```

  **Commit**: YES (groups with T9)
  - Message: `feat(plugin): integrate HTTP server and build pipeline`
  - Files: `scripts/build-plugin-web.sh`, `omem-web/package.json`, `plugins/opencode/package.json`

## Commit Strategy

- **Wave 1**: `feat(plugin): add HTTP server and web build integration` — web-server.ts, vite.config.ts, client.ts
- **Wave 2**: `feat(web): add category, project filter, private memory, and preference management pages` — new view files + API modules
- **Wave 3**: `feat(plugin): integrate HTTP server and build pipeline` — index.ts, build scripts

---

## Success Criteria

### Verification Commands
```bash
# Plugin HTTP server
curl -s -o /dev/null -w "%{http_code}" http://localhost:5212/           # Expected: 200
curl -s -o /dev/null -w "%{http_code}" http://localhost:5212/settings/categories  # Expected: 200 (SPA)
curl -s -I http://localhost:5212/assets/index-*.js | grep content-type  # Expected: application/javascript

# Build pipeline
cd omem-web && npm run build && ls dist/index.html  # Expected: file exists
cd plugins/opencode && npm run build                  # Expected: success

# Tests
cd omem-web && npx vitest run                         # Expected: all pass
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All new pages accessible via URL
- [ ] SPA fallback works for all routes
- [ ] Plugin config apiUrl correctly injected
- [ ] Encryption roundtrip works
- [ ] HTTP server graceful degradation
- [ ] No new npm runtime dependencies
