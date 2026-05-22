import { useState, useEffect, useCallback } from "react"
import { Plus, Pencil, Trash2, Tag, Link2, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Separator } from "@/components/ui/separator"
import { Switch } from "@/components/ui/switch"
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
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog"
import { toast } from "sonner"
import { categoriesApi } from "@/api/categories"
import type { CategoryConfig, AliasResponse } from "@/types/categories"

/* ── Category Form State ── */
interface CategoryFormData {
  name: string
  display_name: string
  description: string
  decision_rule: string
  always_merge: boolean
  append_only: boolean
  temporal_versioned: boolean
  merge_supported: boolean
  admission_weight: number
  importance_base: number
  prompt_format: string
  default_visibility: string
  default_scope: string
  default_ttl_days: number
  sort_order: number
  is_active: boolean
}

const defaultForm: CategoryFormData = {
  name: "",
  display_name: "",
  description: "",
  decision_rule: "",
  always_merge: false,
  append_only: false,
  temporal_versioned: false,
  merge_supported: false,
  admission_weight: 0.5,
  importance_base: 0.5,
  prompt_format: "",
  default_visibility: "global",
  default_scope: "global",
  default_ttl_days: 0,
  sort_order: 100,
  is_active: true,
}

export function CategoriesPage() {
  const [categories, setCategories] = useState<CategoryConfig[]>([])
  const [aliases, setAliases] = useState<AliasResponse[]>([])
  const [loading, setLoading] = useState(true)
  const [editingCategory, setEditingCategory] = useState<CategoryConfig | null>(null)
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState<CategoryFormData>(defaultForm)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [aliasDialogOpen, setAliasDialogOpen] = useState(false)
  const [newAlias, setNewAlias] = useState("")
  const [newAliasTarget, setNewAliasTarget] = useState("")
  const [deleteAliasTarget, setDeleteAliasTarget] = useState<string | null>(null)

  const loadCategories = useCallback(async () => {
    try {
      const res = await categoriesApi.list()
      setCategories(Array.isArray(res) ? res : [])
    } catch {
      toast.error("加载分类失败")
    }
  }, [])

  const loadAliases = useCallback(async () => {
    try {
      const res = await categoriesApi.listAliases()
      setAliases(Array.isArray(res) ? res : [])
    } catch {
      toast.error("加载别名失败")
    }
  }, [])

  useEffect(() => {
    Promise.all([loadCategories(), loadAliases()]).finally(() => setLoading(false))
  }, [loadCategories, loadAliases])

  /* ── Category CRUD ── */
  const handleCreate = () => {
    setEditingCategory(null)
    setForm(defaultForm)
    setShowForm(true)
  }

  const handleEdit = (cat: CategoryConfig) => {
    setEditingCategory(cat)
    setForm({
      name: cat.name,
      display_name: cat.display_name,
      description: cat.description,
      decision_rule: cat.decision_rule || "",
      always_merge: cat.always_merge,
      append_only: cat.append_only,
      temporal_versioned: cat.temporal_versioned,
      merge_supported: cat.merge_supported,
      admission_weight: cat.admission_weight,
      importance_base: cat.importance_base,
      prompt_format: cat.prompt_format || "",
      default_visibility: cat.default_visibility,
      default_scope: cat.default_scope,
      default_ttl_days: cat.default_ttl_days || 0,
      sort_order: cat.sort_order,
      is_active: cat.is_active,
    })
    setShowForm(true)
  }

  const handleSubmit = async () => {
    if (!form.name.trim() || !form.display_name.trim()) {
      toast.error("名称和显示名为必填项")
      return
    }
    try {
      const payload: Record<string, unknown> = {
        ...form,
        decision_rule: form.decision_rule || null,
        prompt_format: form.prompt_format || null,
        default_ttl_days: form.default_ttl_days || null,
      }
      if (editingCategory) {
        await categoriesApi.update(editingCategory.name, payload)
        toast.success("分类已更新")
      } else {
        await categoriesApi.create(payload as Parameters<typeof categoriesApi.create>[0])
        toast.success("分类已创建")
      }
      setShowForm(false)
      loadCategories()
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : "操作失败"
      toast.error(msg)
    }
  }

  const handleDelete = async () => {
    if (!deleteTarget) return
    try {
      await categoriesApi.delete(deleteTarget)
      toast.success("分类已删除")
      setDeleteTarget(null)
      loadCategories()
    } catch {
      toast.error("删除失败")
    }
  }

  /* ── Alias CRUD ── */
  const handleCreateAlias = async () => {
    if (!newAlias.trim() || !newAliasTarget.trim()) {
      toast.error("别名和目标分类为必填项")
      return
    }
    try {
      await categoriesApi.createAlias(newAlias, newAliasTarget)
      toast.success("别名已创建")
      setAliasDialogOpen(false)
      setNewAlias("")
      setNewAliasTarget("")
      loadAliases()
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : "创建别名失败"
      toast.error(msg)
    }
  }

  const handleDeleteAlias = async () => {
    if (!deleteAliasTarget) return
    try {
      await categoriesApi.deleteAlias(deleteAliasTarget)
      toast.success("别名已删除")
      setDeleteAliasTarget(null)
      loadAliases()
    } catch {
      toast.error("删除别名失败")
    }
  }

  if (loading) {
    return <div className="flex items-center justify-center h-64 text-muted-foreground">加载中...</div>
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">分类字典</h1>
          <p className="text-sm text-muted-foreground">管理记忆分类和别名映射</p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => setAliasDialogOpen(true)}>
            <Link2 className="size-4 mr-1" />
            添加别名
          </Button>
          <Button onClick={handleCreate}>
            <Plus className="size-4 mr-1" />
            新建分类
          </Button>
        </div>
      </div>

      <Separator />

      {/* Aliases Section */}
      {aliases.length > 0 && (
        <div className="space-y-3">
          <h2 className="text-lg font-semibold flex items-center gap-2">
            <Link2 className="size-4" />
            别名映射 ({aliases.length})
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
            {aliases.map((a) => (
              <Card key={a.alias} className="p-3 flex items-center justify-between">
                <div className="flex items-center gap-2 text-sm">
                  <code className="bg-muted px-1.5 py-0.5 rounded text-xs">{a.alias}</code>
                  <span className="text-muted-foreground">→</span>
                  <code className="bg-muted px-1.5 py-0.5 rounded text-xs">{a.target}</code>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-7 text-destructive hover:text-destructive"
                  onClick={() => setDeleteAliasTarget(a.alias)}
                >
                  <X className="size-3.5" />
                </Button>
              </Card>
            ))}
          </div>
          <Separator />
        </div>
      )}

      {/* Categories Table */}
      <div className="space-y-3">
        <h2 className="text-lg font-semibold flex items-center gap-2">
          <Tag className="size-4" />
          分类列表 ({categories.length})
        </h2>
        <div className="border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted/50">
              <tr>
                <th className="text-left p-3 font-medium">名称</th>
                <th className="text-left p-3 font-medium">显示名</th>
                <th className="text-left p-3 font-medium hidden md:table-cell">描述</th>
                <th className="text-center p-3 font-medium">权重</th>
                <th className="text-center p-3 font-medium">状态</th>
                <th className="text-right p-3 font-medium">操作</th>
              </tr>
            </thead>
            <tbody className="divide-y">
              {categories.map((cat) => (
                <tr key={cat.name} className="hover:bg-muted/30">
                  <td className="p-3">
                    <code className="bg-muted px-1.5 py-0.5 rounded text-xs">{cat.name}</code>
                  </td>
                  <td className="p-3 font-medium">{cat.display_name}</td>
                  <td className="p-3 text-muted-foreground hidden md:table-cell max-w-[200px] truncate">
                    {cat.description}
                  </td>
                  <td className="p-3 text-center">
                    <span className="text-xs">{cat.admission_weight.toFixed(2)}</span>
                  </td>
                  <td className="p-3 text-center">
                    <span
                      className={`text-xs px-2 py-0.5 rounded-full ${
                        cat.is_active
                          ? "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400"
                          : "bg-gray-100 text-gray-500 dark:bg-gray-800 dark:text-gray-400"
                      }`}
                    >
                      {cat.is_active ? "启用" : "禁用"}
                    </span>
                  </td>
                  <td className="p-3 text-right">
                    <div className="flex justify-end gap-1">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-7"
                        onClick={() => handleEdit(cat)}
                      >
                        <Pencil className="size-3.5" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-7 text-destructive hover:text-destructive"
                        onClick={() => setDeleteTarget(cat.name)}
                      >
                        <Trash2 className="size-3.5" />
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Category Create/Edit Dialog */}
      <Dialog open={showForm} onOpenChange={setShowForm}>
        <DialogContent className="max-w-lg max-h-[80vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>{editingCategory ? "编辑分类" : "新建分类"}</DialogTitle>
          </DialogHeader>
          <div className="space-y-4 py-2">
            {/* Name (only for create) */}
            {!editingCategory && (
              <div className="space-y-1.5">
                <Label>名称 (name)</Label>
                <Input
                  value={form.name}
                  onChange={(e) => setForm((f) => ({ ...f, name: e.target.value.toLowerCase() }))}
                  placeholder="e.g. preferences"
                  maxLength={50}
                />
              </div>
            )}
            <div className="space-y-1.5">
              <Label>显示名 (display_name)</Label>
              <Input
                value={form.display_name}
                onChange={(e) => setForm((f) => ({ ...f, display_name: e.target.value }))}
                placeholder="e.g. 偏好设置"
                maxLength={100}
              />
            </div>
            <div className="space-y-1.5">
              <Label>描述</Label>
              <Input
                value={form.description}
                onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))}
                placeholder="分类描述"
                maxLength={500}
              />
            </div>
            <div className="space-y-1.5">
              <Label>决策规则 (decision_rule)</Label>
              <Input
                value={form.decision_rule}
                onChange={(e) => setForm((f) => ({ ...f, decision_rule: e.target.value }))}
                placeholder="可选"
                maxLength={200}
              />
            </div>

            <Separator />

            {/* Numeric fields */}
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-1.5">
                <Label>准入权重</Label>
                <Input
                  type="number"
                  step={0.05}
                  min={0}
                  max={1}
                  value={form.admission_weight}
                  onChange={(e) => setForm((f) => ({ ...f, admission_weight: +e.target.value }))}
                />
              </div>
              <div className="space-y-1.5">
                <Label>重要性基数</Label>
                <Input
                  type="number"
                  step={0.05}
                  min={0}
                  max={1}
                  value={form.importance_base}
                  onChange={(e) => setForm((f) => ({ ...f, importance_base: +e.target.value }))}
                />
              </div>
              <div className="space-y-1.5">
                <Label>排序</Label>
                <Input
                  type="number"
                  value={form.sort_order}
                  onChange={(e) => setForm((f) => ({ ...f, sort_order: +e.target.value }))}
                />
              </div>
              <div className="space-y-1.5">
                <Label>默认TTL (天)</Label>
                <Input
                  type="number"
                  value={form.default_ttl_days}
                  onChange={(e) => setForm((f) => ({ ...f, default_ttl_days: +e.target.value }))}
                />
              </div>
            </div>

            <div className="space-y-1.5">
              <Label>提示格式 (prompt_format)</Label>
              <Input
                value={form.prompt_format}
                onChange={(e) => setForm((f) => ({ ...f, prompt_format: e.target.value }))}
                placeholder="可选"
                maxLength={500}
              />
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-1.5">
                <Label>默认可见性</Label>
                <select
                  className="w-full h-9 rounded-md border border-input bg-background px-3 text-sm"
                  value={form.default_visibility}
                  onChange={(e) => setForm((f) => ({ ...f, default_visibility: e.target.value }))}
                >
                  <option value="global">global</option>
                  <option value="project">project</option>
                  <option value="private">private</option>
                </select>
              </div>
              <div className="space-y-1.5">
                <Label>默认作用域</Label>
                <select
                  className="w-full h-9 rounded-md border border-input bg-background px-3 text-sm"
                  value={form.default_scope}
                  onChange={(e) => setForm((f) => ({ ...f, default_scope: e.target.value }))}
                >
                  <option value="global">global</option>
                  <option value="project">project</option>
                </select>
              </div>
            </div>

            <Separator />

            {/* Boolean flags */}
            <div className="grid grid-cols-2 gap-3">
              {([
                ["always_merge", "始终合并"],
                ["append_only", "仅追加"],
                ["temporal_versioned", "时序版本"],
                ["merge_supported", "支持合并"],
                ["is_active", "启用"],
              ] as const).map(([key, label]) => (
                <div key={key} className="flex items-center justify-between rounded-md border p-3">
                  <Label className="text-sm">{label}</Label>
                  <Switch
                    checked={form[key as keyof CategoryFormData] as boolean}
                    onCheckedChange={(v) => setForm((f) => ({ ...f, [key]: v }))}
                  />
                </div>
              ))}
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowForm(false)}>
              取消
            </Button>
            <Button onClick={handleSubmit}>{editingCategory ? "保存" : "创建"}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Category Confirm */}
      <AlertDialog open={!!deleteTarget} onOpenChange={() => setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除分类</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除分类 "{deleteTarget}" 吗？此操作不可撤销。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={handleDelete}>删除</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Alias Create Dialog */}
      <Dialog open={aliasDialogOpen} onOpenChange={setAliasDialogOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>添加别名</DialogTitle>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label>别名</Label>
              <Input
                value={newAlias}
                onChange={(e) => setNewAlias(e.target.value)}
                placeholder="e.g. likes"
                maxLength={50}
              />
            </div>
            <div className="space-y-1.5">
              <Label>目标分类</Label>
              <select
                className="w-full h-9 rounded-md border border-input bg-background px-3 text-sm"
                value={newAliasTarget}
                onChange={(e) => setNewAliasTarget(e.target.value)}
              >
                <option value="">选择分类...</option>
                {categories.map((c) => (
                  <option key={c.name} value={c.name}>
                    {c.display_name} ({c.name})
                  </option>
                ))}
              </select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAliasDialogOpen(false)}>
              取消
            </Button>
            <Button onClick={handleCreateAlias}>创建</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Alias Confirm */}
      <AlertDialog open={!!deleteAliasTarget} onOpenChange={() => setDeleteAliasTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除别名</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除别名 "{deleteAliasTarget}" 吗？
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={handleDeleteAlias}>删除</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
