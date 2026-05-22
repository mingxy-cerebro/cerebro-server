# Phase 2: 仪表盘 + 记忆搜索 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现完整版仪表盘（12+指标、多维度图表、最近记忆列表）和记忆搜索页（语义搜索+高级筛选）

**Architecture:** 
- 仪表盘：统计卡片 + ECharts 图表（折线图/饼图/柱状图）+ 最近记忆表格 + 快捷操作
- 记忆搜索：语义搜索 + 全文搜索 + 高级筛选器 + 结果高亮显示

**Tech Stack:** 
- Vue 3 Composition API + Ant Design Vue 4.x
- ECharts 6.x + vue-echarts 8.x
- omem API: /v1/stats, /v1/stats/decay, /v1/memories/search

---

## 文件结构规划

### 新增文件
```
src/
├── api/
│   └── stats.ts                    # 统计 API 客户端（3个端点）
├── views/
│   ├── Dashboard.vue               # 仪表盘页面
│   └── MemorySearch.vue            # 记忆搜索页面
├── components/
│   └── charts/
│       ├── LineChart.vue           # 折线图组件
│       ├── PieChart.vue            # 饼图组件
│       └── BarChart.vue            # 柱状图组件
└── types/
    └── stats.ts                    # 统计相关类型定义
```

### 修改文件
```
src/
├── main.ts                         # 注册 ECharts 组件
├── router/index.ts                 # 添加仪表盘和搜索路由
└── api/types.ts                    # 添加搜索 API 类型
```

---

## Task 1: 统计 API 客户端

**Files:**
- Create: `src/types/stats.ts`
- Create: `src/api/stats.ts`

- [ ] **Step 1: 创建统计类型定义**

创建 `src/types/stats.ts`：

```typescript
// 全局统计请求参数
export interface StatsParams {
  start_date?: string  // ISO 8601格式
  end_date?: string
  space_id?: string
}

// 全局统计响应
export interface StatsResponse {
  total_memories: number
  active_memories: number
  archived_memories: number
  deleted_memories: number
  by_category: Record<string, number>
  by_tier: Record<string, number>
  by_type: Record<string, number>
  avg_importance: number
  avg_confidence: number
  total_access_count: number
  total_spaces: number
  personal_spaces: number
  team_spaces: number
  org_spaces: number
  date_range: {
    start: string
    end: string
  }
  generated_at: string
}

// 衰减曲线请求参数
export interface DecayParams {
  memory_id?: string
  days?: number
  granularity?: 'day' | 'week' | 'month'
}

// 衰减曲线数据点
export interface DecayDataPoint {
  date: string
  importance: number
  access_count: number
  confidence: number
}

// 衰减曲线响应
export interface DecayResponse {
  memory_id?: string
  data_points: DecayDataPoint[]
  summary: {
    initial_importance: number
    current_importance: number
    decay_rate: number
    half_life_days: number
  }
}

// 关系图谱请求参数
export interface RelationsParams {
  memory_id?: string
  depth?: number
  relation_types?: string
  min_importance?: number
}

// 关系图谱节点
export interface RelationNode {
  id: string
  content: string
  category: string
  tier: string
  importance: number
  access_count: number
  created_at: string
}

// 关系图谱边
export interface RelationEdge {
  source_id: string
  target_id: string
  relation_type: 'supersedes' | 'contextualizes' | 'supports' | 'contradicts'
  context_label: string | null
  weight: number
}

// 关系图谱响应
export interface RelationsResponse {
  nodes: RelationNode[]
  edges: RelationEdge[]
  stats: {
    total_nodes: number
    total_edges: number
    max_depth: number
  }
}
```

- [ ] **Step 2: 创建统计 API 客户端**

创建 `src/api/stats.ts`：

```typescript
import client from './client'
import type {
  StatsParams,
  StatsResponse,
  DecayParams,
  DecayResponse,
  RelationsParams,
  RelationsResponse,
} from '@/types/stats'

export const statsApi = {
  // 全局统计
  async getStats(params?: StatsParams): Promise<StatsResponse> {
    const { data } = await client.get<StatsResponse>('/v1/stats', { params })
    return data
  },

  // 衰减曲线
  async getDecay(params?: DecayParams): Promise<DecayResponse> {
    const { data } = await client.get<DecayResponse>('/v1/stats/decay', { params })
    return data
  },

  // 关系图谱
  async getRelations(params?: RelationsParams): Promise<RelationsResponse> {
    const { data } = await client.get<RelationsResponse>('/v1/stats/relations', { params })
    return data
  },
}
```

- [ ] **Step 3: TypeScript 编译验证**

运行：`npx vue-tsc --noEmit`

预期：无类型错误

- [ ] **Step 4: 提交代码**

```bash
git add src/types/stats.ts src/api/stats.ts
git commit -m "feat: add stats API client"
```

---

## Task 2: ECharts 图表组件

**Files:**
- Create: `src/components/charts/LineChart.vue`
- Create: `src/components/charts/PieChart.vue`
- Create: `src/components/charts/BarChart.vue`
- Modify: `package.json`

- [ ] **Step 1: 安装 ECharts 依赖**

```bash
npm install echarts@6.0.0 vue-echarts@8.0.1
```

- [ ] **Step 2: 创建折线图组件**

创建 `src/components/charts/LineChart.vue`：

```vue
<template>
  <v-chart :option="option" :style="{ height: height }" autoresize />
</template>

<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart } from 'echarts/charts'
import {
  TitleComponent,
  TooltipComponent,
  LegendComponent,
  GridComponent,
} from 'echarts/components'

use([CanvasRenderer, LineChart, TitleComponent, TooltipComponent, LegendComponent, GridComponent])

interface Props {
  title?: string
  xData: string[]
  series: Array<{
    name: string
    data: number[]
    color?: string
  }>
  height?: string
}

const props = withDefaults(defineProps<Props>(), {
  title: '',
  height: '400px',
})

const option = computed(() => ({
  title: {
    text: props.title,
    left: 'center',
  },
  tooltip: {
    trigger: 'axis',
  },
  legend: {
    top: 30,
  },
  grid: {
    left: '3%',
    right: '4%',
    bottom: '3%',
    containLabel: true,
  },
  xAxis: {
    type: 'category',
    boundaryGap: false,
    data: props.xData,
  },
  yAxis: {
    type: 'value',
  },
  series: props.series.map((s) => ({
    name: s.name,
    type: 'line',
    data: s.data,
    smooth: true,
    itemStyle: s.color ? { color: s.color } : undefined,
  })),
}))
</script>
```

- [ ] **Step 3: 创建饼图组件**

创建 `src/components/charts/PieChart.vue`：

```vue
<template>
  <v-chart :option="option" :style="{ height: height }" autoresize />
</template>

<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { PieChart } from 'echarts/charts'
import {
  TitleComponent,
  TooltipComponent,
  LegendComponent,
} from 'echarts/components'

use([CanvasRenderer, PieChart, TitleComponent, TooltipComponent, LegendComponent])

interface Props {
  title?: string
  data: Array<{
    name: string
    value: number
  }>
  height?: string
}

const props = withDefaults(defineProps<Props>(), {
  title: '',
  height: '400px',
})

const option = computed(() => ({
  title: {
    text: props.title,
    left: 'center',
  },
  tooltip: {
    trigger: 'item',
    formatter: '{a} <br/>{b}: {c} ({d}%)',
  },
  legend: {
    orient: 'vertical',
    left: 'left',
    top: 'middle',
  },
  series: [
    {
      name: props.title,
      type: 'pie',
      radius: '50%',
      data: props.data,
      emphasis: {
        itemStyle: {
          shadowBlur: 10,
          shadowOffsetX: 0,
          shadowColor: 'rgba(0, 0, 0, 0.5)',
        },
      },
    },
  ],
}))
</script>
```

- [ ] **Step 4: 创建柱状图组件**

创建 `src/components/charts/BarChart.vue`：

```vue
<template>
  <v-chart :option="option" :style="{ height: height }" autoresize />
</template>

<script setup lang="ts">
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { BarChart } from 'echarts/charts'
import {
  TitleComponent,
  TooltipComponent,
  LegendComponent,
  GridComponent,
} from 'echarts/components'

use([CanvasRenderer, BarChart, TitleComponent, TooltipComponent, LegendComponent, GridComponent])

interface Props {
  title?: string
  xData: string[]
  series: Array<{
    name: string
    data: number[]
    color?: string
  }>
  height?: string
}

const props = withDefaults(defineProps<Props>(), {
  title: '',
  height: '400px',
})

const option = computed(() => ({
  title: {
    text: props.title,
    left: 'center',
  },
  tooltip: {
    trigger: 'axis',
    axisPointer: {
      type: 'shadow',
    },
  },
  legend: {
    top: 30,
  },
  grid: {
    left: '3%',
    right: '4%',
    bottom: '3%',
    containLabel: true,
  },
  xAxis: {
    type: 'category',
    data: props.xData,
  },
  yAxis: {
    type: 'value',
  },
  series: props.series.map((s) => ({
    name: s.name,
    type: 'bar',
    data: s.data,
    itemStyle: s.color ? { color: s.color } : undefined,
  })),
}))
</script>
```

- [ ] **Step 5: TypeScript 编译验证**

运行：`npx vue-tsc --noEmit`

预期：无类型错误

- [ ] **Step 6: 提交代码**

```bash
git add package.json package-lock.json src/components/charts/
git commit -m "feat: add ECharts components (Line/Pie/Bar)"
```

---

