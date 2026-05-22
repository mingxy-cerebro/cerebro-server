# omem-web 设计文档

**项目名称**：omem-web - omem自部署版Web管理端  
**创建日期**：2026-04-11  
**作者**：月儿（Sisyphus AI Agent）  
**版本**：v1.0

---

## 一、项目概述

### 1.1 项目背景

omem是一个AI Agent共享持久记忆服务，提供官方托管版（ourmem.ai）和自部署版。官方托管版提供Web管理界面（ourmem.ai/space），但自部署版仅提供REST API，无Web前端。

本项目旨在为omem自部署版开发一套完整的Web管理端，提供可视化的记忆管理、空间管理、统计分析等功能。

### 1.2 核心目标

- 提供完整的记忆CRUD操作界面
- 使用API Key进行登录认证，支持多用户（多API Key）管理
- 实现18个功能模块的完整覆盖
- 现代简约的UI设计风格
- 独立部署，不影响现有omem服务

### 1.3 技术选型

| 维度 | 技术 | 版本 | 理由 |
|------|------|------|------|
| **前端框架** | Vue 3 | 3.4+ | 渐进式、响应式系统简洁 |
| **构建工具** | Vite | 5.2+ | 极速构建、HMR快 |
| **UI组件库** | Ant Design Vue | 4.2+ | 企业级、组件丰富 |
| **状态管理** | Pinia | 2.1+ | Vue官方推荐 |
| **路由** | Vue Router | 4.3+ | 官方路由 |
| **HTTP客户端** | Axios | 1.6+ | 拦截器支持好 |
| **图表** | ECharts | 5.5+ | 功能强大 |
| **图谱** | AntV G6 | 5.x | 关系图谱专业 |

### 1.4 UI设计原则

**设计风格**：现代简约、去AI味、专业工具感

**实施要求**：
- 使用 `frontend-design` 技能指导UI实现
- 避免通用AI生成的视觉风格
- 注重细节打磨和交互体验
- 参考优秀的记忆管理工具（Notion、Obsidian等）
- 色彩方案：专业、克制、有品质感

---

## 二、系统架构

### 2.1 整体架构图

```
┌────────────────────────────────────────────┐
│         用户浏览器                          │
│   Vue 3 + Vite + Ant Design Vue + Pinia   │
│   Axios (X-API-Key Header)                 │
└──────────────────┬─────────────────────────┘
                   │
                   │ HTTPS (/v1/*)
                   ↓
┌────────────────────────────────────────────┐
│         Nginx 服务器                        │
│         www.mengxy.cc                      │
│   静态文件服务 + API反向代理                │
│   /v1/* → localhost:8080/v1/*              │
└──────────────────┬─────────────────────────┘
                   │
                   │ HTTP (localhost:8080)
                   ↓
┌────────────────────────────────────────────┐
│         omem-server                        │
│   Rust + Axum · localhost:8080             │
│   REST API (48+ endpoints)                 │
└────────────────────────────────────────────┘
```

**三层架构说明**：

**第一层：用户浏览器**
- 运行 Vue 3 单页应用（omem-web）
- 使用 Ant Design Vue 组件库
- Pinia 状态管理
- Axios 客户端（自动注入 X-API-Key Header）

**第二层：Nginx 服务器（www.mengxy.cc）**
- 静态文件服务：提供 Vue 构建产物（/index.html, /assets/*）
- API 反向代理：/v1/* → localhost:8080/v1/*
- Health 检查：/health → localhost:8080/health

**第三层：omem-server**
- Rust + Axum 后端服务
- 监听 localhost:8080
- 提供 REST API（48+ 端点）

**数据流**：
- 浏览器 → Nginx：HTTPS 请求（/v1/*）
- Nginx → omem-server：HTTP 反向代理（localhost:8080）

### 2.2 部署架构

**服务器信息**：
- IP：47.93.199.242
- 域名：www.mengxy.cc
- 系统：ECS Linux

**部署方式**：
- omem-server：运行在 `localhost:8080`
- omem-web：构建为静态文件，部署在 `/var/www/omem-web/dist`
- Nginx：监听443端口（HTTPS），反向代理API到本地8080

**Nginx配置示例**：

```nginx
server {
    listen 443 ssl http2;
    server_name www.mengxy.cc;
    
    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;
    
    # 静态文件
    location / {
        root /var/www/omem-web/dist;
        try_files $uri $uri/ /index.html;
    }
    
    # API反向代理
    location /v1/ {
        proxy_pass http://localhost:8080/v1/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
    
    # Health检查
    location /health {
        proxy_pass http://localhost:8080/health;
    }
}
```

---

## 三、功能模块设计

### 3.1 功能清单（18个模块）

| 模块 | 路由 | 对应API | 优先级 |
|------|------|---------|--------|
| 🔑 认证登录 | /login | GET /health | P0 |
| 📊 仪表盘 | /dashboard | GET /v1/stats | P0 |
| 🧠 记忆列表 | /memories | GET /v1/memories | P0 |
| 🔍 记忆搜索 | /memories/search | GET /v1/memories/search | P0 |
| 📝 记忆详情 | /memories/:id | GET /v1/memories/{id} | P0 |
| ✏️ 记忆编辑 | /memories/:id/edit | PUT /v1/memories/{id} | P0 |
| ➕ 记忆创建 | /memories/new | POST /v1/memories | P0 |
| 👤 用户画像 | /profile | GET /v1/profile | P1 |
| 🏠 空间管理 | /spaces | GET /v1/spaces | P1 |
| 👥 成员管理 | /spaces/:id/members | POST/PUT/DELETE /v1/spaces/{id}/members | P1 |
| 📤 记忆分享 | /memories/:id/share | POST /v1/memories/{id}/share | P1 |
| 📥 记忆拉取 | 集成在分享模块 | POST /v1/memories/{id}/pull | P1 |
| 🔄 过时检测 | 集成在记忆详情 | ?check_stale=true + POST reshare | P1 |
| 📁 文件上传 | /import/upload | POST /v1/files | P1 |
| 📥 批量导入 | /import/batch | POST /v1/imports | P1 |
| 📈 统计分析 | /analytics | GET /v1/stats/* | P2 |
| 📉 衰减曲线 | /analytics/decay | GET /v1/stats/decay | P2 |
| 🕸️ 关系图谱 | /analytics/relations | GET /v1/stats/relations | P2 |

### 3.2 多用户认证方案

**LocalStorage数据结构**：

```typescript
interface User {
  id: string;              // 用户唯一ID
  name: string;            // 用户自定义名称
  apiKey: string;          // omem API Key
  apiUrl: string;          // API地址（默认相对路径）
  lastUsed: string;        // 最后使用时间
}

interface AuthState {
  users: User[];           // 用户列表
  currentUserId: string;   // 当前用户ID
}
```

**认证流程**：
1. 用户访问 → 路由守卫检查 → 未登录跳转 `/login`
2. 登录页输入：API Key + 账号名称（API URL默认为相对路径）
3. 验证：调用 `GET /health` 测试连接
4. 成功：保存到Pinia + LocalStorage，跳转仪表盘
5. API请求：Axios拦截器自动注入 `X-API-Key` Header

---

## 四、目录结构

```
omem-web/
├── src/
│   ├── views/                 # 页面组件
│   │   ├── auth/
│   │   │   └── Login.vue
│   │   ├── dashboard/
│   │   │   └── Dashboard.vue
│   │   ├── memories/
│   │   │   ├── MemoryList.vue
│   │   │   ├── MemoryDetail.vue
│   │   │   ├── MemoryEdit.vue
│   │   │   └── MemorySearch.vue
│   │   ├── spaces/
│   │   │   ├── SpaceList.vue
│   │   │   ├── SpaceDetail.vue
│   │   │   └── SpaceMembers.vue
│   │   ├── analytics/
│   │   │   ├── Overview.vue
│   │   │   ├── DecayCurve.vue
│   │   │   └── RelationGraph.vue
│   │   ├── import/
│   │   │   ├── FileUpload.vue
│   │   │   └── ImportHistory.vue
│   │   ├── profile/
│   │   │   └── Profile.vue
│   │   └── settings/
│   │       └── Settings.vue
│   ├── components/           # 可复用组件
│   │   ├── layout/
│   │   │   ├── AppLayout.vue
│   │   │   ├── AppHeader.vue
│   │   │   ├── AppSidebar.vue
│   │   │   └── UserSwitcher.vue
│   │   ├── memories/
│   │   │   ├── MemoryCard.vue
│   │   │   ├── MemoryTable.vue
│   │   │   └── MemoryFilter.vue
│   │   ├── spaces/
│   │   │   ├── SpaceCard.vue
│   │   │   └── MemberManager.vue
│   │   └── charts/
│   │       ├── StatsChart.vue
│   │       └── TimelineChart.vue
│   ├── api/                 # API客户端
│   │   ├── client.ts        # Axios实例
│   │   ├── memories.ts
│   │   ├── spaces.ts
│   │   ├── stats.ts
│   │   ├── files.ts
│   │   └── types.ts
│   ├── stores/              # Pinia状态
│   │   ├── auth.ts
│   │   ├── user.ts
│   │   └── app.ts
│   ├── router/
│   │   └── index.ts
│   ├── composables/
│   │   ├── useMemories.ts
│   │   ├── useSpaces.ts
│   │   └── useAuth.ts
│   ├── utils/
│   │   ├── request.ts
│   │   ├── storage.ts
│   │   └── format.ts
│   ├── types/
│   │   ├── memory.ts
│   │   ├── space.ts
│   │   └── user.ts
│   ├── assets/
│   ├── App.vue
│   └── main.ts
├── public/
├── docs/
│   └── superpowers/
│       └── specs/
│           └── 2026-04-11-omem-web-design.md
├── .env.example
├── vite.config.ts
├── tsconfig.json
├── package.json
└── README.md
```

---

## 五、核心页面设计

> **UI实现指引**：所有页面组件开发时，必须使用 `frontend-design` 技能，确保视觉质量和去AI味。

### 5.1 主布局（AppLayout.vue）

**布局结构**：

**顶部 Header（固定）**：
- 左侧：Logo
- 中间：全局搜索框
- 右侧：用户切换下拉菜单 | 设置按钮

**主体区域**：
- 左侧 Sidebar（固定宽度200px）：
  - 📊 仪表盘
  - 🧠 记忆管理
  - 🏠 空间管理
  - 📈 统计分析
  - 📥 批量导入
  - ⚙️ 系统设置
  
- 右侧 Content Area（自适应宽度）：
  - 页面内容区域
  - 支持路由切换

### 5.2 记忆列表页

**功能**：
- 分页列表展示
- 多维度筛选（分类/层级/标签/类型/状态）
- 排序（创建时间/更新时间/重要性/访问次数）
- 批量操作（删除/分享）
- 虚拟滚动（性能优化）

**布局**：卡片式列表，每张卡片显示：
- content（主内容）
- category | tier | version
- importance | confidence
- tags | agent | space
- created | accessed次数
- 操作按钮：查看/编辑/分享/删除

**设计要点**：
- 卡片设计避免过度圆角和阴影
- 信息层级清晰，重要信息突出
- 交互反馈自然流畅
- 色彩使用克制，避免过度装饰

### 5.3 记忆详情页

**功能**：
- 完整的29个字段展示
- L0/L1/L2三层内容切换
- 关系链可视化
- 溯源信息（如果是共享副本）
- 过时检测（check_stale）
- 操作：编辑/分享/删除/刷新

**设计要点**：
- 内容为主，界面为辅
- 字段展示清晰易读
- 层级切换自然流畅
- 关系图谱简洁专业

---

## 六、技术实现要点

### 6.1 API客户端封装

```typescript
// src/api/client.ts
import axios from 'axios';
import { useAuthStore } from '@/stores/auth';

const client = axios.create({
  baseURL: '/', // 相对路径，由Nginx反向代理
  timeout: 30000,
});

// 请求拦截器：注入API Key
client.interceptors.request.use((config) => {
  const authStore = useAuthStore();
  if (authStore.currentUser?.apiKey) {
    config.headers['X-API-Key'] = authStore.currentUser.apiKey;
  }
  return config;
});

// 响应拦截器：统一错误处理
client.interceptors.response.use(
  (response) => response.data,
  (error) => {
    if (error.response?.status === 401) {
      // 认证失败，跳转登录
      router.push('/login');
    }
    return Promise.reject(error);
  }
);

export default client;
```

### 6.2 路由守卫

```typescript
// src/router/index.ts
router.beforeEach((to, from, next) => {
  const authStore = useAuthStore();
  
  if (to.path !== '/login' && !authStore.isAuthenticated) {
    next('/login');
  } else {
    next();
  }
});
```

### 6.3 状态持久化

```typescript
// src/stores/auth.ts
import { defineStore } from 'pinia';

export const useAuthStore = defineStore('auth', {
  state: () => ({
    users: [] as User[],
    currentUserId: '',
  }),
  
  persist: {
    key: 'omem_auth',
    storage: localStorage,
  },
});
```

---

## 七、开发计划

### 7.1 里程碑

| 阶段 | 时间 | 交付物 |
|------|------|--------|
| **Phase 1** | Week 1-2 | 项目脚手架 + 认证 + 记忆列表/详情 |
| **Phase 2** | Week 3 | 记忆编辑/创建 + 空间管理 |
| **Phase 3** | Week 4 | 分享功能 + 文件导入 |
| **Phase 4** | Week 5 | 统计分析 + 图表可视化 |
| **Phase 5** | Week 6 | 测试 + 优化 + 部署 |

### 7.2 技术风险

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| API接口变更 | 高 | 使用TypeScript类型定义，集中管理API |
| 大数据量性能 | 中 | 虚拟滚动、分页、懒加载 |
| 浏览器兼容性 | 低 | 使用Vite的polyfill配置 |

---

## 八、成功标准

1. ✅ 完整实现18个功能模块
2. ✅ 支持多用户（多API Key）管理
3. ✅ 响应式设计，支持桌面端和平板
4. ✅ 首屏加载时间 < 2秒
5. ✅ 通过Lighthouse性能测试 > 90分
6. ✅ 部署到 www.mengxy.cc 正常访问

---

## 九、附录

### 9.1 omem API参考

- API文档：https://github.com/ourmem/omem/blob/main/docs/API.md
- 端点总数：48+
- 认证方式：X-API-Key Header

### 9.2 环境变量

\`\`\`.env
# 开发环境
VITE_API_BASE_URL=/

# 生产环境（由Nginx反向代理）
VITE_API_BASE_URL=/
\`\`\`

---

**文档版本**：v1.0  
**最后更新**：2026-04-11  
**状态**：待审阅
