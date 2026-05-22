import { useEffect, useState, useMemo } from "react"
import { useNavigate } from "react-router-dom"
import { toast } from "sonner"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Checkbox } from "@/components/ui/checkbox"
import { Button } from "@/components/ui/button"
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
import { listClusters, deleteCluster, batchDeleteClusters, deleteAllClusters } from "@/api/cluster"
import type { Cluster } from "@/types/cluster"
import { getCategoryBadgeClass, getCategoryLabel } from "@/lib/tag-utils"
import { RotateCcw, Inbox, Users, Clock, Filter, ChevronLeft, ChevronRight, Trash2 } from "lucide-react"
import { useSSE } from "@/hooks/use-sse"

function formatDate(dateString: string) {
  return new Date(dateString).toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  })
}

function truncate(text: string, max: number = 100): string {
  if (!text) return "—"
  return text.length <= max ? text : text.slice(0, max) + "..."
}

const PAGE_SIZE = 12

export function ClusterListPage() {
  const navigate = useNavigate()
  const [clusters, setClusters] = useState<Cluster[]>([])
  const [loading, setLoading] = useState(true)
  const [categoryFilter, setCategoryFilter] = useState<string>("all")
  const [page, setPage] = useState(1)
  const [totalCount, setTotalCount] = useState(0)
  const [deleteTarget, setDeleteTarget] = useState<Cluster | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [isBatchDeleting, setIsBatchDeleting] = useState(false)
  const [isDeleteAllDialogOpen, setIsDeleteAllDialogOpen] = useState(false)
  const [isDeletingAll, setIsDeletingAll] = useState(false)
  const [clusteringProgress, setClusteringProgress] = useState<{ processed: number; total: number; pct: number } | null>(null)
  const [clusteringLogs, setClusteringLogs] = useState<{ time: string; preview: string; action: string; stage: string }[]>([])

  const loadData = async (p: number) => {
    try {
      setLoading(true)
      const offset = (p - 1) * PAGE_SIZE
      const res = await listClusters(PAGE_SIZE, offset)
      setClusters(res.clusters || [])
      setTotalCount(res.total || 0)
      setSelectedIds(new Set())
    } catch (err) {
      console.error("Failed to load clusters:", err)
      toast.error("加载簇列表失败")
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadData(page)
  }, [page])

  useSSE("cluster.complete", () => {
    setClusteringProgress(null)
    setTimeout(() => setClusteringLogs([]), 3000)
    loadData(page)
  })

  useSSE("cluster.batch_done", () => {
    loadData(page)
  })

  useSSE("cluster.memory_progress", (e) => {
    const d = e.data as { processed?: number; total?: number; pct?: number; content_preview?: string; action?: string; stage?: string }
    setClusteringProgress(prev => ({
      processed: d.processed ?? prev?.processed ?? 0,
      total: d.total ?? prev?.total ?? 0,
      pct: d.pct ?? prev?.pct ?? 0,
    }))
    if (d.content_preview) {
      const actionLabel: Record<string, string> = { assign_existing: "→ 加入已有簇", create_new: "→ 创建新簇", skip: "⊘ 跳过" }
      const stageLabel: Record<string, string> = { assigning: "匹配中", linking: "关联中", creating_cluster: "建簇中" }
      setClusteringLogs((prev) => [
        { time: new Date().toLocaleTimeString("zh-CN"), preview: d.content_preview || "", action: actionLabel[d.action || ""] || d.action || "", stage: stageLabel[d.stage || ""] || d.stage || "" },
        ...prev,
      ].slice(0, 50))
    }
  })

  const categories = useMemo(() => {
    const set = new Set(clusters.map((c) => c.category).filter(Boolean))
    return Array.from(set).sort()
  }, [clusters])

  const filteredClusters = useMemo(() => {
    if (categoryFilter === "all") return clusters
    return clusters.filter((c) => c.category === categoryFilter)
  }, [clusters, categoryFilter])

  const totalPages = Math.max(1, Math.ceil(totalCount / PAGE_SIZE))
  const safePage = Math.min(page, totalPages)

  const confirmDelete = async () => {
    if (!deleteTarget) return
    setIsDeleting(true)
    try {
      await deleteCluster(deleteTarget.id)
      toast.success(`已删除簇「${deleteTarget.title || "未命名"}」`)
      setDeleteTarget(null)
      loadData(safePage)
    } catch (err) {
      console.error("Failed to delete cluster:", err)
      toast.error("删除失败")
    } finally {
      setIsDeleting(false)
    }
  }

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const toggleSelectAll = () => {
    if (selectedIds.size === filteredClusters.length && filteredClusters.length > 0) {
      setSelectedIds(new Set())
    } else {
      setSelectedIds(new Set(filteredClusters.map((c) => c.id)))
    }
  }

  const handleBatchDelete = async () => {
    if (selectedIds.size === 0) return
    setIsBatchDeleting(true)
    try {
      const res = await batchDeleteClusters(Array.from(selectedIds))
      toast.success(`已删除 ${res.deleted} 个簇，解绑 ${res.unlinked_memories} 条记忆`)
      setSelectedIds(new Set())
      loadData(safePage)
    } catch (err) {
      console.error("Failed to batch delete clusters:", err)
      toast.error("批量删除失败")
    } finally {
      setIsBatchDeleting(false)
    }
  }

  const handleDeleteAll = async () => {
    setIsDeletingAll(true)
    try {
      const res = await deleteAllClusters()
      toast.success(`已删除 ${res.deleted} 个簇，解绑 ${res.unlinked_memories} 条记忆`)
      setIsDeleteAllDialogOpen(false)
      setSelectedIds(new Set())
      loadData(safePage)
    } catch (err) {
      console.error("Failed to delete all clusters:", err)
      toast.error("全部删除失败")
    } finally {
      setIsDeletingAll(false)
    }
  }

  return (
    <div className="space-y-6">
      {clusteringProgress && (
        <div className="bg-primary/5 border border-primary/20 rounded-lg p-3">
          <div className="flex items-center justify-between mb-1.5">
            <span className="text-sm font-medium text-primary">🔄 归簇进行中...</span>
            <span className="text-xs text-muted-foreground">
              {clusteringProgress.processed ?? 0} / {clusteringProgress.total ?? 0}
              {clusteringProgress.pct !== 0 || clusteringProgress.processed !== 0 ? ` (${clusteringProgress.pct ?? 0}%)` : ""}
            </span>
          </div>
          <div className="w-full bg-primary/10 rounded-full h-2">
            <div
              className="bg-primary rounded-full h-2 transition-all duration-300"
              style={{ width: `${clusteringProgress.pct}%` }}
            />
          </div>
          {clusteringLogs.length > 0 && (
            <div className="mt-2 max-h-48 overflow-y-auto text-xs font-mono space-y-0.5">
              {clusteringLogs.map((log, i) => (
                <div key={i} className="flex items-start gap-2 text-muted-foreground">
                  <span className="text-[10px] shrink-0 opacity-50">{log.time}</span>
                  <span className="truncate flex-1">{log.preview}</span>
                  <span className={`shrink-0 font-medium ${log.action.includes("创建") ? "text-green-600" : log.action.includes("加入") ? "text-blue-600" : "text-muted-foreground"}`}>
                    {log.action}
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">簇列表</h1>
          <p className="text-sm text-muted-foreground">
            浏览所有记忆簇及其成员
          </p>
        </div>
        <div className="flex items-center gap-2">
          {categories.length > 1 && (
            <div className="flex items-center gap-2">
              <Filter className="size-3.5 text-muted-foreground" />
              <select
                value={categoryFilter}
                onChange={(e) => setCategoryFilter(e.target.value)}
                className="h-9 rounded-md border border-input bg-background px-3 text-sm"
              >
                <option value="all">全部分类</option>
                {categories.map((cat) => (
                  <option key={cat} value={cat}>
                    {getCategoryLabel(cat)}
                  </option>
                ))}
              </select>
            </div>
          )}
          <Button variant="outline" size="sm" onClick={() => loadData(safePage)} disabled={loading}>
            <RotateCcw className="size-4 mr-1" />
            刷新
          </Button>
        </div>
      </div>

      {!loading && filteredClusters.length > 0 && totalPages > 1 && (
        <div className="flex items-center justify-between">
          <span className="text-xs text-muted-foreground">
            第 {safePage} / {totalPages} 页，共 {totalCount} 个簇
          </span>
          <div className="flex items-center gap-1">
            <Button
              variant="outline"
              size="sm"
              className="h-7 px-2"
              disabled={safePage <= 1}
              onClick={() => setPage(safePage - 1)}
            >
              <ChevronLeft className="size-4" />
            </Button>
            {Array.from({ length: totalPages }, (_, i) => i + 1)
              .filter(p => p === 1 || p === totalPages || Math.abs(p - safePage) <= 2)
              .map((p, i, arr) => (
                <span key={p} className="flex items-center">
                  {i > 0 && arr[i - 1] !== p - 1 && (
                    <span className="px-1 text-xs text-muted-foreground">...</span>
                  )}
                  <Button
                    variant={p === safePage ? "default" : "outline"}
                    size="sm"
                    className="h-7 w-7 px-0"
                    onClick={() => setPage(p)}
                  >
                    {p}
                  </Button>
                </span>
              ))}
            <Button
              variant="outline"
              size="sm"
              className="h-7 px-2"
              disabled={safePage >= totalPages}
              onClick={() => setPage(safePage + 1)}
            >
              <ChevronRight className="size-4" />
            </Button>
          </div>
        </div>
      )}

      {!loading && filteredClusters.length === 0 && (
        <div className="rounded-lg border border-border bg-card p-12 text-center space-y-4">
          <Inbox className="size-12 text-muted-foreground mx-auto" />
          <div className="space-y-2">
            <p className="text-sm font-medium text-muted-foreground">
              {categoryFilter !== "all" ? "未找到匹配的簇" : "暂无簇数据"}
            </p>
            <p className="text-xs text-muted-foreground">
              {categoryFilter !== "all"
                ? "尝试切换分类筛选"
                : "启动归簇任务后，簇将在此处展示"}
            </p>
          </div>
        </div>
      )}

      {!loading && filteredClusters.length > 0 && (
        <div className="flex items-center justify-between rounded-lg border border-border bg-card p-3">
          <div className="flex items-center gap-3">
            <Checkbox
              checked={selectedIds.size === filteredClusters.length && filteredClusters.length > 0}
              onCheckedChange={toggleSelectAll}
              aria-label="全选"
            />
            <span className="text-sm text-muted-foreground">
              已选 {selectedIds.size} 个
            </span>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={selectedIds.size === 0 || isBatchDeleting}
              onClick={handleBatchDelete}
            >
              <Trash2 className="size-4 mr-1" />
              {isBatchDeleting ? "删除中..." : `删除所选 (${selectedIds.size})`}
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="text-red-500 hover:text-red-600 hover:bg-red-500/10"
              disabled={isDeletingAll}
              onClick={() => setIsDeleteAllDialogOpen(true)}
            >
              <Trash2 className="size-4 mr-1" />
              {isDeletingAll ? "删除中..." : "全部删除"}
            </Button>
          </div>
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {loading ? (
          [1, 2, 3, 4, 5, 6].map((i) => (
            <Card key={i} className="cursor-pointer">
              <CardHeader className="pb-2">
                <Skeleton className="h-5 w-3/4" />
              </CardHeader>
              <CardContent className="space-y-3">
                <Skeleton className="h-4 w-full" />
                <Skeleton className="h-4 w-2/3" />
                <div className="flex gap-2">
                  <Skeleton className="h-5 w-12" />
                  <Skeleton className="h-5 w-16" />
                </div>
              </CardContent>
            </Card>
          ))
        ) : (
          filteredClusters.map((cluster) => (
            <Card
              key={cluster.id}
              className="cursor-pointer hover:bg-muted/50 transition-colors group"
              onClick={() => navigate(`/clusters/${cluster.id}`)}
            >
              <CardHeader className="pb-2">
                <div className="flex items-start justify-between gap-2">
                  <div className="flex items-center gap-2 min-w-0">
                    <div onClick={(e) => e.stopPropagation()}>
                      <Checkbox
                        checked={selectedIds.has(cluster.id)}
                        onCheckedChange={() => toggleSelect(cluster.id)}
                        aria-label={`选择 ${cluster.title || "未命名簇"}`}
                      />
                    </div>
                    <CardTitle className="text-base font-semibold line-clamp-1">
                      {cluster.title || "未命名簇"}
                    </CardTitle>
                  </div>
                  <div className="flex items-center gap-1 shrink-0">
                    <Badge
                      variant="outline"
                      className={`text-xs ${getCategoryBadgeClass(cluster.category)}`}
                    >
                      {getCategoryLabel(cluster.category)}
                    </Badge>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6 opacity-0 group-hover:opacity-100 transition-opacity text-red-500 hover:text-red-600 hover:bg-red-500/10"
                      onClick={(e) => { e.stopPropagation(); setDeleteTarget(cluster) }}
                      title="删除此簇"
                    >
                      <Trash2 className="size-3" />
                    </Button>
                  </div>
                </div>
              </CardHeader>
              <CardContent className="space-y-3">
                <p className="text-sm text-muted-foreground line-clamp-3">
                  {truncate(cluster.summary, 150)}
                </p>
                {(cluster.keywords?.length ?? 0) > 0 && (
                  <div className="flex flex-wrap gap-1.5">
                    {cluster.keywords.slice(0, 5).map((kw) => (
                      <Badge
                        key={kw}
                        variant="secondary"
                        className="text-xs font-normal"
                      >
                        {kw}
                      </Badge>
                    ))}
                    {cluster.keywords.length > 5 && (
                      <Badge variant="secondary" className="text-xs font-normal">
                        +{cluster.keywords.length - 5}
                      </Badge>
                    )}
                  </div>
                )}
                <div className="flex items-center gap-3 text-xs text-muted-foreground pt-1">
                  <span className="flex items-center gap-1">
                    <Users className="size-3" />
                    {cluster.member_count} 条记忆
                  </span>
                  <span className="flex items-center gap-1">
                    <Clock className="size-3" />
                    {formatDate(cluster.updated_at)}
                  </span>
                </div>
              </CardContent>
            </Card>
          ))
        )}
      </div>

      {!loading && filteredClusters.length > 0 && (
        <p className="text-sm text-muted-foreground">
          共 {totalCount} 个簇
          {categoryFilter !== "all" && ` (筛选自 ${clusters.length} 个)`}
        </p>
      )}

      <AlertDialog open={!!deleteTarget} onOpenChange={(open) => { if (!open) setDeleteTarget(null) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除簇</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除簇「{deleteTarget?.title || "未命名"}」吗？该簇内的记忆将被解绑但不会被删除。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setDeleteTarget(null)} disabled={isDeleting}>取消</AlertDialogCancel>
            <AlertDialogAction
              disabled={isDeleting}
              onClick={(e) => { e.preventDefault(); confirmDelete() }}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {isDeleting ? "删除中..." : "确认删除"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={isDeleteAllDialogOpen} onOpenChange={setIsDeleteAllDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除全部簇</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除所有簇吗？此操作不可撤销，簇内的记忆将被解绑但不会被删除。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setIsDeleteAllDialogOpen(false)} disabled={isDeletingAll}>取消</AlertDialogCancel>
            <AlertDialogAction
              disabled={isDeletingAll}
              onClick={(e) => { e.preventDefault(); handleDeleteAll() }}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {isDeletingAll ? "删除中..." : "确认删除全部"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
