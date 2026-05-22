import { Link, useLocation } from "react-router-dom"
import {
  LayoutDashboard,
  Brain,
  Home,
  BarChart3,
  Import,
  Settings,
  ChevronRight,
  User,
  History,
  Layers,
  Timer,
  BookOpen,
} from "lucide-react"
import { cn } from "@/lib/utils"

const navItems = [
  { icon: LayoutDashboard, label: "仪表盘", path: "/dashboard" },
  { icon: Brain, label: "记忆管理", path: "/memories" },
  { icon: History, label: "Sessions", path: "/sessions" },
  { icon: Home, label: "空间管理", path: "/spaces" },
  { icon: BarChart3, label: "统计分析", path: "/analytics" },
  { icon: History, label: "等级变更", path: "/tier-history" },
  { icon: Layers, label: "簇列表", path: "/clusters/list" },
  { icon: Layers, label: "归簇管理", path: "/clusters" },
  { icon: Timer, label: "生命周期", path: "/lifecycle" },
  { icon: Import, label: "批量导入", path: "/import" },
  { icon: BookOpen, label: "分类字典", path: "/categories" },
  { icon: User, label: "用户画像", path: "/profile" },
  { icon: Settings, label: "系统设置", path: "/settings" },
]

export function AppSidebar() {
  const location = useLocation()

  return (
    <aside className="flex w-56 flex-col border-r border-border bg-card">
      <div className="flex h-14 items-center px-4 border-b border-border">
        <Link to="/" className="flex items-center gap-2 font-semibold text-sm">
          <Brain className="h-5 w-5" />
          <span>omem</span>
        </Link>
      </div>
      <nav className="flex-1 space-y-0.5 p-2">
        {navItems.map((item) => {
          const isActive = item.path === "/clusters/list"
            ? location.pathname.startsWith("/clusters") && location.pathname !== "/clusters"
            : item.path === "/clusters"
              ? location.pathname === "/clusters"
              : location.pathname.startsWith(item.path)
          return (
            <Link
              key={item.path}
              to={item.path}
              className={cn(
                "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                isActive
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground"
              )}
            >
              <item.icon className="h-4 w-4" />
              <span>{item.label}</span>
              {isActive && (
                <ChevronRight className="ml-auto h-3.5 w-3.5 opacity-50" />
              )}
            </Link>
          )
        })}
      </nav>
    </aside>
  )
}
