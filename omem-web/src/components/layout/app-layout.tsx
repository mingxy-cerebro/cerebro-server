import { useEffect, useState } from "react"
import { Outlet, useNavigate } from "react-router-dom"
import { AppSidebar } from "./app-sidebar"
import { AppHeader } from "./app-header"
import { AppFooter } from "./app-footer"
import { useVaultStore } from "@/stores/vault"
import { useAuthStore } from "@/stores/auth"
import { Button } from "@/components/ui/button"
import { AlertTriangle, X } from "lucide-react"

export function AppLayout() {
  const navigate = useNavigate()
  const isAuthenticated = useAuthStore((state) => state.isAuthenticated)
  const checkStatus = useVaultStore((state) => state.checkStatus)
  const [showBanner, setShowBanner] = useState(false)

  useEffect(() => {
    if (!isAuthenticated) return
    checkStatus().then(() => {
      const vaultState = useVaultStore.getState()
      setShowBanner(!vaultState.hasPassword)
    })
  }, [checkStatus, isAuthenticated])

  return (
    <div className="flex h-screen w-full bg-background text-foreground">
      <div className="hidden md:flex">
        <AppSidebar />
      </div>
      <div className="flex flex-1 flex-col min-w-0">
        <AppHeader />
        {showBanner && (
          <div className="bg-amber-500/10 border-b border-amber-500/20 px-4 py-2.5">
            <div className="flex items-center justify-between max-w-3xl">
              <div className="flex items-center gap-2 text-sm text-amber-700 dark:text-amber-400">
                <AlertTriangle className="size-4 shrink-0" />
                <span>为了您的隐私安全，请先设置私密密码</span>
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant="link"
                  size="sm"
                  className="h-auto p-0 text-amber-700 dark:text-amber-400 font-medium"
                  onClick={() => navigate("/vault")}
                >
                  去设置 →
                </Button>
                <button
                  type="button"
                  onClick={() => setShowBanner(false)}
                  className="text-amber-700/60 dark:text-amber-400/60 hover:text-amber-700 dark:hover:text-amber-400"
                >
                  <X className="size-4" />
                </button>
              </div>
            </div>
          </div>
        )}
        <main className="flex-1 overflow-auto p-4 md:p-6">
          <Outlet />
        </main>
        <AppFooter />
      </div>
    </div>
  )
}
