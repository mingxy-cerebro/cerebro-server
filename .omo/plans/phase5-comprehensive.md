# Phase 5 综合执行计划 — Web端整合 + 额外任务

## 概述

基于已审批的 `.omo/plans/phase5-local-integration.md`（Phase 5 — web端与plugin本地整合），整合3个额外任务：

1. **E1**: confirm取消按钮全局bug修复
2. **E2**: 用户画像页面重设计 + v2 API适配（替代Phase 5 T7）
3. **E3**: 记忆列表project_path下拉筛选（替代Phase 5 T5）

Phase 5原计划T5/T7升级为E3/E2，其余T1-T4/T6/T8-T10不变。

## 与Phase 5原计划差异

| 原任务 | 变更 |
|--------|------|
| T5 项目筛选器 | → 升级为E3：完整的project_path筛选方案 |
| T7 偏好画像管理页（只读） | → 升级为E2：v2 API适配 + 页面重设计 + CRUD |
| 新增 E1 | confirm取消按钮全局bug修复 |

---

## 额外任务详情

### E1: confirm取消按钮bug修复

**问题根因**：
- `omem-web/src/components/ui/alert-dialog.tsx` 的 `AlertDialogCancel` 渲染为普通Button
- 没有使用 base-ui 的 `DialogPrimitive.Close`，缺少内置关闭功能
- memory-detail.tsx L675 的 AlertDialogCancel 没有 onClick 处理器 → 点取消无反应

**影响范围**：13个文件的 AlertDialog 使用

**修复方案**：
1. 修改 `alert-dialog.tsx` 的 AlertDialogCancel，用 `DialogPrimitive.Close` 包装：
   ```tsx
   function AlertDialogCancel({ className, ...props }) {
     return (
       <DialogPrimitive.Close>
         <Button data-slot="alert-dialog-cancel" variant="outline" className={className} {...props} />
       </DialogPrimitive.Close>
     )
   }
   ```
2. 同理修改 AlertDialogAction 也用 `DialogPrimitive.Close` 包装（确保点击后关闭对话框）
3. 审查13个使用文件，移除手动 onClick 关闭逻辑（Close自动处理）
4. 验证所有confirm对话框的取消/确认按钮可正常关闭

**Category**: `quick` | **Wave**: Wave 0（最先执行）

---

### E2: 用户画像v2适配 + 页面重设计

**当前状态**：
- `profile-page.tsx` (548行) 调用 `/v1/profile`
- 返回 `{ dynamic_context, search_results, static_facts }`
- 用关键词匹配（classifyFact）做粗糙分类

**后端v2 API已存在**（profile_v2.rs）：
- `GET /v2/profile` — 完整profile（含偏好列表）
- `GET /v2/profile/preferences` — 偏好列表（支持project_path过滤）
- `GET /v2/profile/preferences/{id}` — 单条偏好
- `POST /v2/profile/preferences` — 创建偏好
- `PUT /v2/profile/preferences/{id}` — 更新偏好(confidence/scope/value)
- `DELETE /v2/profile/preferences/{id}` — 删除偏好
- `GET /v2/profile/stats` — 统计
- `GET /v2/profile/versions` — 版本历史
- `GET /v2/profile/changelog` — 变更日志
- `GET /v2/profile/inject` — injection内容

**方案**：
1. 创建 `omem-web/src/api/profile-v2.ts` — v2 profile API client
2. 创建 `omem-web/src/types/profile-v2.ts` — v2类型定义
3. 重写 profile-page.tsx：
   - **顶部**：画像概览卡片（v2 stats数据：偏好总数、slot分布、最近更新时间）
   - **中部**：偏好列表（按slot分类展示，支持project_path下拉过滤，支持CRUD）
   - **底部**：版本变更历史时间线（changelog）
   - 保留 v1 的动态上下文展示（dynamic_context）作为补充tab
4. 偏好CRUD操作：创建（表单弹窗）、编辑（inline或弹窗）、删除（confirm）

**Category**: `visual-engineering` | **Skills**: `["frontend-design"]` | **Wave**: Wave 2

---

### E3: project_path筛选器

**当前状态**：
- memory-list.tsx (729行) 无project_path筛选
- 后端 `GET /v1/memories` 已支持 `project_path` 参数过滤
- **无专门端点返回project_path唯一列表**

**project_path列表获取方式**（需要师尊确认）：

| 方案 | 做法 | 优缺点 |
|------|------|---------|
| **A（推荐）** | 后端新增 `GET /v1/memories/project-paths` 轻量端点 | 精确高效，但需改Rust代码 |
| B | 前端从stats API获取（如支持） | 不改后端，需确认stats是否返回 |
| C | 前端从记忆列表响应中提取 | 不改后端，但效率低需多次请求 |

**方案**：
1. 确认project_path列表获取方式
2. 创建 `omem-web/src/views/memories/components/project-filter.tsx` — 下拉筛选组件
3. 在 memory-list.tsx 集成ProjectFilter（添加到现有筛选栏）
4. 更新URL参数同步逻辑（添加 `project_path` 参数）
5. 更新 activeFilters 标签显示

**Category**: `quick` | **Wave**: Wave 2

---

## 整合执行策略

```
Wave 0 (Bug修复 — 1 task):
└── E1: confirm取消按钮bug修复 [quick]

Wave 1 (Phase 5基础 — 3 tasks, 并行):
├── T1: Plugin HTTP服务器模块 [deep]
├── T2: omem-web构建流程改造 [quick]
└── T3: 前端API client动态baseURL [quick]

Wave 2 (前端页面 — 5 tasks, 并行):
├── T4: 分类字典管理页 [visual-engineering]
├── E3: project_path筛选器 (替代T5) [quick]
├── T6: 私密记忆管理页 [visual-engineering]
├── E2: 用户画像v2适配 (替代T7) [visual-engineering]
└── T8: 应用层加密模块 [deep]

Wave 3 (集成 — 2 tasks):
├── T9: Plugin入口集成 + 端到端验证 [deep]
└── T10: 构建流水线 + 发布 [quick]

Wave FINAL (审查 — 4 parallel reviews):
├── F1: Plan compliance audit (oracle)
├── F2: Code quality review (unspecified-high)
├── F3: Real manual QA (unspecified-high)
└── F4: Scope fidelity check (deep)
```

## 需要师尊确认的决策点

1. **E3 project_path列表获取**：选方案A（新增后端端点）还是方案B/C（不改后端）？
   - 推荐A：精确高效，一行SQL聚合查询
   - 如果选A，将作为独立commit，不纳入Phase 5 commit group

2. **E2 用户画像CRUD**：偏好CRUD是否现在就做？（后端v2 API已支持）
   - 推荐：做。后端API已存在，前端对接即可

---

*Phase 5原计划详情见 `.omo/plans/phase5-local-integration.md`（942行）*
