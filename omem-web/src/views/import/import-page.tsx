import { useState, useCallback } from "react"
import { useNavigate } from "react-router-dom"
import { ArrowLeft, Upload, FileJson, FileSpreadsheet, FileText, CheckCircle, XCircle } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { toast } from "sonner"
import apiClient from "@/api/client"

interface ImportResult {
  success: boolean
  imported: number
  failed: number
  errors?: string[]
}

export function ImportPage() {
  const navigate = useNavigate()
  const [file, setFile] = useState<File | null>(null)
  const [isUploading, setIsUploading] = useState(false)
  const [result, setResult] = useState<ImportResult | null>(null)
  const [dragActive, setDragActive] = useState(false)

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
      setResult(null)
    }
  }, [])

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files && e.target.files[0]) {
      setFile(e.target.files[0])
      setResult(null)
    }
  }

  const getFileIcon = (filename: string) => {
    if (filename.endsWith('.json')) return <FileJson className="h-8 w-8 text-blue-500" />
    if (filename.endsWith('.csv')) return <FileSpreadsheet className="h-8 w-8 text-green-500" />
    if (filename.endsWith('.md') || filename.endsWith('.markdown')) return <FileText className="h-8 w-8 text-purple-500" />
    return <FileText className="h-8 w-8 text-muted-foreground" />
  }

  const handleImport = async () => {
    if (!file) {
      toast.error("请先选择文件")
      return
    }

    const formData = new FormData()
    formData.append("file", file)

    const format = file.name.endsWith('.json') ? 'json' : 
                   file.name.endsWith('.csv') ? 'csv' : 'md'
    formData.append("format", format)

    setIsUploading(true)
    try {
      const response = await apiClient.post("/v1/imports", formData, {
        headers: {
          "Content-Type": "multipart/form-data",
        },
      })
      setResult({
        success: true,
        imported: response.imported || 0,
        failed: response.failed || 0,
        errors: response.errors,
      })
      toast.success(`成功导入 ${response.imported || 0} 条记忆`)
    } catch (error: any) {
      const message = error.response?.data?.error || error.message || "导入失败"
      setResult({
        success: false,
        imported: 0,
        failed: 0,
        errors: [message],
      })
      toast.error(message)
    } finally {
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
        <div className="flex items-center gap-3 rounded-lg border border-border bg-card p-4">
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
            {isUploading ? "导入中..." : "开始导入"}
          </Button>
        </div>
      )}

      {result && (
        <Card className="p-6">
          <div className="flex items-center gap-2 mb-4">
            {result.success ? (
              <CheckCircle className="h-5 w-5 text-green-500" />
            ) : (
              <XCircle className="h-5 w-5 text-red-500" />
            )}
            <h3 className="font-semibold">
              {result.success ? "导入成功" : "导入失败"}
            </h3>
          </div>
          {result.success && (
            <div className="grid grid-cols-2 gap-4">
              <div className="rounded-lg bg-green-500/10 p-4 text-center">
                <p className="text-2xl font-bold text-green-500">{result.imported}</p>
                <p className="text-xs text-muted-foreground">成功导入</p>
              </div>
              <div className="rounded-lg bg-red-500/10 p-4 text-center">
                <p className="text-2xl font-bold text-red-500">{result.failed}</p>
                <p className="text-xs text-muted-foreground">导入失败</p>
              </div>
            </div>
          )}
          {result.errors && result.errors.length > 0 && (
            <div className="mt-4 space-y-2">
              <p className="text-sm font-medium text-red-500">错误信息：</p>
              {result.errors.map((error) => (
                <p key={error} className="text-xs text-muted-foreground">{error}</p>
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
