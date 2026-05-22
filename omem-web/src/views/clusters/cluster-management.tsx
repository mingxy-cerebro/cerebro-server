import { useEffect, useState } from "react"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { getClusteringStats, triggerClustering, listClusteringJobs, deleteClusteringJob } from "@/api/cluster"
import { getSchedulerStatus, pauseClustering, resumeClustering } from "@/api/scheduler"
import type { ClusteringStats, ClusteringJob } from "@/types/cluster"
import { useSSE } from "@/hooks/use-sse"
import { Play, RotateCcw, Clock, CheckCircle, XCircle, Loader2, Trash2, Radio, Pause, Activity, Zap } from "lucide-react"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"

interface LiveProgress {
  stage: string
  message: string
  processed?: number
  total?: number
}

interface SchedulerState {
  paused: boolean
  running: boolean
}

export function ClusterManagementPage() {
  const [stats, setStats] = useState<ClusteringStats | null>(null)
  const [jobs, setJobs] = useState<ClusteringJob[]>([])
  const [loading, setLoading] = useState(true)
  const [triggering, setTriggering] = useState(false)
  const [deletingJobId, setDeletingJobId] = useState<string | null>(null)
  const [liveProgress, setLiveProgress] = useState<LiveProgress | null>(null)
  const [scheduler, setScheduler] = useState<SchedulerState>({ paused: false, running: false })
  const [schedulerLoading, setSchedulerLoading] = useState(false)
  const [showRebuildDialog, setShowRebuildDialog] = useState(false)

  const loadData = async () => {
    try {
      setLoading(true)
      const [statsRes, jobsRes, schedRes] = await Promise.all([
        getClusteringStats(),
        listClusteringJobs(),
        getSchedulerStatus().catch(() => null),
      ])
      setStats(statsRes)
      setJobs(jobsRes.jobs || [])
      if (schedRes) {
        setScheduler(schedRes.clustering)
      }
    } catch (err) {
      console.error("Failed to load cluster data:", err)
      toast.error("加载簇数据失败")
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadData()
  }, [])

  useSSE("cluster.stage", (e) => {
    const d = e.data as Record<string, unknown>
    if (d) {
      setLiveProgress({
        stage: String(d.stage || ""),
        message: String(d.message || ""),
        processed: typeof d.processed === "number" ? d.processed : undefined,
        total: typeof d.total === "number" ? d.total : undefined,
      })
    }
  })

  useSSE("cluster.started", (e) => {
    const d = e.data as Record<string, unknown>
    if (d) {
      setLiveProgress({
        stage: "started",
        message: String(d.message || `开始归簇 ${d.total || 0} 条记忆`),
        processed: 0,
        total: typeof d.total === "number" ? d.total : undefined,
      })
    }
  })

  useSSE("cluster.memory_progress", (e) => {
    const d = e.data as Record<string, unknown>
    if (d) {
      setLiveProgress(prev => ({
        stage: String(d.stage || prev?.stage || "clustering"),
        message: d.stage === "creating_cluster"
          ? `创建新簇...`
          : d.stage === "linking"
          ? `关联已有簇...`
          : `处理中 ${d.processed ?? prev?.processed ?? 0}/${d.total ?? prev?.total ?? "?"}`,
        processed: typeof d.processed === "number" ? d.processed : prev?.processed,
        total: typeof d.total === "number" ? d.total : prev?.total,
      }))
    }
  })

  useSSE("cluster.batch_done", (e) => {
    const d = e.data as Record<string, unknown>
    if (d) {
      setLiveProgress(prev => ({
        stage: prev?.stage || "batch_done",
        message: `批次 ${d.batch} 完成 · 已处理 ${d.processed}/${d.total}`,
        processed: typeof d.processed === "number" ? d.processed : prev?.processed,
        total: typeof d.total === "number" ? d.total : prev?.total,
      }))
    }
    loadData()
  })

  useSSE("cluster.complete", () => {
    setLiveProgress(null)
    loadData()
    toast.success("归簇任务完成")
  })

  useSSE("cluster.failed", (e) => {
    const d = e.data as Record<string, unknown>
    setLiveProgress(null)
    loadData()
    toast.error(`归簇任务失败: ${d?.error || "未知错误"}`)
  })

  const handleTriggerClustering = async (mode: string = "incremental") => {
    try {
      setTriggering(true)
      const result = await triggerClustering(undefined, undefined, mode)
      toast.success(result.message)
      loadData()
    } catch (err) {
      console.error("Failed to trigger clustering:", err)
      toast.error("触发归簇失败")
    } finally {
      setTriggering(false)
    }
  }

  const handleToggleScheduler = async () => {
    try {
      setSchedulerLoading(true)
      if (scheduler.paused) {
        await resumeClustering()
        toast.success("归簇调度已恢复")
      } else {
        await pauseClustering()
        toast.success("归簇调度已暂停")
      }
      const status = await getSchedulerStatus()
      setScheduler(status.clustering)
    } catch {
      toast.error("操作失败")
    } finally {
      setSchedulerLoading(false)
    }
  }

  const handleDeleteJob = async (jobId: string) => {
    try {
      setDeletingJobId(jobId)
      await deleteClusteringJob(jobId)
      toast.success("任务已删除")
      loadData()
    } catch (err) {
      console.error("Failed to delete job:", err)
      toast.error("删除任务失败")
    } finally {
      setDeletingJobId(null)
    }
  }

  const getStatusIcon = (status: string) => {
    switch (status) {
      case "running":
        return <Loader2 className="h-4 w-4 animate-spin text-blue-500" />
      case "completed":
        return <CheckCircle className="h-4 w-4 text-green-500" />
      case "failed":
        return <XCircle className="h-4 w-4 text-red-500" />
      default:
        return <Clock className="h-4 w-4 text-muted-foreground" />
    }
  }

  const getStatusBadge = (status: string) => {
    const variants: Record<string, string> = {
      pending: "bg-yellow-100 text-yellow-800",
      running: "bg-blue-100 text-blue-800",
      completed: "bg-green-100 text-green-800",
      failed: "bg-red-100 text-red-800",
    }
    return variants[status] || "bg-gray-100 text-gray-800"
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">簇管理</h1>
          <p className="text-sm text-muted-foreground">管理记忆归簇任务和查看统计</p>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={loadData} disabled={loading}>
            <RotateCcw className="h-4 w-4 mr-1" />
            刷新
          </Button>
          <Button
            variant={scheduler.paused ? "default" : "outline"}
            size="sm"
            onClick={handleToggleScheduler}
            disabled={schedulerLoading}
          >
            {schedulerLoading ? (
              <Loader2 className="h-4 w-4 mr-1 animate-spin" />
            ) : scheduler.paused ? (
              <Play className="h-4 w-4 mr-1" />
            ) : (
              <Pause className="h-4 w-4 mr-1" />
            )}
            {scheduler.paused ? "恢复调度" : "暂停调度"}
          </Button>
          <Button
            size="sm"
            onClick={() => handleTriggerClustering("incremental")}
            disabled={triggering}
          >
            {triggering ? (
              <>
                <Loader2 className="h-4 w-4 mr-1 animate-spin" />
                处理中...
              </>
            ) : (
              <>
                <Play className="h-4 w-4 mr-1" />
                增量归簇
              </>
            )}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => setShowRebuildDialog(true)}
            disabled={triggering}
          >
            {triggering ? (
              <>
                <Loader2 className="h-4 w-4 mr-1 animate-spin" />
                处理中...
              </>
            ) : (
              <>
                <Zap className="h-4 w-4 mr-1" />
                全局重建
              </>
            )}
          </Button>
        </div>
      </div>

      <div className="flex items-center gap-3 text-sm text-muted-foreground">
        <div className="flex items-center gap-1.5">
          <Activity className="h-3.5 w-3.5" />
          <span>调度器:</span>
          {scheduler.running ? (
            <Badge className="bg-emerald-100 text-emerald-700">运行中</Badge>
          ) : scheduler.paused ? (
            <Badge className="bg-amber-100 text-amber-700">已暂停</Badge>
          ) : (
            <Badge className="bg-slate-100 text-slate-600">空闲</Badge>
          )}
        </div>
      </div>

      {loading ? (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          {[1, 2, 3].map((i) => (
            <Card key={i}>
              <CardHeader className="pb-2">
                <Skeleton className="h-4 w-24" />
              </CardHeader>
              <CardContent>
                <Skeleton className="h-8 w-16" />
              </CardContent>
            </Card>
          ))}
        </div>
      ) : stats ? (
        <>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">总簇数</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{stats.total_clusters}</div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">已归簇记忆</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{stats.total_memories_in_clusters}</div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">未归簇记忆</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold text-amber-600">{stats.orphaned_memories}</div>
              </CardContent>
            </Card>
          </div>

          {liveProgress && (
            <Card className="border-blue-200 bg-blue-50/50">
              <CardHeader className="pb-2">
                <div className="flex items-center gap-2">
                  <Radio className="h-4 w-4 text-blue-500 animate-pulse" />
                  <CardTitle className="text-sm font-medium text-blue-700">实时进度</CardTitle>
                </div>
              </CardHeader>
              <CardContent className="space-y-2">
                <p className="text-sm text-blue-600">{liveProgress.message}</p>
                {liveProgress.total != null && liveProgress.total > 0 && (
                  <div className="w-full bg-blue-200 rounded-full h-2">
                    <div
                      className="bg-blue-500 h-2 rounded-full transition-all duration-300"
                      style={{ width: `${Math.min(100, ((liveProgress.processed || 0) / liveProgress.total) * 100)}%` }}
                    />
                  </div>
                )}
              </CardContent>
            </Card>
          )}

          <Card>
            <CardHeader>
              <CardTitle>归簇任务历史</CardTitle>
              <CardDescription>最近执行的归簇任务记录</CardDescription>
            </CardHeader>
            <CardContent>
              {jobs.length === 0 ? (
                <p className="text-sm text-muted-foreground text-center py-8">暂无归簇任务记录</p>
              ) : (
                <div className="space-y-3">
                  {jobs.map((job) => (
                    <div
                      key={job.id}
                      className="flex items-center justify-between p-3 rounded-lg border"
                    >
                      <div className="flex items-center gap-3">
                        {getStatusIcon(job.status)}
                        <div>
                          <p className="text-sm font-medium">任务 {job.id.slice(0, 8)}</p>
                          <p className="text-xs text-muted-foreground">
                            {job.total_memories} 条记忆 · 处理 {job.processed_memories} 条
                          </p>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <Badge variant="outline" className={getStatusBadge(job.status)}>
                          {job.status === "running" ? "进行中" :
                           job.status === "completed" ? "已完成" :
                           job.status === "failed" ? "失败" : "待处理"}
                        </Badge>
                        {job.created_at && (
                          <span className="text-xs text-muted-foreground">
                            {new Date(job.created_at).toLocaleString("zh-CN")}
                          </span>
                        )}
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7 text-muted-foreground hover:text-destructive"
                          onClick={() => handleDeleteJob(job.id)}
                          disabled={deletingJobId === job.id}
                        >
                          {deletingJobId === job.id ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          ) : (
                            <Trash2 className="h-3.5 w-3.5" />
                          )}
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </>
      ) : (
        <Card>
          <CardContent className="py-8 text-center">
            <p className="text-sm text-muted-foreground">无法加载统计数据</p>
          </CardContent>
        </Card>
      )}

      <AlertDialog open={showRebuildDialog} onOpenChange={setShowRebuildDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认全局重建？</AlertDialogTitle>
            <AlertDialogDescription>
              全局重建会清空所有簇并用 K-Means 重新聚类，此操作不可撤销。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                setShowRebuildDialog(false)
                handleTriggerClustering("rebuild")
              }}
            >
              确认重建
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
