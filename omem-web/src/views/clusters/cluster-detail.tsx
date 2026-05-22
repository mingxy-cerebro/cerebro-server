import { useEffect, useState, useCallback } from "react"
import { useParams, useNavigate } from "react-router-dom"
import { toast } from "sonner"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Separator } from "@/components/ui/separator"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { getCluster } from "@/api/cluster"
import type { ClusterDetail } from "@/types/cluster"
import { getCategoryBadgeClass, getCategoryLabel } from "@/lib/tag-utils"
import {
  ArrowLeft,
  Calendar,
  Clock,
  Users,
  Tag,
  Hash,
  BarChart3,
  ChevronDown,
  ChevronUp,
  FileText,
  Merge,
} from "lucide-react"

function formatDate(dateString: string) {
  return new Date(dateString).toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  })
}

function formatDateShort(dateString: string) {
  return new Date(dateString).toLocaleDateString("zh-CN")
}

const COLLAPSED_HEIGHT = 160

function MarkdownContent({
  content,
  collapsed,
}: {
  content: string
  collapsed: boolean
}) {
  return (
    <div
      className="prose prose-sm dark:prose-invert max-w-none text-sm leading-relaxed"
      style={collapsed ? { maxHeight: `${COLLAPSED_HEIGHT}px`, overflow: "hidden" } : undefined}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
    </div>
  )
}

function ExpandableMemory({ content }: { content: string }) {
  const [expanded, setExpanded] = useState(false)
  const [needsExpand, setNeedsExpand] = useState(false)
  const measureRef = useCallback(
    (node: HTMLDivElement | null) => {
      if (node) {
        setNeedsExpand(node.scrollHeight > COLLAPSED_HEIGHT + 20)
      }
    },
    [content],
  )

  return (
    <div ref={measureRef}>
      <MarkdownContent content={content} collapsed={!expanded && needsExpand} />
      {needsExpand && (
        <Button
          variant="ghost"
          size="sm"
          className="mt-1 h-7 text-xs text-muted-foreground hover:text-foreground"
          onClick={() => setExpanded((v) => !v)}
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
    </div>
  )
}

export function ClusterDetailPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [detail, setDetail] = useState<ClusterDetail | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!id) return
    const clusterId = id
    async function fetchData() {
      try {
        setLoading(true)
        setError(null)
        const res = await getCluster(clusterId)
        setDetail(res)
      } catch (err) {
        console.error("Failed to load cluster detail:", err)
        setError("加载簇详情失败")
        toast.error("加载簇详情失败")
      } finally {
        setLoading(false)
      }
    }
    fetchData()
  }, [id])

  if (loading) {
    return (
      <div className="space-y-6 max-w-5xl mx-auto">
        <div className="flex items-center gap-2">
          <Skeleton className="size-8" />
          <Skeleton className="h-6 w-32" />
        </div>
        <Skeleton className="h-8 w-3/4" />
        <Skeleton className="h-4 w-1/2" />
        <div className="space-y-3">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-20 w-full rounded-lg" />
          ))}
        </div>
      </div>
    )
  }

  if (error || !detail) {
    return (
      <div className="space-y-4">
        <Button variant="ghost" size="sm" onClick={() => navigate("/clusters/list")}>
          <ArrowLeft className="size-4 mr-1.5" />
          返回列表
        </Button>
        <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-6 text-sm text-destructive">
          {error || "簇不存在"}
        </div>
      </div>
    )
  }

  const { cluster, members } = detail

  return (
    <div className="space-y-6 max-w-5xl mx-auto">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={() => navigate("/clusters/list")}>
          <ArrowLeft className="size-4 mr-1.5" />
          返回列表
        </Button>
        <div className="flex items-center gap-2">
          <Badge
            variant="outline"
            className={`text-xs ${getCategoryBadgeClass(cluster.category)}`}
          >
            {getCategoryLabel(cluster.category)}
          </Badge>
        </div>
      </div>

      <div className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight">
          {cluster.title || "未命名簇"}
        </h1>
        <div className="flex items-center gap-4 text-sm text-muted-foreground flex-wrap">
          <span className="flex items-center gap-1">
            <Calendar className="size-3.5" />
            创建于 {formatDate(cluster.created_at)}
          </span>
          <span className="flex items-center gap-1">
            <Clock className="size-3.5" />
            更新于 {formatDate(cluster.updated_at)}
          </span>
          <span className="flex items-center gap-1">
            <Users className="size-3.5" />
            {cluster.member_count} 条成员记忆
          </span>
          <span className="flex items-center gap-1">
            <Hash className="size-3.5" />
            {cluster.id.slice(0, 8)}...
          </span>
          {cluster.importance > 0 && (
            <span className="flex items-center gap-1">
              <BarChart3 className="size-3.5" />
              重要性 {cluster.importance.toFixed(2)}
            </span>
          )}
        </div>
      </div>

      {cluster.keywords && cluster.keywords.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="flex items-center gap-1.5 text-base">
              <Tag className="size-4 text-muted-foreground" />
              关键词
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex flex-wrap gap-2">
              {cluster.keywords.map((kw) => (
                <Badge key={kw} variant="secondary" className="font-normal">
                  {kw}
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      <Separator />

      <Card className="border-primary/20 bg-primary/5 dark:bg-primary/10">
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-1.5 text-base">
            <Merge className="size-4 text-primary" />
            合并后摘要
            <Badge variant="outline" className="text-xs ml-1 text-primary border-primary/30">
              Merged
            </Badge>
          </CardTitle>
        </CardHeader>
        <CardContent>
          {cluster.summary ? (
            <div className="prose prose-sm dark:prose-invert max-w-none text-sm leading-relaxed">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{cluster.summary}</ReactMarkdown>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground italic">暂无合并摘要</p>
          )}
        </CardContent>
      </Card>

      <div className="space-y-4">
        <h2 className="text-lg font-semibold flex items-center gap-2">
          <FileText className="size-5 text-muted-foreground" />
          原始记忆
          <span className="text-sm font-normal text-muted-foreground">
            ({members.length} 条)
          </span>
        </h2>

        {members.length === 0 ? (
          <div className="rounded-lg border border-border bg-card p-8 text-center space-y-2">
            <p className="text-sm text-muted-foreground">此簇暂无成员记忆</p>
          </div>
        ) : (
          <div className="space-y-3">
            {members.map((member, index) => (
              <div
                key={member.id}
                className="rounded-lg border border-border bg-card p-4 space-y-3"
              >
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  <span className="font-mono bg-muted px-1.5 py-0.5 rounded text-[11px]">
                    #{index + 1}
                  </span>
                  <Badge
                    variant="outline"
                    className={`text-xs ${getCategoryBadgeClass(member.category)}`}
                  >
                    {getCategoryLabel(member.category)}
                  </Badge>
                  {member.importance > 0 && (
                    <span className="flex items-center gap-1">
                      <BarChart3 className="size-3" />
                      {member.importance.toFixed(2)}
                    </span>
                  )}
                  <span className="ml-auto flex items-center gap-1">
                    <Calendar className="size-3" />
                    {formatDateShort(member.created_at)}
                  </span>
                </div>
                <ExpandableMemory content={member.content || "—"} />
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
