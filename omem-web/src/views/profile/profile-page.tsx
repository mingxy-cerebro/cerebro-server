import { useEffect, useMemo, useRef, useState } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { useNavigate } from "react-router-dom"
import { Card, CardContent } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import { Separator } from "@/components/ui/separator"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import apiClient from "@/api/client"
import { profileV2Api } from "@/api/profile-v2"
import { useVaultStore } from "@/stores/vault"
import {
  ArrowLeft,
  User,
  Sparkles,
  Lightbulb,
  BookOpen,
  Zap,
  Lock,
  ChevronDown,
  ChevronLeft,
  ChevronUp,
  ChevronRight,
  Clock,
  Plus,
  Trash2,
  Edit3,
  History,
  Layers,
  FolderOpen,
  Search,
  Shield,
  BarChart3,
  RefreshCw,
  EyeOff,
  Activity,
} from "lucide-react"
import { toast } from "sonner"
import { cn } from "@/lib/utils"
import type {
  PreferenceResponse,
  StatsResponse,
  ChangelogEntry,
  CreatePreferenceBody,
  UpdatePreferenceBody,
} from "@/types/profile-v2"

// ── v1 Profile types (legacy) ──────────────────────────────────────

interface StaticFact {
  content: string
  tags: string[]
  visibility: string
  l2_content?: string
}

interface ProfileData {
  dynamic_context: string[]
  search_results: string[] | null
  static_facts: StaticFact[]
}

type FactType = "fact" | "preference" | "skill" | "project" | "note" | "private"

function classifyFact(fact: string): {
  type: FactType
  icon: typeof Sparkles
  color: string
  bgColor: string
  label: string
} {
  const lower = fact.toLowerCase()
  if (lower.includes("喜欢") || lower.includes("偏好") || lower.includes("习惯")) {
    return { type: "preference", icon: Lightbulb, color: "text-amber-600 dark:text-amber-400", bgColor: "bg-amber-50 dark:bg-amber-500/10 border-amber-200 dark:border-amber-500/30", label: "偏好" }
  }
  if (lower.includes("项目") || lower.includes("工程") || lower.includes("开发")) {
    return { type: "project", icon: Zap, color: "text-blue-600 dark:text-blue-400", bgColor: "bg-blue-50 dark:bg-blue-500/10 border-blue-200 dark:border-blue-500/30", label: "项目" }
  }
  if (lower.includes("技能") || lower.includes("能力") || lower.includes("精通") || lower.includes("熟练")) {
    return { type: "skill", icon: Sparkles, color: "text-violet-600 dark:text-violet-400", bgColor: "bg-violet-50 dark:bg-violet-500/10 border-violet-200 dark:border-violet-500/30", label: "技能" }
  }
  if (lower.includes("笔记") || lower.includes("记录") || lower.includes("文档")) {
    return { type: "note", icon: BookOpen, color: "text-slate-600 dark:text-slate-400", bgColor: "bg-slate-50 dark:bg-slate-500/10 border-slate-200 dark:border-slate-500/30", label: "笔记" }
  }
  return { type: "fact", icon: User, color: "text-emerald-600 dark:text-emerald-400", bgColor: "bg-emerald-50 dark:bg-emerald-500/10 border-emerald-200 dark:border-emerald-500/30", label: "事实" }
}

function getPrivateMeta() {
  return { type: "private" as FactType, icon: Lock, color: "text-amber-600 dark:text-amber-400", bgColor: "bg-amber-50 dark:bg-amber-500/10 border-amber-200 dark:border-amber-500/30", label: "私密" }
}

function formatFact(fact: string): string {
  return fact.replace(/^#+\s*/, "").trim()
}

function isPrivateByTags(tags: string[], visibility?: string): boolean {
  if (visibility === "private") return true
  return tags.some((t) => t === "私密" || t.toLowerCase() === "private")
}

function formatDate(dateString: string) {
  return new Date(dateString).toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  })
}

function formatRelativeDate(dateString: string) {
  const now = Date.now()
  const date = new Date(dateString).getTime()
  const diff = now - date
  const mins = Math.floor(diff / 60000)
  if (mins < 1) return "刚刚"
  if (mins < 60) return `${mins} 分钟前`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours} 小时前`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days} 天前`
  return formatDate(dateString)
}

function ExpandableMarkdown({ content }: { content: string }) {
  const [expanded, setExpanded] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const [isOverflowing, setIsOverflowing] = useState(false)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver(() => setIsOverflowing(el.scrollHeight > el.clientHeight))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  return (
    <>
      <div className={cn("relative", !expanded && "max-h-60 overflow-hidden")}>
        <div ref={ref} className="prose prose-sm dark:prose-invert max-w-none">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
        </div>
        {!expanded && isOverflowing && (
          <div className="absolute bottom-0 left-0 right-0 h-10 bg-gradient-to-t from-background to-transparent pointer-events-none" />
        )}
      </div>
      {isOverflowing && (
        <Button
          variant="ghost"
          size="sm"
          className="mt-1 h-auto py-1 px-2 text-xs text-muted-foreground hover:text-foreground"
          onClick={() => setExpanded(!expanded)}
        >
          {expanded ? (
            <>
              <ChevronUp className="size-3 mr-1" />
              收起
            </>
          ) : (
            <>
              <ChevronDown className="size-3 mr-1" />
              展开全文
            </>
          )}
        </Button>
      )}
    </>
  )
}

// ── Built-in preference slots ──────────────────────────────────────

const BUILTIN_SLOTS = [
  { name: "communication_style", display: "沟通风格" },
  { name: "tone", display: "语气偏好" },
  { name: "code_style", display: "代码风格" },
  { name: "error_handling", display: "错误处理" },
  { name: "naming_convention", display: "命名规范" },
  { name: "testing_strategy", display: "测试策略" },
  { name: "workflow_preference", display: "工作流偏好" },
  { name: "commit_style", display: "提交风格" },
  { name: "emoji_preference", display: "Emoji偏好" },
  { name: "self_reference", display: "自称方式" },
  { name: "address_style", display: "称呼方式" },
  { name: "language", display: "语言" },
  { name: "framework_preference", display: "框架偏好" },
  { name: "preferred_tools", display: "工具偏好" },
] as const

// ── Slot display names ─────────────────────────────────────────────

const SLOT_LABELS: Record<string, string> = {
  communication_style: "沟通风格",
  coding_preferences: "编码偏好",
  project_context: "项目上下文",
  tool_preferences: "工具偏好",
  domain_knowledge: "领域知识",
  work_style: "工作风格",
  language_preference: "语言偏好",
  framework_preferences: "框架偏好",
}

const SLOT_ICONS: Record<string, typeof Layers> = {
  communication_style: Activity,
  coding_preferences: Zap,
  project_context: FolderOpen,
  tool_preferences: Layers,
  domain_knowledge: BookOpen,
  work_style: RefreshCw,
  language_preference: Lightbulb,
  framework_preferences: Sparkles,
}

function getSlotLabel(slot: string): string {
  return SLOT_LABELS[slot] || slot.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())
}

function getSlotIcon(slot: string): typeof Layers {
  return SLOT_ICONS[slot] || Layers
}

const SCOPE_STYLES: Record<string, { className: string; label: string }> = {
  global: { className: "bg-blue-50 dark:bg-blue-500/10 text-blue-700 dark:text-blue-300 border-blue-200 dark:border-blue-500/30", label: "全局" },
  project: { className: "bg-violet-50 dark:bg-violet-500/10 text-violet-700 dark:text-violet-300 border-violet-200 dark:border-violet-500/30", label: "项目" },
  session: { className: "bg-emerald-50 dark:bg-emerald-500/10 text-emerald-700 dark:text-emerald-300 border-emerald-200 dark:border-emerald-500/30", label: "会话" },
}

// ── Main Page ──────────────────────────────────────────────────────

export function ProfilePage() {
  const navigate = useNavigate()
  const vaultUnlock = useVaultStore((s) => s.unlock)
  const vaultHasPassword = useVaultStore((s) => s.hasPassword)
  const vaultIsUnlocked = useVaultStore((s) => s.isUnlocked)

  const [profile, setProfile] = useState<ProfileData | null>(null)
  const [stats, setStats] = useState<StatsResponse | null>(null)
  const [preferences, setPreferences] = useState<PreferenceResponse[]>([])
  const [changelog, setChangelog] = useState<ChangelogEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [activeTab, setActiveTab] = useState("preferences")
  const [projectPath, setProjectPath] = useState<string>("")

  const [vaultDialogOpen, setVaultDialogOpen] = useState(false)
  const [unlockTargetFact, setUnlockTargetFact] = useState<string | null>(null)
  const [unlockPassword, setUnlockPassword] = useState("")
  const [unlockError, setUnlockError] = useState<string | null>(null)
  const [isUnlocking, setIsUnlocking] = useState(false)

  const [unlockedFacts, setUnlockedFacts] = useState<Set<string>>(new Set())

  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [editingPref, setEditingPref] = useState<PreferenceResponse | null>(null)
  const [saving, setSaving] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<PreferenceResponse | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)

  const [searchQuery, setSearchQuery] = useState("")
  const [scopeFilter, setScopeFilter] = useState<string>("all")
  const [slotPage, setSlotPage] = useState(1)
  const SLOTS_PER_PAGE = 5

  useEffect(() => {
    async function loadAll() {
      try {
        setLoading(true)
        const pp = projectPath || undefined
        const [profileRes, statsRes, prefsRes, logRes] = await Promise.all([
          apiClient.get<ProfileData>("/v1/profile").catch(() => null),
          profileV2Api.getStats(pp).catch(() => null),
          profileV2Api.getPreferences(pp).catch(() => [] as PreferenceResponse[]),
          profileV2Api.getChangelog(pp).catch(() => [] as ChangelogEntry[]),
        ])
        if (profileRes) setProfile(profileRes)
        if (statsRes) setStats(statsRes)
        setPreferences(Array.isArray(prefsRes) ? prefsRes : [])
        setChangelog(Array.isArray(logRes) ? logRes : [])
      } catch (err) {
        console.error("Failed to load profile:", err)
        toast.error("加载画像数据失败")
      } finally {
        setLoading(false)
      }
    }
    loadAll()
  }, [projectPath])

  const handleVaultUnlock = async () => {
    if (!unlockPassword.trim()) {
      setUnlockError("请输入密码")
      return
    }
    setIsUnlocking(true)
    const success = await vaultUnlock(unlockPassword)
    setIsUnlocking(false)
    if (!success) {
      setUnlockError("密码错误")
      return
    }
    if (unlockTargetFact) {
      setUnlockedFacts((prev) => new Set(prev).add(unlockTargetFact))
    }
    setVaultDialogOpen(false)
    setUnlockTargetFact(null)
    setUnlockPassword("")
    setUnlockError(null)
    toast.success("已解锁")
  }

  const requestUnlock = (fact: string) => {
    if (vaultIsUnlocked) {
      setUnlockedFacts((prev) => new Set(prev).add(fact))
      return
    }
    if (!vaultHasPassword) {
      toast.error("请先在设置中设置密码")
      return
    }
    setUnlockTargetFact(fact)
    setUnlockPassword("")
    setUnlockError(null)
    setVaultDialogOpen(true)
  }

  const staticFacts = profile?.static_facts || []
  const dynamicContext = profile?.dynamic_context || []

  const factsWithMeta = useMemo(() => {
    return staticFacts.map((factObj) => {
      const tags = factObj.tags || []
      const isPrivate = isPrivateByTags(tags, factObj.visibility)
      const meta = isPrivate ? getPrivateMeta() : classifyFact(factObj.content)
      return { fact: factObj.content, factObj, isPrivate, meta }
    })
  }, [staticFacts])

  const preferencesBySlot = useMemo(() => {
    const map = new Map<string, PreferenceResponse[]>()
    for (const p of preferences) {
      if (scopeFilter !== "all" && p.scope !== scopeFilter) continue
      if (searchQuery.trim()) {
        const q = searchQuery.toLowerCase()
        if (
          !p.value.toLowerCase().includes(q) &&
          !p.slot.toLowerCase().includes(q) &&
          !(p.project_path || "").toLowerCase().includes(q)
        ) {
          continue
        }
      }
      const arr = map.get(p.slot) || []
      arr.push(p)
      map.set(p.slot, arr)
    }
    return map
  }, [preferences, searchQuery, scopeFilter])

  const allSlotEntries = useMemo(() => Array.from(preferencesBySlot.entries()), [preferencesBySlot])

  const totalSlotPages = Math.max(1, Math.ceil(allSlotEntries.length / SLOTS_PER_PAGE))

  const paginatedSlotEntries = useMemo(() => {
    const start = (slotPage - 1) * SLOTS_PER_PAGE
    return allSlotEntries.slice(start, start + SLOTS_PER_PAGE)
  }, [allSlotEntries, slotPage])

  const refreshPreferences = async () => {
    const pp = projectPath || undefined
    const [prefsRes, statsRes] = await Promise.all([
      profileV2Api.getPreferences(pp),
      profileV2Api.getStats(pp).catch(() => null),
    ])
    setPreferences(Array.isArray(prefsRes) ? prefsRes : [])
    if (statsRes) setStats(statsRes)
  }

  const handleCreate = async (data: CreatePreferenceBody | UpdatePreferenceBody) => {
    if (!("slot" in data) || !data.slot) return
    try {
      setSaving(true)
      await profileV2Api.createPreference(data)
      toast.success("偏好已创建")
      setCreateDialogOpen(false)
      await refreshPreferences()
    } catch {
      toast.error("创建失败")
    } finally {
      setSaving(false)
    }
  }

  const handleUpdate = async (id: string, data: UpdatePreferenceBody) => {
    try {
      setSaving(true)
      await profileV2Api.updatePreference(id, data)
      toast.success("偏好已更新")
      setEditingPref(null)
      const pp = projectPath || undefined
      const prefsRes = await profileV2Api.getPreferences(pp)
      setPreferences(Array.isArray(prefsRes) ? prefsRes : [])
    } catch {
      toast.error("更新失败")
    } finally {
      setSaving(false)
    }
  }

  const handleDelete = async () => {
    if (!deleteTarget) return
    try {
      setIsDeleting(true)
      await profileV2Api.deletePreference(deleteTarget.id)
      toast.success("偏好已删除")
      setDeleteTarget(null)
      await refreshPreferences()
    } catch {
      toast.error("删除失败")
    } finally {
      setIsDeleting(false)
    }
  }

  const allProjectPaths = useMemo(() => {
    const set = new Set<string>()
    for (const p of preferences) {
      if (p.project_path) set.add(p.project_path)
    }
    return Array.from(set).sort()
  }, [preferences])

  const totalPrefs = preferences.length

  return (
    <div className="min-h-screen bg-gradient-to-b from-background via-background to-muted/30">
      <div className="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-6 space-y-6">
        {/* Back button */}
        <Button variant="ghost" size="sm" className="text-muted-foreground hover:text-foreground -ml-2" onClick={() => navigate(-1)}>
          <ArrowLeft className="size-4 mr-1.5" />
          返回
        </Button>

        {/* ── Hero Section ──────────────────────────────────────── */}
        <div className="relative overflow-hidden rounded-2xl bg-gradient-to-br from-amber-50 via-orange-50 to-rose-50 dark:from-amber-500/10 dark:via-orange-500/5 dark:to-rose-500/10 border border-amber-200/50 dark:border-amber-500/20 p-6 md:p-8">
          {/* Decorative blobs */}
          <div className="absolute top-0 right-0 -mt-8 -mr-8 w-48 h-48 bg-amber-400/15 dark:bg-amber-500/10 rounded-full blur-3xl" />
          <div className="absolute bottom-0 left-0 -mb-6 -ml-6 w-36 h-36 bg-orange-400/10 dark:bg-orange-500/8 rounded-full blur-3xl" />

          <div className="relative z-10 flex flex-col md:flex-row md:items-center md:justify-between gap-4">
            <div className="flex items-center gap-4">
              <div className="flex items-center justify-center size-14 rounded-2xl bg-gradient-to-br from-amber-400 to-orange-500 shadow-lg shadow-amber-500/25">
                <User className="size-7 text-white" />
              </div>
              <div>
                <h1 className="text-2xl md:text-3xl font-bold tracking-tight text-foreground">用户画像</h1>
                <p className="text-sm text-muted-foreground mt-0.5">偏好 · 记忆 · 洞察</p>
              </div>
            </div>

            {/* Stats cards */}
            <div className="flex flex-wrap gap-3">
              <div className="flex items-center gap-2.5 px-4 py-2.5 rounded-xl bg-white/80 dark:bg-card/80 border border-amber-200/60 dark:border-amber-500/20 shadow-sm">
                <div className="flex items-center justify-center size-8 rounded-lg bg-amber-100 dark:bg-amber-500/15">
                  <Layers className="size-4 text-amber-600 dark:text-amber-400" />
                </div>
                <div>
                  <p className="text-lg font-bold leading-none text-foreground">{totalPrefs}</p>
                  <p className="text-[11px] text-muted-foreground">条偏好</p>
                </div>
              </div>
              <div className="flex items-center gap-2.5 px-4 py-2.5 rounded-xl bg-white/80 dark:bg-card/80 border border-orange-200/60 dark:border-orange-500/20 shadow-sm">
                <div className="flex items-center justify-center size-8 rounded-lg bg-orange-100 dark:bg-orange-500/15">
                  <BarChart3 className="size-4 text-orange-600 dark:text-orange-400" />
                </div>
                <div>
                  <p className="text-lg font-bold leading-none text-foreground">{stats?.total ?? 0}</p>
                  <p className="text-[11px] text-muted-foreground">归纳总量</p>
                </div>
              </div>
              {stats?.last_induction_at && (
                <div className="flex items-center gap-2.5 px-4 py-2.5 rounded-xl bg-white/80 dark:bg-card/80 border border-rose-200/60 dark:border-rose-500/20 shadow-sm">
                  <div className="flex items-center justify-center size-8 rounded-lg bg-rose-100 dark:bg-rose-500/15">
                    <Clock className="size-4 text-rose-600 dark:text-rose-400" />
                  </div>
                  <div>
                    <p className="text-sm font-semibold leading-none text-foreground">{formatRelativeDate(stats.last_induction_at)}</p>
                    <p className="text-[11px] text-muted-foreground">最近归纳</p>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>

        {/* ── Tabs ─────────────────────────────────────────────── */}
        <Tabs value={activeTab} onValueChange={setActiveTab} className="space-y-4">
          <TabsList className="bg-muted/60 p-1 h-auto">
            <TabsTrigger
              value="preferences"
              className="data-[state=active]:bg-background data-[state=active]:shadow-sm gap-1.5 px-4 py-2 text-sm"
            >
              <Sparkles className="size-3.5" />
              偏好管理
            </TabsTrigger>
            <TabsTrigger
              value="overview"
              className="data-[state=active]:bg-background data-[state=active]:shadow-sm gap-1.5 px-4 py-2 text-sm"
            >
              <User className="size-3.5" />
              画像概览
            </TabsTrigger>
            <TabsTrigger
              value="changelog"
              className="data-[state=active]:bg-background data-[state=active]:shadow-sm gap-1.5 px-4 py-2 text-sm"
            >
              <History className="size-3.5" />
              变更历史
            </TabsTrigger>
          </TabsList>

          {/* ── Tab: Preferences ──────────────────────────────── */}
          <TabsContent value="preferences" className="space-y-4">
            {/* Toolbar */}
            <div className="flex flex-col sm:flex-row items-start sm:items-center gap-3">
              <div className="relative flex-1 w-full sm:max-w-xs">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
                <Input
                  placeholder="搜索偏好..."
                  value={searchQuery}
                  onChange={(e) => { setSearchQuery(e.target.value); setSlotPage(1) }}
                  className="pl-9 h-9 bg-muted/40"
                />
              </div>

              <div className="flex items-center gap-2">
                {allProjectPaths.length > 0 && (
                  <Select value={projectPath} onValueChange={(v) => v != null && setProjectPath(v)}>
                    <SelectTrigger className="h-9 w-[180px] text-xs">
                      <FolderOpen className="size-3.5 mr-1.5 text-muted-foreground" />
                      <SelectValue placeholder="全部项目" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__all__">全部项目</SelectItem>
                      {allProjectPaths.map((p) => (
                        <SelectItem key={p} value={p}>
                          {p.split("/").pop() || p}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}

                <Select value={scopeFilter} onValueChange={(v) => { if (v != null) { setScopeFilter(v); setSlotPage(1) } }}>
                  <SelectTrigger className="h-9 w-[100px] text-xs">
                    <SelectValue placeholder="范围" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">全部范围</SelectItem>
                    <SelectItem value="global">全局</SelectItem>
                    <SelectItem value="project">项目</SelectItem>
                    <SelectItem value="session">会话</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <Button
                className="ml-auto h-9 gap-1.5 bg-gradient-to-r from-amber-500 to-orange-500 hover:from-amber-600 hover:to-orange-600 text-white shadow-sm"
                onClick={() => setCreateDialogOpen(true)}
              >
                <Plus className="size-4" />
                新建偏好
              </Button>
            </div>

            {loading ? (
              <div className="space-y-4">
                {[1, 2, 3].map((n) => (
                  <div key={n} className="space-y-2">
                    <Skeleton className="h-5 w-24 rounded-md" />
                    <Skeleton className="h-24 w-full rounded-xl" />
                  </div>
                ))}
              </div>
            ) : preferencesBySlot.size === 0 ? (
              <Card className="border-dashed">
                <CardContent className="py-16 flex flex-col items-center gap-3 text-muted-foreground">
                  <div className="flex items-center justify-center size-12 rounded-2xl bg-muted">
                    <Sparkles className="size-6" />
                  </div>
                  <div className="text-center">
                    <p className="text-sm font-medium">暂无偏好数据</p>
                    <p className="text-xs mt-1">点击「新建偏好」开始构建你的画像</p>
                  </div>
                </CardContent>
              </Card>
            ) : (
              <>
              <div id="pref-slot-list" className="space-y-4">
                {paginatedSlotEntries.map(([slot, prefs]) => (
                  <SlotGroup
                    key={slot}
                    slot={slot}
                    preferences={prefs}
                    onEdit={(pref) => setEditingPref(pref)}
                    onDelete={(pref) => setDeleteTarget(pref)}
                  />
                ))}
              </div>
              {totalSlotPages > 1 && (
                <div className="flex items-center justify-between pt-2">
                  <p className="text-xs text-muted-foreground">
                    第 {slotPage} / {totalSlotPages} 页 · 共 {allSlotEntries.length} 个分组
                  </p>
                  <div className="flex items-center gap-1.5">
                    <Button
                      variant="outline"
                      size="icon"
                      className="size-8"
                      disabled={slotPage <= 1}
                      onClick={() => {
                        setSlotPage(p => Math.max(1, p - 1))
                        document.getElementById('pref-slot-list')?.scrollIntoView({ behavior: 'smooth', block: 'start' })
                      }}
                    >
                      <ChevronLeft className="size-4" />
                    </Button>
                    {Array.from({ length: totalSlotPages }, (_, i) => i + 1).map(page => (
                      <Button
                        key={page}
                        variant={page === slotPage ? "default" : "outline"}
                        size="icon"
                        className={cn("size-8 text-xs", page === slotPage && "bg-gradient-to-r from-amber-500 to-orange-500 text-white border-0")}
                        onClick={() => {
                          setSlotPage(page)
                          document.getElementById('pref-slot-list')?.scrollIntoView({ behavior: 'smooth', block: 'start' })
                        }}
                      >
                        {page}
                      </Button>
                    ))}
                    <Button
                      variant="outline"
                      size="icon"
                      className="size-8"
                      disabled={slotPage >= totalSlotPages}
                      onClick={() => {
                        setSlotPage(p => Math.min(totalSlotPages, p + 1))
                        document.getElementById('pref-slot-list')?.scrollIntoView({ behavior: 'smooth', block: 'start' })
                      }}
                    >
                      <ChevronRight className="size-4" />
                    </Button>
                  </div>
                </div>
              )}
              </>
            )}
          </TabsContent>

          {/* ── Tab: Overview ─────────────────────────────────── */}
          <TabsContent value="overview" className="space-y-6">
            {loading ? (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {[1, 2, 3, 4].map((n) => (
                  <Skeleton key={n} className="h-48 w-full rounded-xl" />
                ))}
              </div>
            ) : (
              <>
                {factsWithMeta.length > 0 && (
                  <div className="space-y-4">
                    <div className="flex items-center gap-2">
                      <Shield className="size-5 text-amber-500" />
                      <h2 className="text-lg font-semibold">画像特征</h2>
                      <Badge variant="secondary" className="text-[10px]">{factsWithMeta.length} 条</Badge>
                    </div>
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                      {factsWithMeta.map(({ fact, factObj, isPrivate, meta }) => {
                        const Icon = meta.icon
                        const isUnlocked = unlockedFacts.has(fact)
                        const displayContent =
                          isPrivate && isUnlocked ? factObj?.l2_content || formatFact(fact) : formatFact(fact)

                        return (
                          <Card
                            key={`sf-${fact.slice(0, 40)}`}
                            className={cn(
                              "relative overflow-hidden border transition-all duration-200 hover:shadow-md",
                              isPrivate && !isUnlocked && "border-amber-200 dark:border-amber-500/30"
                            )}
                          >
                            <CardContent className="p-4">
                              <div className="flex items-center justify-between mb-3">
                                <div className={cn("flex items-center justify-center size-8 rounded-lg border", meta.bgColor)}>
                                  <Icon className={cn("size-4", meta.color)} />
                                </div>
                                <div className="flex items-center gap-2">
                                  {isPrivate && (
                                    <Badge variant="outline" className="text-[10px] gap-1 bg-amber-50 dark:bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-200 dark:border-amber-500/30">
                                      <Lock className="size-2.5" />
                                      私密
                                    </Badge>
                                  )}
                                  <Badge variant="outline" className={cn("text-[10px]", meta.bgColor, meta.color)}>
                                    {meta.label}
                                  </Badge>
                                </div>
                              </div>

                              <div className={cn(isPrivate && !isUnlocked && "blur-[6px] select-none opacity-40")}>
                                <ExpandableMarkdown content={displayContent} />
                              </div>

                              {isPrivate && !isUnlocked && (
                                <button
                                  type="button"
                                  className="mt-3 w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg bg-amber-50 dark:bg-amber-500/10 text-amber-700 dark:text-amber-300 text-xs font-medium hover:bg-amber-100 dark:hover:bg-amber-500/20 transition-colors border border-amber-200 dark:border-amber-500/30 cursor-pointer"
                                  onClick={() => requestUnlock(fact)}
                                >
                                  {isUnlocked ? (
                                    <>
                                      <EyeOff className="size-3.5" />
                                      已解锁
                                    </>
                                  ) : (
                                    <>
                                      <Lock className="size-3.5" />
                                      点击解锁私密内容
                                    </>
                                  )}
                                </button>
                              )}
                            </CardContent>
                          </Card>
                        )
                      })}
                    </div>
                  </div>
                )}

                {dynamicContext.length > 0 && (
                  <div className="space-y-4">
                    <Separator />
                    <div className="flex items-center gap-2">
                      <Activity className="size-5 text-blue-500" />
                      <h2 className="text-lg font-semibold">静态偏好</h2>
                      <Badge variant="secondary" className="text-[10px]">{dynamicContext.length} 条</Badge>
                    </div>
                    <div className="relative pl-6 space-y-3">
                      <div className="absolute left-[11px] top-2 bottom-2 w-px bg-gradient-to-b from-blue-300 via-orange-300 to-transparent dark:from-blue-500/40 dark:via-orange-500/40 dark:to-transparent" />
                      {dynamicContext.map((ctx, idx) => {
                        const ctxMeta = classifyFact(ctx)
                        const CtxIcon = ctxMeta.icon
                        return (
                          <div key={`dc-${idx}`} className="relative group">
                            <div className="absolute -left-6 top-2.5 size-[22px] rounded-full bg-gradient-to-br from-blue-400 to-orange-400 ring-[3px] ring-background flex items-center justify-center shadow-sm group-hover:scale-110 transition-transform">
                              <span className="text-[9px] font-bold text-white">{idx + 1}</span>
                            </div>
                            <Card className="overflow-hidden hover:shadow-sm transition-shadow">
                              <CardContent className="p-3.5">
                                <div className="flex items-center gap-2 mb-2">
                                  <div className={cn("flex items-center justify-center size-5 rounded-md border", ctxMeta.bgColor)}>
                                    <CtxIcon className={cn("size-3", ctxMeta.color)} />
                                  </div>
                                  <span className={cn("text-[10px] font-medium px-1.5 py-0.5 rounded border", ctxMeta.bgColor, ctxMeta.color)}>
                                    {ctxMeta.label}
                                  </span>
                                </div>
                                <ExpandableMarkdown content={formatFact(ctx)} />
                              </CardContent>
                            </Card>
                          </div>
                        )
                      })}
                    </div>
                  </div>
                )}

                {factsWithMeta.length === 0 && dynamicContext.length === 0 && (
                  <Card className="border-dashed">
                    <CardContent className="py-16 flex flex-col items-center gap-3 text-muted-foreground">
                      <div className="flex items-center justify-center size-12 rounded-2xl bg-muted">
                        <User className="size-6" />
                      </div>
                      <div className="text-center">
                        <p className="text-sm font-medium">暂无画像数据</p>
                        <p className="text-xs mt-1">存储更多记忆后，系统将自动构建您的用户画像</p>
                      </div>
                    </CardContent>
                  </Card>
                )}
              </>
            )}
          </TabsContent>

          {/* ── Tab: Changelog ────────────────────────────────── */}
          <TabsContent value="changelog" className="space-y-4">
            {loading ? (
              <div className="space-y-3">
                {[1, 2, 3].map((n) => (
                  <Skeleton key={n} className="h-20 w-full rounded-xl" />
                ))}
              </div>
            ) : changelog.length === 0 ? (
              <Card className="border-dashed">
                <CardContent className="py-16 flex flex-col items-center gap-3 text-muted-foreground">
                  <div className="flex items-center justify-center size-12 rounded-2xl bg-muted">
                    <History className="size-6" />
                  </div>
                  <div className="text-center">
                    <p className="text-sm font-medium">暂无变更记录</p>
                    <p className="text-xs mt-1">偏好变更时将自动记录在这里</p>
                  </div>
                </CardContent>
              </Card>
            ) : (
              <div className="relative pl-6 space-y-3 overflow-hidden">
                <div className="absolute left-[11px] top-2 bottom-2 w-px bg-gradient-to-b from-violet-300 via-sky-300 to-transparent dark:from-violet-500/40 dark:via-sky-500/40 dark:to-transparent" />
                {changelog.map((entry) => {
                  const isCreated = entry.action === "created"
                  const isUpdated = entry.action === "updated"
                  const actionStyle = isCreated
                    ? { bg: "from-emerald-400 to-teal-400", color: "text-emerald-700 dark:text-emerald-300", badgeClass: "bg-emerald-50 dark:bg-emerald-500/10 text-emerald-700 dark:text-emerald-300 border-emerald-200 dark:border-emerald-500/30", symbol: "+", label: "创建" }
                    : isUpdated
                      ? { bg: "from-blue-400 to-sky-400", color: "text-blue-700 dark:text-blue-300", badgeClass: "bg-blue-50 dark:bg-blue-500/10 text-blue-700 dark:text-blue-300 border-blue-200 dark:border-blue-500/30", symbol: "~", label: "更新" }
                      : { bg: "from-rose-400 to-red-400", color: "text-rose-700 dark:text-rose-300", badgeClass: "bg-rose-50 dark:bg-rose-500/10 text-rose-700 dark:text-rose-300 border-rose-200 dark:border-rose-500/30", symbol: "−", label: "删除" }

                  return (
                    <div key={entry.id} className="relative group">
                      <div className={cn(
                        "absolute -left-6 top-2.5 size-[22px] rounded-full bg-gradient-to-br ring-[3px] ring-background flex items-center justify-center shadow-sm group-hover:scale-110 transition-transform",
                        actionStyle.bg
                      )}>
                        <span className="text-[9px] font-bold text-white">{actionStyle.symbol}</span>
                      </div>
                      <Card className="overflow-hidden hover:shadow-sm transition-shadow">
                        <CardContent className="p-4 space-y-2">
                          <div className="flex items-center gap-2 flex-wrap">
                            <Badge variant="outline" className={cn("text-[10px] gap-1", actionStyle.badgeClass)}>
                              {isCreated ? <Plus className="size-2.5" /> : isUpdated ? <Edit3 className="size-2.5" /> : <Trash2 className="size-2.5" />}
                              {actionStyle.label}
                            </Badge>
                            {entry.source && (
                              <span className="text-[10px] text-muted-foreground">{entry.source}</span>
                            )}
                            <span className="text-[10px] text-muted-foreground ml-auto">{formatRelativeDate(entry.created_at)}</span>
                          </div>
                          {entry.old_value && (
                            <p className="text-xs text-muted-foreground line-through truncate">{entry.old_value}</p>
                          )}
                          {entry.new_value && (
                            <p className="text-sm leading-relaxed">{entry.new_value}</p>
                          )}
                        </CardContent>
                      </Card>
                    </div>
                  )
                })}
              </div>
            )}
          </TabsContent>
        </Tabs>
      </div>

      {/* ── Delete AlertDialog ─────────────────────────────────── */}
      <AlertDialog open={!!deleteTarget} onOpenChange={(open) => { if (!open && !isDeleting) setDeleteTarget(null) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除偏好</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除这条偏好吗？此操作不可撤销。
              {deleteTarget && (
                <span className="block mt-2 px-3 py-2 rounded-lg bg-muted text-sm text-foreground truncate">
                  {deleteTarget.value}
                </span>
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeleting}>取消</AlertDialogCancel>
            <AlertDialogAction
              disabled={isDeleting}
              onClick={handleDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {isDeleting ? "删除中..." : "删除"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* ── Create Preference Dialog ───────────────────────────── */}
      <PreferenceFormDialog
        open={createDialogOpen}
        onOpenChange={setCreateDialogOpen}
        saving={saving}
        onSubmit={handleCreate}
        title="新建偏好"
      />

      {/* ── Edit Preference Dialog ─────────────────────────────── */}
      <PreferenceFormDialog
        open={!!editingPref}
        onOpenChange={(open) => { if (!open) setEditingPref(null) }}
        saving={saving}
        onSubmit={(data) => {
          if (!editingPref) return
          handleUpdate(editingPref.id, data)
        }}
        pref={editingPref}
        title="编辑偏好"
      />

      {/* ── Vault Unlock Dialog ────────────────────────────────── */}
      <Dialog open={vaultDialogOpen} onOpenChange={(open) => { if (!open) { setVaultDialogOpen(false); setUnlockPassword(""); setUnlockError(null) } }}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Lock className="size-4 text-amber-500" />
              解锁私密记忆
            </DialogTitle>
            <DialogDescription>请输入您的 Vault 密码以解锁受保护的内容。</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1.5">
              <Label className="text-xs">密码</Label>
              <Input
                type="password"
                placeholder="输入 Vault 密码..."
                value={unlockPassword}
                onChange={(e) => { setUnlockPassword(e.target.value); setUnlockError(null) }}
                onKeyDown={(e) => e.key === "Enter" && handleVaultUnlock()}
                className={unlockError ? "border-destructive" : ""}
                autoFocus
              />
              {unlockError && <p className="text-xs text-destructive">{unlockError}</p>}
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setVaultDialogOpen(false)} disabled={isUnlocking}>
              取消
            </Button>
            <Button onClick={handleVaultUnlock} disabled={isUnlocking || !unlockPassword.trim()}>
              {isUnlocking ? "解锁中..." : "解锁"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

// ── Slot Group (collapsible) ─────────────────────────────────────

function SlotGroup({
  slot,
  preferences,
  onEdit,
  onDelete,
}: {
  slot: string
  preferences: PreferenceResponse[]
  onEdit: (pref: PreferenceResponse) => void
  onDelete: (pref: PreferenceResponse) => void
}) {
  const [collapsed, setCollapsed] = useState(false)
  const [visibleCount, setVisibleCount] = useState(4)
  const SlotIcon = getSlotIcon(slot)
  const visiblePrefs = preferences.slice(0, visibleCount)
  const hasMore = visibleCount < preferences.length

  return (
    <div className="space-y-2">
      <button
        type="button"
        className="flex items-center gap-2.5 w-full group cursor-pointer text-left border-none bg-transparent p-0"
        onClick={() => { setCollapsed(!collapsed); setVisibleCount(4) }}
      >
        <div className="flex items-center justify-center size-7 rounded-lg bg-gradient-to-br from-amber-100 to-orange-100 dark:from-amber-500/15 dark:to-orange-500/15 border border-amber-200/60 dark:border-amber-500/20">
          <SlotIcon className="size-3.5 text-amber-600 dark:text-amber-400" />
        </div>
        <span className="text-sm font-semibold text-foreground">{getSlotLabel(slot)}</span>
        <Badge variant="secondary" className="text-[10px]">{preferences.length}</Badge>
        <div className="flex-1" />
        <ChevronRight className={cn(
          "size-4 text-muted-foreground transition-transform duration-200",
          !collapsed && "rotate-90"
        )} />
      </button>

      {!collapsed && (
        <div className="space-y-2.5 pl-9">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-2.5">
            {visiblePrefs.map((pref) => (
              <PreferenceCard key={pref.id} pref={pref} onEdit={onEdit} onDelete={onDelete} />
            ))}
          </div>
          {hasMore && (
            <button
              type="button"
              className="w-full flex items-center justify-center gap-1.5 py-2 text-xs text-muted-foreground hover:text-foreground transition-colors rounded-lg hover:bg-muted/50 cursor-pointer"
              onClick={() => setVisibleCount(c => Math.min(c + 4, preferences.length))}
            >
              <ChevronDown className="size-3.5" />
              加载更多（剩余 {preferences.length - visibleCount} 条）
            </button>
          )}
        </div>
      )}
    </div>
  )
}

// ── Preference Card ──────────────────────────────────────────────

function PreferenceCard({
  pref,
  onEdit,
  onDelete,
}: {
  pref: PreferenceResponse
  onEdit: (pref: PreferenceResponse) => void
  onDelete: (pref: PreferenceResponse) => void
}) {
  const scopeInfo = SCOPE_STYLES[pref.scope] || { className: "bg-slate-50 dark:bg-slate-500/10 text-slate-700 dark:text-slate-300 border-slate-200 dark:border-slate-500/30", label: pref.scope }

  return (
    <Card className="group hover:shadow-md transition-all duration-200 border-border/60">
      <CardContent className="p-3.5 space-y-2.5">
        <p className="text-sm leading-relaxed whitespace-pre-wrap break-words">{pref.value}</p>
        <div className="flex items-center gap-1.5 flex-wrap">
          <Badge variant="outline" className={cn("text-[10px] gap-1", scopeInfo.className)}>
            {scopeInfo.label}
          </Badge>
          {pref.project_path && (
            <Badge variant="outline" className="text-[10px] gap-1 bg-violet-50 dark:bg-violet-500/10 text-violet-700 dark:text-violet-300 border-violet-200 dark:border-violet-500/30">
              <FolderOpen className="size-2.5" />
              {pref.project_path.split("/").pop()}
            </Badge>
          )}
          <span className="text-[10px] text-muted-foreground ml-auto">
            {formatRelativeDate(pref.updated_at)}
          </span>
        </div>

        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity pt-1 border-t border-border/40">
          <span className="text-[10px] text-muted-foreground mr-auto">置信度 {(pref.confidence * 100).toFixed(0)}%</span>
          <Button
            variant="ghost"
            size="icon"
            className="size-7 text-muted-foreground hover:text-foreground"
            onClick={() => onEdit(pref)}
          >
            <Edit3 className="size-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-7 text-muted-foreground hover:text-destructive"
            onClick={() => onDelete(pref)}
          >
            <Trash2 className="size-3.5" />
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

// ── Preference Form Dialog ───────────────────────────────────────

function PreferenceFormDialog({
  open,
  onOpenChange,
  saving,
  onSubmit,
  pref,
  title,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  saving: boolean
  onSubmit: (data: CreatePreferenceBody | UpdatePreferenceBody) => void
  pref?: PreferenceResponse | null
  title: string
}) {
  const isEdit = !!pref

  const [slot, setSlot] = useState(pref?.slot || "")
  const [customSlotName, setCustomSlotName] = useState("")
  const [value, setValue] = useState(pref?.value || "")
  const [confidence, setConfidence] = useState(String(pref?.confidence ?? 0.8))
  const [scope, setScope] = useState(pref?.scope || "global")
  const [projectPath, setProjectPath] = useState(pref?.project_path || "")

  useEffect(() => {
    if (open) {
      const initSlot = pref?.slot || ""
      setSlot(initSlot)
      setCustomSlotName(initSlot.startsWith("custom:") ? initSlot.slice(7) : "")
      setValue(pref?.value || "")
      setConfidence(String(pref?.confidence ?? 0.8))
      setScope(pref?.scope || "global")
      setProjectPath(pref?.project_path || "")
    }
  }, [open, pref])

  const isCustomSlot = slot === "__custom__"

  const handleSubmit = () => {
    const actualSlot = isCustomSlot ? `custom:${customSlotName.trim()}` : slot.trim()
    if (!actualSlot || !value.trim()) {
      toast.error("请填写 Slot 和内容")
      return
    }
    if (isCustomSlot && !customSlotName.trim()) {
      toast.error("请填写自定义 Slot 名")
      return
    }

    if (isEdit) {
      const data: UpdatePreferenceBody = {}
      if (value !== (pref?.value || "")) data.value = value.trim()
      if (confidence !== String(pref?.confidence ?? 0.8)) data.confidence = parseFloat(confidence)
      if (scope !== (pref?.scope || "global")) data.scope = scope
      if (projectPath !== (pref?.project_path || "")) data.project_path = projectPath.trim() || undefined
      onSubmit(data)
    } else {
      onSubmit({
        slot: actualSlot,
        value: value.trim(),
        confidence: parseFloat(confidence) || 0.8,
        scope,
        project_path: projectPath.trim() || undefined,
      })
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-4 text-amber-500" />
            {title}
          </DialogTitle>
          <DialogDescription>
            {isEdit ? "修改偏好信息。" : "添加一条新的偏好。"}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-1">
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label className="text-xs">Slot (类别)</Label>
              {isEdit ? (
                <Input value={slot} disabled className="h-9 bg-muted/60" />
              ) : (
                <>
                  <Select value={slot || undefined} onValueChange={(v) => v != null && setSlot(v)}>
                    <SelectTrigger className="h-9 w-full">
                      <SelectValue placeholder="选择类别..." />
                    </SelectTrigger>
                    <SelectContent>
                      {BUILTIN_SLOTS.map((s) => (
                        <SelectItem key={s.name} value={s.name}>{s.display}</SelectItem>
                      ))}
                      <SelectItem value="__custom__">自定义 (custom:...)</SelectItem>
                    </SelectContent>
                  </Select>
                  {isCustomSlot && (
                    <Input
                      placeholder="自定义 slot 名 (小写字母+数字+下划线)"
                      value={customSlotName}
                      onChange={(e) => {
                        const v = e.target.value.replace(/[^a-z0-9_]/g, "")
                        setCustomSlotName(v)
                      }}
                      className="h-9"
                      maxLength={50}
                    />
                  )}
                </>
              )}
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs">范围</Label>
              <Select value={scope} onValueChange={(v) => v != null && setScope(v)}>
                <SelectTrigger className="h-9">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="global">全局</SelectItem>
                  <SelectItem value="project">项目</SelectItem>
                  <SelectItem value="session">会话</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label className="text-xs">内容</Label>
            <Textarea
              placeholder="偏好内容..."
              value={value}
              onChange={(e) => setValue(e.target.value)}
              rows={3}
              className="resize-none"
              maxLength={500}
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label className="text-xs">置信度 (0-1)</Label>
              <Input
                type="number"
                min="0"
                max="1"
                step="0.1"
                value={confidence}
                onChange={(e) => setConfidence(e.target.value)}
                className="h-9"
              />
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs">项目路径 (可选)</Label>
              <Input
                placeholder="例: /path/to/project"
                value={projectPath}
                onChange={(e) => setProjectPath(e.target.value)}
                className="h-9"
                maxLength={200}
              />
            </div>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>
            取消
          </Button>
          <Button
            onClick={handleSubmit}
            disabled={saving || !value.trim() || (!isCustomSlot ? !slot.trim() : !customSlotName.trim())}
            className="bg-gradient-to-r from-amber-500 to-orange-500 hover:from-amber-600 hover:to-orange-600 text-white"
          >
            {saving ? (isEdit ? "保存中..." : "创建中...") : isEdit ? "保存" : "创建"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
