import { useCallback, useEffect, useState } from "react"
import { toast } from "sonner"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"

function formatMarkdownContent(text: string): string {
  if (!text || text.includes("\n")) return text
  return text
    .replace(/(#{1,3}\s+[^#])/g, "\n$1")
    .replace(/(\d+\.\s+[^\d])/g, "\n$1")
    .replace(/(\s-\s)/g, "\n- ")
    .trim()
}
import { useParams, useNavigate, useLocation } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import apiClient from "@/api/client"
import { useVaultStore } from "@/stores/vault"
import { getCategoryBadgeClass } from "@/lib/tag-utils"
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
import {
  ArrowLeft,
  Clock,
  ChevronDown,
  ChevronUp,
  Zap,
  MousePointerClick,
  Search,
  BrainCircuit,
  BarChart3,
  Trash2,
  Lock,
  Unlock,
  ChevronLeft,
  ChevronRight,
  Code,
} from "lucide-react"

interface RecallEvent {
  id: string
  session_id: string
  recall_type: "auto" | "manual"
  query_text: string
  max_score: number
  llm_confidence: number
  profile_injected: boolean
  kept_count: number
  discarded_count: number
  injected_count?: number
  profile_content?: string
  injected_content?: string
  tenant_id: string
  created_at: string
}

interface RecallItem {
  id: string
  event_id: string
  memory_id: string
  score: number
  refine_relevance: "high" | "medium" | "irrelevant"
  refine_reasoning: string
  is_kept: boolean
  tenant_id: string
  created_at: string
}

interface MemoryDetail {
  id: string
  content: string
  l0_abstract: string
  category: string
  memory_type: string
  visibility?: string
  tags?: string[]
}

function formatDate(dateString: string) {
  const date = new Date(dateString)
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  })
}

function shortSessionId(sessionId: string) {
  if (!sessionId) return "—"
  if (sessionId.length <= 16) return sessionId
  return sessionId.slice(0, 8) + "..." + sessionId.slice(-8)
}

function truncateQuery(text: string, max = 80) {
  if (!text) return "—"
  if (text.length <= max) return text
  return text.slice(0, max) + "..."
}

function RecallTypeBadge({ type }: { type: "auto" | "manual" }) {
  if (type === "auto") {
    return (
      <Badge variant="secondary" className="text-xs bg-blue-100 text-blue-700 hover:bg-blue-100 border-blue-200">
        <Zap className="size-3 mr-1" />
        自动注入
      </Badge>
    )
  }
  return (
    <Badge variant="outline" className="text-xs bg-emerald-50 text-emerald-700 hover:bg-emerald-50 border-emerald-200">
      <MousePointerClick className="size-3 mr-1" />
      手动注入
    </Badge>
  )
}

function CategoryBadge({ category }: { category?: string }) {
  return <Badge variant="outline" className={`text-xs font-normal ${getCategoryBadgeClass(category)}`}>{category || "未分类"}</Badge>
}

function ScoreBar({ label, value, max = 1 }: { label: string; value: number; max?: number }) {
  const safeValue = Math.max(0, Math.min(value, max))
  const percentage = (safeValue / max) * 100
  const isZero = value === 0
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-xs">
        <span className="text-muted-foreground">{label}</span>
        <span className="font-medium">{isZero ? "—" : `${percentage.toFixed(1)}%`}</span>
      </div>
      <div className="h-1.5 w-full rounded-full bg-muted overflow-hidden">
        <div
          className="h-full rounded-full bg-primary transition-all"
          style={{ width: `${percentage}%` }}
        />
      </div>
    </div>
  )
}

function RefineBadge({ relevance, isKept }: { relevance: string; isKept: boolean }) {
  if (!isKept) {
    return (
      <Badge variant="outline" className="text-xs bg-red-500/10 text-red-600 border-red-500/30">
        🔴 被精炼掉
      </Badge>
    )
  }
  if (relevance === "high") {
    return (
      <Badge variant="outline" className="text-xs bg-emerald-500/10 text-emerald-600 border-emerald-500/30">
        🟢 高相关
      </Badge>
    )
  }
  if (relevance === "medium") {
    return (
      <Badge variant="outline" className="text-xs bg-yellow-500/10 text-yellow-600 border-yellow-500/30">
        🟡 中相关
      </Badge>
    )
  }
  return (
    <Badge variant="outline" className="text-xs bg-muted text-muted-foreground border-border">
      {relevance || "—"}
    </Badge>
  )
}

function ItemCard({
  item,
  memory,
  memoryLoading,
  vaultUnlocked,
  memoryUnlocked,
  onUnlock,
  onLock,
  onVaultUnlock,
}: {
  item: RecallItem
  memory: MemoryDetail | null
  memoryLoading: boolean
  vaultUnlocked?: boolean
  memoryUnlocked?: boolean
  onUnlock?: (memoryId: string) => void
  onLock?: (memoryId: string) => void
  onVaultUnlock?: () => void
}) {
  const [activeTab, setActiveTab] = useState<"refine" | "raw">("refine")
  const [showPwInput, setShowPwInput] = useState(false)
  const [pw, setPw] = useState("")
  const [pwErr, setPwErr] = useState<string | null>(null)
  const vaultUnlock = useVaultStore((s) => s.unlock)
  const vaultIsUnlocked = useVaultStore((s) => s.isUnlocked)

  const isPrivate =
    memory?.visibility === "private" ||
    (memory?.tags || []).some((t) => t === "私密" || t.toLowerCase() === "private")
  const isLocked = isPrivate && !memoryUnlocked

  const handleToggle = async () => {
    if (!memory) return
    if (isLocked) {
      if (vaultUnlocked || vaultIsUnlocked) {
        onUnlock?.(memory.id)
      } else {
        setShowPwInput(true)
      }
    } else {
      onLock?.(memory.id)
      setShowPwInput(false)
    }
  }

  const handlePwSubmit = async () => {
    if (!pw.trim()) {
      setPwErr("请输入密码")
      return
    }
    const valid = await vaultUnlock(pw)
    if (valid) {
      setShowPwInput(false)
      setPw("")
      setPwErr(null)
      onVaultUnlock?.()
      if (memory) onUnlock?.(memory.id)
    } else {
      setPwErr("密码错误")
    }
  }

  const borderClass = item.is_kept
    ? "border-border"
    : "border-dashed border-red-500/30 opacity-75"

  return (
    <div className={`rounded-md border bg-muted/50 p-3 space-y-3 ${borderClass}`}>
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 flex-wrap">
          <RefineBadge relevance={item.refine_relevance} isKept={item.is_kept} />
          <CategoryBadge category={memory?.category} />
          {isPrivate && (
            <Badge variant="secondary" className="text-xs bg-amber-100 text-amber-700 hover:bg-amber-100 border-amber-200">
              <Lock className="size-3 mr-1" />
              私密
            </Badge>
          )}
          <span className="text-xs text-muted-foreground font-mono">
            {item.memory_id?.slice(0, 8)}...
          </span>
        </div>
        {isPrivate && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation()
              handleToggle()
            }}
            className="text-xs text-amber-500 hover:text-amber-600 flex items-center gap-1 shrink-0"
          >
            {isLocked ? <Lock className="size-3" /> : <Unlock className="size-3" />}
            {isLocked ? "解锁" : "锁定"}
          </button>
        )}
      </div>

      <div className="flex items-center gap-1 border-b border-border pb-1">
        <button
          type="button"
          onClick={() => setActiveTab("refine")}
          className={`text-xs px-2 py-1 rounded transition-colors ${
            activeTab === "refine"
              ? "bg-primary/10 text-primary font-medium"
              : "text-muted-foreground hover:text-foreground"
          }`}
        >
          精炼
        </button>
        <button
          type="button"
          onClick={() => setActiveTab("raw")}
          className={`text-xs px-2 py-1 rounded transition-colors ${
            activeTab === "raw"
              ? "bg-primary/10 text-primary font-medium"
              : "text-muted-foreground hover:text-foreground"
          }`}
        >
          原始
        </button>
      </div>

      {activeTab === "refine" ? (
        <div className="space-y-2">
          <div className="text-sm text-foreground">
            <span className="font-medium">推理说明：</span>
            <span className="text-muted-foreground">{item.refine_reasoning || "—"}</span>
          </div>
          <ScoreBar label="相似度得分" value={item.score} max={1} />
        </div>
      ) : (
        <div className="space-y-2">
          {memoryLoading ? (
            <div className="space-y-2">
              <Skeleton className="h-4 w-3/4" />
              <Skeleton className="h-4 w-1/2" />
            </div>
          ) : memory ? (
            <>
              {isLocked ? (
                <div className="space-y-2">
                  <div className="flex items-center gap-2 text-sm text-muted-foreground">
                    <Lock className="size-4" />
                    <span>私密记忆内容已隐藏</span>
                  </div>
                  {showPwInput && (
                    <div className="space-y-2">
                      <div className="flex items-center gap-2">
                        <Input
                          type="password"
                          placeholder="输入 Vault 密码..."
                          value={pw}
                          onChange={(e) => {
                            setPw(e.target.value)
                            setPwErr(null)
                          }}
                          onKeyDown={(e) => e.key === "Enter" && handlePwSubmit()}
                          className={pwErr ? "border-destructive flex-1" : "flex-1"}
                        />
                        <Button size="sm" onClick={handlePwSubmit}>
                          解锁
                        </Button>
                      </div>
                      {pwErr && <p className="text-xs text-destructive">{pwErr}</p>}
                    </div>
                  )}
                </div>
              ) : (
                <div className="prose prose-sm dark:prose-invert max-w-none">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {formatMarkdownContent(memory.content || memory.l0_abstract || "—")}
                  </ReactMarkdown>
                </div>
              )}
            </>
          ) : (
            <p className="text-sm text-muted-foreground">记忆内容加载失败</p>
          )}
        </div>
      )}
    </div>
  )
}

function EventCard({
  event,
  initialItems,
  isLast,
  defaultExpanded,
  vaultUnlocked,
  unlockedMemories,
  manuallyLocked,
  onUnlock,
  onLock,
  onVaultUnlock,
}: {
  event: RecallEvent
  initialItems?: RecallItem[]
  isLast: boolean
  defaultExpanded?: boolean
  vaultUnlocked?: boolean
  unlockedMemories: Set<string>
  manuallyLocked: Set<string>
  onUnlock?: (memoryId: string) => void
  onLock?: (memoryId: string) => void
  onVaultUnlock?: () => void
}) {
  const [expanded, setExpanded] = useState(defaultExpanded || false)
  const [showInjected, setShowInjected] = useState(true)
  const [items, setItems] = useState<RecallItem[] | null>(initialItems ?? null)
  const [memories, setMemories] = useState<Map<string, MemoryDetail>>(new Map())
  const [memoriesLoading, setMemoriesLoading] = useState<Set<string>>(new Set())

  useEffect(() => {
    if (initialItems && items === null) {
      setItems(initialItems)
    }
  }, [initialItems, items])

  const loadMemory = useCallback(async (memoryId: string) => {
    if (memories.has(memoryId) || memoriesLoading.has(memoryId)) return
    try {
      setMemoriesLoading((prev) => new Set(prev).add(memoryId))
      const data = await apiClient.get<MemoryDetail>(`/v1/memories/${memoryId}`)
      setMemories((prev) => {
        const next = new Map(prev)
        next.set(memoryId, data)
        return next
      })
    } catch (err) {
      console.error("Failed to fetch memory:", err)
    } finally {
      setMemoriesLoading((prev) => {
        const next = new Set(prev)
        next.delete(memoryId)
        return next
      })
    }
  }, [memories, memoriesLoading])

  useEffect(() => {
    if (!expanded || !items || items.length === 0) return
    items.forEach((item) => {
      if (!memories.has(item.memory_id) && !memoriesLoading.has(item.memory_id)) {
        loadMemory(item.memory_id)
      }
    })
  }, [expanded, items, memories, memoriesLoading, loadMemory])

  const memoryUnlockedFor = (memoryId: string) => {
    return (vaultUnlocked && !manuallyLocked.has(memoryId)) || unlockedMemories.has(memoryId)
  }

  return (
    <div className="flex gap-4">
      <div className="flex flex-col items-center">
        <div className="w-2.5 h-2.5 rounded-full bg-primary ring-4 ring-primary/20" />
        {!isLast && <div className="w-px flex-1 bg-border mt-1" />}
      </div>

      <div className="flex-1 pb-6">
        <div className="rounded-lg border border-border bg-card transition-colors hover:bg-muted/50">
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            className="w-full text-left p-4 cursor-pointer"
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2 flex-wrap">
                <RecallTypeBadge type={event.recall_type} />
                <span className="text-xs text-muted-foreground flex items-center gap-1">
                  <Clock className="size-3" />
                  {formatDate(event.created_at)}
                </span>
              </div>
              {expanded ? (
                <ChevronUp className="size-4 text-muted-foreground" />
              ) : (
                <ChevronDown className="size-4 text-muted-foreground" />
              )}
            </div>

            <div className="mt-2 text-sm text-muted-foreground flex items-center gap-1.5">
              <Search className="size-3.5" />
              <span className="line-clamp-1" title={event.query_text || "—"}>
                {truncateQuery(event.query_text)}
              </span>
            </div>
          </button>

          {expanded && (
            <div className="px-4 pb-4 space-y-4 border-t border-border pt-4">
              {event.profile_content && (
                <div className="space-y-2">
                  <h4 className="text-xs font-medium text-muted-foreground flex items-center gap-1">
                    <BrainCircuit className="size-3" />
                    画像注入内容
                  </h4>
                  <div className="rounded-md border border-indigo-500/20 bg-indigo-500/5 p-3">
                    <div className="prose prose-sm dark:prose-invert max-w-none text-sm text-indigo-700 dark:text-indigo-300">
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>
                        {event.profile_content.replace(/<\/?cerebro-profile>/g, "").trim()}
                      </ReactMarkdown>
                    </div>
                  </div>
                </div>
              )}

              {event.injected_content && (
                <div className="space-y-2">
                  <button
                    type="button"
                    onClick={() => setShowInjected(!showInjected)}
                    className="flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground transition-colors cursor-pointer"
                  >
                    <Code className="size-3" />
                    注入内容
                    {showInjected ? (
                      <ChevronUp className="size-3" />
                    ) : (
                      <ChevronDown className="size-3" />
                    )}
                  </button>
                  {showInjected && (
                    <div className="rounded-md border border-emerald-500/20 bg-emerald-500/5 p-3 max-h-96 overflow-y-auto">
                      <div className="prose prose-sm dark:prose-invert max-w-none text-sm text-emerald-700 dark:text-emerald-300">
                        <ReactMarkdown remarkPlugins={[remarkGfm]}>
                          {event.injected_content!.replace(/<\/?cerebro-context>/g, "").trim()}
                        </ReactMarkdown>
                      </div>
                    </div>
                  )}
                </div>
              )}

              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <h4 className="text-xs font-medium text-muted-foreground flex items-center gap-1">
                    <BrainCircuit className="size-3" />
                    关联记忆
                  </h4>
                </div>

                {items && items.length > 0 ? (
                  <div className="space-y-3">
                    {items.map((item) => (
                      <ItemCard
                        key={item.id}
                        item={item}
                        memory={memories.get(item.memory_id) || null}
                        memoryLoading={memoriesLoading.has(item.memory_id)}
                        vaultUnlocked={vaultUnlocked}
                        memoryUnlocked={memoryUnlockedFor(item.memory_id)}
                        onUnlock={onUnlock}
                        onLock={onLock}
                        onVaultUnlock={onVaultUnlock}
                      />
                    ))}
                  </div>
                ) : (
                  <p className="text-sm text-muted-foreground">暂无召回项</p>
                )}
              </div>

              <div className="space-y-3">
                <h4 className="text-xs font-medium text-muted-foreground flex items-center gap-1">
                  <BarChart3 className="size-3" />
                  匹配指标
                </h4>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <ScoreBar label="最大相似度" value={event.max_score} />
                  <ScoreBar label="LLM 置信度" value={event.llm_confidence} />
                </div>
              </div>

              <div className="flex items-center gap-3 text-xs text-muted-foreground flex-wrap">
                <span className="font-medium text-foreground">统计：</span>
                <span className="text-emerald-600 dark:text-emerald-400">
                  保留 {event.kept_count} 条
                </span>
                <span className="text-red-600 dark:text-red-400">
                  精炼掉 {event.discarded_count} 条
                </span>
                {event.injected_count != null && event.injected_count > 0 && (
                  <span className="text-blue-600 dark:text-blue-400">
                    实际注入 {event.injected_count} 条
                  </span>
                )}
                {event.profile_injected && (
                  <span className="text-indigo-600 dark:text-indigo-400">
                    👤 画像已注入
                  </span>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

export function SessionDetailPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const location = useLocation()

  const handleGoBack = () => {
    const fromState = location.state as { from?: string } | null
    if (fromState?.from) {
      navigate(fromState.from, { replace: true })
    } else {
      navigate(-1)
    }
  }

  const sessionId = id ? decodeURIComponent(id) : ""

  const [events, setEvents] = useState<RecallEvent[]>([])
  const [eventItemsMap, setEventItemsMap] = useState<Map<string, RecallItem[]>>(new Map())
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [currentPage, setCurrentPage] = useState(1)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [showVaultInput, setShowVaultInput] = useState(false)
  const [vaultPassword, setVaultPassword] = useState("")
  const [vaultError, setVaultError] = useState<string | null>(null)

  const vaultUnlock = useVaultStore((s) => s.unlock)
  const vaultLock = useVaultStore((s) => s.lock)
  const [sessionVaultUnlocked, setSessionVaultUnlocked] = useState(false)
  const [unlockedMemories, setUnlockedMemories] = useState<Set<string>>(new Set())
  const [manuallyLocked, setManuallyLocked] = useState<Set<string>>(new Set())
  const [pendingUnlockMemoryId, setPendingUnlockMemoryId] = useState<string | null>(null)

  const PAGE_SIZE = 10
  const totalPages = Math.ceil(events.length / PAGE_SIZE)
  const paginatedEvents = events.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE)

  useEffect(() => {
    if (!sessionId) return

    async function fetchData() {
      try {
        setLoading(true)
        setError(null)

        const data = await apiClient.get<{
          events: RecallEvent[]
          event_items?: {
            id: string
            items: RecallItem[]
          }[]
          limit: number
          offset: number
        }>("/v1/recall-events", {
          params: { session_id: sessionId, limit: 10000, offset: 0, expand: "items" },
        })
        const list = (data?.events || []).sort(
          (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
        )
        setEvents(list)

        if (data?.event_items) {
          const map = new Map<string, RecallItem[]>()
          data.event_items.forEach((ei) => map.set(ei.id, ei.items || []))
          setEventItemsMap(map)
        }
      } catch (err) {
        console.error("Failed to fetch session detail:", err)
        setError("加载 Session 详情失败")
        toast.error("加载 Session 详情失败")
      } finally {
        setLoading(false)
      }
    }

    fetchData()
  }, [sessionId])

  const handleDeleteSession = () => {
    setDeleteDialogOpen(true)
  }

  const confirmDeleteSession = async () => {
    try {
      await apiClient.delete(`/v1/session-recalls/session/${sessionId}`)
      toast.success("Session 记录已删除")
      navigate(-1)
    } catch (err) {
      console.error("Failed to delete session records:", err)
      toast.error("删除失败")
    } finally {
      setDeleteDialogOpen(false)
    }
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
    setSessionVaultUnlocked(true)
    if (pendingUnlockMemoryId) {
      setUnlockedMemories((prev) => new Set(prev).add(pendingUnlockMemoryId))
      setPendingUnlockMemoryId(null)
    }
    setVaultError(null)
    setShowVaultInput(false)
    setVaultPassword("")
  }

  const handleVaultLock = () => {
    setSessionVaultUnlocked(false)
    setUnlockedMemories(new Set())
    setManuallyLocked(new Set())
    vaultLock()
  }

  const handleToggleMemoryLock = (memoryId: string) => {
    const isCurrentlyUnlocked =
      unlockedMemories.has(memoryId) ||
      (sessionVaultUnlocked && !manuallyLocked.has(memoryId))

    if (isCurrentlyUnlocked) {
      setManuallyLocked((prev) => new Set(prev).add(memoryId))
      setUnlockedMemories((prev) => {
        const next = new Set(prev)
        next.delete(memoryId)
        return next
      })
    } else if (sessionVaultUnlocked) {
      setManuallyLocked((prev) => {
        const next = new Set(prev)
        next.delete(memoryId)
        return next
      })
    } else {
      setPendingUnlockMemoryId(memoryId)
      setShowVaultInput(true)
    }
  }

  const stats = {
    total: events.length,
    auto: events.filter((e) => e.recall_type === "auto").length,
    manual: events.filter((e) => e.recall_type === "manual").length,
    totalKept: events.reduce((sum, e) => sum + e.kept_count, 0),
    totalDiscarded: events.reduce((sum, e) => sum + e.discarded_count, 0),
  }

  if (loading) {
    return (
      <div className="space-y-6 max-w-3xl">
        <div className="flex items-center gap-2">
          <Skeleton className="h-8 w-8" />
          <Skeleton className="h-6 w-32" />
        </div>
        <Skeleton className="h-4 w-1/2" />
        <div className="space-y-4">
          {[1, 2, 3].map((n) => (
            <div key={n} className="flex gap-4">
              <Skeleton className="h-3 w-3 rounded-full" />
              <div className="flex-1 space-y-2">
                <Skeleton className="h-20 w-full" />
              </div>
            </div>
          ))}
        </div>
      </div>
    )
  }

  if (error) {
    return (
      <div className="space-y-4">
        <Button variant="ghost" size="sm" onClick={() => handleGoBack()}>
          <ArrowLeft className="size-4 mr-1.5" />
          返回
        </Button>
        <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-6 text-sm text-destructive">
          {error}
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6 max-w-3xl">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={() => handleGoBack()}>
            <ArrowLeft className="size-4 mr-1.5" />
            返回
          </Button>
        </div>
      </div>

      <div className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight">
          Session 详情 - {shortSessionId(sessionId)}
        </h1>
        <div className="flex items-center gap-3 text-sm text-muted-foreground flex-wrap">
          <code className="font-mono text-xs bg-muted px-1.5 py-0.5 rounded">
            {sessionId}
          </code>
        </div>
      </div>

      <div className="grid grid-cols-3 gap-4">
        <Card>
          <CardContent className="pt-4 text-center">
            <div className="text-2xl font-semibold">{stats.total}</div>
            <div className="text-xs text-muted-foreground">总召回事件</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-4 text-center">
            <div className="text-2xl font-semibold">{stats.auto}</div>
            <div className="text-xs text-muted-foreground">自动注入</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-4 text-center">
            <div className="text-2xl font-semibold">{stats.manual}</div>
            <div className="text-xs text-muted-foreground">手动注入</div>
          </CardContent>
        </Card>
      </div>

      {(stats.totalKept > 0 || stats.totalDiscarded > 0) && (
        <div className="flex items-center gap-4 text-sm">
          <span className="text-emerald-600 dark:text-emerald-400 font-medium">
            共保留 {stats.totalKept} 条记忆
          </span>
          <span className="text-red-600 dark:text-red-400 font-medium">
            共精炼掉 {stats.totalDiscarded} 条记忆
          </span>
        </div>
      )}

      <div className="space-y-4">
        <div className="flex items-center justify-between flex-wrap gap-2">
          <h2 className="text-sm font-medium text-muted-foreground">召回事件时间线</h2>
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground">
              共 {events.length} 个事件
            </span>
            {totalPages > 1 && (
              <div className="flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6"
                  onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
                  disabled={currentPage === 1}
                >
                  <ChevronLeft className="size-3" />
                </Button>
                <span className="text-xs text-muted-foreground min-w-[3ch] text-center">
                  {currentPage}/{totalPages}
                </span>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6"
                  onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
                  disabled={currentPage === totalPages}
                >
                  <ChevronRight className="size-3" />
                </Button>
              </div>
            )}
            {sessionVaultUnlocked ? (
              <Button variant="outline" size="sm" onClick={handleVaultLock}>
                <Lock className="size-3.5 mr-1" />
                锁定 Vault
              </Button>
            ) : (
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowVaultInput(!showVaultInput)}
              >
                <Unlock className="size-3.5 mr-1" />
                解锁 Vault
              </Button>
            )}
          </div>
        </div>

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

        {events.length === 0 ? (
          <div className="rounded-lg border border-border bg-card p-8 text-center text-sm text-muted-foreground">
            暂无召回事件
          </div>
        ) : (
          <>
            {paginatedEvents.map((event, index) => (
              <EventCard
                key={event.id}
                event={event}
                initialItems={eventItemsMap.get(event.id)}
                isLast={index === paginatedEvents.length - 1 && currentPage === totalPages}
                defaultExpanded={index === 0 && currentPage === 1}
                vaultUnlocked={sessionVaultUnlocked}
                unlockedMemories={unlockedMemories}
                manuallyLocked={manuallyLocked}
                onUnlock={handleToggleMemoryLock}
                onLock={handleToggleMemoryLock}
                onVaultUnlock={() => setSessionVaultUnlocked(true)}
              />
            ))}

            {totalPages > 1 && (
              <div className="flex items-center justify-between gap-2 pt-4">
                <span className="text-xs text-muted-foreground">
                  共 {events.length} 个事件
                </span>
                <div className="flex items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
                    disabled={currentPage === 1}
                  >
                    <ChevronLeft className="size-4" />
                  </Button>
                  <span className="text-sm text-muted-foreground">
                    {currentPage} / {totalPages}
                  </span>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
                    disabled={currentPage === totalPages}
                  >
                    <ChevronRight className="size-4" />
                  </Button>
                </div>
              </div>
            )}
          </>
        )}
      </div>

      {events.length > 0 && (
        <div className="pt-4 border-t border-border">
          <Button
            variant="outline"
            size="sm"
            className="text-destructive hover:bg-destructive/10 hover:text-destructive"
            onClick={handleDeleteSession}
          >
            <Trash2 className="size-3.5 mr-1" />
            删除 Session 所有记录
          </Button>
        </div>
      )}

      <AlertDialog open={deleteDialogOpen} onOpenChange={(open) => !open && setDeleteDialogOpen(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除 Session 记录</AlertDialogTitle>
            <AlertDialogDescription>
              此操作将删除该 Session 的所有召回记录，不可撤销。确定要继续吗？
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setDeleteDialogOpen(false)}>取消</AlertDialogCancel>
            <AlertDialogAction
              onClick={confirmDeleteSession}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
