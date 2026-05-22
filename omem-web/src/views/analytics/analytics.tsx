import { useEffect, useState, useMemo } from "react"
import { useNavigate } from "react-router-dom"
import apiClient from "@/api/client"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { getTierLabel, getTierBadgeClass } from "@/lib/tag-utils"
import { toast } from "sonner"
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
} from "recharts"
import {
  TrendingUp,
  Layers,
  Brain,
  ArrowLeft,
  BarChart3,
  Star,
  Target,
  History,
  ArrowUpRight,
  ArrowDownRight,
} from "lucide-react"

interface TimelineEntry {
  date: string
  count: number
  by_type: Record<string, number>
}

interface StatsData {
  total: number
  total_memories: number
  total_spaces: number
  by_type: Record<string, number>
  by_category: Record<string, number>
  by_tier: Record<string, number>
  by_state: Record<string, number>
  by_space: Record<string, number>
  timeline: TimelineEntry[]
  avg_importance: number
  avg_confidence: number
  total_access_count: number
}

const COLORS = [
  "#8b5cf6",
  "#ec4899",
  "#06b6d4",
  "#f59e0b",
  "#10b981",
  "#ef4444",
  "#6366f1",
  "#f97316",
]

function toChartData(record: Record<string, number> | undefined, limit?: number) {
  if (!record) return []
  const entries = Object.entries(record)
    .map(([name, value]) => ({ name, value }))
    .sort((a, b) => b.value - a.value)
  return limit ? entries.slice(0, limit) : entries
}

export function AnalyticsPage() {
  const navigate = useNavigate()
  const [stats, setStats] = useState<StatsData | null>(null)
  const [memories, setMemories] = useState<Array<{ id: string; tier_history?: string }>>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    async function fetchData() {
      try {
        setLoading(true)
        const [statsRes, memRes] = await Promise.all([
          apiClient.get("/v1/stats"),
          apiClient.get("/v1/memories"),
        ])
        setStats(statsRes as StatsData)
        setMemories((memRes as any)?.memories || [])
      } catch (err: any) {
        toast.error("加载统计数据失败: " + err.message)
      } finally {
        setLoading(false)
      }
    }
    fetchData()
  }, [])

  const todayCount = useMemo(() => {
    if (!stats?.timeline?.length) return 0
    const today = new Date().toISOString().split("T")[0]
    const entry = stats.timeline.find((t: any) => t.date === today)
    return entry?.count ?? 0
  }, [stats])

  const categoryData = useMemo(() => toChartData(stats?.by_category, 8), [stats])
  const typeData = useMemo(() => toChartData(stats?.by_type), [stats])
  const tierData = useMemo(() => toChartData(stats?.by_tier), [stats])
  const stateData = useMemo(() => toChartData(stats?.by_state), [stats])

  const activityRate = useMemo(() => {
    if (!stats || !stats.total || !stats.total_access_count) return "0%"
    return `${Math.min(100, Math.round((stats.total_access_count / Math.max(1, stats.total)) * 100))}%`
  }, [stats])

  const tierChangeEvents = useMemo(() => {
    const events: Array<{
      memoryId: string
      from: string
      to: string
      reason: string
      at: string
      access_count: number
    }> = []
    for (const mem of memories) {
      if (!mem.tier_history) continue
      try {
        const history = JSON.parse(mem.tier_history) as Array<{
          from: string; to: string; reason: string; at: string; access_count: number
        }>
        for (const e of history) {
          events.push({ memoryId: mem.id, ...e })
        }
      } catch { /* ignore parse errors */ }
    }
    events.sort((a, b) => b.at.localeCompare(a.at))
    return events.slice(0, 50)
  }, [memories])

  const reasonMap: Record<string, string> = {
    access_via_get: "访问触发",
    access_via_search: "搜索触发",
    access_via_recall: "召回触发",
    access_via_cross_space_search: "跨空间搜索触发",
    scheduled_evaluation: "定时评估",
  }

  if (loading) {
    return (
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <Skeleton className="h-8 w-48" />
          <Skeleton className="h-8 w-24" />
        </div>
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          <Skeleton className="h-24" />
          <Skeleton className="h-24" />
          <Skeleton className="h-24" />
          <Skeleton className="h-24" />
        </div>
        <div className="grid gap-4 md:grid-cols-2">
          <Skeleton className="h-80" />
          <Skeleton className="h-80" />
          <Skeleton className="h-80" />
          <Skeleton className="h-80" />
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
            <BarChart3 className="size-5" />
            统计分析
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            记忆库数据洞察与可视化
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={() => navigate("/memories")}>
          <ArrowLeft className="size-4 mr-1.5" />
          返回记忆
        </Button>
      </div>

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              总记忆数
            </CardTitle>
            <Brain className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{stats?.total ?? 0}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              今日新增
            </CardTitle>
            <TrendingUp className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{todayCount}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              空间数
            </CardTitle>
            <Layers className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {stats?.total_spaces ?? Object.keys(stats?.by_space ?? {}).length}
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              活跃度
            </CardTitle>
            <Target className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{activityRate}</div>
          </CardContent>
        </Card>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              平均重要性
            </CardTitle>
            <Star className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {stats?.avg_importance ? `${(stats.avg_importance * 100).toFixed(0)}%` : "—"}
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              平均置信度
            </CardTitle>
            <BarChart3 className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {stats?.avg_confidence ? `${(stats.avg_confidence * 100).toFixed(0)}%` : "—"}
            </div>
          </CardContent>
        </Card>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">分类分布</CardTitle>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={280}>
              <PieChart>
                <Pie
                  data={categoryData}
                  cx="50%"
                  cy="50%"
                  innerRadius={60}
                  outerRadius={100}
                  paddingAngle={2}
                  dataKey="value"
                >
                  {categoryData.map((entry, index) => (
                    <Cell key={`cell-${entry.name}`} fill={COLORS[index % COLORS.length]} />
                  ))}
                </Pie>
                <Tooltip />
              </PieChart>
            </ResponsiveContainer>
            <div className="flex flex-wrap gap-2 mt-4 justify-center">
              {categoryData.map((entry, index) => (
                <Badge
                  key={entry.name}
                  variant="secondary"
                  style={{
                    backgroundColor: `${COLORS[index % COLORS.length]}20`,
                    color: COLORS[index % COLORS.length],
                  }}
                >
                  {entry.name} ({entry.value})
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">记忆类型分布</CardTitle>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={280}>
              <BarChart data={typeData}>
                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                <XAxis dataKey="name" tick={{ fontSize: 12 }} />
                <YAxis tick={{ fontSize: 12 }} />
                <Tooltip />
                <Bar dataKey="value" fill="#8b5cf6" radius={[4, 4, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">层级分布</CardTitle>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={280}>
              <PieChart>
                <Pie
                  data={tierData}
                  cx="50%"
                  cy="50%"
                  outerRadius={100}
                  dataKey="value"
                  label={({ name, value }) => `${name}: ${value}`}
                >
                  {tierData.map((entry, index) => (
                    <Cell key={`tier-${entry.name}`} fill={COLORS[index % COLORS.length]} />
                  ))}
                </Pie>
                <Tooltip />
              </PieChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">状态分布</CardTitle>
          </CardHeader>
          <CardContent>
            <ResponsiveContainer width="100%" height={280}>
              <BarChart data={stateData}>
                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                <XAxis dataKey="name" tick={{ fontSize: 12 }} />
                <YAxis tick={{ fontSize: 12 }} />
                <Tooltip />
                <Bar dataKey="value" fill="#10b981" radius={[4, 4, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      </div>

      <Card className="mt-4">
        <CardHeader>
          <CardTitle className="flex items-center gap-1.5 text-base">
            <History className="size-4 text-muted-foreground" />
            Tier 变更日志
          </CardTitle>
        </CardHeader>
        <CardContent>
          {tierChangeEvents.length === 0 ? (
            <p className="text-sm text-muted-foreground py-4 text-center">暂无变更记录</p>
          ) : (
            <div className="space-y-1">
              {tierChangeEvents.map((e) => {
                const tierOrder: Record<string, number> = { peripheral: 0, working: 1, core: 2 }
                const promoted = (tierOrder[e.from] ?? 0) < (tierOrder[e.to] ?? 0)
                return (
                  <button
                    key={e.memoryId + e.at}
                    type="button"
                    className="flex items-center gap-2 text-sm py-2 border-b last:border-b-0 hover:bg-muted/50 px-2 rounded cursor-pointer w-full text-left"
                    onClick={() => navigate(`/memories/${e.memoryId}`)}
                  >
                    {promoted ? (
                      <ArrowUpRight className="size-4 text-emerald-500 shrink-0" />
                    ) : (
                      <ArrowDownRight className="size-4 text-red-500 shrink-0" />
                    )}
                    <span className="text-muted-foreground text-xs w-36 shrink-0">
                      {new Date(e.at).toLocaleString("zh-CN")}
                    </span>
                    <span className="text-muted-foreground font-mono text-xs">
                      {e.memoryId.slice(0, 8)}...
                    </span>
                    <Badge variant="outline" className={getTierBadgeClass(e.from)}>
                      {getTierLabel(e.from)}
                    </Badge>
                    <span className="text-muted-foreground">→</span>
                    <Badge variant="outline" className={getTierBadgeClass(e.to)}>
                      {getTierLabel(e.to)}
                    </Badge>
                    <span className="text-xs text-muted-foreground ml-auto">
                      {reasonMap[e.reason] || e.reason} · #{e.access_count}
                    </span>
                  </button>
                )
              })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
