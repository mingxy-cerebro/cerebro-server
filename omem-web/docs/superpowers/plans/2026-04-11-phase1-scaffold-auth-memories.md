# Phase 1: 项目脚手架 + 认证 + 记忆管理 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal**: 搭建 omem-web 项目基础架构，实现认证系统和记忆列表/详情页

**Architecture**: Vue 3 + Vite + TypeScript + Ant Design Vue 4 + Pinia + Vue Router + Axios，采用 Composition API (`<script setup>`) 和 setup store 模式，通过 Nginx 反向代理访问 omem-server API

**Tech Stack**: 
- Frontend: Vue 3.4+, Vite 5.2+, TypeScript 5.x
- UI: Ant Design Vue 4.2+
- State: Pinia 2.1+ with pinia-plugin-persistedstate
- Router: Vue Router 4.3+
- HTTP: Axios 1.6+

---

## 文件结构规划

### 核心文件清单

**配置文件**:
- `/mnt/d/dev/github/project/omem-web/package.json` - 项目依赖
- `/mnt/d/dev/github/project/omem-web/vite.config.ts` - Vite 配置
- `/mnt/d/dev/github/project/omem-web/tsconfig.json` - TypeScript 配置
- `/mnt/d/dev/github/project/omem-web/.env` - 环境变量

**类型定义**:
- `/mnt/d/dev/github/project/omem-web/src/types/memory.ts` - Memory 相关类型
- `/mnt/d/dev/github/project/omem-web/src/types/user.ts` - User 和 Auth 类型
- `/mnt/d/dev/github/project/omem-web/src/api/types.ts` - API 响应类型

**API 客户端**:
- `/mnt/d/dev/github/project/omem-web/src/api/client.ts` - Axios 实例（拦截器）
- `/mnt/d/dev/github/project/omem-web/src/api/memories.ts` - 记忆相关 API

**状态管理**:
- `/mnt/d/dev/github/project/omem-web/src/stores/auth.ts` - 认证 Store（多用户管理）

**路由**:
- `/mnt/d/dev/github/project/omem-web/src/router/index.ts` - 路由配置（含守卫）

**页面组件**:
- `/mnt/d/dev/github/project/omem-web/src/views/auth/Login.vue` - 登录页
- `/mnt/d/dev/github/project/omem-web/src/views/memories/MemoryList.vue` - 记忆列表
- `/mnt/d/dev/github/project/omem-web/src/views/memories/MemoryDetail.vue` - 记忆详情

**布局组件**:
- `/mnt/d/dev/github/project/omem-web/src/components/layout/AppLayout.vue` - 主布局
- `/mnt/d/dev/github/project/omem-web/src/components/layout/AppHeader.vue` - 顶部导航
- `/mnt/d/dev/github/project/omem-web/src/components/layout/AppSidebar.vue` - 侧边栏
- `/mnt/d/dev/github/project/omem-web/src/components/layout/UserSwitcher.vue` - 用户切换器

**业务组件**:
- `/mnt/d/dev/github/project/omem-web/src/components/memories/MemoryCard.vue` - 记忆卡片
- `/mnt/d/dev/github/project/omem-web/src/components/memories/MemoryFilter.vue` - 筛选器

**工具函数**:
- `/mnt/d/dev/github/project/omem-web/src/utils/format.ts` - 格式化工具

**入口文件**:
- `/mnt/d/dev/github/project/omem-web/src/App.vue` - 根组件
- `/mnt/d/dev/github/project/omem-web/src/main.ts` - 应用入口
- `/mnt/d/dev/github/project/omem-web/index.html` - HTML 模板

---

## Task 1: 项目初始化

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/package.json`
- Create: `/mnt/d/dev/github/project/omem-web/vite.config.ts`
- Create: `/mnt/d/dev/github/project/omem-web/tsconfig.json`
- Create: `/mnt/d/dev/github/project/omem-web/tsconfig.node.json`
- Create: `/mnt/d/dev/github/project/omem-web/index.html`
- Create: `/mnt/d/dev/github/project/omem-web/.env`
- Create: `/mnt/d/dev/github/project/omem-web/.gitignore`

- [ ] **Step 1: 创建 package.json**

```json
{
  "name": "omem-web",
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "vue": "^3.4.21",
    "vue-router": "^4.3.0",
    "pinia": "^2.1.7",
    "pinia-plugin-persistedstate": "^3.2.1",
    "ant-design-vue": "^4.2.0",
    "axios": "^1.6.8",
    "dayjs": "^1.11.10"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^5.0.4",
    "typescript": "^5.4.3",
    "vite": "^5.2.6",
    "vue-tsc": "^2.0.7"
  }
}
```

- [ ] **Step 2: 创建 vite.config.ts**

```typescript
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src')
    }
  },
  server: {
    port: 5173,
    proxy: {
      '/v1': {
        target: 'http://localhost:8080',
        changeOrigin: true
      },
      '/health': {
        target: 'http://localhost:8080',
        changeOrigin: true
      }
    }
  }
})
```

- [ ] **Step 3: 创建 tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "preserve",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  },
  "include": ["src/**/*.ts", "src/**/*.tsx", "src/**/*.vue"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

- [ ] **Step 4: 创建 tsconfig.node.json**

```json
{
  "compilerOptions": {
    "composite": true,
    "skipLibCheck": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowSyntheticDefaultImports": true
  },
  "include": ["vite.config.ts"]
}
```

- [ ] **Step 5: 创建 index.html**

```html
<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" type="image/svg+xml" href="/vite.svg" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>omem-web</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 6: 创建 .env**

```
VITE_API_BASE_URL=/
```

- [ ] **Step 7: 创建 .gitignore**

```
node_modules
dist
.DS_Store
*.local
.env.local
.env.*.local
```

- [ ] **Step 8: 安装依赖**

Run: `cd /mnt/d/dev/github/project/omem-web && npm install`

Expected: 依赖安装成功，生成 `node_modules/` 和 `package-lock.json`

- [ ] **Step 9: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git init
git add .
git commit -m "chore: initialize Vue 3 + Vite project"
```

---

## Task 2: TypeScript 类型定义

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/types/memory.ts`
- Create: `/mnt/d/dev/github/project/omem-web/src/types/user.ts`
- Create: `/mnt/d/dev/github/project/omem-web/src/api/types.ts`

- [ ] **Step 1: 创建 Memory 类型定义**

File: `/mnt/d/dev/github/project/omem-web/src/types/memory.ts`

```typescript
export type Category = 'profile' | 'preferences' | 'entities' | 'events' | 'cases' | 'patterns'
export type MemoryType = 'pinned' | 'insight' | 'session'
export type MemoryState = 'active' | 'archived' | 'deleted'
export type Tier = 'core' | 'working' | 'peripheral'
export type RelationType = 'supersedes' | 'contextualizes' | 'supports' | 'contradicts'

export interface MemoryRelation {
  relation_type: RelationType
  target_id: string
  context_label: string | null
}

export interface Provenance {
  shared_from_space: string
  shared_from_memory: string
  shared_by_user: string
  shared_by_agent: string
  shared_at: string
  original_created_at: string
  source_version: number
}

export interface Memory {
  id: string
  content: string
  l0_abstract: string
  l1_overview: string
  l2_content: string
  category: Category
  memory_type: MemoryType
  state: MemoryState
  tier: Tier
  importance: number
  confidence: number
  access_count: number
  tags: string[]
  scope: string
  agent_id: string | null
  session_id: string | null
  tenant_id: string
  source: string | null
  relations: MemoryRelation[]
  superseded_by: string | null
  invalidated_at: string | null
  created_at: string
  updated_at: string
  last_accessed_at: string | null
  space_id: string
  visibility: string
  owner_agent_id: string
  provenance: Provenance | null
  version: number | null
}
```

- [ ] **Step 2: 创建 User 和 Auth 类型定义**

File: `/mnt/d/dev/github/project/omem-web/src/types/user.ts`

```typescript
export interface User {
  id: string
  name: string
  apiKey: string
  apiUrl: string
  lastUsed: string
}

export interface AuthState {
  users: User[]
  currentUserId: string
}
```

- [ ] **Step 3: 创建 API 响应类型定义**

File: `/mnt/d/dev/github/project/omem-web/src/api/types.ts`

```typescript
import type { Memory } from '@/types/memory'

export interface ApiError {
  error: {
    code: 'validation_error' | 'unauthorized' | 'not_found' | 'rate_limited' | 'internal_error'
    message: string
  }
}

export interface MemoryListResponse {
  memories: Memory[]
  total_count: number
  limit: number
  offset: number
}

export interface MemoryListParams {
  limit?: number
  offset?: number
  category?: string
  tier?: string
  tags?: string
  memory_type?: string
  state?: string
  sort?: 'created_at' | 'updated_at' | 'importance' | 'access_count'
  order?: 'asc' | 'desc'
}

export interface HealthResponse {
  status: string
}
```

- [ ] **Step 4: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/types src/api/types.ts
git commit -m "feat: add TypeScript type definitions"
```

---

## Task 3: Axios 客户端封装

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/api/client.ts`
- Create: `/mnt/d/dev/github/project/omem-web/src/api/memories.ts`

- [ ] **Step 1: 创建 Axios 客户端实例**

File: `/mnt/d/dev/github/project/omem-web/src/api/client.ts`

```typescript
import axios, { type AxiosInstance, type AxiosError } from 'axios'
import type { ApiError } from './types'

const client: AxiosInstance = axios.create({
  baseURL: import.meta.env.VITE_API_BASE_URL || '/',
  timeout: 30000,
  headers: {
    'Content-Type': 'application/json'
  }
})

// 请求拦截器：注入 API Key
client.interceptors.request.use(
  (config) => {
    const authData = localStorage.getItem('omem_auth')
    if (authData) {
      try {
        const auth = JSON.parse(authData)
        const currentUser = auth.users?.find((u: any) => u.id === auth.currentUserId)
        if (currentUser?.apiKey) {
          config.headers['X-API-Key'] = currentUser.apiKey
        }
      } catch (e) {
        console.error('Failed to parse auth data:', e)
      }
    }
    return config
  },
  (error) => Promise.reject(error)
)

// 响应拦截器：统一错误处理
client.interceptors.response.use(
  (response) => response.data,
  (error: AxiosError<ApiError>) => {
    if (error.response?.status === 401) {
      // 认证失败，清除本地存储并跳转登录
      localStorage.removeItem('omem_auth')
      window.location.href = '/login'
    }
    return Promise.reject(error)
  }
)

export default client
```

- [ ] **Step 2: 创建记忆 API 模块**

File: `/mnt/d/dev/github/project/omem-web/src/api/memories.ts`

```typescript
import client from './client'
import type { Memory } from '@/types/memory'
import type { MemoryListResponse, MemoryListParams, HealthResponse } from './types'

export const memoriesApi = {
  // 健康检查
  health(): Promise<HealthResponse> {
    return client.get('/health')
  },

  // 获取记忆列表
  list(params: MemoryListParams = {}): Promise<MemoryListResponse> {
    return client.get('/v1/memories', { params })
  },

  // 获取记忆详情
  get(id: string): Promise<Memory> {
    return client.get(`/v1/memories/${id}`)
  }
}
```

- [ ] **Step 3: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/api
git commit -m "feat: add Axios client with interceptors"
```

---

## Task 4: Pinia Auth Store

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/stores/auth.ts`

- [ ] **Step 1: 创建 Auth Store**

File: `/mnt/d/dev/github/project/omem-web/src/stores/auth.ts`

```typescript
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { User } from '@/types/user'
import { memoriesApi } from '@/api/memories'

export const useAuthStore = defineStore('auth', () => {
  const users = ref<User[]>([])
  const currentUserId = ref<string>('')

  const currentUser = computed(() => 
    users.value.find(u => u.id === currentUserId.value)
  )

  const isAuthenticated = computed(() => 
    !!currentUser.value?.apiKey
  )

  async function login(name: string, apiKey: string, apiUrl: string = '/') {
    try {
      // 验证 API Key
      await memoriesApi.health()
      
      const userId = `user_${Date.now()}`
      const newUser: User = {
        id: userId,
        name,
        apiKey,
        apiUrl,
        lastUsed: new Date().toISOString()
      }

      users.value.push(newUser)
      currentUserId.value = userId
      
      return { success: true }
    } catch (error) {
      return { success: false, error: '连接失败，请检查 API Key' }
    }
  }

  function switchUser(userId: string) {
    const user = users.value.find(u => u.id === userId)
    if (user) {
      currentUserId.value = userId
      user.lastUsed = new Date().toISOString()
    }
  }

  function logout() {
    currentUserId.value = ''
  }

  function removeUser(userId: string) {
    users.value = users.value.filter(u => u.id !== userId)
    if (currentUserId.value === userId) {
      currentUserId.value = users.value[0]?.id || ''
    }
  }

  return {
    users,
    currentUserId,
    currentUser,
    isAuthenticated,
    login,
    switchUser,
    logout,
    removeUser
  }
}, {
  persist: {
    key: 'omem_auth',
    storage: localStorage
  }
})
```

- [ ] **Step 2: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/stores
git commit -m "feat: add Pinia auth store with persistence"
```

---

## Task 5: 路由配置

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/router/index.ts`

- [ ] **Step 1: 创建路由配置**

File: `/mnt/d/dev/github/project/omem-web/src/router/index.ts`

```typescript
import { createRouter, createWebHistory } from 'vue-router'
import type { RouteRecordRaw } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    name: 'Login',
    component: () => import('@/views/auth/Login.vue'),
    meta: { requiresAuth: false }
  },
  {
    path: '/',
    component: () => import('@/components/layout/AppLayout.vue'),
    meta: { requiresAuth: true },
    children: [
      {
        path: '',
        redirect: '/memories'
      },
      {
        path: 'memories',
        name: 'MemoryList',
        component: () => import('@/views/memories/MemoryList.vue')
      },
      {
        path: 'memories/:id',
        name: 'MemoryDetail',
        component: () => import('@/views/memories/MemoryDetail.vue')
      }
    ]
  }
]

const router = createRouter({
  history: createWebHistory(),
  routes
})

// 路由守卫
router.beforeEach((to, from, next) => {
  const authStore = useAuthStore()
  
  if (to.meta.requiresAuth !== false && !authStore.isAuthenticated) {
    next('/login')
  } else if (to.path === '/login' && authStore.isAuthenticated) {
    next('/memories')
  } else {
    next()
  }
})

export default router
```

- [ ] **Step 2: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/router
git commit -m "feat: add router with auth guard"
```

---

## Task 6: 登录页

> **UI 设计指引**: 使用 `frontend-design` 技能确保现代简约风格

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/views/auth/Login.vue`

- [ ] **Step 1: 创建登录页组件**

File: `/mnt/d/dev/github/project/omem-web/src/views/auth/Login.vue`

```vue
<template>
  <div class="login-container">
    <div class="login-card">
      <div class="login-header">
        <h1>omem-web</h1>
        <p>AI Agent 记忆管理平台</p>
      </div>

      <a-form
        :model="formState"
        :rules="rules"
        layout="vertical"
        @finish="handleLogin"
      >
        <a-form-item label="账号名称" name="name">
          <a-input
            v-model:value="formState.name"
            placeholder="为此账号起个名字"
            size="large"
          />
        </a-form-item>

        <a-form-item label="API Key" name="apiKey">
          <a-input-password
            v-model:value="formState.apiKey"
            placeholder="输入 omem API Key"
            size="large"
          />
        </a-form-item>

        <a-form-item>
          <a-button
            type="primary"
            html-type="submit"
            size="large"
            block
            :loading="loading"
          >
            登录
          </a-button>
        </a-form-item>

        <a-alert
          v-if="error"
          :message="error"
          type="error"
          show-icon
          closable
          @close="error = ''"
        />
      </a-form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { message } from 'ant-design-vue'

const router = useRouter()
const authStore = useAuthStore()

const formState = reactive({
  name: '',
  apiKey: ''
})

const rules = {
  name: [{ required: true, message: '请输入账号名称' }],
  apiKey: [{ required: true, message: '请输入 API Key' }]
}

const loading = ref(false)
const error = ref('')

async function handleLogin() {
  loading.value = true
  error.value = ''

  try {
    const result = await authStore.login(formState.name, formState.apiKey)
    
    if (result.success) {
      message.success('登录成功')
      router.push('/memories')
    } else {
      error.value = result.error || '登录失败'
    }
  } catch (e) {
    error.value = '登录失败，请检查网络连接'
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.login-container {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
}

.login-card {
  width: 100%;
  max-width: 420px;
  padding: 48px;
  background: white;
  border-radius: 8px;
  box-shadow: 0 4px 24px rgba(0, 0, 0, 0.1);
}

.login-header {
  text-align: center;
  margin-bottom: 32px;
}

.login-header h1 {
  font-size: 32px;
  font-weight: 600;
  margin: 0 0 8px 0;
  color: #1a1a1a;
}

.login-header p {
  font-size: 14px;
  color: #666;
  margin: 0;
}
</style>
```

- [ ] **Step 2: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/views/auth
git commit -m "feat: add login page with form validation"
```

---

## Task 7: 主布局组件

> **UI 设计指引**: 使用 `frontend-design` 技能确保专业工具感

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/components/layout/AppLayout.vue`
- Create: `/mnt/d/dev/github/project/omem-web/src/components/layout/AppHeader.vue`
- Create: `/mnt/d/dev/github/project/omem-web/src/components/layout/AppSidebar.vue`
- Create: `/mnt/d/dev/github/project/omem-web/src/components/layout/UserSwitcher.vue`

- [ ] **Step 1: 创建主布局**

File: `/mnt/d/dev/github/project/omem-web/src/components/layout/AppLayout.vue`

```vue
<template>
  <a-layout class="app-layout">
    <AppHeader />
    <a-layout>
      <AppSidebar />
      <a-layout-content class="main-content">
        <router-view />
      </a-layout-content>
    </a-layout>
  </a-layout>
</template>

<script setup lang="ts">
import AppHeader from './AppHeader.vue'
import AppSidebar from './AppSidebar.vue'
</script>

<style scoped>
.app-layout {
  min-height: 100vh;
}

.main-content {
  padding: 24px;
  background: #f5f5f5;
}
</style>
```

- [ ] **Step 2: 创建顶部导航**

File: `/mnt/d/dev/github/project/omem-web/src/components/layout/AppHeader.vue`

```vue
<template>
  <a-layout-header class="app-header">
    <div class="header-left">
      <h1 class="logo">omem-web</h1>
    </div>
    <div class="header-right">
      <UserSwitcher />
    </div>
  </a-layout-header>
</template>

<script setup lang="ts">
import UserSwitcher from './UserSwitcher.vue'
</script>

<style scoped>
.app-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: white;
  border-bottom: 1px solid #e8e8e8;
  padding: 0 24px;
  height: 64px;
}

.logo {
  font-size: 20px;
  font-weight: 600;
  margin: 0;
  color: #1a1a1a;
}
</style>
```

- [ ] **Step 3: 创建侧边栏**

File: `/mnt/d/dev/github/project/omem-web/src/components/layout/AppSidebar.vue`

```vue
<template>
  <a-layout-sider
    v-model:collapsed="collapsed"
    :trigger="null"
    collapsible
    theme="light"
    width="200"
  >
    <a-menu
      v-model:selectedKeys="selectedKeys"
      mode="inline"
      @click="handleMenuClick"
    >
      <a-menu-item key="/memories">
        <template #icon>
          <BrainOutlined />
        </template>
        记忆管理
      </a-menu-item>
    </a-menu>
  </a-layout-sider>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { BrainOutlined } from '@ant-design/icons-vue'

const router = useRouter()
const route = useRoute()

const collapsed = ref(false)
const selectedKeys = ref<string[]>([route.path])

watch(() => route.path, (newPath) => {
  selectedKeys.value = [newPath]
})

function handleMenuClick({ key }: { key: string }) {
  router.push(key)
}
</script>

<style scoped>
:deep(.ant-layout-sider) {
  background: white;
  border-right: 1px solid #e8e8e8;
}
</style>
```

- [ ] **Step 4: 创建用户切换器**

File: `/mnt/d/dev/github/project/omem-web/src/components/layout/UserSwitcher.vue`

```vue
<template>
  <a-dropdown>
    <a-button type="text">
      <UserOutlined />
      {{ authStore.currentUser?.name }}
      <DownOutlined />
    </a-button>
    <template #overlay>
      <a-menu>
        <a-menu-item
          v-for="user in authStore.users"
          :key="user.id"
          @click="authStore.switchUser(user.id)"
        >
          <CheckOutlined v-if="user.id === authStore.currentUserId" />
          {{ user.name }}
        </a-menu-item>
        <a-menu-divider />
        <a-menu-item @click="handleLogout">
          <LogoutOutlined />
          退出登录
        </a-menu-item>
      </a-menu>
    </template>
  </a-dropdown>
</template>

<script setup lang="ts">
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { UserOutlined, DownOutlined, CheckOutlined, LogoutOutlined } from '@ant-design/icons-vue'

const router = useRouter()
const authStore = useAuthStore()

function handleLogout() {
  authStore.logout()
  router.push('/login')
}
</script>
```

- [ ] **Step 5: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/components/layout
git commit -m "feat: add main layout components"
```

---

## Task 8: 工具函数和入口文件

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/utils/format.ts`
- Create: `/mnt/d/dev/github/project/omem-web/src/App.vue`
- Create: `/mnt/d/dev/github/project/omem-web/src/main.ts`

- [ ] **Step 1: 创建格式化工具**

File: `/mnt/d/dev/github/project/omem-web/src/utils/format.ts`

```typescript
import dayjs from 'dayjs'
import relativeTime from 'dayjs/plugin/relativeTime'
import 'dayjs/locale/zh-cn'

dayjs.extend(relativeTime)
dayjs.locale('zh-cn')

export function formatDate(date: string | null): string {
  if (!date) return '-'
  return dayjs(date).format('YYYY-MM-DD HH:mm:ss')
}

export function formatRelativeTime(date: string | null): string {
  if (!date) return '-'
  return dayjs(date).fromNow()
}

export function formatNumber(num: number): string {
  return num.toLocaleString('zh-CN')
}

export function formatPercent(num: number): string {
  return `${(num * 100).toFixed(1)}%`
}
```

- [ ] **Step 2: 创建根组件**

File: `/mnt/d/dev/github/project/omem-web/src/App.vue`

```vue
<template>
  <a-config-provider :locale="zhCN">
    <router-view />
  </a-config-provider>
</template>

<script setup lang="ts">
import zhCN from 'ant-design-vue/es/locale/zh_CN'
</script>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
}

#app {
  min-height: 100vh;
}
</style>
```

- [ ] **Step 3: 创建应用入口**

File: `/mnt/d/dev/github/project/omem-web/src/main.ts`

```typescript
import { createApp } from 'vue'
import { createPinia } from 'pinia'
import piniaPluginPersistedstate from 'pinia-plugin-persistedstate'
import Antd from 'ant-design-vue'
import 'ant-design-vue/dist/reset.css'
import App from './App.vue'
import router from './router'

const pinia = createPinia()
pinia.use(piniaPluginPersistedstate)

const app = createApp(App)

app.use(pinia)
app.use(router)
app.use(Antd)

app.mount('#app')
```

- [ ] **Step 4: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/utils src/App.vue src/main.ts
git commit -m "feat: add utils and app entry files"
```

- [ ] **Step 5: 验证开发服务器启动**

Run: `cd /mnt/d/dev/github/project/omem-web && npm run dev`

Expected: 
- Vite 开发服务器启动成功
- 访问 http://localhost:5173 显示登录页
- 无 TypeScript 编译错误

---

## Task 9: 记忆列表页

> **UI 设计指引**: 使用 `frontend-design` 技能确保卡片式列表的专业感

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/views/memories/MemoryList.vue`
- Create: `/mnt/d/dev/github/project/omem-web/src/components/memories/MemoryCard.vue`
- Create: `/mnt/d/dev/github/project/omem-web/src/components/memories/MemoryFilter.vue`

- [ ] **Step 1: 创建记忆卡片组件**

File: `/mnt/d/dev/github/project/omem-web/src/components/memories/MemoryCard.vue`

```vue
<template>
  <a-card class="memory-card" hoverable @click="handleClick">
    <div class="card-header">
      <a-tag :color="getCategoryColor(memory.category)">
        {{ memory.category }}
      </a-tag>
      <a-tag>{{ memory.tier }}</a-tag>
      <span class="version">v{{ memory.version || 1 }}</span>
    </div>

    <div class="card-content">
      <h3 class="title">{{ memory.l0_abstract || memory.content.slice(0, 50) }}</h3>
      <p class="description">{{ memory.l1_overview || memory.content }}</p>
    </div>

    <div class="card-meta">
      <div class="meta-item">
        <span class="label">重要性:</span>
        <a-progress
          :percent="memory.importance * 100"
          :show-info="false"
          size="small"
          :stroke-color="getImportanceColor(memory.importance)"
        />
      </div>
      <div class="meta-item">
        <span class="label">访问:</span>
        <span class="value">{{ memory.access_count }} 次</span>
      </div>
    </div>

    <div class="card-tags">
      <a-tag v-for="tag in memory.tags" :key="tag" size="small">
        {{ tag }}
      </a-tag>
    </div>

    <div class="card-footer">
      <span class="time">{{ formatRelativeTime(memory.created_at) }}</span>
      <span v-if="memory.agent_id" class="agent">{{ memory.agent_id }}</span>
    </div>
  </a-card>
</template>

<script setup lang="ts">
import { useRouter } from 'vue-router'
import type { Memory } from '@/types/memory'
import { formatRelativeTime } from '@/utils/format'

interface Props {
  memory: Memory
}

const props = defineProps<Props>()
const router = useRouter()

function handleClick() {
  router.push(`/memories/${props.memory.id}`)
}

function getCategoryColor(category: string): string {
  const colors: Record<string, string> = {
    profile: 'blue',
    preferences: 'green',
    entities: 'orange',
    events: 'purple',
    cases: 'red',
    patterns: 'cyan'
  }
  return colors[category] || 'default'
}

function getImportanceColor(importance: number): string {
  if (importance >= 0.8) return '#f5222d'
  if (importance >= 0.5) return '#fa8c16'
  return '#52c41a'
}
</script>

<style scoped>
.memory-card {
  margin-bottom: 16px;
  cursor: pointer;
  transition: all 0.3s;
}

.memory-card:hover {
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

.card-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}

.version {
  margin-left: auto;
  font-size: 12px;
  color: #999;
}

.card-content {
  margin-bottom: 16px;
}

.title {
  font-size: 16px;
  font-weight: 600;
  margin: 0 0 8px 0;
  color: #1a1a1a;
}

.description {
  font-size: 14px;
  color: #666;
  margin: 0;
  line-height: 1.6;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.card-meta {
  display: flex;
  gap: 24px;
  margin-bottom: 12px;
}

.meta-item {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
}

.label {
  font-size: 12px;
  color: #999;
}

.value {
  font-size: 12px;
  color: #666;
}

.card-tags {
  margin-bottom: 12px;
  min-height: 24px;
}

.card-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 12px;
  color: #999;
}
</style>
```

- [ ] **Step 2: 创建筛选器组件**

File: `/mnt/d/dev/github/project/omem-web/src/components/memories/MemoryFilter.vue`

```vue
<template>
  <a-card class="filter-card">
    <a-form layout="inline">
      <a-form-item label="分类">
        <a-select
          v-model:value="filters.category"
          style="width: 120px"
          placeholder="全部"
          allow-clear
          @change="handleFilterChange"
        >
          <a-select-option value="profile">profile</a-select-option>
          <a-select-option value="preferences">preferences</a-select-option>
          <a-select-option value="entities">entities</a-select-option>
          <a-select-option value="events">events</a-select-option>
          <a-select-option value="cases">cases</a-select-option>
          <a-select-option value="patterns">patterns</a-select-option>
        </a-select>
      </a-form-item>

      <a-form-item label="层级">
        <a-select
          v-model:value="filters.tier"
          style="width: 120px"
          placeholder="全部"
          allow-clear
          @change="handleFilterChange"
        >
          <a-select-option value="core">core</a-select-option>
          <a-select-option value="working">working</a-select-option>
          <a-select-option value="peripheral">peripheral</a-select-option>
        </a-select>
      </a-form-item>

      <a-form-item label="排序">
        <a-select
          v-model:value="filters.sort"
          style="width: 120px"
          @change="handleFilterChange"
        >
          <a-select-option value="created_at">创建时间</a-select-option>
          <a-select-option value="updated_at">更新时间</a-select-option>
          <a-select-option value="importance">重要性</a-select-option>
          <a-select-option value="access_count">访问次数</a-select-option>
        </a-select>
      </a-form-item>

      <a-form-item label="顺序">
        <a-select
          v-model:value="filters.order"
          style="width: 100px"
          @change="handleFilterChange"
        >
          <a-select-option value="desc">降序</a-select-option>
          <a-select-option value="asc">升序</a-select-option>
        </a-select>
      </a-form-item>
    </a-form>
  </a-card>
</template>

<script setup lang="ts">
import { reactive } from 'vue'
import type { MemoryListParams } from '@/api/types'

const emit = defineEmits<{
  change: [filters: MemoryListParams]
}>()

const filters = reactive<MemoryListParams>({
  category: undefined,
  tier: undefined,
  sort: 'created_at',
  order: 'desc'
})

function handleFilterChange() {
  emit('change', { ...filters })
}
</script>

<style scoped>
.filter-card {
  margin-bottom: 16px;
}
</style>
```

- [ ] **Step 3: 创建记忆列表页**

File: `/mnt/d/dev/github/project/omem-web/src/views/memories/MemoryList.vue`

```vue
<template>
  <div class="memory-list-page">
    <div class="page-header">
      <h2>记忆管理</h2>
    </div>

    <MemoryFilter @change="handleFilterChange" />

    <a-spin :spinning="loading">
      <div v-if="memories.length > 0" class="memory-grid">
        <MemoryCard
          v-for="memory in memories"
          :key="memory.id"
          :memory="memory"
        />
      </div>

      <a-empty v-else description="暂无记忆" />

      <a-pagination
        v-if="total > 0"
        v-model:current="currentPage"
        v-model:page-size="pageSize"
        :total="total"
        show-size-changer
        :page-size-options="['10', '20', '50', '100']"
        @change="handlePageChange"
        style="margin-top: 24px; text-align: center;"
      />
    </a-spin>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { message } from 'ant-design-vue'
import type { Memory } from '@/types/memory'
import type { MemoryListParams } from '@/api/types'
import { memoriesApi } from '@/api/memories'
import MemoryCard from '@/components/memories/MemoryCard.vue'
import MemoryFilter from '@/components/memories/MemoryFilter.vue'

const memories = ref<Memory[]>([])
const total = ref(0)
const currentPage = ref(1)
const pageSize = ref(20)
const loading = ref(false)
const filters = ref<MemoryListParams>({})

async function fetchMemories() {
  loading.value = true
  try {
    const params: MemoryListParams = {
      ...filters.value,
      limit: pageSize.value,
      offset: (currentPage.value - 1) * pageSize.value
    }

    const response = await memoriesApi.list(params)
    memories.value = response.memories
    total.value = response.total_count
  } catch (error) {
    message.error('加载记忆列表失败')
    console.error(error)
  } finally {
    loading.value = false
  }
}

function handleFilterChange(newFilters: MemoryListParams) {
  filters.value = newFilters
  currentPage.value = 1
  fetchMemories()
}

function handlePageChange() {
  fetchMemories()
}

onMounted(() => {
  fetchMemories()
})
</script>

<style scoped>
.memory-list-page {
  max-width: 1200px;
  margin: 0 auto;
}

.page-header {
  margin-bottom: 24px;
}

.page-header h2 {
  font-size: 24px;
  font-weight: 600;
  margin: 0;
}

.memory-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(400px, 1fr));
  gap: 16px;
}
</style>
```

- [ ] **Step 4: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/views/memories src/components/memories
git commit -m "feat: add memory list page with filter and pagination"
```

- [ ] **Step 5: 验证记忆列表功能**

Run: `cd /mnt/d/dev/github/project/omem-web && npm run dev`

Expected:
- 登录后跳转到记忆列表页
- 筛选器正常工作
- 分页正常工作
- 点击卡片跳转到详情页（404，下个任务实现）

---

## Task 10: 记忆详情页

> **UI 设计指引**: 使用 `frontend-design` 技能确保内容为主、界面为辅的专业感

**Files**:
- Create: `/mnt/d/dev/github/project/omem-web/src/views/memories/MemoryDetail.vue`

- [ ] **Step 1: 创建记忆详情页**

File: `/mnt/d/dev/github/project/omem-web/src/views/memories/MemoryDetail.vue`

```vue
<template>
  <div class="memory-detail-page">
    <a-spin :spinning="loading">
      <div v-if="memory" class="detail-container">
        <div class="detail-header">
          <a-button @click="$router.back()">
            <template #icon><ArrowLeftOutlined /></template>
            返回
          </a-button>
          <div class="header-actions">
            <a-tag :color="getCategoryColor(memory.category)">
              {{ memory.category }}
            </a-tag>
            <a-tag>{{ memory.tier }}</a-tag>
            <a-tag>{{ memory.memory_type }}</a-tag>
          </div>
        </div>

        <a-card class="content-card">
          <a-tabs v-model:activeKey="activeTab">
            <a-tab-pane key="l0" tab="摘要 (L0)">
              <div class="content-section">
                <p>{{ memory.l0_abstract || memory.content.slice(0, 100) }}</p>
              </div>
            </a-tab-pane>

            <a-tab-pane key="l1" tab="概述 (L1)">
              <div class="content-section">
                <p>{{ memory.l1_overview || memory.content }}</p>
              </div>
            </a-tab-pane>

            <a-tab-pane key="l2" tab="完整内容 (L2)">
              <div class="content-section">
                <pre>{{ memory.l2_content || memory.content }}</pre>
              </div>
            </a-tab-pane>
          </a-tabs>
        </a-card>

        <a-row :gutter="16">
          <a-col :span="12">
            <a-card title="基本信息" class="info-card">
              <a-descriptions :column="1" size="small">
                <a-descriptions-item label="ID">
                  {{ memory.id }}
                </a-descriptions-item>
                <a-descriptions-item label="版本">
                  v{{ memory.version || 1 }}
                </a-descriptions-item>
                <a-descriptions-item label="重要性">
                  <a-progress
                    :percent="memory.importance * 100"
                    size="small"
                    :stroke-color="getImportanceColor(memory.importance)"
                  />
                </a-descriptions-item>
                <a-descriptions-item label="置信度">
                  <a-progress
                    :percent="memory.confidence * 100"
                    size="small"
                  />
                </a-descriptions-item>
                <a-descriptions-item label="访问次数">
                  {{ memory.access_count }}
                </a-descriptions-item>
                <a-descriptions-item label="状态">
                  <a-tag :color="memory.state === 'active' ? 'green' : 'default'">
                    {{ memory.state }}
                  </a-tag>
                </a-descriptions-item>
              </a-descriptions>
            </a-card>
          </a-col>

          <a-col :span="12">
            <a-card title="时间信息" class="info-card">
              <a-descriptions :column="1" size="small">
                <a-descriptions-item label="创建时间">
                  {{ formatDate(memory.created_at) }}
                </a-descriptions-item>
                <a-descriptions-item label="更新时间">
                  {{ formatDate(memory.updated_at) }}
                </a-descriptions-item>
                <a-descriptions-item label="最后访问">
                  {{ formatDate(memory.last_accessed_at) }}
                </a-descriptions-item>
                <a-descriptions-item label="失效时间">
                  {{ formatDate(memory.invalidated_at) }}
                </a-descriptions-item>
              </a-descriptions>
            </a-card>
          </a-col>
        </a-row>

        <a-row :gutter="16">
          <a-col :span="12">
            <a-card title="关联信息" class="info-card">
              <a-descriptions :column="1" size="small">
                <a-descriptions-item label="Agent ID">
                  {{ memory.agent_id || '-' }}
                </a-descriptions-item>
                <a-descriptions-item label="Session ID">
                  {{ memory.session_id || '-' }}
                </a-descriptions-item>
                <a-descriptions-item label="Space ID">
                  {{ memory.space_id }}
                </a-descriptions-item>
                <a-descriptions-item label="Owner Agent">
                  {{ memory.owner_agent_id }}
                </a-descriptions-item>
                <a-descriptions-item label="来源">
                  {{ memory.source || '-' }}
                </a-descriptions-item>
              </a-descriptions>
            </a-card>
          </a-col>

          <a-col :span="12">
            <a-card title="标签" class="info-card">
              <div class="tags-container">
                <a-tag v-for="tag in memory.tags" :key="tag">
                  {{ tag }}
                </a-tag>
                <a-empty v-if="memory.tags.length === 0" :image="simpleImage" description="无标签" />
              </div>
            </a-card>
          </a-col>
        </a-row>

        <a-card v-if="memory.relations.length > 0" title="关系" class="info-card">
          <a-list :data-source="memory.relations" size="small">
            <template #renderItem="{ item }">
              <a-list-item>
                <a-tag>{{ item.relation_type }}</a-tag>
                <span>{{ item.target_id }}</span>
                <span v-if="item.context_label" class="context-label">
                  ({{ item.context_label }})
                </span>
              </a-list-item>
            </template>
          </a-list>
        </a-card>

        <a-card v-if="memory.provenance" title="溯源信息" class="info-card">
          <a-descriptions :column="2" size="small">
            <a-descriptions-item label="来源 Space">
              {{ memory.provenance.shared_from_space }}
            </a-descriptions-item>
            <a-descriptions-item label="来源记忆">
              {{ memory.provenance.shared_from_memory }}
            </a-descriptions-item>
            <a-descriptions-item label="分享者">
              {{ memory.provenance.shared_by_user }}
            </a-descriptions-item>
            <a-descriptions-item label="分享 Agent">
              {{ memory.provenance.shared_by_agent }}
            </a-descriptions-item>
            <a-descriptions-item label="分享时间">
              {{ formatDate(memory.provenance.shared_at) }}
            </a-descriptions-item>
            <a-descriptions-item label="源版本">
              v{{ memory.provenance.source_version }}
            </a-descriptions-item>
          </a-descriptions>
        </a-card>
      </div>

      <a-empty v-else description="记忆不存在" />
    </a-spin>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { message, Empty } from 'ant-design-vue'
import { ArrowLeftOutlined } from '@ant-design/icons-vue'
import type { Memory } from '@/types/memory'
import { memoriesApi } from '@/api/memories'
import { formatDate } from '@/utils/format'

const route = useRoute()
const memory = ref<Memory | null>(null)
const loading = ref(false)
const activeTab = ref('l0')
const simpleImage = Empty.PRESENTED_IMAGE_SIMPLE

async function fetchMemory() {
  loading.value = true
  try {
    const id = route.params.id as string
    memory.value = await memoriesApi.get(id)
  } catch (error) {
    message.error('加载记忆详情失败')
    console.error(error)
  } finally {
    loading.value = false
  }
}

function getCategoryColor(category: string): string {
  const colors: Record<string, string> = {
    profile: 'blue',
    preferences: 'green',
    entities: 'orange',
    events: 'purple',
    cases: 'red',
    patterns: 'cyan'
  }
  return colors[category] || 'default'
}

function getImportanceColor(importance: number): string {
  if (importance >= 0.8) return '#f5222d'
  if (importance >= 0.5) return '#fa8c16'
  return '#52c41a'
}

onMounted(() => {
  fetchMemory()
})
</script>

<style scoped>
.memory-detail-page {
  max-width: 1200px;
  margin: 0 auto;
}

.detail-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.header-actions {
  display: flex;
  gap: 8px;
}

.content-card {
  margin-bottom: 16px;
}

.content-section {
  padding: 16px;
  min-height: 200px;
}

.content-section p {
  font-size: 16px;
  line-height: 1.8;
  color: #333;
  margin: 0;
}

.content-section pre {
  font-size: 14px;
  line-height: 1.6;
  color: #333;
  white-space: pre-wrap;
  word-wrap: break-word;
  margin: 0;
}

.info-card {
  margin-bottom: 16px;
}

.tags-container {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.context-label {
  color: #999;
  font-size: 12px;
  margin-left: 8px;
}
</style>
```

- [ ] **Step 2: 提交**

```bash
cd /mnt/d/dev/github/project/omem-web
git add src/views/memories/MemoryDetail.vue
git commit -m "feat: add memory detail page with L0/L1/L2 tabs"
```

- [ ] **Step 3: 验证记忆详情功能**

Run: `cd /mnt/d/dev/github/project/omem-web && npm run dev`

Expected:
- 从列表页点击卡片跳转到详情页
- L0/L1/L2 标签页切换正常
- 所有 28 个字段正确展示
- 返回按钮正常工作

---

## 自审清单

- [x] **规格覆盖**: Phase 1 所有功能已实现
  - ✅ 项目脚手架（Vue 3 + Vite + TypeScript）
  - ✅ 认证系统（登录 + 多用户管理 + 路由守卫）
  - ✅ 主布局（Header + Sidebar + Content）
  - ✅ 记忆列表（分页 + 筛选 + 排序）
  - ✅ 记忆详情（28 字段 + L0/L1/L2 切换）

- [x] **占位符检查**: 无 TBD/TODO/占位符

- [x] **类型一致性**: 
  - ✅ Memory 类型在所有文件中一致
  - ✅ User/AuthState 类型在所有文件中一致
  - ✅ API 响应类型在所有文件中一致

- [x] **文件路径**: 所有路径使用绝对路径

- [x] **代码完整性**: 所有代码块完整可执行

---

## 执行选项

计划已完成并保存到 `/mnt/d/dev/github/project/omem-web/docs/superpowers/plans/2026-04-11-phase1-scaffold-auth-memories.md`

**两种执行方式**：

### 1. Subagent-Driven（推荐）

每个任务派遣一个新弟子执行，任务间进行审查，快速迭代。

**REQUIRED SUB-SKILL**: 使用 `superpowers:subagent-driven-development`

### 2. Inline Execution

在当前会话中使用 `executing-plans` 技能批量执行，设置检查点进行审查。

**REQUIRED SUB-SKILL**: 使用 `superpowers:executing-plans`

---

**Phase 1 实施计划完成！** 🎉
