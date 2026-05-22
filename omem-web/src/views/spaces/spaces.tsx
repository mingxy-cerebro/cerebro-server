import { useEffect, useState } from "react"
import { Card, CardHeader, CardContent } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import apiClient from "@/api/client"
import { useAuthStore } from "@/stores/auth"
import { toast } from "sonner"
import { Users, Shield, Calendar, Plus, Trash2, UserPlus, User, KeyRound, Copy, Check } from "lucide-react"

interface Space {
  id: string
  space_type: string
  name: string
  owner_id: string
  members: Array<{
    user_id: string
    role: string
    joined_at: string
  }>
  auto_share_rules: unknown[]
  created_at: string
  updated_at: string
}

export function SpacesPage() {
  const [spaces, setSpaces] = useState<Space[]>([])
  const [loading, setLoading] = useState(true)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [newSpaceName, setNewSpaceName] = useState("")
  const [newSpaceType, setNewSpaceType] = useState("team")
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [manageSpaceId, setManageSpaceId] = useState<string | null>(null)
  const [newMemberId, setNewMemberId] = useState("")
  const [newMemberRole, setNewMemberRole] = useState("member")
  const [addingMember, setAddingMember] = useState(false)
  const [memberInfos, setMemberInfos] = useState<Record<string, { name: string; created_at: string }>>({})
  const [loadingMemberInfo, setLoadingMemberInfo] = useState(false)
  const [createUserOpen, setCreateUserOpen] = useState(false)
  const [newUserName, setNewUserName] = useState("")
  const [newUserRole, setNewUserRole] = useState("member")
  const [creatingUser, setCreatingUser] = useState(false)
  const [generatedKey, setGeneratedKey] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  const currentUserId = useAuthStore((s) => s.currentUserId)
  const currentUser = useAuthStore((s) => s.users.find((u) => u.id === currentUserId))
  const currentManageSpace = spaces.find((s) => s.id === manageSpaceId)

  const isSpaceAdmin = (space: Space) => {
    if (!currentUser) return false
    return space.owner_id === currentUser.apiKey || space.members.some(
      (m) => m.user_id === currentUser.apiKey && (m.role === "admin" || m.role === "Admin")
    )
  }

  useEffect(() => {
    async function fetchMemberInfos() {
      if (!currentManageSpace?.members?.length) return
      setLoadingMemberInfo(true)
      const infos: Record<string, { name: string; created_at: string }> = {}
      await Promise.all(
        currentManageSpace.members.map(async (m) => {
          try {
            const res = await apiClient.get<{ id: string; name: string; created_at: string }>(
              `/v1/tenants/${encodeURIComponent(m.user_id)}`
            )
            if (res) infos[m.user_id] = { name: res.name, created_at: res.created_at }
          } catch {
            void 0
          }
        })
      )
      setMemberInfos(infos)
      setLoadingMemberInfo(false)
    }
    if (manageSpaceId) {
      fetchMemberInfos()
    }
  }, [manageSpaceId, currentManageSpace?.members])

  useEffect(() => {
    async function fetchSpaces() {
      try {
        setLoading(true)
        const response = await apiClient.get<Space[]>('/v1/spaces')
        setSpaces(response || [])
      } catch (err) {
        console.error("Failed to fetch spaces:", err)
        toast.error("加载空间列表失败")
      } finally {
        setLoading(false)
      }
    }
    fetchSpaces()
  }, [])

  async function createSpace() {
    if (!newSpaceName.trim()) {
      toast.error("请输入空间名称")
      return
    }
    try {
      await apiClient.post('/v1/spaces', {
        name: newSpaceName.trim(),
        space_type: newSpaceType,
      })
      toast.success("空间创建成功")
      setDialogOpen(false)
      setNewSpaceName("")
      const response = await apiClient.get<Space[]>('/v1/spaces')
      setSpaces(response || [])
    } catch (err) {
      console.error("Failed to create space:", err)
      toast.error("空间创建失败")
    }
  }

  async function confirmDeleteSpace() {
    if (!deleteTarget) return
    const space = spaces.find((s) => s.id === deleteTarget)
    const memberApiKeys = space?.members
      .map((m) => m.user_id)
      .filter((key) => key !== currentUser?.apiKey) || []
    try {
      await apiClient.delete(`/v1/spaces/${encodeURIComponent(deleteTarget)}`)
      toast.success("空间已删除")
      setSpaces((prev) => prev.filter((s) => s.id !== deleteTarget))
      useAuthStore.getState().removeUsersByApiKeys(memberApiKeys)
    } catch (err) {
      console.error("Failed to delete space:", err)
      toast.error("空间删除失败")
    } finally {
      setDeleteTarget(null)
    }
  }

  async function createUserAndAdd() {
    if (!manageSpaceId) return
    setCreatingUser(true)
    try {
      const res = await apiClient.post<{ id: string; api_key: string; status: string }>(
        '/v1/tenants',
        { name: newUserName.trim() || undefined }
      )
      const userId = res.id
      const apiKey = res.api_key

      await apiClient.post(`/v1/spaces/${encodeURIComponent(manageSpaceId)}/members`, {
        user_id: userId,
        role: newUserRole,
      })

      setGeneratedKey(apiKey)
      toast.success("新用户创建并加入空间成功")
      const response = await apiClient.get<Space[]>('/v1/spaces')
      setSpaces(response || [])
    } catch (err: any) {
      console.error("Failed to create user:", err)
      const msg = err.response?.data?.error || err.message || ""
      toast.error(`创建用户失败：${msg || "请稍后重试"}`)
    } finally {
      setCreatingUser(false)
    }
  }

  async function removeMember(userId: string) {
    if (!manageSpaceId) return
    try {
      await apiClient.delete(`/v1/spaces/${encodeURIComponent(manageSpaceId)}/members/${encodeURIComponent(userId)}`)
      toast.success("成员已移除")
      const response = await apiClient.get<Space[]>('/v1/spaces')
      setSpaces(response || [])
    } catch (err: any) {
      console.error("Failed to remove member:", err)
      const msg = err.response?.data?.error || err.message || ""
      toast.error(`移除成员失败：${msg || "请稍后重试"}`)
    }
  }

  async function addMember() {
    if (!manageSpaceId || !newMemberId.trim()) {
      toast.error("请输入用户ID")
      return
    }
    const userId = newMemberId.trim()
    const uuidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
    if (!uuidRegex.test(userId)) {
      toast.error("用户ID格式不正确，请输入有效的 UUID（如：c60beb98-7aab-4985-8c1d-29ffd6aff75a）")
      return
    }
    setAddingMember(true)
    try {
      await apiClient.post(`/v1/spaces/${encodeURIComponent(manageSpaceId)}/members`, {
        user_id: userId,
        role: newMemberRole,
      })
      toast.success("成员添加成功")
      setNewMemberId("")
      const response = await apiClient.get<Space[]>('/v1/spaces')
      setSpaces(response || [])
    } catch (err: any) {
      console.error("Failed to add member:", err)
      const msg = err.response?.data?.error || err.message || ""
      if (msg.includes("not found") || msg.includes("不存在")) {
        toast.error("该用户不存在，请确认用户ID正确")
      } else if (msg.includes("permission") || msg.includes("权限")) {
        toast.error("您没有权限添加成员")
      } else {
        toast.error(`添加成员失败：${msg || "请检查用户ID是否正确"}`)
      }
    } finally {
      setAddingMember(false)
    }
  }

  if (loading) {
    return (
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">空间管理</h1>
            <p className="text-sm text-muted-foreground">管理您的记忆空间</p>
          </div>
          <Skeleton className="h-9 w-24" />
        </div>
        <div className="grid gap-4">
          {[1, 2].map((i) => (
            <Skeleton key={i} className="h-32 w-full" />
          ))}
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">空间管理</h1>
          <p className="text-sm text-muted-foreground">管理您的记忆空间</p>
        </div>
        <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
          <DialogTrigger render={
            <Button size="sm">
              <Plus className="size-4 mr-1.5" />
              新建空间
            </Button>
          } />
          <DialogContent>
            <DialogHeader>
              <DialogTitle>新建空间</DialogTitle>
            </DialogHeader>
            <div className="space-y-4 pt-2">
              <div className="space-y-2">
                <span className="text-sm font-medium">空间名称</span>
                <Input
                  placeholder="输入空间名称"
                  value={newSpaceName}
                  onChange={(e) => setNewSpaceName(e.target.value)}
                />
              </div>
              <div className="space-y-2">
                <span className="text-sm font-medium">空间类型</span>
                <Select value={newSpaceType} onValueChange={(v) => setNewSpaceType(v || 'team')}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="team">团队</SelectItem>
                    <SelectItem value="personal">个人</SelectItem>
                    <SelectItem value="shared">共享</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <Button className="w-full" onClick={createSpace}>
                创建
              </Button>
            </div>
          </DialogContent>
        </Dialog>
      </div>

      {spaces.length === 0 ? (
        <div className="text-center py-12 text-muted-foreground border border-dashed border-border rounded-lg">
          <p>暂无空间</p>
          <p className="text-sm mt-1">点击右上角按钮创建新空间</p>
        </div>
      ) : (
        <div className="grid gap-4">
          {spaces.map((space) => (
            <Card key={space.id} className="group transition-colors hover:bg-accent/50">
              <CardHeader className="pb-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <h3 className="text-lg font-semibold">{space.name}</h3>
                    <Badge variant="outline">{space.space_type}</Badge>
                  </div>
                  <div className="flex items-center gap-2">
                    <div className="flex items-center gap-1 text-xs text-muted-foreground">
                      <Shield className="size-3" />
                      {space.members.length} 成员
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 opacity-0 group-hover:opacity-100 transition-opacity"
                      onClick={() => {
                        setManageSpaceId(space.id)
                        setNewMemberId("")
                        setNewMemberRole("member")
                      }}
                    >
                      <UserPlus className="size-3.5 mr-1" />
                      管理成员
                    </Button>
                    {space.space_type !== 'personal' && isSpaceAdmin(space) && (
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-7 opacity-0 group-hover:opacity-100 transition-opacity"
                        onClick={() => setDeleteTarget(space.id)}
                      >
                        <Trash2 className="size-3.5 text-destructive" />
                      </Button>
                    )}
                  </div>
                </div>
              </CardHeader>
              <CardContent className="pt-0 space-y-3">
                <div className="flex items-center gap-4 text-sm text-muted-foreground flex-wrap">
                  <span className="flex items-center gap-1">
                    <Users className="size-3.5" />
                    {space.members.map((m) => m.role).join(', ')}
                  </span>
                  <span className="flex items-center gap-1">
                    <Calendar className="size-3.5" />
                    {new Date(space.created_at).toLocaleDateString('zh-CN')}
                  </span>
                </div>
                <p className="text-xs text-muted-foreground font-mono">
                  ID: {space.id}
                </p>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      <AlertDialog open={!!deleteTarget} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认删除空间</AlertDialogTitle>
            <AlertDialogDescription>
              此操作不可撤销。空间内的数据不会被删除，但空间配置将被移除。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setDeleteTarget(null)}>取消</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDeleteSpace} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <Dialog open={!!manageSpaceId} onOpenChange={(open) => !open && setManageSpaceId(null)}>
        <DialogContent style={{ width: 680, maxWidth: '95vw' }}>
          <DialogHeader>
            <DialogTitle>管理成员 - {currentManageSpace?.name}</DialogTitle>
          </DialogHeader>
          <div className="space-y-4 pt-2">
            <div className="space-y-2">
              <span className="text-sm font-medium">当前成员</span>
              {loadingMemberInfo && (
                <div className="text-xs text-muted-foreground">加载用户信息中...</div>
              )}
              <div className="space-y-1">
                {currentManageSpace?.members.map((m) => {
                  const info = memberInfos[m.user_id]
                  return (
                    <div key={m.user_id} className="grid grid-cols-[auto_1fr_80px_60px] items-center gap-3 text-sm border rounded-md px-3 py-2">
                      <User className="size-3.5 text-muted-foreground" />
                      <div className="min-w-0">
                        <div className="font-mono text-xs truncate" title={m.user_id}>{m.user_id}</div>
                        {info && (
                          <div className="text-xs text-muted-foreground truncate">
                            {info.name} · {new Date(info.created_at).toLocaleDateString('zh-CN')}
                          </div>
                        )}
                      </div>
                      <Badge variant="outline" className="justify-self-center">{m.role}</Badge>
                      <div className="flex items-center gap-2 justify-self-end">
                        <button
                          type="button"
                          onClick={() => {
                            navigator.clipboard.writeText(m.user_id)
                            toast.success("API Key 已复制")
                          }}
                          className="text-muted-foreground hover:text-foreground transition-colors"
                          title="复制 API Key"
                        >
                          <Copy className="size-3.5" />
                        </button>
                        {currentManageSpace && isSpaceAdmin(currentManageSpace) && m.user_id !== currentUser?.apiKey && m.user_id !== currentManageSpace?.owner_id && (
                          <button
                            type="button"
                            onClick={() => removeMember(m.user_id)}
                            className="text-muted-foreground hover:text-destructive transition-colors"
                            title="移除成员"
                          >
                            <Trash2 className="size-3.5" />
                          </button>
                        )}
                      </div>
                    </div>
                  )
                })}
                {(!currentManageSpace?.members || currentManageSpace.members.length === 0) && (
                  <p className="text-sm text-muted-foreground">暂无成员</p>
                )}
              </div>
            </div>
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">添加成员</span>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    setCreateUserOpen(true)
                    setGeneratedKey(null)
                    setNewUserName("")
                    setNewUserRole("member")
                  }}
                >
                  <KeyRound className="size-3.5 mr-1.5" />
                  创建新用户
                </Button>
              </div>

              {createUserOpen && (
                <div className="rounded-lg border border-border bg-muted/30 p-3 space-y-3">
                  <div className="flex items-center justify-between">
                    <span className="text-sm font-medium">创建新用户</span>
                    <button
                      type="button"
                      onClick={() => {
                        setCreateUserOpen(false)
                        setGeneratedKey(null)
                      }}
                      className="text-xs text-muted-foreground hover:text-foreground"
                    >
                      取消
                    </button>
                  </div>
                  <div className="flex items-center gap-2">
                    <Input
                      placeholder="用户名（可选）"
                      value={newUserName}
                      onChange={(e) => setNewUserName(e.target.value)}
                      className="flex-1"
                    />
                    <Select value={newUserRole} onValueChange={(v) => setNewUserRole(v || 'member')}>
                      <SelectTrigger className="w-28">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="admin">管理员</SelectItem>
                        <SelectItem value="member">成员</SelectItem>
                        <SelectItem value="reader">只读</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <Button
                    size="sm"
                    className="w-full"
                    onClick={createUserAndAdd}
                    disabled={creatingUser}
                  >
                    {creatingUser ? "生成中..." : "生成用户并添加到空间"}
                  </Button>

                  {generatedKey && (
                    <div className="space-y-2">
                      <div className="text-xs text-muted-foreground">新用户 API Key（请复制保存）：</div>
                      <div className="flex items-center gap-2">
                        <code className="flex-1 text-xs bg-background border rounded px-2 py-1.5 font-mono truncate">
                          {generatedKey}
                        </code>
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => {
                            navigator.clipboard.writeText(generatedKey)
                            setCopied(true)
                            setTimeout(() => setCopied(false), 2000)
                          }}
                        >
                          {copied ? (
                            <Check className="size-3.5" />
                          ) : (
                            <Copy className="size-3.5" />
                          )}
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              )}

              {!createUserOpen && (
                <div className="flex items-center gap-2">
                  <Input
                    placeholder="输入对方 API Key（UUID格式）"
                    value={newMemberId}
                    onChange={(e) => setNewMemberId(e.target.value)}
                    className="flex-1"
                  />
                  <Select value={newMemberRole} onValueChange={(v) => setNewMemberRole(v || 'member')}>
                    <SelectTrigger className="w-28">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="admin">管理员</SelectItem>
                      <SelectItem value="member">成员</SelectItem>
                      <SelectItem value="reader">只读</SelectItem>
                    </SelectContent>
                  </Select>
                  <Button size="sm" onClick={addMember} disabled={addingMember}>
                    {addingMember ? "添加中..." : "添加"}
                  </Button>
                </div>
              )}
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  )
}
