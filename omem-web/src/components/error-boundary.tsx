import { Component, type ErrorInfo, type ReactNode } from "react"
import { Home, RefreshCw } from "lucide-react"
import { Button } from "@/components/ui/button"

interface Props {
  children: ReactNode
}

interface State {
  hasError: boolean
  error?: Error
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false }
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("ErrorBoundary caught an error:", error, errorInfo)
  }

  handleRefresh = () => {
    window.location.reload()
  }

  handleGoHome = () => {
    window.location.href = "/"
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen w-full flex-col items-center justify-center bg-background text-foreground">
          <div className="text-center space-y-6 max-w-md px-4">
            <div className="text-6xl font-bold text-destructive/30">⚠️</div>
            <h1 className="text-2xl font-semibold tracking-tight">
              出错了
            </h1>
            <p className="text-sm text-muted-foreground">
              应用遇到了意外错误。请尝试刷新页面，或返回首页。
            </p>
            {this.state.error && (
              <div className="rounded-lg bg-muted p-4 text-left">
                <p className="text-xs font-mono text-muted-foreground break-all">
                  {this.state.error.message}
                </p>
              </div>
            )}
            <div className="flex items-center justify-center gap-3">
              <Button variant="outline" onClick={this.handleRefresh}>
                <RefreshCw className="h-4 w-4 mr-2" />
                刷新页面
              </Button>
              <Button onClick={this.handleGoHome}>
                <Home className="h-4 w-4 mr-2" />
                返回首页
              </Button>
            </div>
          </div>
        </div>
      )
    }

    return this.props.children
  }
}
