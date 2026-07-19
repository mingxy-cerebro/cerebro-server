import { useState, useCallback, useRef, useEffect } from "react"
import { useNavigate } from "react-router-dom"
import { ArrowLeft, Upload, FileJson, FileSpreadsheet, FileText, CheckCircle, XCircle, Loader2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import { toast } from "sonner"
import apiClient from "@/api/client"

interface ImportTask {
  id: string
  status: string
  filename: string
  storage_stored: number
  storage_skipped: number
  extraction_status: string
  extraction_facts: number
  extraction_progress: number
  reconcile_status: string
  reconcile_merged: number
  reconcile_progress: number
  errors: string[]
}

const POLL_INTERVAL_MS = 2000
const POLL_MAX_ATTEMPTS = 150

const STATUS_ICON: Record<string, string> = {
  completed: "✓",
  failed: "✗",
  pending: "⋯",
  processing: "⋯",
  skipped: "–",
}

function ProgressRow({ label, status, detail }: { label: string; status: string; detail: string }) {
  const icon = STATUS_ICON[status] ?? "·"
  const tone =
    status === "completed" ? "text-green-500" :
    status === "failed" ? "text-red-500" :
    status === "processing" || status === "pending" ? "text-blue-500 animate-pulse" :
    "text-muted-foreground"
  return (
    <div className="flex items-center gap-3 text-sm">
      <span className={`font-mono w-4 ${tone}`}>{icon}</span>
      <span className="font-medium w-24 shrink-0">{label}</span>
      <span className="text-xs text-muted-foreground flex-1">{detail}</span>
    </div>
  )
}

export function ImportPage() {
  const navigate = useNavigate()
  const [file, setFile] = useState<File | null>(null)
  const [isUploading, setIsUploading] = useState(false)
  const [forceImport, setForceImport] = useState(false)
  const [task, setTask] = useState<ImportTask | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [dragActive, setDragActive] = useState(false)
  const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    return () => {
      if (pollTimerRef.current) clearTimeout(pollTimerRef.current)
    }
  }, [])

  const handleDrag = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    if (e.type === "dragenter" || e.type === "dragover") {
      setDragActive(true)
    } else if (e.type === "dragleave") {
      setDragActive(false)
    }
  }, [])

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setDragActive(false)
    if (e.dataTransfer.files && e.dataTransfer.files[0]) {
      setFile(e.dataTransfer.files[0])
      setTask(null)
      setErrorMsg(null)
    }
  }, [])

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files && e.target.files[0]) {
      setFile(e.target.files[0])
      setTask(null)
      setErrorMsg(null)
    }
  }

  const getFileIcon = (filename: string) => {
    if (filename.endsWith('.json')) return <FileJson className="h-8 w-8 text-blue-500" />
    if (filename.endsWith('.csv')) return <FileSpreadsheet className="h-8 w-8 text-green-500" />
    if (filename.endsWith('.md') || filename.endsWith('.markdown')) return <FileText className="h-8 w-8 text-purple-500" />
    return <FileText className="h-8 w-8 text-muted-foreground" />
  }

  const pollTask = useCallback(async (taskId: string, attempt: number) => {
    if (attempt >= POLL_MAX_ATTEMPTS) {
      setErrorMsg("导入超时（5分钟未完成），请稍后在记忆列表查看结果")
      setIsUploading(false)
      return
    }
    try {
      const t = await apiClient.get<ImportTask>(`/v1/imports/${taskId}`)
      setTask(t)
      if (t.status === "completed" || t.status === "failed") {
        setIsUploading(false)
        if (t.status === "completed") {
          toast.success(`导入完成：提取 ${t.extraction_facts} 条，合并 ${t.reconcile_merged} 条`)
        } else {
          setErrorMsg(t.errors?.join("\n") || "导入失败")
          toast.error("导入失败")
        }
        return
      }
      pollTimerRef.current = setTimeout(() => pollTask(taskId, attempt + 1), POLL_INTERVAL_MS)
    } catch (e: any) {
      setErrorMsg(e.message || "查询导入状态失败")
      setIsUploading(false)
    }
  }, [])

  const handleImport = async () => {
    if (!file) {
      toast.error("请先选择文件")
      return
    }

    const formData = new FormData()
    formData.append("file", file)
    formData.append("force", forceImport ? "true" : "false")

    setIsUploading(true)
    setErrorMsg(null)
    setTask(null)
    try {
      const resp = await apiClient.post<ImportTask>("/v1/imports", formData, {
        timeout: 120000,
      })
      setTask(resp)
      if (resp.status === "completed") {
        setIsUploading(false)
        toast.success(`导入完成：提取 ${resp.extraction_facts} 条，合并 ${resp.reconcile_merged} 条`)
      } else {
        pollTask(resp.id, 0)
      }
    } catch (error: any) {
      const errData = error.response?.data
      const message = typeof errData === 'string' ? errData
        : errData?.error?.message || errData?.error
        || errData?.message
        || error.message || "导入失败"
      const msgText = typeof message === 'string' ? message : JSON.stringify(message)
      setErrorMsg(msgText)
      toast.error(msgText)
      setIsUploading(false)
    }
  }

  return (
    <div className="space-y-6 max-w-3xl">
      <div className="flex items-center gap-4">
        <Button variant="ghost" size="icon" onClick={() => navigate("/memories")}>
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">批量导入</h1>
          <p className="text-sm text-muted-foreground">
            从 JSON、CSV 或 Markdown 文件导入记忆
          </p>
        </div>
      </div>

      <Card
        className={`border-2 border-dashed p-8 transition-colors ${
          dragActive ? "border-primary bg-primary/5" : "border-border"
        }`}
        onDragEnter={handleDrag}
        onDragLeave={handleDrag}
        onDragOver={handleDrag}
        onDrop={handleDrop}
      >
        <div className="flex flex-col items-center justify-center gap-4 text-center">
          <Upload className="h-10 w-10 text-muted-foreground" />
          <div className="space-y-1">
            <p className="text-sm font-medium">
              {file ? file.name : "拖拽文件到此处，或点击选择"}
            </p>
            <p className="text-xs text-muted-foreground">
              支持 JSON、CSV、Markdown 格式
            </p>
          </div>
          <input
            type="file"
            accept=".json,.csv,.md,.markdown"
            onChange={handleFileChange}
            className="hidden"
            id="file-upload"
          />
          <Button
            variant="outline"
            size="sm"
            onClick={() => document.getElementById("file-upload")?.click()}
          >
            选择文件
          </Button>
        </div>
      </Card>

      {file && (
        <div className="rounded-lg border border-border bg-card p-4 space-y-3">
          <div className="flex items-center gap-3">
            {getFileIcon(file.name)}
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium truncate">{file.name}</p>
              <p className="text-xs text-muted-foreground">
                {(file.size / 1024).toFixed(1)} KB
              </p>
            </div>
            <Button
              onClick={handleImport}
              disabled={isUploading}
              size="sm"
            >
              {isUploading ? (
                <>
                  <Loader2 className="h-4 w-4 mr-1 animate-spin" />
                  处理中
                </>
              ) : "开始导入"}
            </Button>
          </div>
          <label htmlFor="force-import" className="flex items-center gap-2 text-xs text-muted-foreground cursor-pointer">
            <Checkbox
              id="force-import"
              checked={forceImport}
              onCheckedChange={(v) => setForceImport(v === true)}
            />
            <span>强制导入（忽略 SHA256 重复检查）</span>
          </label>
        </div>
      )}

      {errorMsg && (
        <Card className="p-6 border-red-500/30">
          <div className="flex items-center gap-2 mb-3">
            <XCircle className="h-5 w-5 text-red-500" />
            <h3 className="font-semibold text-red-500">导入失败</h3>
          </div>
          <pre className="text-xs text-muted-foreground whitespace-pre-wrap break-all">{errorMsg}</pre>
        </Card>
      )}

      {task && (
        <Card className="p-6">
          <div className="flex items-center gap-2 mb-4">
            {task.status === "completed" ? (
              <CheckCircle className="h-5 w-5 text-green-500" />
            ) : task.status === "failed" ? (
              <XCircle className="h-5 w-5 text-red-500" />
            ) : (
              <Loader2 className="h-5 w-5 text-blue-500 animate-spin" />
            )}
            <h3 className="font-semibold">
              {task.status === "completed" ? "导入完成" :
               task.status === "failed" ? "导入失败" : "后台处理中"}
            </h3>
            <span className="ml-auto text-xs text-muted-foreground font-mono">
              task: {task.id.slice(0, 8)}…
            </span>
          </div>

          <div className="space-y-3">
            <ProgressRow
              label="存储阶段"
              status={task.storage_stored > 0 ? "completed" : "skipped"}
              detail={`${task.storage_stored} 已存 / ${task.storage_skipped} 跳过`}
            />
            <ProgressRow
              label="LLM 提取"
              status={task.extraction_status}
              detail={`${task.extraction_facts} 条 facts（${task.extraction_progress}%）`}
            />
            <ProgressRow
              label="Reconcile 合并"
              status={task.reconcile_status}
              detail={`${task.reconcile_merged} 条合并到记忆库（${task.reconcile_progress}%）`}
            />
          </div>

          {task.status === "completed" && (
            <div className="mt-4 rounded-lg bg-blue-500/10 p-3 text-xs text-blue-600">
              导入流程已完成。新记忆会在 <button type="button" onClick={() => navigate("/memories")} className="underline font-medium">记忆列表</button> 中显示。
            </div>
          )}

          {task.errors.length > 0 && (
            <div className="mt-4 space-y-1">
              <p className="text-sm font-medium text-red-500">错误：</p>
              {task.errors.map((e) => (
                <p key={e.slice(0, 40)} className="text-xs text-muted-foreground">{e}</p>
              ))}
            </div>
          )}
        </Card>
      )}

      <Card className="p-6">
        <h3 className="font-semibold mb-3">支持格式说明</h3>
        <div className="space-y-3 text-sm">
          <div className="flex items-start gap-3">
            <FileJson className="h-4 w-4 text-blue-500 mt-0.5" />
            <div>
              <p className="font-medium">JSON</p>
              <p className="text-muted-foreground text-xs">
                标准 JSON 数组格式，每条记忆包含 content, tags, category 等字段
              </p>
            </div>
          </div>
          <div className="flex items-start gap-3">
            <FileSpreadsheet className="h-4 w-4 text-green-500 mt-0.5" />
            <div>
              <p className="font-medium">CSV</p>
              <p className="text-muted-foreground text-xs">
                包含列：content, tags, category, importance, confidence
              </p>
            </div>
          </div>
          <div className="flex items-start gap-3">
            <FileText className="h-4 w-4 text-purple-500 mt-0.5" />
            <div>
              <p className="font-medium">Markdown</p>
              <p className="text-muted-foreground text-xs">
                每条记忆用标题分隔，支持 frontmatter 元数据
              </p>
            </div>
          </div>
        </div>
      </Card>
    </div>
  )
}
