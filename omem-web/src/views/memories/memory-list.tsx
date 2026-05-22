import { useCallback, useEffect, useRef, useState } from "react"
import { toast } from "sonner"
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
import { useNavigate, useSearchParams, useLocation } from "react-router-dom"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import apiClient from "@/api/client"
import { Search, ChevronLeft, ChevronRight, Lock, Unlock, Plus, Trash2, SlidersHorizontal, ArrowUpDown, RotateCcw, X, Inbox, Calendar, Clock, FolderOpen } from "lucide-react"
import { cn } from "@/lib/utils"
import { useVaultStore } from "@/stores/vault"
import {
  isPrivateMemory,
  getTagClassName,
  getTierLabel,
  getTierBadgeClass,
  getCategoryBadgeClass,
} from "@/lib/tag-utils"

interface MemoryItem {
  id: string
  content: string
  l0_abstract: string
  l1_overview: string
  l2_content: string
  category: string
  memory_type: string
  state: string
  tier: string
  importance: number
  confidence: number
  access_count: number
  tags: string[]
  scope: string
  visibility?: string
  created_at: string
  updated_at: string
}

interface MemoriesResponse {
  memories: MemoryItem[]
  total_count: number
  limit: number
  offset: number
}

const SEARCH_DEBOUNCE_MS = 300

export function formatContent(content: string | undefined, maxLength: number = 120) {
  if (!content) return "—"
  if (content.length <= maxLength) return content
  return content.slice(0, maxLength) + "..."
}

export function formatDate(dateString: string) {
  const date = new Date(dateString)
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  })
}

function PrivateContent({ memory, unlocked }: { memory: MemoryItem; unlocked: boolean }) {
  if (!isPrivateMemory(memory.tags, memory.visibility)) {
    return (
      <p className="text-sm text-foreground line-clamp-3">
        {formatContent(memory.content || memory.l0_abstract)}
      </p>
    )
  }

  if (!unlocked) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Lock className="h-3.5 w-3.5 text-amber-500" />
        <span>🔒 私密记忆 · 已加密</span>
      </div>
    )
  }

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-1.5">
        <Unlock className="h-3 w-3 text-amber-500" />
        <span className="text-xs text-amber-500 font-medium">已解锁</span>
      </div>
      <p className="text-sm text-foreground line-clamp-3">
        {formatContent(memory.content || memory.l0_abstract)}
      </p>
    </div>
  )
}

export function MemoryListPage() {
  const navigate = useNavigate()
  const location = useLocation()
  const [searchParams, setSearchParams] = useSearchParams()

  const getParam = (key: string, defaultValue: string) => searchParams.get(key) || defaultValue
  const getNumParam = (key: string, defaultValue: number) => {
    const v = searchParams.get(key)
    return v ? parseInt(v, 10) : defaultValue
  }

  const [memories, setMemories] = useState<MemoryItem[]>([])
  const [loading, setLoading] = useState(true)
  const [, setError] = useState<string | null>(null)
  const [page, setPage] = useState(getNumParam("page", 1))
  const [searchQuery, setSearchQuery] = useState(getParam("q", ""))
  const [totalCount, setTotalCount] = useState(0)
  const vaultUnlocked = useVaultStore((s) => s.isUnlocked)
  const vaultLock = useVaultStore((s) => s.lock)
  const vaultUnlock = useVaultStore((s) => s.unlock)
  const [showVaultInput, setShowVaultInput] = useState(false)
  const [vaultPassword, setVaultPassword] = useState("")
  const [vaultError, setVaultError] = useState<string | null>(null)
  const [tierFilter, setTierFilter] = useState(getParam("tier", "all"))
  const [sortBy, setSortBy] = useState(getParam("sort", "created_at"))
  const [pageSize, setPageSize] = useState(getNumParam("size", 20))
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)
  const [visibilityFilter, setVisibilityFilter] = useState<'all' | 'global' | 'private'>(getParam("privacy", "all") as 'all' | 'global' | 'private')
  const [debouncedQuery, setDebouncedQuery] = useState(getParam("q", ""))
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false)
  const [batchMode, setBatchMode] = useState(false)
  const [refreshKey, setRefreshKey] = useState(0)
  const [projectPathFilter, setProjectPathFilter] = useState(getParam("project", ""))
  const [projectPaths, setProjectPaths] = useState<string[]>([])
  const isInitialMount = useRef(true)

  useEffect(() => {
    async function loadProjectPaths() {
      try {
        const paths = await apiClient.get<string[]>("/v1/memories/project-paths")
        setProjectPaths(Array.isArray(paths) ? paths : [])
      } catch {
        setProjectPaths([])
      }
    }
    loadProjectPaths()
  }, [refreshKey])

  useEffect(() => {
    if (isInitialMount.current) {
      isInitialMount.current = false
      return
    }
    const timer = setTimeout(() => {
      setPage(1)
      setDebouncedQuery(searchQuery)
    }, SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [searchQuery])

  const loadMemories = useCallback(async () => {
    try {
      setLoading(true)
      setError(null)
      const offset = (page - 1) * pageSize
      const params: Record<string, string | number | undefined> = {
        offset,
        limit: pageSize,
        sort: sortBy,
      }
      if (debouncedQuery.trim()) {
        params.q = debouncedQuery.trim()
      }
      if (tierFilter !== "all") {
        params.tier = tierFilter
      }
      if (visibilityFilter === "private") {
        params.visibility = "private"
      } else if (visibilityFilter === "global") {
        params.visibility = "global"
      }
      if (projectPathFilter) {
        params.project_path = projectPathFilter
      }
      const response = await apiClient.get<MemoriesResponse>("/v1/memories", {
        params,
      })
      setMemories(response.memories || [])
      setTotalCount(response.total_count || 0)
    } catch (err) {
      console.error("Failed to fetch memories:", err)
      setError("加载记忆列表失败")
    } finally {
      setLoading(false)
    }
  }, [page, debouncedQuery, tierFilter, sortBy, pageSize, visibilityFilter, projectPathFilter])

  useEffect(() => {
    loadMemories()
  }, [loadMemories, refreshKey])

  useEffect(() => {
    const params = new URLSearchParams()
    if (page > 1) params.set("page", String(page))
    if (searchQuery) params.set("q", searchQuery)
    if (tierFilter !== "all") params.set("tier", tierFilter)
    if (sortBy !== "created_at") params.set("sort", sortBy)
    if (pageSize !== 20) params.set("size", String(pageSize))
    if (visibilityFilter !== "all") params.set("privacy", visibilityFilter)
    if (projectPathFilter) params.set("project", projectPathFilter)
    setSearchParams(params, { replace: true })
  }, [page, searchQuery, tierFilter, sortBy, pageSize, visibilityFilter, projectPathFilter, setSearchParams])

  const handleRowClick = (id: string) => {
    const from = location.pathname + location.search
    sessionStorage.setItem("memories-list-from", from)
    navigate(`/memories/${id}`, { state: { from } })
  }

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setSearchQuery(e.target.value)
  }

  const activeFilters = [
    searchQuery ? { label: `搜索: "${searchQuery}"`, onClear: () => setSearchQuery("") } : null,
    tierFilter !== "all" ? { label: `分类: ${tierFilter}`, onClear: () => setTierFilter("all") } : null,
    sortBy !== "created_at" ? { label: `排序: ${sortBy}`, onClear: () => setSortBy("created_at") } : null,
    visibilityFilter !== "all" ? { label: visibilityFilter === "private" ? "仅私密" : "仅普通", onClear: () => setVisibilityFilter("all") } : null,
    projectPathFilter ? { label: `项目: ${projectPathFilter.split("/").pop()}`, onClear: () => { setProjectPathFilter(""); setPage(1) } } : null,
  ].filter(Boolean) as { label: string; onClear: () => void }[]

  const handleResetFilters = () => {
    setSearchQuery("")
    setTierFilter("all")
    setSortBy("created_at")
    setVisibilityFilter("all")
    setProjectPathFilter("")
    setPage(1)
  }

  const handlePreviousPage = () => {
    if (page > 1) setPage(page - 1)
  }

  const handleNextPage = () => {
    if (page * pageSize < totalCount) setPage(page + 1)
  }

  const handleVaultUnlock = async () => {
    if (!vaultPassword.trim()) {
      setVaultError("请输入密码")
      return
    }
    const isValid = await vaultUnlock(vaultPassword)
    if (!isValid) {
      setVaultError("密码错误")
      return
    }
    setVaultError(null)
    setShowVaultInput(false)
    setVaultPassword("")
  }

  const handleVaultLock = () => {
    vaultLock()
  }

  const handleDeleteClick = (id: string, e: React.MouseEvent) => {
    e.stopPropagation()
    setDeleteTarget(id)
  }

  const confirmDelete = async () => {
    if (!deleteTarget) return
    const targetId = deleteTarget

    setIsDeleting(true)

    try {
      await apiClient.delete(`/v1/memories/${targetId}`)
      toast.success("记忆已删除")
      setDeleteTarget(null)
      setRefreshKey((k) => k + 1)
    } catch (err) {
      console.error("Failed to delete memory:", err)
      toast.error("删除失败，请重试")
      setDeleteTarget(null)
    } finally {
      setIsDeleting(false)
    }
  }

  const toggleSelection = (id: string) => {
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

  const selectAll = () => {
    setSelectedIds(new Set(memories.map((m) => m.id)))
  }

  const clearSelection = () => {
    setSelectedIds(new Set())
  }

  const confirmBatchDelete = async () => {
    if (selectedIds.size === 0) return
    const ids = Array.from(selectedIds)
    setIsDeleting(true)
    try {
      await apiClient.post("/v1/memories/batch-delete", { memory_ids: ids, confirm: true })
      toast.success(`已删除 ${ids.length} 条记忆`)
      setSelectedIds(new Set())
      setBatchDeleteOpen(false)
      setRefreshKey((k) => k + 1)
    } catch (err) {
      console.error("Failed to batch delete:", err)
      toast.error("批量删除失败")
    } finally {
      setIsDeleting(false)
    }
  }

  const hasNext = page * pageSize < totalCount
  const hasPrev = page > 1
  const totalPages = Math.ceil(totalCount / pageSize)

  return (
    <div className="space-y-6">
      <div className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight">记忆列表</h1>
        <p className="text-sm text-muted-foreground">
          浏览和管理您的所有记忆
        </p>
      </div>

      <div className="flex items-center gap-4 flex-wrap">
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
          <Input
            type="text"
            placeholder="搜索记忆内容..."
            value={searchQuery}
            onChange={handleSearchChange}
            className="pl-9"
          />
        </div>

        <div className="flex items-center gap-2">
          <SlidersHorizontal className="size-3.5 text-muted-foreground" />
          <select
            value={tierFilter}
            onChange={(e) => { setTierFilter(e.target.value); setPage(1) }}
            className="h-9 rounded-md border border-input bg-background px-3 text-sm"
          >
            <option value="all">全部分类</option>
            <option value="core">核心</option>
            <option value="working">工作区</option>
            <option value="peripheral">边缘</option>
          </select>

          <ArrowUpDown className="size-3.5 text-muted-foreground" />
          <select
            value={sortBy}
            onChange={(e) => { setSortBy(e.target.value); setPage(1) }}
            className="h-9 rounded-md border border-input bg-background px-3 text-sm"
          >
            <option value="updated_at">按更新时间</option>
            <option value="created_at">按创建时间</option>
            <option value="importance">按重要性</option>
            <option value="confidence">按置信度</option>
            <option value="access_count">按访问次数</option>
          </select>

          <select
            value={pageSize}
            onChange={(e) => { setPageSize(Number(e.target.value)); setPage(1) }}
            className="h-9 rounded-md border border-input bg-background px-3 text-sm"
          >
            <option value={20}>20条/页</option>
            <option value={50}>50条/页</option>
            <option value={100}>100条/页</option>
          </select>

          <Lock className="size-3.5 text-muted-foreground" />
          <select
            value={visibilityFilter}
            onChange={(e) => { setVisibilityFilter(e.target.value as 'all' | 'global' | 'private'); setPage(1) }}
            className="h-9 rounded-md border border-input bg-background px-3 text-sm"
          >
            <option value="all">全部记忆</option>
            <option value="global">普通记忆</option>
            <option value="private">私密记忆</option>
          </select>

          {projectPaths.length > 0 && (
            <>
              <FolderOpen className="size-3.5 text-muted-foreground" />
              <select
                value={projectPathFilter}
                onChange={(e) => { setProjectPathFilter(e.target.value); setPage(1) }}
                className="h-9 rounded-md border border-input bg-background px-3 text-sm max-w-[200px] truncate"
              >
                <option value="">全部项目</option>
                {projectPaths.map((p) => <option key={p} value={p}>{p.split("/").pop() || p}</option>)}
              </select>
            </>
          )}

          {activeFilters.length > 0 && (
            <Button variant="ghost" size="sm" onClick={handleResetFilters} className="text-muted-foreground hover:text-foreground">
              <RotateCcw className="size-3.5 mr-1" />
              重置筛选
            </Button>
          )}
        </div>

        <Button size="sm" onClick={() => navigate("/memories/new")}>
          <Plus className="size-3.5 mr-1.5" />
          新建
        </Button>

        <Button
          variant={batchMode ? "default" : "outline"}
          size="sm"
          onClick={() => {
            setBatchMode(!batchMode)
            if (batchMode) clearSelection()
          }}
        >
          {batchMode ? (
            <>
              <X className="size-3.5 mr-1.5" />
              退出管理
            </>
          ) : (
            <>
              <SlidersHorizontal className="size-3.5 mr-1.5" />
              批量管理
            </>
          )}
        </Button>

        {vaultUnlocked ? (
          <Button variant="outline" size="sm" onClick={handleVaultLock}>
            <Lock className="size-3.5 mr-1.5" />
            锁定 Vault
          </Button>
        ) : (
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowVaultInput(!showVaultInput)}
          >
            <Unlock className="size-3.5 mr-1.5" />
            解锁 Vault
          </Button>
        )}
      </div>

      {activeFilters.length > 0 && (
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-xs text-muted-foreground">活跃筛选:</span>
          {activeFilters.map((filter) => (
            <button
              key={filter.label}
              type="button"
              onClick={filter.onClear}
              className="inline-flex items-center gap-1 px-2 py-1 rounded-full text-xs bg-primary/10 text-primary hover:bg-primary/20 transition-colors"
            >
              {filter.label}
              <X className="size-3" />
            </button>
          ))}
        </div>
      )}

      {batchMode && (
        <div className="flex items-center justify-between bg-muted/50 rounded-lg px-4 py-3">
          <div className="flex items-center gap-3">
            <span className="text-sm font-medium">
              已选择 {selectedIds.size} 条记忆
            </span>
            {selectedIds.size > 0 && (
              <>
                <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={clearSelection}>
                  取消选择
                </Button>
                <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={selectAll}>
                  全选本页
                </Button>
              </>
            )}
          </div>
          {selectedIds.size > 0 && (
            <Button
              variant="destructive"
              size="sm"
              className="h-7"
              onClick={() => setBatchDeleteOpen(true)}
            >
              <Trash2 className="size-3.5 mr-1" />
              删除 ({selectedIds.size})
            </Button>
          )}
        </div>
      )}

      {showVaultInput && (
        <div className="space-y-2 max-w-md">
          <div className="flex items-center gap-2">
            <Input
              type="password"
              placeholder="输入 Vault 密码..."
              value={vaultPassword}
              onChange={(e) => {
                setVaultPassword(e.target.value)
                setVaultError(null)
              }}
              onKeyDown={(e) => e.key === "Enter" && handleVaultUnlock()}
              className={vaultError ? "border-destructive flex-1" : "flex-1"}
            />
            <Button size="sm" onClick={handleVaultUnlock}>
              解锁
            </Button>
          </div>
          {vaultError && (
            <p className="text-xs text-destructive">{vaultError}</p>
          )}
        </div>
      )}

      {!loading && memories.length > 0 && totalPages > 1 && (
        <div className="flex items-center justify-start gap-2">
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              onClick={handlePreviousPage}
              disabled={!hasPrev || loading}
            >
              <ChevronLeft className="size-3" />
            </Button>
            <span className="text-xs text-muted-foreground min-w-[3ch] text-center">
              {page}/{totalPages}
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              onClick={handleNextPage}
              disabled={!hasNext || loading}
            >
              <ChevronRight className="size-3" />
            </Button>
          </div>
          <span className="text-xs text-muted-foreground">
            共 {totalCount} 条
          </span>
        </div>
      )}

      <div className="space-y-3">
        {loading ? (
          [1, 2, 3, 4, 5].map((n) => (
            <div
              key={`sk-${n}`}
              className="rounded-lg border border-border bg-card p-4 space-y-2"
            >
              <Skeleton className="h-4 w-[95%]" />
              <Skeleton className="h-4 w-[80%]" />
              <div className="flex gap-2 pt-2">
                <Skeleton className="h-5 w-16" />
                <Skeleton className="h-5 w-12" />
              </div>
            </div>
          ))
        ) : memories.length === 0 ? (
          <div className="rounded-lg border border-border bg-card p-12 text-center space-y-4">
            <Inbox className="h-12 w-12 text-muted-foreground mx-auto" />
            <div className="space-y-2">
              <p className="text-sm font-medium text-muted-foreground">
                {searchQuery || visibilityFilter !== 'all' || tierFilter !== 'all'
                  ? "未找到匹配的记忆"
                  : "暂无记忆数据"}
              </p>
              <p className="text-xs text-muted-foreground">
                {searchQuery || visibilityFilter !== 'all' || tierFilter !== 'all'
                  ? "尝试调整筛选条件或清除搜索"
                  : "开始记录您的第一条记忆吧"}
              </p>
            </div>
            {activeFilters.length > 0 && (
              <Button variant="outline" size="sm" onClick={handleResetFilters}>
                <RotateCcw className="size-3.5 mr-1.5" />
                清除筛选
              </Button>
            )}
          </div>
        ) : (
          memories.map((memory) => {
            const isSelected = selectedIds.has(memory.id)
            return (
              <div
                key={memory.id}
                onClick={() => batchMode ? toggleSelection(memory.id) : handleRowClick(memory.id)}
                className={cn(
                  "w-full text-left rounded-lg border p-4 cursor-pointer transition-colors relative",
                  isPrivateMemory(memory.tags, memory.visibility)
                    ? "border-amber-500/30 bg-amber-500/5 hover:bg-amber-500/10"
                    : "border-border bg-card hover:bg-muted/50",
                  batchMode && isSelected && "ring-2 ring-primary bg-primary/5"
                )}
              >
                {batchMode && (
                  <div className="absolute left-3 top-1/2 -translate-y-1/2 z-10">
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={(e) => {
                        e.stopPropagation()
                        toggleSelection(memory.id)
                      }}
                      onClick={(e) => e.stopPropagation()}
                      className="size-4 cursor-pointer"
                    />
                  </div>
                )}
                <div className={cn(batchMode && "pl-8")}>
                  <PrivateContent memory={memory} unlocked={vaultUnlocked} />
                  <div className="flex items-center gap-2 mt-3 flex-wrap">
                    <Badge variant="outline" className={`font-normal text-xs ${getCategoryBadgeClass(memory.category)}`}>
                      {memory.category || "未分类"}
                    </Badge>
                    <Badge variant="outline" className={`text-xs ${getTierBadgeClass(memory.tier)}`}>
                      {getTierLabel(memory.tier)}
                    </Badge>
                    {isPrivateMemory(memory.tags, memory.visibility) && (
                      <Badge
                        variant="outline"
                        className={getTagClassName("私密", "text-xs")}
                      >
                        <Lock className="size-2.5 mr-1" />
                        私密
                      </Badge>
                    )}
                    <span className="text-xs text-muted-foreground ml-auto flex items-center gap-1.5">
                      <span className="flex items-center gap-1">
                        <Calendar className="size-3" />
                        {formatDate(memory.created_at)}
                      </span>
                      {memory.updated_at !== memory.created_at && (
                        <span className="flex items-center gap-1">
                          <span className="text-muted-foreground/50">·</span>
                          <Clock className="size-3" />
                          {formatDate(memory.updated_at)}
                        </span>
                      )}
                    </span>
                    {!batchMode && (
                      <button
                        type="button"
                        onClick={(e) => handleDeleteClick(memory.id, e)}
                        className="ml-2 p-1 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive transition-colors"
                        title="删除"
                      >
                        <Trash2 className="size-3.5" />
                      </button>
                    )}
                  </div>
                </div>
              </div>
            )
          })
        )}
      </div>

      {!loading && (
        <div className="flex items-center justify-between">
          <p className="text-sm text-muted-foreground">
            共 {totalCount} 条记忆
          </p>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={handlePreviousPage}
              disabled={!hasPrev || loading}
            >
              <ChevronLeft className="size-4" />
              上一页
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={handleNextPage}
              disabled={!hasNext || loading}
            >
              下一页
              <ChevronRight className="size-4" />
            </Button>
          </div>
        </div>
      )}

      <AlertDialog open={!!deleteTarget} onOpenChange={(open) => { if (!open && !isDeleting) setDeleteTarget(null) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除记忆</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除这条记忆吗？此操作不可撤销。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setDeleteTarget(null)} disabled={isDeleting}>取消</AlertDialogCancel>
            <AlertDialogAction disabled={isDeleting} onClick={confirmDelete} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
              {isDeleting ? "删除中..." : "删除"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={batchDeleteOpen} onOpenChange={(open) => { if (!open && !isDeleting) setBatchDeleteOpen(false) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认批量删除</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除选中的 {selectedIds.size} 条记忆吗？此操作不可撤销。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setBatchDeleteOpen(false)} disabled={isDeleting}>取消</AlertDialogCancel>
            <AlertDialogAction disabled={isDeleting} onClick={(e) => { e.preventDefault(); confirmBatchDelete() }} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
              {isDeleting ? "删除中..." : `删除 ${selectedIds.size} 条`}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
