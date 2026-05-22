# Phase 1 修复方案实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 Phase 1 遗留问题：简化登录页、优化记忆列表视觉、实现 CRUD 功能、修复筛选器中文显示

**Architecture:** 
- 登录页简化为只输入 API Key，通过 `/v1/profile` 自动获取用户名
- 记忆列表从卡片布局改为表格布局，增强视觉设计，添加操作列（编辑/删除）
- 筛选器下拉框 label 改为中文显示
- 新增 Modal 表单实现记忆的创建和编辑

**Tech Stack:** Vue 3 + Ant Design Vue 4.2+ + TypeScript + Pinia

---

## Task 1: 创建 Profile API 客户端

**Files:**
- Create: `src/api/profile.ts`
- Modify: `src/api/index.ts` (如果存在)

- [ ] **Step 1: 创建 profile.ts API 模块**

```typescript
// src/api/profile.ts
import client from './client'

export interface ProfileResponse {
  name: string
  email?: string
  created_at?: string
}

export const profileApi = {
  /**
   * 获取当前用户信息
   */
  async get(): Promise<ProfileResponse> {
    const { data } = await client.get<ProfileResponse>('/v1/profile')
    return data
  }
}
```

- [ ] **Step 2: 验证 TypeScript 编译**

Run: `cd /mnt/d/dev/github/project/omem-web && npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 3: Commit**

```bash
git add src/api/profile.ts
git commit -m "feat: 添加 profile API 客户端"
```

---

## Task 2: 简化登录页

**Files:**
- Modify: `src/views/Login.vue:1-200`
- Modify: `src/stores/auth.ts:1-100`

- [ ] **Step 1: 修改 Login.vue 表单结构**

移除 API URL 和 Username 输入框，只保留 API Key：

```vue
<!-- src/views/Login.vue -->
<template>
  <div class="login-container">
    <a-card title="登录 omem" :bordered="false" style="width: 400px">
      <a-form
        :model="formState"
        :rules="rules"
        @finish="handleLogin"
        layout="vertical"
      >
        <a-form-item label="API Key" name="apiKey">
          <a-input-password
            v-model:value="formState.apiKey"
            placeholder="请输入 API Key"
            size="large"
          />
        </a-form-item>

        <a-form-item>
          <a-button
            type="primary"
            html-type="submit"
            :loading="loading"
            block
            size="large"
          >
            登录
          </a-button>
        </a-form-item>
      </a-form>
    </a-card>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { message } from 'ant-design-vue'
import { useAuthStore } from '@/stores/auth'
import { profileApi } from '@/api/profile'
import axios from 'axios'

const router = useRouter()
const authStore = useAuthStore()
const loading = ref(false)

const formState = reactive({
  apiKey: ''
})

const rules = {
  apiKey: [{ required: true, message: '请输入 API Key', trigger: 'blur' }]
}

const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || 'https://www.mengxy.cc'

const handleLogin = async () => {
  loading.value = true
  try {
    // 1. 验证 API Key 有效性
    const healthUrl = `${API_BASE_URL}/health`
    await axios.get(healthUrl, {
      headers: { 'X-API-Key': formState.apiKey }
    })

    // 2. 获取用户信息
    const profile = await profileApi.get()

    // 3. 保存用户信息到 Store
    authStore.addUser({
      id: formState.apiKey.substring(0, 8),
      name: profile.name || 'Unknown User',
      api_key: formState.apiKey,
      api_url: API_BASE_URL,
      last_used: new Date().toISOString()
    })

    message.success('登录成功')
    router.push('/memories')
  } catch (error: any) {
    console.error('Login failed:', error)
    message.error(error?.error?.message || '登录失败，请检查 API Key')
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.login-container {
  display: flex;
  justify-content: center;
  align-items: center;
  min-height: 100vh;
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
}
</style>
```

- [ ] **Step 2: 修改 client.ts 初始化逻辑**

确保登录后 baseURL 正确设置：

```typescript
// src/api/client.ts (在 handleLogin 成功后调用)
import { updateBaseURL } from '@/api/client'

// 在 handleLogin 的 try 块中，profileApi.get() 之前添加：
updateBaseURL(API_BASE_URL)
```

实际修改位置：在 `Login.vue` 的 `handleLogin` 函数中，`profileApi.get()` 调用之前插入：

```typescript
// 2. 更新 client baseURL
import { updateBaseURL } from '@/api/client'
updateBaseURL(API_BASE_URL)

// 3. 获取用户信息
const profile = await profileApi.get()
```

- [ ] **Step 3: 验证编译和运行**

Run: `cd /mnt/d/dev/github/project/omem-web && npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git add src/views/Login.vue
git commit -m "feat: 简化登录页，只需输入 API Key"
```

---

## Task 3: 创建枚举中文映射工具

**Files:**
- Create: `src/utils/enums.ts`

- [ ] **Step 1: 创建枚举映射文件**

```typescript
// src/utils/enums.ts
import type { Category, Tier, MemoryType, MemoryState } from '@/types/memory'

export const CATEGORY_LABELS: Record<Category, string> = {
  profile: '个人资料',
  preferences: '偏好设置',
  entities: '实体',
  events: '事件',
  cases: '案例',
  patterns: '模式'
}

export const TIER_LABELS: Record<Tier, string> = {
  core: '核心',
  working: '工作',
  peripheral: '外围'
}

export const MEMORY_TYPE_LABELS: Record<MemoryType, string> = {
  pinned: '置顶',
  insight: '洞察',
  session: '会话'
}

export const STATE_LABELS: Record<MemoryState, string> = {
  active: '活跃',
  archived: '已归档',
  deleted: '已删除'
}

// 生成 Select options
export const CATEGORY_OPTIONS = Object.entries(CATEGORY_LABELS).map(([value, label]) => ({
  label,
  value
}))

export const TIER_OPTIONS = Object.entries(TIER_LABELS).map(([value, label]) => ({
  label,
  value
}))

export const MEMORY_TYPE_OPTIONS = Object.entries(MEMORY_TYPE_LABELS).map(([value, label]) => ({
  label,
  value
}))

export const STATE_OPTIONS = Object.entries(STATE_LABELS).map(([value, label]) => ({
  label,
  value
}))
```

- [ ] **Step 2: 验证编译**

Run: `cd /mnt/d/dev/github/project/omem-web && npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 3: Commit**

```bash
git add src/utils/enums.ts
git commit -m "feat: 添加枚举中文映射工具"
```

---

## Task 4: 重构记忆列表为表格布局

**Files:**
- Modify: `src/views/MemoryList.vue:1-300`

- [ ] **Step 1: 修改为表格布局并添加操作列**

```vue
<!-- src/views/MemoryList.vue -->
<template>
  <div class="memory-list">
    <a-card :bordered="false">
      <!-- 筛选器 -->
      <a-form layout="inline" style="margin-bottom: 16px">
        <a-form-item label="分类">
          <a-select
            v-model:value="filters.category"
            :options="CATEGORY_OPTIONS"
            placeholder="全部"
            allow-clear
            style="width: 120px"
          />
        </a-form-item>
        <a-form-item label="层级">
          <a-select
            v-model:value="filters.tier"
            :options="TIER_OPTIONS"
            placeholder="全部"
            allow-clear
            style="width: 100px"
          />
        </a-form-item>
        <a-form-item label="类型">
          <a-select
            v-model:value="filters.memory_type"
            :options="MEMORY_TYPE_OPTIONS"
            placeholder="全部"
            allow-clear
            style="width: 100px"
          />
        </a-form-item>
        <a-form-item label="状态">
          <a-select
            v-model:value="filters.state"
            :options="STATE_OPTIONS"
            placeholder="全部"
            allow-clear
            style="width: 100px"
          />
        </a-form-item>
        <a-form-item label="标签">
          <a-input
            v-model:value="filters.tags"
            placeholder="输入标签"
            allow-clear
            style="width: 150px"
          />
        </a-form-item>
        <a-form-item>
          <a-button type="primary" @click="fetchMemories">查询</a-button>
          <a-button style="margin-left: 8px" @click="resetFilters">重置</a-button>
          <a-button type="primary" style="margin-left: 8px" @click="openModal()">新增记忆</a-button>
        </a-form-item>
      </a-form>

      <!-- 表格 -->
      <a-table
        :columns="columns"
        :data-source="memories"
        :loading="loading"
        :pagination="pagination"
        :row-key="(record) => record.id"
        bordered
        size="middle"
        @change="handleTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.dataIndex === 'category'">
            <a-tag :color="getCategoryColor(record.category)">
              {{ CATEGORY_LABELS[record.category] }}
            </a-tag>
          </template>
          <template v-else-if="column.dataIndex === 'tier'">
            <a-tag :color="getTierColor(record.tier)">
              {{ TIER_LABELS[record.tier] }}
            </a-tag>
          </template>
          <template v-else-if="column.dataIndex === 'memory_type'">
            <a-tag>{{ MEMORY_TYPE_LABELS[record.memory_type] }}</a-tag>
          </template>
          <template v-else-if="column.dataIndex === 'content'">
            <div class="content-cell">{{ record.content }}</div>
          </template>
          <template v-else-if="column.dataIndex === 'tags'">
            <a-tag v-for="tag in record.tags?.slice(0, 3)" :key="tag" color="blue">
              {{ tag }}
            </a-tag>
            <span v-if="record.tags && record.tags.length > 3">...</span>
          </template>
          <template v-else-if="column.dataIndex === 'action'">
            <a-space>
              <a-button type="link" size="small" @click="openModal(record)">编辑</a-button>
              <a-popconfirm
                title="确定要删除这条记忆吗？"
                ok-text="确定"
                cancel-text="取消"
                @confirm="handleDelete(record.id)"
              >
                <a-button type="link" size="small" danger>删除</a-button>
              </a-popconfirm>
              <a-button type="link" size="small" @click="viewDetail(record.id)">详情</a-button>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <!-- 新增/编辑 Modal -->
    <a-modal
      v-model:open="modalVisible"
      :title="modalTitle"
      :confirm-loading="modalLoading"
      width="800px"
      destroy-on-close
      @ok="handleModalOk"
    >
      <a-form
        ref="formRef"
        :model="formState"
        :rules="formRules"
        layout="vertical"
      >
        <a-form-item label="内容" name="content">
          <a-textarea
            v-model:value="formState.content"
            placeholder="请输入记忆内容"
            :rows="6"
          />
        </a-form-item>
        <a-row :gutter="16">
          <a-col :span="8">
            <a-form-item label="分类" name="category">
              <a-select
                v-model:value="formState.category"
                :options="CATEGORY_OPTIONS"
                placeholder="请选择分类"
              />
            </a-form-item>
          </a-col>
          <a-col :span="8">
            <a-form-item label="层级" name="tier">
              <a-select
                v-model:value="formState.tier"
                :options="TIER_OPTIONS"
                placeholder="请选择层级"
              />
            </a-form-item>
          </a-col>
          <a-col :span="8">
            <a-form-item label="类型" name="memory_type">
              <a-select
                v-model:value="formState.memory_type"
                :options="MEMORY_TYPE_OPTIONS"
                placeholder="请选择类型"
              />
            </a-form-item>
          </a-col>
        </a-row>
        <a-form-item label="标签" name="tags">
          <a-select
            v-model:value="formState.tags"
            mode="tags"
            placeholder="输入标签后按回车"
            :token-separators="[',']"
          />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted, computed } from 'vue'
import { useRouter } from 'vue-router'
import { message } from 'ant-design-vue'
import type { TableProps, FormInstance } from 'ant-design-vue'
import { memoriesApi } from '@/api/memories'
import type { Memory, Category, Tier, MemoryType } from '@/types/memory'
import {
  CATEGORY_OPTIONS,
  TIER_OPTIONS,
  MEMORY_TYPE_OPTIONS,
  STATE_OPTIONS,
  CATEGORY_LABELS,
  TIER_LABELS,
  MEMORY_TYPE_LABELS
} from '@/utils/enums'

const router = useRouter()
const loading = ref(false)
const memories = ref<Memory[]>([])
const formRef = ref<FormInstance>()

// 筛选器
const filters = reactive({
  category: undefined as Category | undefined,
  tier: undefined as Tier | undefined,
  memory_type: undefined as MemoryType | undefined,
  state: undefined,
  tags: ''
})

// 分页
const pagination = reactive({
  current: 1,
  pageSize: 12,
  total: 0,
  showSizeChanger: true,
  pageSizeOptions: ['12', '24', '48', '96']
})

// 表格列定义
const columns = [
  {
    title: '分类',
    dataIndex: 'category',
    width: 100,
    align: 'center' as const
  },
  {
    title: '层级',
    dataIndex: 'tier',
    width: 80,
    align: 'center' as const
  },
  {
    title: '类型',
    dataIndex: 'memory_type',
    width: 80,
    align: 'center' as const
  },
  {
    title: '内容',
    dataIndex: 'content',
    ellipsis: true
  },
  {
    title: '标签',
    dataIndex: 'tags',
    width: 200
  },
  {
    title: '创建时间',
    dataIndex: 'created_at',
    width: 180,
    customRender: ({ text }: { text: string }) => {
      return new Date(text).toLocaleString('zh-CN')
    }
  },
  {
    title: '操作',
    dataIndex: 'action',
    width: 180,
    fixed: 'right' as const,
    align: 'center' as const
  }
]

// Modal 状态
const modalVisible = ref(false)
const modalLoading = ref(false)
const editingId = ref<string | null>(null)
const modalTitle = computed(() => (editingId.value ? '编辑记忆' : '新增记忆'))

const formState = reactive({
  content: '',
  category: undefined as Category | undefined,
  tier: undefined as Tier | undefined,
  memory_type: undefined as MemoryType | undefined,
  tags: [] as string[]
})

const formRules = {
  content: [{ required: true, message: '请输入内容', trigger: 'blur' }],
  category: [{ required: true, message: '请选择分类', trigger: 'change' }],
  tier: [{ required: true, message: '请选择层级', trigger: 'change' }],
  memory_type: [{ required: true, message: '请选择类型', trigger: 'change' }]
}

// 获取记忆列表
const fetchMemories = async () => {
  loading.value = true
  try {
    const params: any = {
      limit: pagination.pageSize,
      offset: (pagination.current - 1) * pagination.pageSize
    }
    if (filters.category) params.category = filters.category
    if (filters.tier) params.tier = filters.tier
    if (filters.memory_type) params.memory_type = filters.memory_type
    if (filters.state) params.state = filters.state
    if (filters.tags) params.tags = filters.tags

    const response = await memoriesApi.list(params)
    memories.value = response.memories
    pagination.total = response.total
  } catch (error: any) {
    message.error(error?.error?.message || '获取记忆列表失败')
  } finally {
    loading.value = false
  }
}

// 重置筛选器
const resetFilters = () => {
  filters.category = undefined
  filters.tier = undefined
  filters.memory_type = undefined
  filters.state = undefined
  filters.tags = ''
  pagination.current = 1
  fetchMemories()
}

// 表格变化处理
const handleTableChange: TableProps['onChange'] = (pag) => {
  pagination.current = pag.current || 1
  pagination.pageSize = pag.pageSize || 12
  fetchMemories()
}

// 打开 Modal
const openModal = (record?: Memory) => {
  if (record) {
    editingId.value = record.id
    formState.content = record.content
    formState.category = record.category
    formState.tier = record.tier
    formState.memory_type = record.memory_type
    formState.tags = record.tags || []
  } else {
    editingId.value = null
    formState.content = ''
    formState.category = undefined
    formState.tier = undefined
    formState.memory_type = undefined
    formState.tags = []
  }
  modalVisible.value = true
}

// Modal 确定
const handleModalOk = async () => {
  try {
    await formRef.value?.validate()
    modalLoading.value = true

    const payload: any = {
      content: formState.content,
      category: formState.category,
      tier: formState.tier,
      memory_type: formState.memory_type,
      tags: formState.tags
    }

    if (editingId.value) {
      await memoriesApi.update(editingId.value, payload)
      message.success('更新成功')
    } else {
      await memoriesApi.create(payload)
      message.success('创建成功')
    }

    modalVisible.value = false
    fetchMemories()
  } catch (error: any) {
    if (error?.errorFields) return // 表单验证失败
    message.error(error?.error?.message || '操作失败')
  } finally {
    modalLoading.value = false
  }
}

// 删除记忆
const handleDelete = async (id: string) => {
  try {
    await memoriesApi.delete(id)
    message.success('删除成功')
    fetchMemories()
  } catch (error: any) {
    message.error(error?.error?.message || '删除失败')
  }
}

// 查看详情
const viewDetail = (id: string) => {
  router.push(`/memories/${id}`)
}

// 颜色映射
const getCategoryColor = (category: Category) => {
  const colors: Record<Category, string> = {
    profile: 'blue',
    preferences: 'green',
    entities: 'purple',
    events: 'orange',
    cases: 'red',
    patterns: 'cyan'
  }
  return colors[category]
}

const getTierColor = (tier: Tier) => {
  const colors: Record<Tier, string> = {
    core: 'gold',
    working: 'blue',
    peripheral: 'default'
  }
  return colors[tier]
}

onMounted(() => {
  fetchMemories()
})
</script>

<style scoped>
.memory-list {
  padding: 24px;
}

.content-cell {
  max-width: 400px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

:deep(.ant-table) {
  --ant-table-header-bg: #fafafa;
  --ant-table-row-hover-bg: #e6f7ff;
}
</style>
```

- [ ] **Step 2: 验证编译**

Run: `cd /mnt/d/dev/github/project/omem-web && npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 3: Commit**

```bash
git add src/views/MemoryList.vue
git commit -m "feat: 重构记忆列表为表格布局，添加 CRUD 功能"
```

---

## Task 5: 验证功能完整性

**Files:**
- Test: 所有修改的文件

- [ ] **Step 1: 启动开发服务器**

Run: `cd /mnt/d/dev/github/project/omem-web && npm run dev`
Expected: 服务器启动在 http://localhost:5173

- [ ] **Step 2: 测试登录流程**

1. 访问 http://localhost:5173/login
2. 输入有效的 API Key
3. 点击登录
4. Expected: 跳转到 /memories，显示用户名

- [ ] **Step 3: 测试记忆列表**

1. 验证表格正常显示
2. 验证筛选器下拉框显示中文 label
3. 点击"新增记忆"，填写表单，提交
4. Expected: 创建成功，列表刷新
5. 点击"编辑"，修改内容，提交
6. Expected: 更新成功，列表刷新
7. 点击"删除"，确认
8. Expected: 删除成功，列表刷新

- [ ] **Step 4: 验证 TypeScript 和 Lint**

Run: `cd /mnt/d/dev/github/project/omem-web && npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 5: 最终 Commit**

```bash
git add -A
git commit -m "test: 验证 Phase 1 修复功能完整性"
```

---

## 自检清单

### 1. 规格覆盖
- [x] 登录页简化（只输入 API Key）
- [x] 用户名自动获取（通过 /v1/profile）
- [x] 记忆列表视觉优化（表格布局 + 颜色）
- [x] 筛选器中文显示
- [x] 新增记忆功能
- [x] 编辑记忆功能
- [x] 删除记忆功能（带二次确认）

### 2. 占位符检查
- [x] 所有代码块完整，无 TBD/TODO
- [x] 所有类型定义完整
- [x] 所有 API 调用完整

### 3. 类型一致性
- [x] Category/Tier/MemoryType 枚举类型一致
- [x] Memory 接口字段一致
- [x] API 响应类型一致

---

## 执行选项

计划已完成并保存到 `docs/superpowers/plans/2026-04-11-phase1-fixes.md`。

**两种执行方式：**

1. **Subagent-Driven（推荐）** - 每个任务派遣新弟子，任务间审查，快速迭代
2. **Inline Execution** - 在当前会话执行，批量执行带检查点

**师尊选择哪种方式？**
