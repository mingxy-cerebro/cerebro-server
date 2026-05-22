import { useEffect, useState } from "react"
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
} from "lucide-react"

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

          <div className="w-72 shrink-0">
              <Card className="h-full">
                <CardHeader>
                  <CardTitle className="flex items-center gap-1.5 text-base">
                    <Clock className="size-4 text-muted-foreground" />
                    等级变更历史
                  </CardTitle>
                </CardHeader>
                <CardContent className="overflow-y-auto flex-1">
                  {(() => {
                    const tierOrder = { peripheral: 0, working: 1, core: 2 }
                    const reasonMap: Record<string, string> = {
                      access_via_get: "访问触发",
                      access_via_search: "搜索触发",
                      access_via_recall: "召回触发",
                      access_via_cross_space_search: "跨空间搜索触发",
                      scheduled_evaluation: "定时评估",
                    }
                    let events: Array<{
                      from: string; to: string; reason: string; at: string; access_count: number
                    }> = []
                    if (memory.tier_history) {
                      try {
                        events = JSON.parse(memory.tier_history)
                      } catch { /* ignore parse errors */ }
                    }
                    if (events.length === 0) {
                      return <p className="text-sm text-muted-foreground py-8 text-center">暂无变更记录</p>
                    }
                    return (
                      <div className="relative pl-6">
                        <div className="absolute left-2 top-0 bottom-0 w-px bg-border" />
                        {events.reverse().map((e) => {
                          const promoted = (tierOrder[e.from as keyof typeof tierOrder] ?? 0) < (tierOrder[e.to as keyof typeof tierOrder] ?? 0)
                          return (
                            <div key={e.at + e.from + e.to} className="relative pb-4 last:pb-0">
                              <div className={`absolute -left-4 top-1 size-3 rounded-full border-2 ${promoted ? 'border-emerald-500 bg-emerald-500/20' : 'border-red-500 bg-red-500/20'}`} />
                              <div className="space-y-1">
                                <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                                  <span>{new Date(e.at).toLocaleDateString("zh-CN")}</span>
                                  <span className="text-[10px]">{new Date(e.at).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}</span>
                                </div>
                                <div className="flex items-center gap-1">
                                  {promoted ? (
                                    <ArrowUpRight className="size-3.5 text-emerald-500 shrink-0" />
                                  ) : (
                                    <ArrowDownRight className="size-3.5 text-red-500 shrink-0" />
                                  )}
                                  <Badge variant="outline" className={`text-[10px] px-1 py-0 ${getTierBadgeClass(e.from)}`}>
                                    {getTierLabel(e.from)}
                                  </Badge>
                                  <span className="text-muted-foreground text-xs">→</span>
                                  <Badge variant="outline" className={`text-[10px] px-1 py-0 ${getTierBadgeClass(e.to)}`}>
                                    {getTierLabel(e.to)}
                                  </Badge>
                                </div>
                                <p className="text-[10px] text-muted-foreground">
                                  {reasonMap[e.reason] || e.reason} · #{e.access_count}
                                </p>
                              </div>
                            </div>
                          )
                        })}
                      </div>
                    )
                  })()}
                </CardContent>
              </Card>
          </div>
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
