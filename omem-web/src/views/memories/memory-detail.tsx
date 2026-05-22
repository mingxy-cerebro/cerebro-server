import { useEffect, useState, useRef, useMemo } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import ForceGraph2D from "react-force-graph-2d"

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
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { Separator } from "@/components/ui/separator"
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
  Card,
  CardHeader,
  CardTitle,
  CardContent,
} from "@/components/ui/card"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import apiClient from "@/api/client"
import { useVaultStore } from "@/stores/vault"
import { toast } from "sonner"
import {
  isPrivateMemory,
  getTagClassName,
  getTierLabel,
  getTierBadgeClass,
  PRIVATE_TAG,
} from "@/lib/tag-utils"
import {
  ArrowLeft,
  Lock,
  Calendar,
  Hash,
  Tag,
  Eye,
  Unlock,
  Layers,
  Info,
  BarChart3,
  Globe,
  Clock,
  ArrowUpRight,
  ArrowDownRight,
  Trash2,
  Edit3,
  Link2,
} from "lucide-react"

interface MemoryRelation {
  relation_type: string
  target_id: string
  context_label?: string
}

interface MemoryDetail {
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
  source: string
  agent_id: string
  session_id: string
  tier_history?: string
  visibility?: string
  created_at: string
  updated_at: string
  project_path?: string
  relations?: MemoryRelation[]
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

function VaultUnlock() {
  const [password, setPassword] = useState("")
  const [error, setError] = useState<string | null>(null)
  const hasPassword = useVaultStore((s) => s.hasPassword)
  const unlock = useVaultStore((s) => s.unlock)

  const handleSubmit = async () => {
    if (!password.trim()) {
      setError("请输入密码")
      return
    }
    if (!hasPassword) {
      setError("您还没设置密码，请先设置密码")
      return
    }
    const success = await unlock(password)
    if (!success) {
      setError("密码错误")
    }
  }

  return (
    <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-8 text-center space-y-4">
      <Lock className="h-10 w-10 text-amber-500 mx-auto" />
      <h3 className="text-lg font-semibold text-amber-500">
        {hasPassword ? "Vault 已锁定" : "无法解锁"}
      </h3>
      <p className="text-sm text-muted-foreground max-w-xs mx-auto">
        {hasPassword
          ? "此记忆已加密，请输入 Vault 密码查看"
          : "您尚未设置 Vault 密码，请先前往设置"}
      </p>
      <div className="flex items-center gap-2 max-w-xs mx-auto">
        <Input
          type="password"
          placeholder={hasPassword ? "输入密码..." : "请先设置密码..."}
          value={password}
          onChange={(e) => {
            setPassword(e.target.value)
            setError(null)
          }}
          onKeyDown={(e) => e.key === "Enter" && handleSubmit()}
          className={error ? "border-destructive" : ""}
          disabled={!hasPassword}
        />
        <Button size="sm" onClick={handleSubmit} disabled={!hasPassword}>
          解锁
        </Button>
      </div>
      {error && (
        <p className="text-xs text-destructive">{error}</p>
      )}
    </div>
  )
}

function ContentTabs({ memory }: { memory: MemoryDetail }) {
  const levels: { key: keyof MemoryDetail; label: string }[] = [
    { key: "l0_abstract", label: "摘要" },
    { key: "l1_overview", label: "概览" },
    { key: "l2_content", label: "详情" },
    { key: "content", label: "原文" },
  ]

  const available = levels.filter((l) => {
    const v = memory[l.key]
    return v !== undefined && v !== null && String(v).trim().length > 0
  })

  const storageKey = `omem-memory-tab-${memory.id}`
  const [activeTab, setActiveTab] = useState(() => {
    try {
      const saved = sessionStorage.getItem(storageKey)
      if (saved && available.some((l) => l.key === saved)) return saved
    } catch {}
    return (
      available.find((l) => l.key === "content")?.key ||
      available.find((l) => l.key === "l2_content")?.key ||
      available[0]?.key
    )
  })

  useEffect(() => {
    if (!activeTab || !available.some((l) => l.key === activeTab)) {
      setActiveTab(available[0]?.key)
    }
  }, [available, activeTab])

  if (available.length === 0) {
    return (
      <div className="rounded-lg border border-border bg-card p-4 text-sm text-muted-foreground">
        无内容
      </div>
    )
  }

  if (available.length === 1) {
    const content = memory[available[0].key] as string
    const isEmpty = !content || content.trim().length === 0
    return (
      <div className="rounded-lg border border-border bg-card p-4 prose prose-sm dark:prose-invert max-w-none break-words">
        {isEmpty ? (
          <div className="text-sm text-muted-foreground">无内容</div>
        ) : (
          <ReactMarkdown remarkPlugins={[remarkGfm]}>
            {formatMarkdownContent(content)}
          </ReactMarkdown>
        )}
      </div>
    )
  }

  return (
    <Tabs
      value={activeTab}
      onValueChange={(value) => {
        setActiveTab(value)
        try {
          sessionStorage.setItem(storageKey, value)
        } catch {}
      }}
      className="w-full"
    >
      <TabsList className="mb-2">
        {available.map((l) => (
          <TabsTrigger key={l.key} value={l.key}>
            <Layers className="size-3 mr-1" />
            {l.label}
          </TabsTrigger>
        ))}
      </TabsList>
      {available.map((l) => {
        const content = memory[l.key] as string
        const isEmpty = !content || content.trim().length === 0
        return (
          <TabsContent key={l.key} value={l.key}>
            <div className="rounded-lg border border-border bg-card p-4 prose prose-sm dark:prose-invert max-w-none break-words">
              {isEmpty ? (
                <div className="text-sm text-muted-foreground">无内容</div>
              ) : (
                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {formatMarkdownContent(content)}
                </ReactMarkdown>
              )}
            </div>
          </TabsContent>
        )
      })}
    </Tabs>
  )
}

function MetaItem({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <span className="text-xs text-muted-foreground">{label}</span>
      <div className="text-sm font-medium">{value}</div>
    </div>
  )
}

const RELATION_COLORS: Record<string, string> = {
  supersedes: '#ef4444',
  contextualizes: '#3b82f6',
  supports: '#10b981',
  contradicts: '#a855f7',
  continues: '#06b6d4',
  continued_by: '#06b6d4',
}

function RelationGraph({ memoryId, relations }: { memoryId: string; relations: MemoryRelation[] }) {
  const graphRef = useRef<HTMLDivElement>(null)
  const fgRef = useRef<any>(null)
  const navigate = useNavigate()

  const graphData = useMemo(() => {
    const uniqueTargets = new Map<string, { id: string; name: string; isCenter: boolean }>()
    relations.forEach((r) => {
      if (!uniqueTargets.has(r.target_id)) {
        uniqueTargets.set(r.target_id, {
          id: r.target_id,
          name: r.target_id.slice(0, 8),
          isCenter: false,
        })
      }
    })

    const nodes = [
      { id: memoryId, name: memoryId.slice(0, 8), isCenter: true },
      ...Array.from(uniqueTargets.values()),
    ]

    const links = relations.map((r) => ({
      source: memoryId,
      target: r.target_id,
      color: RELATION_COLORS[r.relation_type] || '#6b7280',
      relationType: r.relation_type,
      contextLabel: r.context_label,
    }))

    return { nodes, links }
  }, [memoryId, relations])

  return (
    <div ref={graphRef} className="h-[300px] w-full">
      <ForceGraph2D
        ref={fgRef}
        graphData={graphData}
        nodeCanvasObject={(node, ctx, globalScale) => {
          const x = node.x ?? 0
          const y = node.y ?? 0
          if (!isFinite(x) || !isFinite(y)) return
          const isCenter = (node as Record<string, unknown>).isCenter as boolean
          const radius = isCenter ? 8 : 5

          // Glow for center node
          if (isCenter) {
            const glow = ctx.createRadialGradient(x, y, radius * 0.5, x, y, radius * 2.5)
            glow.addColorStop(0, 'rgba(245,158,11,0.25)')
            glow.addColorStop(1, 'rgba(245,158,11,0)')
            ctx.fillStyle = glow
            ctx.beginPath()
            ctx.arc(x, y, radius * 2.5, 0, Math.PI * 2)
            ctx.fill()
          }

          // Node body
          if (isCenter) {
            const grad = ctx.createRadialGradient(x - radius * 0.3, y - radius * 0.3, 0, x, y, radius)
            grad.addColorStop(0, '#fbbf24')
            grad.addColorStop(1, '#f97316')
            ctx.fillStyle = grad
            ctx.strokeStyle = '#f59e0b'
            ctx.lineWidth = 2
          } else {
            ctx.fillStyle = '#3b82f6'
            ctx.strokeStyle = '#60a5fa'
            ctx.lineWidth = 1.5
          }

          ctx.beginPath()
          ctx.arc(x, y, radius, 0, Math.PI * 2)
          ctx.fill()
          ctx.stroke()

          // Label
          const name = (node as Record<string, unknown>).name as string
          const fontSize = Math.max(8 / globalScale, 6)
          ctx.font = `${fontSize}px sans-serif`
          ctx.textAlign = 'center'
          ctx.textBaseline = 'bottom'
          ctx.fillStyle = isCenter ? '#fbbf24' : '#94a3b8'
          ctx.fillText(name, x, y - radius - 4 / globalScale)
        }}
        nodePointerAreaPaint={(node, color, ctx) => {
          const x = node.x ?? 0
          const y = node.y ?? 0
          if (!isFinite(x) || !isFinite(y)) return
          const isCenter = (node as Record<string, unknown>).isCenter as boolean
          const r = isCenter ? 12 : 10
          ctx.fillStyle = color
          ctx.beginPath()
          ctx.arc(x, y, r, 0, Math.PI * 2)
          ctx.fill()
        }}
        nodeLabel={(node) => {
          const n = node as Record<string, unknown>
          if (n.isCenter) return `当前记忆: ${n.name as string}`
          const link = graphData.links.find((l) => {
            const targetId = typeof l.target === 'string' ? l.target : (l.target as Record<string, unknown>).id
            return targetId === n.id
          })
          if (!link) return n.name as string
          const parts = [`ID: ${n.name as string}`]
          if (link.relationType) parts.push(`关系: ${link.relationType}`)
          if (link.contextLabel) parts.push(link.contextLabel)
          return parts.join(' | ')
        }}
        linkDirectionalArrowLength={4}
        linkDirectionalArrowRelPos={1}
        linkCurvature={0.15}
        linkColor={(link) => (link as Record<string, unknown>).color as string}
        linkWidth={1.5}
        backgroundColor="transparent"
        enableNodeDrag={false}
        enableZoomInteraction={true}
        enablePanInteraction={true}
        nodeRelSize={0}
        cooldownTime={2000}
        onEngineStop={() => {
          if (fgRef.current) {
            fgRef.current.zoomToFit(0, 30)
            fgRef.current.zoom(fgRef.current.zoom() * 0.65, 0)
          }
        }}
        onNodeClick={(node) => {
          const n = node as Record<string, unknown>
          if (!n.isCenter && n.id) {
            navigate(`/memories/${n.id as string}`)
          }
        }}
      />
    </div>
  )
}

function TierHistoryTimeline({ memory }: { memory: MemoryDetail }) {
  const tierOrder: Record<string, number> = { peripheral: 0, working: 1, core: 2 }
  const reasonMap: Record<string, string> = {
    access_via_get: "访问触发",
    access_via_search: "搜索触发",
    access_via_recall: "召回触发",
    access_via_cross_space_search: "跨空间搜索触发",
    scheduled_evaluation: "定时评估",
  }

  let events: Array<{
    from: string
    to: string
    reason: string
    at: string
    access_count: number
  }> = []

  if (memory.tier_history) {
    try {
      events = JSON.parse(memory.tier_history)
    } catch {
      /* ignore */
    }
  }

  if (events.length === 0) return null

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="flex items-center gap-1.5 text-base">
          <Clock className="size-4 text-muted-foreground" />
          等级变更历史
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="overflow-x-auto">
          <div className="flex items-start min-w-max">
            {events.map((e, i) => {
              const promoted =
                (tierOrder[e.from] ?? 0) < (tierOrder[e.to] ?? 0)
              return (
                <div
                  key={e.at + e.from + e.to}
                  className="flex flex-col items-center"
                  style={{ minWidth: '140px' }}
                >
                  <div className="text-[11px] text-muted-foreground mb-3 whitespace-nowrap text-center">
                    {new Date(e.at).toLocaleDateString('zh-CN', {
                      month: '2-digit',
                      day: '2-digit',
                    })}
                    <span className="text-[10px] ml-1 opacity-70">
                      {new Date(e.at).toLocaleTimeString('zh-CN', {
                        hour: '2-digit',
                        minute: '2-digit',
                      })}
                    </span>
                  </div>

                  <div className="flex items-center w-full">
                    {i > 0 && (
                      <div className="flex-1 h-px bg-border" />
                    )}
                    <div
                      className={`size-3 rounded-full border-2 shrink-0 z-10 ring-2 ring-background ${
                        promoted
                          ? 'bg-emerald-500/20 border-emerald-500'
                          : 'bg-red-500/20 border-red-500'
                      }`}
                    />
                    {i < events.length - 1 && (
                      <div className="flex-1 h-px bg-border" />
                    )}
                  </div>

                  <div className="mt-3 flex flex-col items-center gap-1.5">
                    <div className="flex items-center gap-1">
                      {promoted ? (
                        <ArrowUpRight className="size-3.5 text-emerald-500 shrink-0" />
                      ) : (
                        <ArrowDownRight className="size-3.5 text-red-500 shrink-0" />
                      )}
                      <Badge
                        variant="outline"
                        className={`text-[10px] px-1.5 py-0 ${getTierBadgeClass(e.from)}`}
                      >
                        {getTierLabel(e.from)}
                      </Badge>
                      <span className="text-muted-foreground text-[10px]">→</span>
                      <Badge
                        variant="outline"
                        className={`text-[10px] px-1.5 py-0 ${getTierBadgeClass(e.to)}`}
                      >
                        {getTierLabel(e.to)}
                      </Badge>
                    </div>
                    <p className="text-[10px] text-muted-foreground text-center">
                      {reasonMap[e.reason] || e.reason} · #{e.access_count}
                    </p>
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

export function MemoryDetailPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const location = useLocation()

  const handleGoBack = () => {
    const fromState = location.state as { from?: string } | null
    const rawFrom = fromState?.from || sessionStorage.getItem("memories-list-from") || "/memories"
    const from = rawFrom.startsWith("/") ? rawFrom : `/${rawFrom}`
    navigate(from)
  }
  const [memory, setMemory] = useState<MemoryDetail | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [isDeleting, setIsDeleting] = useState(false)
  const [isEditingScope, setIsEditingScope] = useState(false)
  const [isUpdatingScope, setIsUpdatingScope] = useState(false)
  const vaultUnlocked = useVaultStore((s) => s.isUnlocked)
  const vaultLock = useVaultStore((s) => s.lock)

  useEffect(() => {
    if (!id) return

    async function fetchMemory() {
      try {
        setLoading(true)
        setError(null)
        const response = await apiClient.get<MemoryDetail>(`/v1/memories/${id}`, { params: { skip_access: true } })
        console.log("Memory detail raw response:", response)
        // 兼容后端可能返回的不同字段名
        const mapped: MemoryDetail = {
          ...response,
          l0_abstract: (response as any).l0_abstract ?? (response as any).abstract ?? "",
          l1_overview: (response as any).l1_overview ?? (response as any).overview ?? "",
          l2_content: (response as any).l2_content ?? (response as any).detail ?? "",
        }
        setMemory(mapped)
      } catch (err) {
        console.error("Failed to fetch memory:", err)
        setError("加载记忆详情失败")
      } finally {
        setLoading(false)
      }
    }

    fetchMemory()
  }, [id])

  if (loading) {
    return (
    <div className="space-y-6 max-w-6xl mx-auto">
        <div className="flex items-center gap-2">
          <Skeleton className="h-8 w-8" />
          <Skeleton className="h-6 w-32" />
        </div>
        <Skeleton className="h-8 w-3/4" />
        <Skeleton className="h-4 w-1/2" />
        <div className="space-y-3">
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-4/5" />
        </div>
      </div>
    )
  }

  if (error || !memory) {
    return (
      <div className="space-y-4">
        <Button variant="ghost" size="sm" onClick={() => handleGoBack()}>
          <ArrowLeft className="size-4 mr-1.5" />
          返回
        </Button>
        <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-6 text-sm text-destructive">
          {error || "记忆不存在"}
        </div>
      </div>
    )
  }

  const isPrivate = isPrivateMemory(memory.tags, memory.visibility)
  const showContent = !isPrivate || vaultUnlocked

  const handleUpdateScope = async (newScope: string | null) => {
    if (!memory || !newScope || newScope === memory.scope) {
      setIsEditingScope(false)
      return
    }
    setIsUpdatingScope(true)
    try {
      await apiClient.post("/v1/memories/batch-visibility", {
        memory_ids: [memory.id],
        scope: newScope,
      })
      setMemory((prev) => (prev ? { ...prev, scope: newScope } : null))
      toast.success("范围已更新")
    } catch (err) {
      console.error("Update scope failed:", err)
      toast.error("更新失败，请重试")
    } finally {
      setIsUpdatingScope(false)
      setIsEditingScope(false)
    }
  }

  return (
    <div className="space-y-6 max-w-6xl mx-auto">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={() => handleGoBack()}>
            <ArrowLeft className="size-4 mr-1.5" />
            返回
          </Button>
          {isPrivate && (
            <Badge
              variant="outline"
              className="border-amber-500/50 text-amber-500 bg-amber-500/10"
            >
              <Lock className="size-3 mr-1" />
              私密记忆
            </Badge>
          )}
          <Badge
            variant="outline"
            className={getTierBadgeClass(memory.tier)}
          >
            {getTierLabel(memory.tier)}
          </Badge>
        </div>
        <div className="flex items-center gap-2">
          {showContent && (
            <Button
              variant="ghost"
              size="sm"
              className="text-muted-foreground hover:text-destructive"
              onClick={() => setDeleteDialogOpen(true)}
            >
              <Trash2 className="size-3.5 mr-1.5" />
              删除
            </Button>
          )}
          {vaultUnlocked && isPrivate && (
            <Button variant="ghost" size="sm" onClick={() => {
              vaultLock()
            }}>
              <Lock className="size-3.5 mr-1.5" />
              锁定
            </Button>
          )}
        </div>
      </div>

      <div className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight">
          {isPrivate && !showContent ? "🔒 私密记忆" : "记忆详情"}
        </h1>
        <div className="flex items-center gap-4 text-sm text-muted-foreground flex-wrap">
          <span className="flex items-center gap-1">
            <Calendar className="size-3.5" />
            {formatDate(memory.created_at)}
          </span>
          {memory.updated_at !== memory.created_at && (
            <span className="flex items-center gap-1">
              <Clock className="size-3.5" />
              {formatDate(memory.updated_at)}
            </span>
          )}
          <span className="flex items-center gap-1">
            <Eye className="size-3.5" />
            访问 {memory.access_count || 0} 次
          </span>
          <span className="flex items-center gap-1">
            <Hash className="size-3.5" />
            {memory.id.slice(0, 8)}...
          </span>
        </div>
      </div>

      <Separator />

      {showContent ? (
        <div className="flex gap-6">
          <div className="flex-1 min-w-0 space-y-6">
          <TierHistoryTimeline memory={memory} />
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-1.5 text-base">
                <Layers className="size-4 text-muted-foreground" />
                内容
                {isPrivate && (
                  <span className="flex items-center gap-1 text-xs text-amber-500 ml-auto font-normal">
                    <Unlock className="size-3" />
                    已解锁
                  </span>
                )}
              </CardTitle>
            </CardHeader>
            <CardContent>
              <ContentTabs memory={memory} />
            </CardContent>
          </Card>

          {memory.tags && memory.tags.length > 0 && (
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-1.5 text-base">
                  <Tag className="size-4 text-muted-foreground" />
                  标签
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="flex flex-wrap gap-2">
                  {memory.tags.map((tag) => (
                    <Badge
                      key={tag}
                      variant="outline"
                      className={getTagClassName(tag)}
                    >
                      {tag === PRIVATE_TAG && (
                        <Lock className="size-2.5 mr-1" />
                      )}
                      {tag}
                    </Badge>
                  ))}
                </div>
              </CardContent>
            </Card>
          )}

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-1.5 text-base">
                  <Info className="size-4 text-muted-foreground" />
                  基本信息
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <MetaItem label="分类" value={memory.category || "—"} />
                <MetaItem label="类型" value={memory.memory_type || "—"} />
                <MetaItem label="状态" value={memory.state || "—"} />
                <MetaItem
                  label="等级"
                  value={
                    <Badge variant="outline" className={getTierBadgeClass(memory.tier)}>
                      {getTierLabel(memory.tier)}
                    </Badge>
                  }
                />
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-1.5 text-base">
                  <BarChart3 className="size-4 text-muted-foreground" />
                  质量指标
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <MetaItem
                  label="重要性"
                  value={memory.importance?.toFixed(2) ?? "—"}
                />
                <MetaItem
                  label="置信度"
                  value={memory.confidence?.toFixed(2) ?? "—"}
                />
                <MetaItem
                  label="访问次数"
                  value={memory.access_count ?? "—"}
                />
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-1.5 text-base">
                  <Globe className="size-4 text-muted-foreground" />
                  来源信息
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <MetaItem label="来源" value={memory.source || "—"} />
                {memory.project_path && (
                  <MetaItem label="项目路径" value={memory.project_path} />
                )}
                <div className="space-y-1">
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-muted-foreground">范围</span>
                    {!isEditingScope ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-5 px-1.5 text-xs text-muted-foreground hover:text-foreground"
                        onClick={() => setIsEditingScope(true)}
                      >
                        <Edit3 className="size-3 mr-1" />
                        编辑
                      </Button>
                    ) : null}
                  </div>
                  {isEditingScope ? (
                    <Select
                      defaultValue={memory.scope || "global"}
                      onValueChange={handleUpdateScope}
                      disabled={isUpdatingScope}
                    >
                      <SelectTrigger className="h-8 text-sm">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="public">public</SelectItem>
                        <SelectItem value="private">private</SelectItem>
                        <SelectItem value="global">global</SelectItem>
                      </SelectContent>
                    </Select>
                  ) : (
                    <div className="text-sm font-medium">{memory.scope || "—"}</div>
                  )}
                </div>
                <MetaItem label="Agent ID" value={memory.agent_id || "—"} />
                <MetaItem label="Session ID" value={memory.session_id || "—"} />
              </CardContent>
            </Card>
           </div>
          </div>

          {memory.relations && memory.relations.length > 0 && (
            <div className="w-80 shrink-0">
              <Card className="h-full">
                <CardHeader>
                  <CardTitle className="flex items-center gap-1.5 text-base">
                    <Link2 className="size-4 text-muted-foreground" />
                    关联图谱
                  </CardTitle>
                </CardHeader>
                <CardContent className="p-2">
                  <RelationGraph
                    key={memory.id + memory.relations.length}
                    memoryId={memory.id}
                    relations={memory.relations}
                  />
                </CardContent>
              </Card>
            </div>
          )}
        </div>
      ) : (
        <VaultUnlock />
      )}

      <AlertDialog open={deleteDialogOpen} onOpenChange={(open) => { if (!open && !isDeleting) setDeleteDialogOpen(false) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除记忆</AlertDialogTitle>
            <AlertDialogDescription>确定要删除这条记忆吗？此操作不可撤销。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeleting}>取消</AlertDialogCancel>
            <AlertDialogAction
              disabled={isDeleting}
              onClick={async () => {
                setIsDeleting(true)
                try {
                  await apiClient.delete(`/v1/memories/${id}`)
                  toast.success("记忆已删除")
                  navigate("/memories")
                } catch (err) {
                  console.error("Delete failed:", err)
                  toast.error("删除失败，请重试")
                  setDeleteDialogOpen(false)
                } finally {
                  setIsDeleting(false)
                }
              }}
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
