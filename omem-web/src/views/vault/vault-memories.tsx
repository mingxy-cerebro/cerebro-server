import { useEffect, useState, useCallback } from "react"
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
import { useNavigate } from "react-router-dom"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import apiClient from "@/api/client"
import {
  Search,
  ChevronLeft,
  ChevronRight,
  Lock,
  Trash2,
  SlidersHorizontal,
  ArrowUpDown,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { useVaultStore } from "@/stores/vault"
import {
  isPrivateMemory,
  getTagClassName,
  getTierLabel,
  getTierVariant,
} from "@/lib/tag-utils"
import { formatDate } from "@/views/memories/memory-list"

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

function VaultUnlock({ onUnlock }: { onUnlock: () => void }) {
  const [password, setPassword] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [isFirstTime, setIsFirstTime] = useState(false)
  const setVaultPassword = useVaultStore((s) => s.setPassword)
  const verifyPassword = useVaultStore((s) => s.verifyPassword)
  const checkStatus = useVaultStore((s) => s.checkStatus)
  const hasPassword = useVaultStore((s) => s.hasPassword)

  useEffect(() => {
    checkStatus().then(() => {
      setIsFirstTime(!hasPassword)
    })
  }, [checkStatus, hasPassword])

  const handleSubmit = async () => {
    if (!password.trim()) {
      setError("请输入密码")
      return
    }
    if (isFirstTime) {
      await setVaultPassword(password)
      onUnlock()
    } else if (await verifyPassword(password)) {
      setError(null)
      onUnlock()
    } else {
      setError("密码错误")
    }
  }

  return (
    <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-8 text-center space-y-4 max-w-md mx-auto mt-12">
      <Lock className="h-10 w-10 text-amber-500 mx-auto" />
      <h3 className="text-lg font-semibold text-amber-500">
        {isFirstTime ? "设置 Vault 密码" : "Vault 已锁定"}
      </h3>
      <p className="text-sm text-muted-foreground max-w-xs mx-auto">
        {isFirstTime
          ? "首次查看私密记忆，请设置 Vault 密码"
          : "私密记忆已加密，请输入 Vault 密码查看"}
      </p>
      <div className="flex items-center gap-2 max-w-xs mx-auto">
        <Input
          type="password"
          placeholder={isFirstTime ? "设置密码..." : "输入密码..."}
          value={password}
          onChange={(e) => {
            setPassword(e.target.value)
            setError(null)
          }}
          onKeyDown={(e) => e.key === "Enter" && handleSubmit()}
          className={error ? "border-destructive" : ""}
        />
        <Button size="sm" onClick={handleSubmit}>
          {isFirstTime ? "设置" : "解锁"}
        </Button>
      </div>
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  )
}

export function VaultMemoriesPage() {
  const navigate = useNavigate()
  const vaultUnlocked = useVaultStore((s) => s.isUnlocked)
  const vaultLock = useVaultStore((s) => s.lock)
  const [localUnlocked, setLocalUnlocked] = useState(false)

  const [memories, setMemories] = useState<MemoryItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [page, setPage] = useState(1)
  const [searchQuery, setSearchQuery] = useState("")
  const [totalCount, setTotalCount] = useState(0)
  const [tierFilter, setTierFilter] = useState<string>("all")
  const [sortBy, setSortBy] = useState<string>("created_at")
  const [pageSize, setPageSize] = useState<number>(50)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)

  const isUnlocked = vaultUnlocked || localUnlocked

  const fetchMemories = useCallback(
    async (pageNum: number, query: string) => {
      try {
        setLoading(true)
        setError(null)
        const offset = (pageNum - 1) * pageSize
        const params: Record<string, string | number | undefined> = {
          offset,
          limit: pageSize,
          search: query || undefined,
          sort: sortBy,
        }
        if (tierFilter !== "all") {
          params.tier = tierFilter
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
    },
    [tierFilter, sortBy, pageSize]
  )

  useEffect(() => {
    if (!isUnlocked) return
    const timer = setTimeout(() => {
      setPage(1)
      fetchMemories(1, searchQuery)
    }, SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [searchQuery, fetchMemories, isUnlocked])

  useEffect(() => {
    if (!isUnlocked) return
    fetchMemories(page, searchQuery)
  }, [page, searchQuery, fetchMemories, isUnlocked])

  const privateMemories = memories.filter((m) => isPrivateMemory(m.tags, m.visibility))

  const handleRowClick = (id: string) => {
    navigate(`/memories/${id}`)
  }

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setSearchQuery(e.target.value)
  }

  const handlePreviousPage = () => {
    if (page > 1) setPage(page - 1)
  }

  const handleNextPage = () => {
    if (page * pageSize < totalCount) setPage(page + 1)
  }

  const handleDeleteClick = (id: string, e: React.MouseEvent) => {
    e.stopPropagation()
    setDeleteTarget(id)
  }

  const confirmDelete = async () => {
    if (!deleteTarget) return
    const targetId = deleteTarget
    const previousMemories = memories

    setMemories((prev) => prev.filter((m) => m.id !== targetId))
    setIsDeleting(true)

    try {
      await apiClient.delete(`/v1/memories/${targetId}`)
      toast.success("记忆已删除")
      setDeleteTarget(null)

      const remainingCount = previousMemories.filter(
        (m) => m.id !== targetId
      ).length
      if (remainingCount === 0 && page > 1) {
        setPage(page - 1)
        fetchMemories(page - 1, searchQuery)
      } else {
        fetchMemories(page, searchQuery)
      }
    } catch (err) {
      console.error("Failed to delete memory:", err)
      toast.error("删除失败，请重试")
      setMemories(previousMemories)
      setDeleteTarget(null)
    } finally {
      setIsDeleting(false)
    }
  }

  const hasNext = page * pageSize < totalCount
  const hasPrev = page > 1

  if (!isUnlocked) {
    return (
      <div className="space-y-6">
        <div className="space-y-2">
          <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
            <Lock className="size-6 text-amber-500" />
            私密记忆
          </h1>
          <p className="text-sm text-muted-foreground">
            管理您的私密记忆内容
          </p>
        </div>
        <VaultUnlock onUnlock={() => setLocalUnlocked(true)} />
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-2">
          <h1 className="text-2xl font-semibold tracking-tight flex items-center gap-2">
            <Lock className="size-6 text-amber-500" />
            私密记忆
          </h1>
          <p className="text-sm text-muted-foreground">
            管理您的私密记忆内容
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={() => vaultLock()}>
          <Lock className="size-3.5 mr-1.5" />
          锁定 Vault
        </Button>
      </div>

      <div className="flex items-center gap-4 flex-wrap">
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
          <Input
            type="text"
            placeholder="搜索私密记忆内容..."
            value={searchQuery}
            onChange={handleSearchChange}
            className="pl-9"
          />
        </div>

        <div className="flex items-center gap-2">
          <SlidersHorizontal className="size-3.5 text-muted-foreground" />
          <select
            value={tierFilter}
            onChange={(e) => {
              setTierFilter(e.target.value)
              setPage(1)
            }}
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
            onChange={(e) => {
              setSortBy(e.target.value)
              setPage(1)
            }}
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
            onChange={(e) => {
              setPageSize(Number(e.target.value))
              setPage(1)
            }}
            className="h-9 rounded-md border border-input bg-background px-3 text-sm"
          >
            <option value={20}>20条/页</option>
            <option value={50}>50条/页</option>
            <option value={100}>100条/页</option>
          </select>
        </div>
      </div>

      {error && (
        <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-4 text-sm text-destructive">
          {error}
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
        ) : privateMemories.length === 0 ? (
          <div className="rounded-lg border border-border bg-card p-8 text-center text-muted-foreground">
            暂无私密记忆
          </div>
        ) : (
          privateMemories.map((memory) => (
            <button
              type="button"
              key={memory.id}
              onClick={() => handleRowClick(memory.id)}
              className={cn(
                "w-full text-left rounded-lg border p-4 cursor-pointer transition-colors",
                "border-amber-500/30 bg-amber-500/5 hover:bg-amber-500/10"
              )}
            >
              <div className="space-y-1">
                <div className="flex items-center gap-1.5">
                  <Lock className="h-3 w-3 text-amber-500" />
                  <span className="text-xs text-amber-500 font-medium">
                    私密记忆
                  </span>
                </div>
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <Lock className="h-3.5 w-3.5 text-amber-500" />
                  <span>🔒 已加密，请点击解锁</span>
                </div>
              </div>
              <div className="flex items-center gap-2 mt-3 flex-wrap">
                <Badge variant="outline" className="font-normal text-xs">
                  {memory.category || "未分类"}
                </Badge>
                <Badge variant={getTierVariant(memory.tier)} className="text-xs">
                  {getTierLabel(memory.tier)}
                </Badge>
                <Badge
                  variant="outline"
                  className={getTagClassName("私密", "text-xs")}
                >
                  <Lock className="size-2.5 mr-1" />
                  私密
                </Badge>
                <span className="text-xs text-muted-foreground ml-auto">
                  {formatDate(memory.created_at)}
                </span>
                <button
                  type="button"
                  onClick={(e) => handleDeleteClick(memory.id, e)}
                  className="ml-2 p-1 rounded hover:bg-destructive/10 text-muted-foreground hover:text-destructive transition-colors"
                  title="删除"
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
            </button>
          ))
        )}
      </div>

      {!loading && (
        <div className="flex items-center justify-between">
          <p className="text-sm text-muted-foreground">
            本页共 {privateMemories.length} 条私密记忆
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

      <AlertDialog
        open={!!deleteTarget}
        onOpenChange={(open) => {
          if (!open && !isDeleting) setDeleteTarget(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除记忆</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除这条记忆吗？此操作不可撤销。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel
              onClick={() => setDeleteTarget(null)}
              disabled={isDeleting}
            >
              取消
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={isDeleting}
              onClick={confirmDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {isDeleting ? "删除中..." : "删除"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
