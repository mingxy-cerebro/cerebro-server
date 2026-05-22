# omem-web — AGENTS.md

## Overview

OurMemory Web 前端，React SPA，提供 AI Agent 共享记忆服务的可视化管理界面。
连接 omem-server-source 后端（REST API），支持多用户多 API Key 切换。

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | React 19 |
| Build | Vite 8 |
| Styling | Tailwind CSS 3 + tailwindcss-animate |
| UI Library | shadcn/ui (20 components in `src/components/ui/`) |
| State | Zustand 5 (with persist middleware) |
| Routing | react-router-dom 7 |
| HTTP | axios |
| Charts | recharts |
| Validation | zod |
| Toast | sonner |
| Animation | framer-motion |
| Icons | lucide-react |
| Markdown | react-markdown + remark-gfm |
| Testing | Vitest 4 (unit) + Playwright (E2E) + Testing Library + MSW |

## Project Structure

```
src/
  api/
    client.ts          — axios 实例, 请求拦截注入 X-API-Key, 响应拦截 401 跳登录
    axios.d.ts         — axios 类型扩展
  components/
    layout/            — app-layout, app-header, app-sidebar, app-footer, mobile-nav, theme-toggle
    ui/                — 20 个 shadcn/ui 组件 (alert-dialog, avatar, badge, button, card,
                        checkbox, dialog, dropdown-menu, input, label, scroll-area, select,
                        separator, sheet, skeleton, switch, table, tabs, textarea, tooltip)
    error-boundary.tsx — 全局错误边界
  hooks/               — (空目录, 待扩展)
  lib/
    utils.ts           — cn() (clsx + tailwind-merge)
    tag-utils.ts       — 标签工具函数
  providers/
    theme-provider.tsx — 主题 Provider (dark/light)
    toast-provider.tsx — sonner toast Provider
  stores/
    auth.ts            — 认证状态: 多用户管理, sessionStorage 持久化
    vault.ts           — 保险箱状态: 密码设置/验证/解锁
  test/
    setupTests.ts      — localStorage mock, jest-dom 扩展
    unit/              — 单元测试 (stores/auth.test.ts, stores/vault.test.ts, components/memory-utils.test.ts)
    e2e/               — E2E 辅助 (smoke-test.ts)
  types/
    sonner.d.ts        — sonner 类型声明
  views/               — 12 个页面视图模块 (见下)
  App.tsx              — 路由定义 + ProtectedRoute 守卫
  main.tsx             — 入口
  index.css            — 全局样式
```

## Views (路由映射)

| View Directory | Route(s) | Description |
|---------------|----------|-------------|
| `auth/` | `/login` | 登录页, API Key 认证, 多用户切换 |
| `dashboard/` | `/dashboard` | 仪表盘, 记忆统计概览 |
| `memories/` | `/memories`, `/memories/:id`, `/memories/new`, `/memories/:id/edit`, `/memories/:id/edit-insight` | 记忆 CRUD + 列表 + 详情 + Insight 编辑 |
| `vault/` | `/vault` | 保险箱管理, 加密记忆查看 |
| `spaces/` | `/spaces` | 团队/组织空间管理 |
| `sessions/` | `/sessions`, `/sessions/:id` | 会话列表 + 详情 |
| `analytics/` | `/analytics` | 数据分析图表 (recharts) |
| `tier-history/` | `/tier-history` | 记忆层级变更历史 |
| `import/` | `/import` | 记忆批量导入 |
| `settings/` | `/settings` | 应用设置页 |
| `profile/` | `/profile` | 用户资料页 |
| `error/` | `*` | 404 Not Found 页 |

## State Management

**Zustand** — 两个全局 store, 无中间件依赖:

- `useAuthStore` (`stores/auth.ts`): 多用户 Profile 管理, sessionStorage 持久化 (key: `omem-auth`), 提供 `users[]`, `currentUserId`, `isAuthenticated`, 登录/登出/切换用户
- `useVaultStore` (`stores/vault.ts`): 保险箱解锁状态, 调用 `/v1/vault/*` API

## API Client

- 实例: `src/api/client.ts`, axios, baseURL `/`, timeout 30s
- 认证: 请求拦截从 `useAuthStore` 读取当前用户的 `apiKey`, 注入 `X-API-Key` + `X-Agent-ID: omem-web`
- 响应拦截: 401 时自动 logout 并跳转 `/login`
- 响应拦截: 成功响应自动解包 `response.data`
- 代理: Vite dev server 将 `/v1` 和 `/health` 代理到后端 (vite.config.ts)

## Testing

### Unit Tests
- Runner: Vitest 4 (jsdom)
- Setup: `src/test/setupTests.ts` (localStorage mock)
- Location: `src/test/unit/**/*.test.ts`
- Coverage 排除: `node_modules/`, `src/test/`, `src/components/ui/`
- Commands:
  - `npm test` — 运行全部单元测试
  - `npm run test:watch` — 监听模式
  - `npm run test:ui` — Vitest UI

### E2E Tests
- Runner: Playwright
- Setup: `e2e/auth.setup.ts` (auth bypass)
- Location: `e2e/*.spec.ts` (login, dashboard, memories, navigation)
- Command: `npm run test:e2e`
- Auth bypass: 设置 `localStorage.e2e_bypass_auth = 'true'` 跳过登录

## Commands

```bash
npm run dev          # Vite dev server (热更新)
npm run build        # tsc + vite build (生产)
npm run preview      # 预览生产构建
npm run lint         # ESLint 检查
npm test             # Vitest 单元测试
npm run test:watch   # Vitest 监听模式
npm run test:e2e     # Playwright E2E 测试
```

## Conventions

- Path alias: `@/` 映射 `src/` (TypeScript + Vite 统一配置)
- 组件样式: Tailwind CSS 类名, 通过 `cn()` (clsx + tailwind-merge) 合并
- shadcn/ui 组件: `src/components/ui/`, 通过 `shadcn` CLI 添加, 不手动编辑
- 路由: 嵌套布局, `AppLayout` 包裹所有认证页面, `ProtectedRoute` 守卫
- 状态: 全局用 Zustand, 局部用 React useState/useReducer
- API 调用: 统一通过 `src/api/client.ts`, 不直接使用 axios
- 类型: 共享类型定义在 `src/types/`, 组件内联类型按需定义
- 认证: 多用户支持, sessionStorage 存储, API Key 模式 (非 JWT)
- ESM: `"type": "module"` in package.json
