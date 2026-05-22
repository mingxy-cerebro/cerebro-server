import { useEffect, useState } from "react"
import { toast } from "sonner"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import {
  getSchedulerStatus,
  pauseLifecycle,
  resumeLifecycle,
  pauseClustering,
  resumeClustering,
} from "@/api/scheduler"
import {
  Play,
  Pause,
  RefreshCw,
  Activity,
  CheckCircle,
  XCircle,
  Loader2,
} from "lucide-react"
import { useSSE } from "@/hooks/use-sse"

interface SchedulerState {
  paused: boolean
  running: boolean
}

interface StatusData {
  lifecycle: SchedulerState
  clustering: SchedulerState
}

function StatusSkeleton() {
  return (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
      {[1, 2].map((i) => (
        <Card key={i}>
          <CardHeader className="pb-2">
            <Skeleton className="h-4 w-32" />
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center gap-3">
              <Skeleton className="h-5 w-16" />
              <Skeleton className="h-4 w-4" />
            </div>
            <Skeleton className="h-9 w-24" />
          </CardContent>
        </Card>
      ))}
    </div>
  )
}

function getStatusBadge(state: SchedulerState) {
  if (state.paused) {
    return (
      <Badge variant="outline" className="bg-amber-500/10 text-amber-600 border-amber-500/30">
        已暂停
      </Badge>
    )
  }
  if (state.running) {
    return (
      <Badge variant="outline" className="bg-emerald-500/10 text-emerald-600 border-emerald-500/30">
        运行中
      </Badge>
    )
  }
  return (
    <Badge variant="outline" className="bg-slate-500/10 text-slate-600 border-slate-500/30">
      空闲
    </Badge>
  )
}

function getStatusIcon(state: SchedulerState) {
  if (state.paused) {
    return <XCircle className="h-4 w-4 text-amber-500" />
  }
  if (state.running) {
    return <CheckCircle className="h-4 w-4 text-emerald-500" />
  }
  return <Activity className="h-4 w-4 text-slate-400" />
}

function SchedulerCard({
  title,
  description,
  state,
  loading,
  onPause,
  onResume,
}: {
  title: string
  description: string
  state: SchedulerState | null
  loading: boolean
  onPause: () => void
  onResume: () => void
}) {
  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-sm font-medium flex items-center gap-2">
          <Activity className="size-4 text-violet-500" />
          {title}
        </CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {state && (
          <div className="flex items-center gap-3">
            {getStatusBadge(state)}
            {getStatusIcon(state)}
            {state.running && (
              <span className="text-xs text-muted-foreground">当前正在执行任务</span>
            )}
          </div>
        )}
        <div className="flex items-center gap-2">
          {state?.paused ? (
            <Button size="sm" onClick={onResume} disabled={loading}>
              {loading ? (
                <Loader2 className="h-4 w-4 mr-1 animate-spin" />
              ) : (
                <Play className="h-4 w-4 mr-1" />
              )}
              恢复
            </Button>
          ) : (
            <Button variant="outline" size="sm" onClick={onPause} disabled={loading}>
              {loading ? (
                <Loader2 className="h-4 w-4 mr-1 animate-spin" />
              ) : (
                <Pause className="h-4 w-4 mr-1" />
              )}
              暂停
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

export function SchedulerPage() {
  const [status, setStatus] = useState<StatusData | null>(null)
  const [loading, setLoading] = useState(true)
  const [actionLoading, setActionLoading] = useState<string | null>(null)

  const loadStatus = async () => {
    try {
      setLoading(true)
      const res = await getSchedulerStatus()
      setStatus(res)
    } catch (err) {
      console.error("Failed to load scheduler status:", err)
      toast.error("加载调度器状态失败")
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadStatus()
  }, [])

  useSSE("scheduler.status", () => {
    loadStatus()
  })

  useSSE("scheduler.update", () => {
    loadStatus()
  })

  const handlePauseLifecycle = async () => {
    try {
      setActionLoading("pause-lifecycle")
      const res = await pauseLifecycle()
      toast.success(res.action)
      loadStatus()
    } catch (err) {
      console.error("Failed to pause lifecycle:", err)
      toast.error("暂停生命周期调度器失败")
    } finally {
      setActionLoading(null)
    }
  }

  const handleResumeLifecycle = async () => {
    try {
      setActionLoading("resume-lifecycle")
      const res = await resumeLifecycle()
      toast.success(res.action)
      loadStatus()
    } catch (err) {
      console.error("Failed to resume lifecycle:", err)
      toast.error("恢复生命周期调度器失败")
    } finally {
      setActionLoading(null)
    }
  }

  const handlePauseClustering = async () => {
    try {
      setActionLoading("pause-clustering")
      const res = await pauseClustering()
      toast.success(res.action)
      loadStatus()
    } catch (err) {
      console.error("Failed to pause clustering:", err)
      toast.error("暂停归簇调度器失败")
    } finally {
      setActionLoading(null)
    }
  }

  const handleResumeClustering = async () => {
    try {
      setActionLoading("resume-clustering")
      const res = await resumeClustering()
      toast.success(res.action)
      loadStatus()
    } catch (err) {
      console.error("Failed to resume clustering:", err)
      toast.error("恢复归簇调度器失败")
    } finally {
      setActionLoading(null)
    }
  }

  return (
    <div className="space-y-6 max-w-6xl mx-auto">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
            <Activity className="size-5" />
            调度器控制
          </h1>
          <p className="text-sm text-muted-foreground mt-1">
            管理生命周期和归簇调度器的运行状态
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={loadStatus} disabled={loading}>
          <RefreshCw className="h-4 w-4 mr-1" />
          刷新
        </Button>
      </div>

      {loading && !status ? (
        <StatusSkeleton />
      ) : status ? (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <SchedulerCard
            title="Lifecycle Scheduler"
            description="生命周期评估与衰减计算"
            state={status.lifecycle}
            loading={actionLoading === "pause-lifecycle" || actionLoading === "resume-lifecycle"}
            onPause={handlePauseLifecycle}
            onResume={handleResumeLifecycle}
          />
          <SchedulerCard
            title="Clustering Scheduler"
            description="记忆归簇与关联任务"
            state={status.clustering}
            loading={actionLoading === "pause-clustering" || actionLoading === "resume-clustering"}
            onPause={handlePauseClustering}
            onResume={handleResumeClustering}
          />
        </div>
      ) : (
        <Card>
          <CardContent className="py-8 text-center">
            <p className="text-sm text-muted-foreground">无法加载调度器状态</p>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
