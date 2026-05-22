import { useEffect, useState, useMemo } from "react"
import { useNavigate } from "react-router-dom"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import apiClient from "@/api/client"
import { useAuthStore } from "@/stores/auth"
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
  AreaChart,
  Area,
} from "recharts"
import {
  Brain,
  TrendingUp,
  Home,
  Eye,
  BarChart3,
  CheckCircle,
  Star,
  Activity,
  Plus,
  Upload,
  FileText,
  type LucideIcon,
} from "lucide-react"

interface TimelineEntry {
  date: string
  count: number
  by_type: Record<string, number>
}

interface StatsResponse {
  total: number
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

interface StatCardProps {
  label: string
  value: string | number
  icon: LucideIcon
  loading?: boolean
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

const TYPE_LABELS: Record<string, string> = {
  episodic: "情景记忆",
  semantic: "语义记忆",
  procedural: "程序记忆",
  emotional: "情感记忆",
  working: "工作记忆",
}

const TIER_COLORS: Record<string, string> = {
  core: "#8b5cf6",
  working: "#06b6d4",
  peripheral: "#9ca3af",
}

const TIER_LABELS: Record<string, string> = {
  core: "核心",
  working: "工作区",
  peripheral: "边缘",
}

function StatCard({ label, value, icon: Icon, loading }: StatCardProps) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground">
          {label}
        </CardTitle>
        <Icon className="size-4 text-muted-foreground" />
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold">
          {loading ? <Skeleton className="h-8 w-20" /> : value}
        </div>
      </CardContent>
    </Card>
  )
}

function ChartSkeleton({ className }: { className?: string }) {
  return (
    <div className={cn("flex flex-col gap-4", className)}>
      <Skeleton className="h-6 w-32" />
      <Skeleton className="flex-1 min-h-[240px]" />
    </div>
  )
}

export function DashboardPage() {
  const navigate = useNavigate()
  const [stats, setStats] = useState<StatsResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const { users, currentUserId } = useAuthStore()
  const currentUser = users.find((u) => u.id === currentUserId)
  const spaceName = currentUser?.spaceName || "默认"

  useEffect(() => {
    async function fetchStats() {
      try {
        setLoading(true)
        setError(null)
        const response = await apiClient.get<StatsResponse>("/v1/stats")
        setStats(response)
      } catch (err) {
        console.error("Failed to fetch stats:", err)
        setError("加载统计数据失败")
      } finally {
        setLoading(false)
      }
    }

    fetchStats()
  }, [])

  const getTodayCount = () => {
    if (!stats?.timeline?.length) return 0
    const today = new Date().toISOString().split("T")[0]
    const todayEntry = stats.timeline.find((t) => t.date === today)
    return todayEntry?.count ?? 0
  }

  const getSpaceCount = () => {
    if (!stats?.by_space) return 0
    return Object.keys(stats.by_space).length
  }

  const getActiveCount = () => {
    if (!stats?.by_state) return 0
    return stats.by_state.active ?? stats.by_state.活跃 ?? 0
  }

  const getCoreCount = () => {
    if (!stats?.by_tier) return 0
    return stats.by_tier.core ?? 0
  }

  const typeData = useMemo(() => {
    if (!stats?.by_type) return []
    return Object.entries(stats.by_type).map(([key, value]) => ({
      name: TYPE_LABELS[key] || key,
      value,
      key,
    }))
  }, [stats])

  const tierData = useMemo(() => {
    if (!stats?.by_tier) return []
    return Object.entries(stats.by_tier).map(([key, value]) => ({
      name: TIER_LABELS[key] || key,
      value,
      key,
    }))
  }, [stats])

  const timelineData = useMemo(() => {
    if (!stats?.timeline?.length) return []
    return stats.timeline.slice(-30)
  }, [stats])

  const categoryData = useMemo(() => {
    if (!stats?.by_category) return []
    return Object.entries(stats.by_category)
      .map(([name, value]) => ({ name, value }))
      .sort((a, b) => b.value - a.value)
      .slice(0, 10)
  }, [stats])

  return (
    <div className="space-y-8">
      <div className="space-y-2">
        <h1 className="text-3xl font-semibold tracking-tight">{spaceName}空间的记忆库状态</h1>
        <p className="text-muted-foreground">
          概览当前空间的记忆数据
        </p>
      </div>

      {error && (
        <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-4 text-sm text-destructive">
          {error}
        </div>
      )}

      <div className="flex flex-wrap gap-3">
        <Button size="sm" onClick={() => navigate("/memories/new")}>
          <Plus className="size-4 mr-1.5" />
          新建记忆
        </Button>
        <Button variant="outline" size="sm" onClick={() => navigate("/import")}>
          <Upload className="size-4 mr-1.5" />
          批量导入
        </Button>
        <Button variant="outline" size="sm" onClick={() => navigate("/analytics")}>
          <FileText className="size-4 mr-1.5" />
          查看统计报告
        </Button>
      </div>

      <div className={cn(
        "grid gap-4",
        "grid-cols-1",
        "md:grid-cols-2",
        "lg:grid-cols-4"
      )}>
        <StatCard
          label="总记忆数"
          value={stats?.total ?? 0}
          icon={Brain}
          loading={loading}
        />
        <StatCard
          label="今日新增"
          value={getTodayCount()}
          icon={TrendingUp}
          loading={loading}
        />
        <StatCard
          label="空间数"
          value={getSpaceCount()}
          icon={Home}
          loading={loading}
        />
        <StatCard
          label="总访问次数"
          value={stats?.total_access_count ?? 0}
          icon={Eye}
          loading={loading}
        />
      </div>

      <div className={cn(
        "grid gap-4",
        "grid-cols-1",
        "md:grid-cols-2",
        "lg:grid-cols-4"
      )}>
        <StatCard
          label="平均重要度"
          value={stats?.avg_importance ? `${(stats.avg_importance * 100).toFixed(0)}%` : "—"}
          icon={BarChart3}
          loading={loading}
        />
        <StatCard
          label="平均置信度"
          value={stats?.avg_confidence ? `${(stats.avg_confidence * 100).toFixed(0)}%` : "—"}
          icon={CheckCircle}
          loading={loading}
        />
        <StatCard
          label="核心记忆数"
          value={getCoreCount()}
          icon={Star}
          loading={loading}
        />
        <StatCard
          label="活跃记忆数"
          value={getActiveCount()}
          icon={Activity}
          loading={loading}
        />
      </div>

      <div className={cn(
        "grid gap-4",
        "grid-cols-1",
        "lg:grid-cols-2"
      )}>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">记忆类型分布</CardTitle>
          </CardHeader>
          <CardContent>
            {loading ? (
              <ChartSkeleton />
            ) : typeData.length === 0 ? (
              <div className="flex items-center justify-center h-[240px] text-muted-foreground text-sm">
                暂无数据
              </div>
            ) : (
              <ResponsiveContainer width="100%" height={280}>
                <PieChart>
                  <Pie
                    data={typeData}
                    cx="50%"
                    cy="50%"
                    outerRadius={100}
                    dataKey="value"
                    label={({ name, percent }) =>
                      `${name} ${(percent * 100).toFixed(0)}%`
                    }
                  >
                    {typeData.map((entry, index) => (
                      <Cell
                        key={`cell-${entry.key}`}
                        fill={COLORS[index % COLORS.length]}
                      />
                    ))}
                  </Pie>
                  <Tooltip />
                </PieChart>
              </ResponsiveContainer>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">记忆等级分布</CardTitle>
          </CardHeader>
          <CardContent>
            {loading ? (
              <ChartSkeleton />
            ) : tierData.length === 0 ? (
              <div className="flex items-center justify-center h-[240px] text-muted-foreground text-sm">
                暂无数据
              </div>
            ) : (
              <div className="relative">
                <ResponsiveContainer width="100%" height={280}>
                  <PieChart>
                    <Pie
                      data={tierData}
                      cx="50%"
                      cy="50%"
                      innerRadius={70}
                      outerRadius={100}
                      paddingAngle={4}
                      dataKey="value"
                    >
                      {tierData.map((entry) => (
                        <Cell
                          key={`tier-${entry.key}`}
                          fill={TIER_COLORS[entry.key] || "#9ca3af"}
                        />
                      ))}
                    </Pie>
                    <Tooltip />
                  </PieChart>
                </ResponsiveContainer>
                <div className="absolute inset-0 flex flex-col items-center justify-center pointer-events-none">
                  <span className="text-3xl font-bold">{stats?.total ?? 0}</span>
                  <span className="text-xs text-muted-foreground">总记忆数</span>
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle className="text-base">
              新增趋势（最近{timelineData.length}天）
            </CardTitle>
          </CardHeader>
          <CardContent>
            {loading ? (
              <ChartSkeleton />
            ) : timelineData.length === 0 ? (
              <div className="flex items-center justify-center h-[240px] text-muted-foreground text-sm">
                暂无数据
              </div>
            ) : (
              <ResponsiveContainer width="100%" height={280}>
                <AreaChart data={timelineData}>
                  <defs>
                    <linearGradient id="colorCount" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#8b5cf6" stopOpacity={0.3} />
                      <stop offset="95%" stopColor="#8b5cf6" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                  <XAxis
                    dataKey="date"
                    tick={{ fontSize: 12 }}
                    tickFormatter={(value: string) => {
                      const [, month, day] = value.split("-")
                      return `${month}-${day}`
                    }}
                  />
                  <YAxis tick={{ fontSize: 12 }} />
                  <Tooltip
                    labelFormatter={(label: string) => `日期: ${label}`}
                    formatter={(value: number) => [value, "新增数量"]}
                  />
                  <Area
                    type="monotone"
                    dataKey="count"
                    stroke="#8b5cf6"
                    fillOpacity={1}
                    fill="url(#colorCount)"
                    strokeWidth={2}
                  />
                </AreaChart>
              </ResponsiveContainer>
            )}
          </CardContent>
        </Card>

        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle className="text-base">分类 TOP10</CardTitle>
          </CardHeader>
          <CardContent>
            {loading ? (
              <ChartSkeleton />
            ) : categoryData.length === 0 ? (
              <div className="flex items-center justify-center h-[240px] text-muted-foreground text-sm">
                暂无数据
              </div>
            ) : (
              <ResponsiveContainer width="100%" height={280}>
                <BarChart data={categoryData} layout="vertical">
                  <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                  <XAxis type="number" tick={{ fontSize: 12 }} />
                  <YAxis
                    dataKey="name"
                    type="category"
                    tick={{ fontSize: 11 }}
                    width={100}
                  />
                  <Tooltip />
                  <Bar
                    dataKey="value"
                    fill="#06b6d4"
                    radius={[0, 4, 4, 0]}
                  />
                </BarChart>
              </ResponsiveContainer>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
