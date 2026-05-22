import { useEffect, useState } from "react"
import { useParams, useNavigate } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import apiClient from "@/api/client"
import { ArrowLeft, Save, Plus, Lock } from "lucide-react"
import { useVaultStore } from "@/stores/vault"
import { isPrivateMemory, getTagClassName } from "@/lib/tag-utils"

interface MemoryFormData {
  content: string
  tags: string[]
  source: string
}

interface MemoryDetail {
  id: string
  content: string
  tags: string[]
  visibility?: string
  source: string
  category: string
  memory_type: string
  created_at: string
}

function parseTags(input: string): string[] {
  return input
    .split(/[,，]/)
    .map((t) => t.trim())
    .filter((t) => t.length > 0)
}

function formatTags(tags: string[]): string {
  return tags.join(", ")
}

export function MemoryFormPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const isEditing = !!id

  const [formData, setFormData] = useState<MemoryFormData>({
    content: "",
    tags: [],
    source: "",
  })
  const [tagInput, setTagInput] = useState("")
  const [loading, setLoading] = useState(isEditing)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [isPrivate, setIsPrivate] = useState(false)
  const vaultUnlocked = useVaultStore((s) => s.isUnlocked)

  useEffect(() => {
    if (!isEditing) return

    async function fetchMemory() {
      try {
        setLoading(true)
        const response = await apiClient.get<MemoryDetail>(`/v1/memories/${id}`)
        setFormData({
          content: response.content || "",
          tags: response.tags || [],
          source: response.source || "",
        })
        setTagInput(formatTags(response.tags || []))
        setIsPrivate(isPrivateMemory(response.tags, response.visibility))
      } catch (err) {
        console.error("Failed to fetch memory:", err)
        setError("加载记忆失败")
      } finally {
        setLoading(false)
      }
    }

    fetchMemory()
  }, [id, isEditing])

  const handleSubmit = async () => {
    if (!formData.content.trim()) {
      setError("内容不能为空")
      return
    }

    try {
      setSaving(true)
      setError(null)

      const payload = {
        content: formData.content.trim(),
        tags: formData.tags,
        source: formData.source.trim() || undefined,
      }

      if (isEditing) {
        await apiClient.put(`/v1/memories/${id}`, payload)
        navigate(`/memories/${id}`)
      } else {
        const response = await apiClient.post<{ id: string }>("/v1/memories", payload)
        navigate(`/memories/${response.id}`)
      }
    } catch (err) {
      console.error("Failed to save memory:", err)
      setError(isEditing ? "保存记忆失败" : "创建记忆失败")
    } finally {
      setSaving(false)
    }
  }

  const handleTagInputChange = (value: string) => {
    setTagInput(value)
    setFormData((prev) => ({
      ...prev,
      tags: parseTags(value),
    }))
  }

  const handleTagKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault()
      const newTag = tagInput.trim()
      if (newTag && !formData.tags.includes(newTag)) {
        setFormData((prev) => ({
          ...prev,
          tags: [...prev.tags, newTag],
        }))
        setTagInput("")
      }
    }
  }

  const removeTag = (tagToRemove: string) => {
    setFormData((prev) => ({
      ...prev,
      tags: prev.tags.filter((t) => t !== tagToRemove),
    }))
  }

  if (loading) {
    return (
      <div className="space-y-6 max-w-2xl">
        <Skeleton className="h-8 w-32" />
        <div className="space-y-4">
          <Skeleton className="h-4 w-16" />
          <Skeleton className="h-32 w-full" />
          <Skeleton className="h-4 w-16" />
          <Skeleton className="h-10 w-full" />
        </div>
      </div>
    )
  }

  if (isEditing && isPrivate && !vaultUnlocked) {
    return (
      <div className="space-y-6 max-w-2xl">
        <Button variant="ghost" size="sm" onClick={() => navigate(-1)}>
          <ArrowLeft className="size-4 mr-1.5" />
          返回
        </Button>
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-8 text-center space-y-4">
          <Lock className="h-10 w-10 text-amber-500 mx-auto" />
          <h3 className="text-lg font-semibold text-amber-500">Vault 已锁定</h3>
          <p className="text-sm text-muted-foreground max-w-xs mx-auto">
            此记忆为私密记忆，请先解锁 Vault 后再进行编辑
          </p>
          <Button size="sm" onClick={() => navigate(`/memories/${id}`)}>
            返回详情页
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6 max-w-2xl">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={() => navigate(-1)}>
          <ArrowLeft className="size-4 mr-1.5" />
          返回
        </Button>
        <h1 className="text-xl font-semibold">
          {isEditing ? "编辑记忆" : "新建记忆"}
        </h1>
      </div>

      {error && (
        <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-4 text-sm text-destructive">
          {error}
        </div>
      )}

      <div className="space-y-6">
        <div className="space-y-2">
          <Label htmlFor="content">
            内容 <span className="text-destructive">*</span>
          </Label>
          <Textarea
            id="content"
            value={formData.content}
            onChange={(e) =>
              setFormData((prev) => ({ ...prev, content: e.target.value }))
            }
            placeholder="输入记忆内容..."
            rows={8}
            className="resize-none"
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="tags">标签</Label>
          <div className="space-y-2">
            <Input
              id="tags"
              value={tagInput}
              onChange={(e) => handleTagInputChange(e.target.value)}
              onKeyDown={handleTagKeyDown}
              placeholder="输入标签，按回车或逗号分隔..."
            />
            <div className="flex flex-wrap gap-2">
              {formData.tags.map((tag) => (
                <Badge
                  key={tag}
                  variant="outline"
                  className={getTagClassName(tag, "cursor-pointer hover:opacity-60")}
                  onClick={() => removeTag(tag)}
                  title="点击移除"
                >
                  {tag} ×
                </Badge>
              ))}
            </div>
          </div>
        </div>

        <div className="space-y-2">
          <Label htmlFor="source">来源</Label>
          <Input
            id="source"
            value={formData.source}
            onChange={(e) =>
              setFormData((prev) => ({ ...prev, source: e.target.value }))
            }
            placeholder="记忆来源（可选）..."
          />
        </div>

        <div className="flex justify-end gap-3 pt-4">
          <Button
            variant="outline"
            onClick={() => navigate(-1)}
            disabled={saving}
          >
            取消
          </Button>
          <Button onClick={handleSubmit} disabled={saving}>
            {saving ? (
              "保存中..."
            ) : isEditing ? (
              <>
                <Save className="size-4 mr-1.5" />
                保存
              </>
            ) : (
              <>
                <Plus className="size-4 mr-1.5" />
                创建
              </>
            )}
          </Button>
        </div>
      </div>
    </div>
  )
}
