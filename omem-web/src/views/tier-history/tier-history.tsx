import { useCallback, useEffect, useState } from "react"
import { useNavigate, useLocation } from "react-router-dom"
import { ArrowUpRight, ArrowDownRight, Eye, ChevronLeft, ChevronRight, History, Search, Trash2 } from "lucide-react"
import { Card, CardContent } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { toast } from "sonner"
import apiClient from "@/api/client"
import { getTierLabel, getTierBadgeClass } from "@/lib/tag-utils"

interface TierChangeEvent {
  memoryId: string
  memoryTitle: string
  from: string
  to: string
  reason: string
  at: string
  accessCount: number
}

const reasonMap: Record<string, string> = {
  access_via_get: "访问触发",
  access_via_search: "搜索触发",
  access_via_recall: "召回触发",
  access_via_cross_space_search: "跨空间搜索触发",
  scheduled_evaluation: "定时评估",
}

const PAGE_SIZE = 20

export function TierHistoryPage() {
  const navigate = useNavigate()
  const location = useLocation()
  const [allEvents, setAllEvents] = useState<TierChangeEvent[]>([])
  const [loading, setLoading] = useState(true)
  const [page, setPage] = useState(1)
  const [filterType, setFilterType] = useState<string>("all")
  const [searchQuery, setSearchQuery] = useState("")
  const [debouncedQuery, setDebouncedQuery] = useState("")
  const [deleteTarget, setDeleteTarget] = useState<TierChangeEvent | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)
  const [totalCount, setTotalCount] = useState(0)

  useEffect(() => {
    if (location.state?.from === "tier-history" && location.state?.page) {
      setPage(location.state.page)
    }
  }, [location.state])

  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedQuery(searchQuery)
    }, 300)
    return () => clearTimeout(timer)
  }, [searchQuery])

  const fetchEvents = useCallback(async (p: number) => {
    setLoading(true)
    try {
      const offset = (p - 1) * PAGE_SIZE
      const params: Record<string, string> = {
        limit: String(PAGE_SIZE),
        offset: String(offset),
      }
      if (filterType === "promote") params.filter = "promote"
      else if (filterType === "demote") params.filter = "demote"
      if (debouncedQuery.trim()) params.search = debouncedQuery.trim()

      const res = await apiClient.get<{
        changes: TierChangeEvent[]
        totalCount: number
      }>("/v1/tier-changes", { params })

      setAllEvents(res.changes)
      setTotalCount(res.totalCount)
    } catch (err) {
      console.error("Failed to fetch tier history:", err)
    } finally {
      setLoading(false)
    }
  }, [filterType, debouncedQuery])

  useEffect(() => { fetchEvents(page) }, [fetchEvents, page])

  const totalPages = Math.max(1, Math.ceil(totalCount / PAGE_SIZE))
  const safePage = Math.min(page, totalPages)

  const tierOrder: Record<string, number> = { peripheral: 0, working: 1, core: 2 }

  const confirmDelete = async () => {
    if (!deleteTarget) return
    const event = deleteTarget
    setIsDeleting(true)
    try {
      await apiClient.post("/v1/tier-changes/delete", {
        memory_id: event.memoryId,
        from: event.from,
        to: event.to,
        at: event.at,
        reason: event.reason,
      })
      toast.success("已删除该条变更记录")
      setAllEvents(prev => prev.filter(e =>
        !(e.memoryId === event.memoryId && e.from === event.from && e.to === event.to && e.at === event.at)
      ))
      setTotalCount(prev => Math.max(0, prev - 1))
    } catch (err) {
      console.error("Failed to delete tier history entry:", err)
      toast.error("删除失败")
    } finally {
      setIsDeleting(false)
      setDeleteTarget(null)
    }
  }

  const goToDetail = (memoryId: string) => {
    navigate(`/memories/${memoryId}`, {
      state: { from: "tier-history", page: safePage },
    })
  }

  return (
    <div className="space-y-6 max-w-5xl mx-auto">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
          <History className="size-5" />
          等级变更历史
        </h1>
        <span className="text-sm text-muted-foreground">共 {totalCount} 条记录</span>
      </div>

      <div className="flex items-center gap-3 flex-wrap">
        <div className="relative flex-1 min-w-[200px] max-w-xs">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
          <input
            type="text"
            placeholder="搜索标题、ID、原因..."
            value={searchQuery}
            onChange={(e) => { setSearchQuery(e.target.value); setPage(1) }}
            className="w-full h-9 pl-8 pr-3 rounded-md border border-input bg-background text-sm focus:outline-none focus:ring-1 focus:ring-ring"
          />
        </div>
        <Select value={filterType} onValueChange={(v) => { if (v) { setFilterType(v); setPage(1) } }}>
          <SelectTrigger className="w-[130px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">全部</SelectItem>
            <SelectItem value="promote">仅晋升</SelectItem>
            <SelectItem value="demote">仅降级</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {!loading && allEvents.length > 0 && totalPages > 1 && (
        <div className="flex items-center justify-between">
          <span className="text-xs text-muted-foreground">
            第 {safePage} / {totalPages} 页
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

      <Card>
        <CardContent className="p-0">
          {loading ? (
            <p className="text-sm text-muted-foreground py-12 text-center">加载中...</p>
          ) : allEvents.length === 0 ? (
            <p className="text-sm text-muted-foreground py-12 text-center">暂无变更记录</p>
          ) : (
            <div className="divide-y">
              {allEvents.map((e) => {
                const promoted = (tierOrder[e.from] ?? 0) < (tierOrder[e.to] ?? 0)
                return (
                  <div
                    key={`${e.memoryId}-${e.at}-${e.from}-${e.to}`}
                    className="flex items-center gap-3 px-4 py-3 hover:bg-muted/30 transition-colors"
                  >
                    {promoted ? (
                      <ArrowUpRight className="size-4 text-emerald-500 shrink-0" />
                    ) : (
                      <ArrowDownRight className="size-4 text-red-500 shrink-0" />
                    )}

                    <span className="text-xs text-muted-foreground w-36 shrink-0">
                      {new Date(e.at).toLocaleString("zh-CN")}
                    </span>

                    <Badge variant="outline" className={getTierBadgeClass(e.from)}>
                      {getTierLabel(e.from)}
                    </Badge>
                    <span className="text-muted-foreground text-xs">→</span>
                    <Badge variant="outline" className={getTierBadgeClass(e.to)}>
                      {getTierLabel(e.to)}
                    </Badge>

                    <span className="text-xs text-muted-foreground w-20 shrink-0">
                      {reasonMap[e.reason] || e.reason}
                    </span>

                    <span className="text-xs text-muted-foreground w-16 shrink-0">
                      #{e.accessCount}
                    </span>

                    <button
                      type="button"
                      className="flex-1 min-w-0 text-left text-sm truncate hover:underline cursor-pointer"
                      onClick={() => goToDetail(e.memoryId)}
                      title={e.memoryTitle}
                    >
                      {e.memoryTitle}
                    </button>

                    <span className="text-xs font-mono text-muted-foreground shrink-0">
                      {e.memoryId.slice(0, 8)}...
                    </span>

                    <div className="flex items-center gap-1 shrink-0">
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 px-2"
                        onClick={() => goToDetail(e.memoryId)}
                        title="查看详情"
                      >
                        <Eye className="size-3.5" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 px-2 text-red-500 hover:text-red-600 hover:bg-red-500/10"
                        onClick={() => setDeleteTarget(e)}
                        title="删除此条记录"
                      >
                        <Trash2 className="size-3.5" />
                      </Button>
                    </div>
                  </div>
                )
              })}
            </div>
          )}

          {totalPages > 1 && (
            <div className="flex items-center justify-between px-4 py-3 border-t">
              <span className="text-xs text-muted-foreground">
                第 {safePage} / {totalPages} 页，共 {totalCount} 条
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
        </CardContent>
      </Card>

      <AlertDialog open={!!deleteTarget} onOpenChange={(open) => { if (!open) setDeleteTarget(null) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除变更记录</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除这条 {deleteTarget && `${getTierLabel(deleteTarget.from)} → ${getTierLabel(deleteTarget.to)}`} 的变更记录吗？记忆本身不会被删除。
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
    </div>
  )
}
