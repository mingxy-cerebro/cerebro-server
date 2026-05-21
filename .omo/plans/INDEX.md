# OMEM 开发计划索引

> 本文档是所有开发计划的入口索引。使用 `/start-work` 时输入此文件路径即可查看所有计划。
> 每个阶段任务完成后，需同时更新此索引和对应的详细计划文档。

---

## 索引使用方式

1. 新窗口执行 `/start-work`，输入此文件路径：`.omo/plans/INDEX.md`
2. 选择要执行的计划文件
3. 完成后更新：(1) 详细计划中的 TODO 勾选 (2) 本索引中的状态

---

## 执行优先级

```
1️⃣ Phase 3: 记忆隔离 (project_path)     ← 下一个执行
   ↓
2️⃣ Phase 6a: 独立画像系统
   ↓
3️⃣ Session Ingest 质量优化 (独立)
```

---

## 计划总览

| # | 计划文件 | 标题 | 状态 | 阶段 | 优先级 |
|---|---------|------|------|------|--------|
| 1 | `phase3-memory-isolation.md` | Phase 3: 记忆隔离 (project_path) | 📋 待开始 | Phase 3 | 🔴 P0 |
| 2 | `phase6a-independent-profile.md` | Phase 6a: 独立画像系统 | 📋 待开始 | Phase 6a | 🟡 P1 |
| 3 | `session-ingest-optimization.md` | Session Ingest 质量优化 | 📋 待开始 | 独立 | 🟢 P2 |

### 已完成归档 (不显示详细计划)

| 阶段 | 已完成计划 |
|------|-----------|
| Phase 0-2 | v1.8.0-ingest-fixes, v2.0-session-ingest-overhaul, memory-quality-fix, lifecycle-prompt, memory-system-refactor-final, v1.9.0-audit-fixes, phase2-categories-dict, fix-agent-readonly-bypass |
| Phase 3b | phase3b-private-memory |
| Phase 4 | recall-optimization |
| Phase 5-5b | phase5-local-integration, plugin-config-recall-log |

---

## 待开始计划详情

### 1️⃣ Phase 3: 记忆隔离 (project_path) — 🔴 P0 下一个执行
- **文件**: `phase3-memory-isolation.md`
- **范围**: Memory 新增 `project_path` 字段，召回时按项目路径隔离
- **核心机制**: 项目工作记忆按目录隔离，全局偏好/画像/情感跨项目共享
- **预估工作量**: Medium (~600行, 8-10文件)
- **关键改动**: 
  - Memory.project_path + LanceDB schema migration
  - WHERE 子句硬过滤（非 tags 排名）
  - sanitize_project_path() 安全函数
  - should_recall 两阶段搜索适配
- **Momus 审查**: 待执行
- **Wave 结构**: 3 waves

### 2️⃣ Phase 6a: 独立画像系统 — 🟡 P1
- **文件**: `phase6a-independent-profile.md`
- **范围**: 用户画像的自动生成和独立管理
- **预估工作量**: Large
- **前置依赖**: 无 (独立于 P3)
- **关键改动**: ProfileService 增强, auto-generation, profile API

### 3️⃣ Session Ingest 质量优化 — 🟢 P2
- **文件**: `session-ingest-optimization.md`
- **范围**: session_ingest 的去重质量和噪声过滤优化
- **预估工作量**: Medium (2-3天)
- **前置依赖**: 无
- **关键改动**: 
  - replaces tracking (LLM 智能合并)
  - VALUE FILTER prompt 增强
  - Batch cluster assignment
  - Session ingest 信号量 + session_locks TTL
- **Momus 审查**: OKAY ✅
- **Wave 结构**: 3 waves, 7 tasks + 4 review tasks

---

## 阶段路线图

```
Phase 0-2 (基础+质量+分类)  ✅ 已完成，计划已归档
  ↓
Phase 3 (记忆隔离)          📋 待开始 ← 下一个
  ↓
Phase 3b (私密记忆)         ✅ 已完成
Phase 4  (检索优化)         ✅ 已完成
Phase 5  (本地集成+Plugin)  ✅ 已完成
  ↓
Phase 6a (画像系统)         📋 待开始
  ↓
独立优化 (Session Ingest)   📋 待开始
```

---

## 更新日志

| 日期 | 操作 | 计划 |
|------|------|------|
| 2026-05-19 | 创建索引 | 初始版本 |
| 2026-05-19 | 新增计划 | session-ingest-optimization (Momus OKAY) |
| 2026-05-19 | 归档旧计划 | 删除 Phase 0-2 的 8 个已完成计划 |
| 2026-05-19 | 调整优先级 | 师尊指定: P3 → 6a → session-ingest |
