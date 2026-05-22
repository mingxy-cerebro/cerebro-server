import { useState } from "react"
import { Moon, Sun, Monitor, Trash2, Lock, Download, Info, Database } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { Label } from "@/components/ui/label"
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
import { useTheme } from "@/providers/theme-provider"
import { useVaultStore } from "@/stores/vault"
import { toast } from "sonner"
import apiClient from "@/api/client"

export function SettingsPage() {
  const { theme, setTheme } = useTheme()
  const { hasPassword, setPassword, lock } = useVaultStore()
  const [newPassword, setNewPassword] = useState("")
  const [showPasswordForm, setShowPasswordForm] = useState(false)
  const [exportingMemories, setExportingMemories] = useState(false)
  const [resetDialogOpen, setResetDialogOpen] = useState(false)

  const handleClearCache = () => {
    sessionStorage.clear()
    toast.success("缓存已清除", {
      description: "所有本地数据已清除，刷新页面后生效",
    })
  }

  const handleResetVault = async () => {
    try {
      await apiClient.delete("/v1/vault/password")
      lock()
      toast.success("Vault 已重置")
      setShowPasswordForm(false)
      setResetDialogOpen(false)
    } catch {
      toast.error("重置 Vault 失败")
    }
  }

  const handleSetVaultPassword = () => {
    if (!newPassword || newPassword.length < 4) {
      toast.error("密码至少需要 4 位")
      return
    }
    setPassword(newPassword)
    toast.success("Vault 密码已设置")
    setNewPassword("")
    setShowPasswordForm(false)
  }

  const handleExportConfig = () => {
    const data = {
      auth: JSON.parse(sessionStorage.getItem("omem-auth") || "{}"),
      vault: "已迁移到服务端存储",
      theme: localStorage.getItem("omem-theme") || "system",
      exportedAt: new Date().toISOString(),
    }
    const blob = new Blob([JSON.stringify(data, null, 2)], {
      type: "application/json",
    })
    const url = URL.createObjectURL(blob)
    const a = document.createElement("a")
    a.href = url
    a.download = `omem-config-${new Date().toISOString().slice(0, 10)}.json`
    a.click()
    URL.revokeObjectURL(url)
    toast.success("本地配置已导出")
  }

  const handleExportMemories = async () => {
    setExportingMemories(true)
    try {
      const response = await apiClient.get("/v1/memories", {
        params: { limit: 10000 },
      })
      const data = {
        memories: response.memories || [],
        total_count: response.total_count || 0,
        exportedAt: new Date().toISOString(),
      }
      const blob = new Blob([JSON.stringify(data, null, 2)], {
        type: "application/json",
      })
      const url = URL.createObjectURL(blob)
      const a = document.createElement("a")
      a.href = url
      a.download = `omem-memories-${new Date().toISOString().slice(0, 10)}.json`
      a.click()
      URL.revokeObjectURL(url)
      toast.success(`已导出 ${data.total_count} 条记忆`)
    } catch (err) {
      console.error("Failed to export memories:", err)
      toast.error("导出记忆数据失败")
    } finally {
      setExportingMemories(false)
    }
  }

  return (
    <div className="space-y-6 max-w-2xl">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">系统设置</h1>
        <p className="text-sm text-muted-foreground">
          管理您的偏好设置和数据
        </p>
      </div>

      <Card className="p-6">
        <h3 className="font-semibold mb-4">外观</h3>
        <div className="space-y-4">
          <div>
            <Label className="text-sm font-medium">主题模式</Label>
            <div className="flex gap-2 mt-2">
              <Button
                variant={theme === "light" ? "default" : "outline"}
                size="sm"
                onClick={() => setTheme("light")}
                className="gap-2"
              >
                <Sun className="h-4 w-4" />
                亮色
              </Button>
              <Button
                variant={theme === "dark" ? "default" : "outline"}
                size="sm"
                onClick={() => setTheme("dark")}
                className="gap-2"
              >
                <Moon className="h-4 w-4" />
                暗黑
              </Button>
              <Button
                variant={theme === "system" ? "default" : "outline"}
                size="sm"
                onClick={() => setTheme("system")}
                className="gap-2"
              >
                <Monitor className="h-4 w-4" />
                跟随系统
              </Button>
            </div>
          </div>
        </div>
      </Card>

      <Card className="p-6">
        <h3 className="font-semibold mb-4">私密空间 Vault</h3>
        <div className="space-y-4">
          {hasPassword ? (
            <>
              <p className="text-sm text-muted-foreground">
                Vault 密码已设置。私密记忆需要输入密码才能查看。
              </p>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowPasswordForm(!showPasswordForm)}
                >
                  <Lock className="h-4 w-4 mr-2" />
                  修改密码
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => setResetDialogOpen(true)}
                >
                  <Trash2 className="h-4 w-4 mr-2" />
                  重置 Vault
                </Button>
              </div>
            </>
          ) : (
            <>
              <p className="text-sm text-muted-foreground">
                尚未设置 Vault 密码。设置后，私密记忆将受到密码保护。
              </p>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowPasswordForm(true)}
              >
                <Lock className="h-4 w-4 mr-2" />
                设置 Vault 密码
              </Button>
            </>
          )}

          {showPasswordForm && (
            <div className="flex gap-2 mt-4">
              <input
                type="password"
                placeholder="输入新密码"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                className="flex-1 rounded-md border border-input bg-background px-3 py-2 text-sm"
              />
              <Button size="sm" onClick={handleSetVaultPassword}>
                确认
              </Button>
            </div>
          )}
        </div>
      </Card>

      <Card className="p-6">
        <h3 className="font-semibold mb-4">数据管理</h3>
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium">导出记忆数据</p>
              <p className="text-xs text-muted-foreground">
                从服务端导出所有记忆为 JSON 文件
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={handleExportMemories} disabled={exportingMemories}>
              <Database className="h-4 w-4 mr-2" />
              {exportingMemories ? "导出中..." : "导出"}
            </Button>
          </div>
          <Separator />
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium">导出本地配置</p>
              <p className="text-xs text-muted-foreground">
                将登录信息、主题等本地设置导出备份
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={handleExportConfig}>
              <Download className="h-4 w-4 mr-2" />
              导出
            </Button>
          </div>
          <Separator />
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium">清除缓存</p>
              <p className="text-xs text-muted-foreground">
                清除所有本地存储的数据（不可逆）
              </p>
            </div>
            <Button variant="destructive" size="sm" onClick={handleClearCache}>
              <Trash2 className="h-4 w-4 mr-2" />
              清除
            </Button>
          </div>
        </div>
      </Card>

      <Card className="p-6">
        <h3 className="font-semibold mb-4">关于</h3>
        <div className="space-y-2 text-sm">
          <div className="flex items-center gap-2">
            <Info className="h-4 w-4 text-muted-foreground" />
            <span>omem-web v0.1.0</span>
          </div>
          <p className="text-xs text-muted-foreground">
            omem 自部署版 Web 管理端
          </p>
          <p className="text-xs text-muted-foreground">
            备案号：<a href="https://beian.miit.gov.cn" target="_blank" rel="noopener noreferrer" className="underline">吉ICP备2026003061号</a>
          </p>
        </div>
      </Card>

      <AlertDialog open={resetDialogOpen} onOpenChange={setResetDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>确认重置 Vault</AlertDialogTitle>
            <AlertDialogDescription>
              此操作不可恢复。重置后所有受 Vault 保护的私密记忆将无法访问。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setResetDialogOpen(false)}>取消</AlertDialogCancel>
            <AlertDialogAction onClick={handleResetVault} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
              确认重置
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
