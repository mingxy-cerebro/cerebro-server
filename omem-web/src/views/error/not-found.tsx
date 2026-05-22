import { useNavigate } from "react-router-dom"
import { Home, ArrowLeft } from "lucide-react"
import { Button } from "@/components/ui/button"

export function NotFoundPage() {
  const navigate = useNavigate()

  return (
    <div className="flex h-screen w-full flex-col items-center justify-center bg-background text-foreground">
      <div className="text-center space-y-6">
        <div className="text-8xl font-bold text-muted-foreground/30">404</div>
        <h1 className="text-2xl font-semibold tracking-tight">页面不存在</h1>
        <p className="text-sm text-muted-foreground max-w-sm">
          您访问的页面可能已经删除、移动，或者从未存在过。
        </p>
        <div className="flex items-center justify-center gap-3">
          <Button variant="outline" onClick={() => navigate(-1)}>
            <ArrowLeft className="h-4 w-4 mr-2" />
            返回上一页
          </Button>
          <Button onClick={() => navigate("/dashboard")}>
            <Home className="h-4 w-4 mr-2" />
            返回首页
          </Button>
        </div>
      </div>
    </div>
  )
}
