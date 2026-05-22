import { useState } from "react"
import { Link, useLocation } from "react-router-dom"
import { Menu, LayoutDashboard, Brain, Home, BarChart3, Import, Settings } from "lucide-react"
import { Sheet, SheetContent, SheetTrigger } from "@/components/ui/sheet"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

const navItems = [
  { icon: LayoutDashboard, label: "仪表盘", path: "/dashboard" },
  { icon: Brain, label: "记忆管理", path: "/memories" },
  { icon: Home, label: "空间管理", path: "/spaces" },
  { icon: BarChart3, label: "统计分析", path: "/analytics" },
  { icon: Import, label: "批量导入", path: "/import" },
  { icon: Settings, label: "系统设置", path: "/settings" },
]

export function MobileNav() {
  const [open, setOpen] = useState(false)
  const location = useLocation()

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetTrigger render={<Button variant="ghost" size="icon" className="md:hidden" />}>
        <Menu className="h-5 w-5" />
        <span className="sr-only">打开菜单</span>
      </SheetTrigger>
      <SheetContent side="left" className="w-64 p-0">
        <div className="flex h-14 items-center px-4 border-b border-border">
          <Link to="/" className="flex items-center gap-2 font-semibold text-sm" onClick={() => setOpen(false)}>
            <Brain className="h-5 w-5" />
            <span>omem</span>
          </Link>
        </div>
        <nav className="flex-1 space-y-0.5 p-2">
          {navItems.map((item) => {
            const isActive = location.pathname.startsWith(item.path)
            return (
              <Link
                key={item.path}
                to={item.path}
                onClick={() => setOpen(false)}
                className={cn(
                  "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                  isActive
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground hover:bg-muted hover:text-foreground"
                )}
              >
                <item.icon className="h-4 w-4" />
                <span>{item.label}</span>
              </Link>
            )
          })}
        </nav>
      </SheetContent>
    </Sheet>
  )
}
