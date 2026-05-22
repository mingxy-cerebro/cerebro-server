import { useEffect, useState } from "react"
import { toast } from "sonner"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { getLifecycleConfig, getTierChanges, triggerLifecycle } from "@/api/lifecycle"
import type { LifecycleConfig, TierChange } from "@/types/lifecycle"
import { getTierLabel, getTierBadgeClass } from "@/lib/tag-utils"
import {
  Timer,
  RotateCcw,
  ArrowUpRight,
  ArrowDownRight,
  Activity,
  TrendingUp,
  Clock,
  Zap,
  Eye,
  Loader2,
} from "lucide-react"
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from "recharts"

const SCHEDULER_INTERVAL_HOURS = 6

const reasonMap: Record<string, string> = {
  access_via_get: "访问触发",
  access_via_search: "搜索触发",
  access_via_recall: "召回触发",
  access_via_cross_space_search: "跨空间搜索触发",
  scheduled_evaluation: "定时评估",
}

const tierOrder: Record<string, number> = { peripheral: 0, working: 1, core: 2 }

function formatNextRun(): string {
  const now = new Date()
  const nextHours = SCHEDULER_INTERVAL_HOURS - (now.getHours() % SCHEDULER_INTERVAL_HOURS)
  const next = new Date(now.getTime() + nextHours * 3600000)
  next.setMinutes(0, 0, 0)
  return next.toLocaleString("zh-CN")
}

function generateDecayCurvePreview(beta: number, halfLife: number) {
  const points = []
  for (let day = 0; day <= 120; day += 2) {
    const lambda = Math.log(2) / halfLife
    const score = Math.exp(-lambda * Math.pow(day, beta))
    points.push({ day: `D${day}`, Core: beta < 1 ? Math.max(score, 0.9) : score * 0.95, Working: score, Peripheral: score * 0.85 })
  }
  return points
}

function ConfigSkeleton() {
  return (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
      {[1, 2].map((i) => (
        <Card key={i}>
          <CardHeader className="pb-2">
            <Skeleton className="h-4 w-28" />
          </CardHeader>
          <CardContent className="space-y-2">
            <Skeleton className="h-4 w-full" />
            <Skeleton className="h-4 w-3/4" />
            <Skeleton className="h-4 w-1/2" />
          </CardContent>
        </Card>
      ))}
    </div>
  )
}

function TierChangesSkeleton() {
  return (
    <div className="space-y-2">
      {[1, 2, 3, 4].map((i) => (
        <div key={i} className="flex items-center gap-3 px-4 py-3">
          <Skeleton className="h-4 w-4" />
          <Skeleton className="h-3 w-24" />
          <Skeleton className="h-5 w-14" />
          <Skeleton className="h-3 w-4" />
          <Skeleton className="h-5 w-14" />
          <Skeleton className="h-3 flex-1" />
        </div>
      ))}
    </div>
  )
}

export function LifecyclePage() {
  const [config, setConfig] = useState<LifecycleConfig | null>(null)
  const [changes, setChanges] = useState<TierChange[]>([])
  const [totalCount, setTotalCount] = useState(0)
  const [loadingConfig, setLoadingConfig] = useState(true)
  const [loadingChanges, setLoadingChanges] = useState(true)
  const [changesLimit, setChangesLimit] = useState(20)
  const [triggering, setTriggering] = useState(false)

  const loadConfig = async () => {
    try {
      setLoadingConfig(true)
      const res = await getLifecycleConfig()
      setConfig(res)
    } catch (err) {
      console.error("Failed to load lifecycle config:", err)
      toast.error("加载生命周期配置失败")
    } finally {
      setLoadingConfig(false)
    }
  }

  const loadChanges = async () => {
    try {
      setLoadingChanges(true)
      const res = await getTierChanges(changesLimit)
      setChanges(res.changes || [])
      setTotalCount(res.totalCount || 0)
    } catch (err) {
      console.error("Failed to load tier changes:", err)
      toast.error("加载升降级历史失败")
    } finally {
      setLoadingChanges(false)
    }
  }

  useEffect(() => {
    loadConfig()
    loadChanges()
  }, [])

  useEffect(() => {
    loadChanges()
  }, [changesLimit])

  const handleRefresh = () => {
    loadConfig()
    loadChanges()
  }

  const handleTriggerLifecycle = async () => {
    try {
      setTriggering(true)
      const result = await triggerLifecycle()
      toast.success(result.message)
      loadConfig()
      loadChanges()
    } catch (err) {
      console.error("Failed to trigger lifecycle:", err)
      toast.error("触发生命周期失败")
    } finally {
      setTriggering(false)
    }
  }

  const decayPreviewData = config
    ? generateDecayCurvePreview(config.decay.tiers?.working?.beta ?? 1.0, config.decay.half_life_days)
    : []

  const promoteCount = changes.filter((c) => (tierOrder[c.from] ?? 0) < (tierOrder[c.to] ?? 0)).length
  const demoteCount = changes.filter((c) => (tierOrder[c.from] ?? 0) >= (tierOrder[c.to] ?? 0)).length

  return (
    <div className="space-y-6 max-w-6xl mx-auto">
      {/* 页面标题 */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
            <Timer className="size-5" />
            生命周期管理
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            记忆衰减、升降级、自动遗忘的生命周期策略
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={handleRefresh} disabled={loadingConfig && loadingChanges}>
            <RotateCcw className="h-4 w-4 mr-1" />
            刷新
          </Button>
          <Button variant="default" size="sm" onClick={handleTriggerLifecycle} disabled={triggering}>
            {triggering ? (
              <Loader2 className="h-4 w-4 mr-1 animate-spin" />
            ) : (
              <Zap className="h-4 w-4 mr-1" />
            )}
            立即执行
          </Button>
        </div>
      </div>

      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            <Clock className="size-4 text-violet-500" />
            调度器状态
          </CardTitle>
          <CardDescription>自动评估与遗忘任务的运行状态</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="p-3 rounded-lg border">
              <span className="text-xs text-muted-foreground">调度间隔</span>
              <p className="text-lg font-semibold mt-1">每 {SCHEDULER_INTERVAL_HOURS} 小时</p>
            </div>
            <div className="p-3 rounded-lg border">
              <span className="text-xs text-muted-foreground">预计下次运行</span>
              <p className="text-sm font-semibold mt-1">{formatNextRun()}</p>
            </div>
            <div className="p-3 rounded-lg border">
              <span className="text-xs text-muted-foreground">启动时运行</span>
              <div className="mt-1">
                <Badge variant="outline" className="bg-emerald-500/10 text-emerald-600 border-emerald-500/30">
                  已启用
                </Badge>
              </div>
            </div>
          </div>

          <div className="mt-4 p-3 rounded-lg border border-dashed text-xs text-muted-foreground space-y-1">
            <div className="flex items-center gap-2">
              <Eye className="size-3.5" />
              <span>调度器将自动执行：Weibull 衰减计算、Tier 评估升降级、关键词 TTL 检测、Superseded 归档清理</span>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* 区块A：系统配置概览 */}
      {loadingConfig ? (
        <ConfigSkeleton />
      ) : config ? (
        <>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* 衰减参数卡片 */}
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-sm font-medium flex items-center gap-2">
                  <Activity className="size-4 text-blue-500" />
                  衰减参数
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="grid grid-cols-2 gap-3 text-sm">
                  <div>
                    <span className="text-muted-foreground">默认半衰期</span>
                    <p className="font-semibold">{config.decay.half_life_days} 天</p>
                  </div>
                  <div>
                    <span className="text-muted-foreground">Stale 阈值</span>
                    <p className="font-semibold">0.3</p>
                  </div>
                </div>

                <div className="space-y-2">
                  <span className="text-xs text-muted-foreground">权重分配</span>
                  <div className="flex gap-2">
                    <Badge variant="outline" className="bg-blue-500/10 text-blue-600 border-blue-500/30 text-xs">
                      时效 {config.decay.recency_weight}
                    </Badge>
                    <Badge variant="outline" className="bg-emerald-500/10 text-emerald-600 border-emerald-500/30 text-xs">
                      频率 {config.decay.frequency_weight}
                    </Badge>
                    <Badge variant="outline" className="bg-violet-500/10 text-violet-600 border-violet-500/30 text-xs">
                      内在 {config.decay.intrinsic_weight}
                    </Badge>
                  </div>
                </div>

                <div className="space-y-2">
                  <span className="text-xs text-muted-foreground">Tier 衰减速率 (β)</span>
                  <div className="flex gap-2">
                    {Object.entries(config.decay.tiers || {}).map(([tier, params]) => (
                      <Badge key={tier} variant="outline" className={`text-xs ${getTierBadgeClass(tier)}`}>
                        {getTierLabel(tier)} β={params.beta}
                      </Badge>
                    ))}
                  </div>
                </div>

                <div className="space-y-2">
                  <span className="text-xs text-muted-foreground">Tier 底线分数</span>
                  <div className="flex gap-2">
                    {Object.entries(config.decay.tiers || {}).map(([tier, params]) => (
                      <Badge key={tier} variant="outline" className={`text-xs ${getTierBadgeClass(tier)}`}>
                        {getTierLabel(tier)} ≥{params.floor}
                      </Badge>
                    ))}
                  </div>
                </div>
              </CardContent>
            </Card>

            {/* 升降级阈值卡片 */}
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-sm font-medium flex items-center gap-2">
                  <TrendingUp className="size-4 text-emerald-500" />
                  升降级规则
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="p-3 rounded-lg border space-y-2">
                  <div className="flex items-center gap-2 text-sm font-medium">
                    <ArrowUpRight className="size-4 text-emerald-500" />
                    <Badge variant="outline" className={getTierBadgeClass("peripheral")}>
                      {getTierLabel("peripheral")}
                    </Badge>
                    <span className="text-muted-foreground">→</span>
                    <Badge variant="outline" className={getTierBadgeClass("working")}>
                      {getTierLabel("working")}
                    </Badge>
                  </div>
                  <div className="flex gap-2 text-xs">
                    <span className="text-muted-foreground">
                      访问 ≥{config.promotion.peripheral_to_working.min_access_count}次
                    </span>
                    <span className="text-muted-foreground">·</span>
                    <span className="text-muted-foreground">
                      综合 ≥{config.promotion.peripheral_to_working.min_composite}
                    </span>
                  </div>
                </div>

                <div className="p-3 rounded-lg border space-y-2">
                  <div className="flex items-center gap-2 text-sm font-medium">
                    <ArrowUpRight className="size-4 text-emerald-500" />
                    <Badge variant="outline" className={getTierBadgeClass("working")}>
                      {getTierLabel("working")}
                    </Badge>
                    <span className="text-muted-foreground">→</span>
                    <Badge variant="outline" className={getTierBadgeClass("core")}>
                      {getTierLabel("core")}
                    </Badge>
                  </div>
                  <div className="flex gap-2 text-xs">
                    <span className="text-muted-foreground">
                      访问 ≥{config.promotion.working_to_core.min_access_count}次
                    </span>
                    <span className="text-muted-foreground">·</span>
                    <span className="text-muted-foreground">
                      综合 ≥{config.promotion.working_to_core.min_composite}
                    </span>
                    {config.promotion.working_to_core.min_importance != null && (
                      <>
                        <span className="text-muted-foreground">·</span>
                        <span className="text-muted-foreground">
                          重要性 ≥{config.promotion.working_to_core.min_importance}
                        </span>
                      </>
                    )}
                  </div>
                </div>

                <div className="p-3 rounded-lg border border-dashed space-y-1 text-xs text-muted-foreground">
                  <div className="flex items-center gap-2">
                    <ArrowDownRight className="size-3.5 text-red-400" />
                    <span>不满足条件时自动降级至低等级</span>
                  </div>
                </div>
              </CardContent>
            </Card>
          </div>

          {/* 衰减曲线预览 */}
          {decayPreviewData.length > 0 && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium flex items-center gap-2">
                  <Zap className="size-4 text-amber-500" />
                  衰减曲线预览
                </CardTitle>
                <CardDescription>基于当前配置参数生成的理论衰减趋势</CardDescription>
              </CardHeader>
              <CardContent>
                <div className="h-64">
                  <ResponsiveContainer width="100%" height="100%">
                    <LineChart data={decayPreviewData}>
                      <CartesianGrid strokeDasharray="3 3" className="opacity-30" />
                      <XAxis
                        dataKey="day"
                        tick={{ fontSize: 11 }}
                        interval={14}
                        label={{ value: "天数", position: "insideBottomRight", offset: -5, fontSize: 11 }}
                      />
                      <YAxis
                        domain={[0, 1]}
                        tick={{ fontSize: 11 }}
                        label={{ value: "分数", angle: -90, position: "insideLeft", fontSize: 11 }}
                      />
                      <Tooltip
                        contentStyle={{ fontSize: 12, borderRadius: 8 }}
                        formatter={(value: number) => [value.toFixed(3)]}
                      />
                      <Legend iconType="line" wrapperStyle={{ fontSize: 12 }} />
                      <Line
                        type="monotone"
                        dataKey="Core"
                        stroke="#f59e0b"
                        strokeWidth={2}
                        dot={false}
                        name="核心 (Core)"
                      />
                      <Line
                        type="monotone"
                        dataKey="Working"
                        stroke="#10b981"
                        strokeWidth={2}
                        dot={false}
                        name="工作区 (Working)"
                      />
                      <Line
                        type="monotone"
                        dataKey="Peripheral"
                        stroke="#94a3b8"
                        strokeWidth={2}
                        dot={false}
                        name="边缘 (Peripheral)"
                      />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
              </CardContent>
            </Card>
          )}
        </>
      ) : (
        <Card>
          <CardContent className="py-8 text-center">
            <p className="text-sm text-muted-foreground">无法加载生命周期配置</p>
          </CardContent>
        </Card>
      )}

      {/* 区块B：升降级历史 */}
      <Card>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="text-sm font-medium">升降级历史</CardTitle>
              <CardDescription>最近的 Tier 变更记录</CardDescription>
            </div>
            <div className="flex items-center gap-2">
              <Badge variant="outline" className="bg-emerald-500/10 text-emerald-600 border-emerald-500/30 text-xs">
                ↑ 升级 {promoteCount}
              </Badge>
              <Badge variant="outline" className="bg-red-500/10 text-red-600 border-red-500/30 text-xs">
                ↓ 降级 {demoteCount}
              </Badge>
              <span className="text-xs text-muted-foreground">共 {totalCount} 条</span>
            </div>
          </div>
        </CardHeader>
        <CardContent className="p-0">
          {loadingChanges ? (
            <TierChangesSkeleton />
          ) : changes.length === 0 ? (
            <p className="text-sm text-muted-foreground py-12 text-center">暂无变更记录</p>
          ) : (
            <>
              <div className="divide-y">
                {changes.map((c) => {
                  const promoted = (tierOrder[c.from] ?? 0) < (tierOrder[c.to] ?? 0)
                  return (
                    <div
                      key={`${c.memoryId}-${c.at}-${c.from}-${c.to}`}
                      className="flex items-center gap-3 px-4 py-3 hover:bg-muted/30 transition-colors"
                    >
                      {promoted ? (
                        <ArrowUpRight className="size-4 text-emerald-500 shrink-0" />
                      ) : (
                        <ArrowDownRight className="size-4 text-red-500 shrink-0" />
                      )}

                      <span className="text-xs text-muted-foreground w-36 shrink-0">
                        {new Date(c.at).toLocaleString("zh-CN")}
                      </span>

                      <Badge variant="outline" className={getTierBadgeClass(c.from)}>
                        {getTierLabel(c.from)}
                      </Badge>
                      <span className="text-muted-foreground text-xs">→</span>
                      <Badge variant="outline" className={getTierBadgeClass(c.to)}>
                        {getTierLabel(c.to)}
                      </Badge>

                      <span className="text-xs text-muted-foreground w-20 shrink-0">
                        {reasonMap[c.reason] || c.reason}
                      </span>

                      <span className="text-xs text-muted-foreground w-12 shrink-0">
                        #{c.accessCount}
                      </span>

                      <span className="text-xs flex-1 min-w-0 truncate text-muted-foreground">
                        {c.memoryTitle || c.memoryId?.slice(0, 12) || "—"}
                      </span>

                      <span className="text-xs font-mono text-muted-foreground shrink-0">
                        {c.memoryId ? `${c.memoryId.slice(0, 8)}...` : "—"}
                      </span>
                    </div>
                  )
                })}
              </div>

              {changes.length < totalCount && (
                <div className="flex items-center justify-center px-4 py-3 border-t">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setChangesLimit((prev) => prev + 20)}
                  >
                    加载更多
                  </Button>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>

    </div>
  )
}
